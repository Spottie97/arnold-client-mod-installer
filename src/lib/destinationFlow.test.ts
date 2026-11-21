import { readFileSync } from "node:fs";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Detection, ImportOffer, InstallPlan, PackLocation, Prefs, ProgressEvent, Selection } from "../types";
import * as api from "./api";
import { createDestinationFlow } from "./destinationFlow";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

let progressHandler: ((event: { payload: ProgressEvent }) => void) | null = null;
let unlisten = vi.fn();

const ROOT = "/home/sam/.minecraft";
const SELECTION: Selection = {
  kit: "basic",
  customComponents: [],
  ocean: false,
  addons: {},
  quality: null,
};

interface Folder {
  owner: string | null;
  arnoldJars: string[];
  stashes: string[];
}

interface PendingInstall {
  resolve: (value: string) => void;
  reject: (reason?: unknown) => void;
  requestId: string | undefined;
}

interface Backend {
  prefs: Prefs;
  root: string;
  folders: Record<string, Folder>;
  calls: string[];
  installed: InstallPlan | null;
  holdInstall: boolean;
  pending: PendingInstall | null;
  legacyOffer: ImportOffer | null;
  /** Mirrors the Rust CancelRegistry: cancels are kept per request id. */
  cancelledIds: Set<string>;
  installIds: (string | undefined)[];
  cancelIds: (string | undefined)[];
}

function folderOf(folders: Record<string, Folder>, path: string): Folder {
  const existing = folders[path];
  if (existing) return existing;
  const created: Folder = { owner: null, arnoldJars: [], stashes: [] };
  folders[path] = created;
  return created;
}

function selectedId(prefs: Prefs): string {
  return prefs.selectedPack ?? "";
}

function resolvesDefault(prefs: Prefs, id: string): boolean {
  const location = prefs.packLocations?.[id];
  if (location === "ownFolder") return false;
  if (location === "defaultFolder") return true;
  return Boolean(prefs.useDefaultFolder);
}

function resolvedDir(backend: Backend): string {
  const id = selectedId(backend.prefs);
  return resolvesDefault(backend.prefs, id) ? backend.root : `/own/${id}`;
}

function detectionOf(backend: Backend): Detection {
  return {
    candidates: [],
    minecraftVersion: "26.3",
    minecraftVersionInstalled: true,
    fabric: { installed: true, willInstall: false },
    gameRunning: false,
    importedFromLegacy: false,
    packId: selectedId(backend.prefs),
    packDir: resolvedDir(backend),
    gameDir: backend.root,
    needsFolderChoice: null,
  };
}

function planOf(backend: Backend): InstallPlan {
  return {
    add: [],
    update: [],
    remove: [],
    configs: [],
    loader: null,
    quality: "normal",
    kit: "basic",
    components: [],
    warnings: [],
    bytes: 0,
    packId: selectedId(backend.prefs),
    packDir: resolvedDir(backend),
  };
}

function cloneFolders(folders: Record<string, Folder>): Record<string, Folder> {
  return Object.fromEntries(
    Object.entries(folders).map(([path, folder]) => [
      path,
      {
        owner: folder.owner,
        arnoldJars: [...folder.arnoldJars],
        stashes: [...folder.stashes],
      },
    ]),
  );
}

function enter(folders: Record<string, Folder>, id: string, root: string) {
  const folder = folderOf(folders, root);
  if (folder.arnoldJars.length > 0 && !folder.owner) {
    throw {
      code: "E-FOLDER-OCCUPIED",
      title: "This Minecraft folder already has Arnold mods we didn't set up",
      hint: "Nothing was moved or changed",
      detail: folder.arnoldJars.join(", "),
    };
  }
  if (folder.owner && folder.owner !== id) {
    folder.stashes.push(folder.owner);
    folder.owner = id;
  } else if (!folder.owner) {
    folder.owner = id;
  }
}

function commit(backend: Backend, next: Prefs, root: string, shouldEnter: boolean) {
  const folders = cloneFolders(backend.folders);
  if (shouldEnter) {
    enter(folders, selectedId(next), root);
  }
  backend.prefs = next;
  backend.root = root;
  backend.folders = folders;
}

function fakeBackend(seed: Pick<Backend, "prefs" | "root" | "folders">): Backend {
  const backend: Backend = {
    prefs: seed.prefs,
    root: seed.root,
    folders: cloneFolders(seed.folders),
    calls: [],
    installed: null,
    holdInstall: false,
    pending: null,
    legacyOffer: null,
    cancelledIds: new Set(),
    installIds: [],
    cancelIds: [],
  };
  unlisten = vi.fn();
  progressHandler = null;
  vi.mocked(listen).mockImplementation((async (_name: string, cb: (event: { payload: ProgressEvent }) => void) => {
    backend.calls.push("listen");
    progressHandler = cb;
    return unlisten;
  }) as unknown as typeof listen);
  const handle = (cmd: string, args: Record<string, unknown>) => {
    if (cmd === "select_pack") {
      backend.calls.push(cmd);
      const next: Prefs = { ...backend.prefs, selectedPack: String(args.id) };
      commit(backend, next, backend.root, resolvesDefault(next, selectedId(next)));
      return backend.prefs;
    }
    if (cmd === "set_use_default_folder") {
      backend.calls.push(cmd);
      const next: Prefs = { ...backend.prefs, useDefaultFolder: Boolean(args.enabled) };
      commit(
        backend,
        next,
        backend.root,
        Boolean(args.enabled) && resolvesDefault(next, selectedId(next)),
      );
      return backend.prefs;
    }
    if (cmd === "choose_pack_folder") {
      backend.calls.push(cmd);
      const id = selectedId(backend.prefs);
      const choice = args.choice === "keep" ? "defaultFolder" : "ownFolder";
      const next: Prefs = {
        ...backend.prefs,
        packLocations: { ...backend.prefs.packLocations, [id]: choice as PackLocation },
        lastGameDir: args.choice === "keep" ? backend.root : backend.prefs.lastGameDir,
      };
      commit(backend, next, backend.root, choice === "defaultFolder");
      return backend.prefs;
    }
    if (cmd === "set_game_dir") {
      backend.calls.push(cmd);
      const path = String(args.path);
      const next: Prefs = { ...backend.prefs, lastGameDir: path };
      commit(backend, next, path, resolvesDefault(next, selectedId(next)));
      return detectionOf(backend);
    }
    if (cmd === "detect") {
      backend.calls.push(cmd);
      return detectionOf(backend);
    }
    if (cmd === "plan") {
      backend.calls.push(cmd);
      return planOf(backend);
    }
    if (cmd === "install") {
      backend.calls.push(cmd);
      const plan = args.plan as InstallPlan;
      const dir = resolvedDir(backend);
      const owner = folderOf(backend.folders, dir).owner;
      if (plan.packDir !== dir || owner !== selectedId(backend.prefs)) {
        throw {
          code: "E-STATE",
          title: "That install plan is for a different folder",
          hint: "Open the pack again.",
          detail: `${plan.packDir} vs ${dir}`,
        };
      }
      const requestId = args.requestId as string | undefined;
      backend.installIds.push(requestId);
      if (backend.holdInstall) {
        return new Promise<string>((resolve, reject) => {
          backend.pending = { resolve, reject, requestId };
        });
      }
      if (requestId !== undefined && backend.cancelledIds.has(requestId)) {
        throw { code: "E-CANCELLED", title: "Cancelled", hint: "", detail: "" };
      }
      backend.installed = plan;
      return "run-1";
    }
    if (cmd === "cancel") {
      backend.calls.push(cmd);
      const requestId = args.requestId as string | undefined;
      backend.cancelIds.push(requestId);
      if (requestId !== undefined) backend.cancelledIds.add(requestId);
      const pending = backend.pending;
      if (pending && (requestId === undefined || pending.requestId === requestId)) {
        pending.reject({ code: "E-CANCELLED", title: "Cancelled", hint: "", detail: "" });
      }
      return null;
    }
    if (cmd === "detect_legacy") {
      backend.calls.push(cmd);
      return backend.legacyOffer;
    }
    if (cmd === "confirm_import") {
      backend.calls.push(cmd);
      const folder = folderOf(backend.folders, backend.root);
      folder.owner = "better-adventures-plus";
      folder.arnoldJars = [];
      return {
        schema: 1,
        source: "imported",
        manifestVersion: "2.3.x",
        components: [],
        files: [],
        updatedAt: "now",
      };
    }
    throw new Error(`unexpected command ${cmd}`);
  };
  vi.mocked(invoke).mockImplementation(((cmd: string, args?: Record<string, unknown>) =>
    Promise.resolve(handle(cmd, args ?? {}))) as typeof invoke);
  return backend;
}

function flowFor() {
  return createDestinationFlow({
    invalidate: () => undefined,
    detect: () => api.detect(),
    replan: () => api.plan(SELECTION),
  });
}

const noHooks = { onStart: () => undefined, onProgress: () => undefined };

function barePlan(packDir: string): InstallPlan {
  return {
    add: [],
    update: [],
    remove: [],
    configs: [],
    loader: null,
    quality: "normal",
    kit: "basic",
    components: [],
    warnings: [],
    bytes: 0,
    packId: "pack-x",
    packDir,
  };
}

describe("destinationFlow", () => {
  beforeEach(() => {
    vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.clearAllMocks();
  });

  it("selection then default-folder toggle then install sends the replanned plan (legacy owner)", async () => {
    const backend = fakeBackend({
      prefs: { tourDone: true, selectedPack: "better-adventures-plus", useDefaultFolder: false },
      root: ROOT,
      folders: { [ROOT]: { owner: "better-adventures-plus", arnoldJars: [], stashes: [] } },
    });
    const flow = flowFor();
    await flow.selectPack("pack-x");
    const out = await flow.useDefaultFolder(true);
    const runId = await flow.install(out.plan!, out.detection, "pack-x", false, noHooks);
    expect(runId).toBe("run-1");
    expect(backend.calls).toEqual([
      "select_pack",
      "detect",
      "set_use_default_folder",
      "detect",
      "plan",
      "listen",
      "install",
    ]);
    expect(backend.installed?.packDir).toBe(ROOT);
    expect(backend.folders[ROOT].owner).toBe("pack-x");
    expect(backend.folders[ROOT].stashes).toContain("better-adventures-plus");
  });

  it("selection then keep then install (marked other pack)", async () => {
    const backend = fakeBackend({
      prefs: { tourDone: true, selectedPack: "pack-a", useDefaultFolder: false },
      root: ROOT,
      folders: { [ROOT]: { owner: "pack-a", arnoldJars: [], stashes: [] } },
    });
    const flow = flowFor();
    await flow.selectPack("pack-x");
    const out = await flow.chooseFolder("keep");
    const runId = await flow.install(out.plan!, out.detection, "pack-x", false, noHooks);
    expect(runId).toBe("run-1");
    expect(backend.calls).toEqual([
      "select_pack",
      "detect",
      "choose_pack_folder",
      "detect",
      "plan",
      "listen",
      "install",
    ]);
    expect(backend.installed?.packDir).toBe(ROOT);
    expect(backend.folders[ROOT].owner).toBe("pack-x");
    expect(backend.folders[ROOT].stashes).toContain("pack-a");
  });

  it("selection then set_game_dir then install uses the new root", async () => {
    const nextRoot = "/games/new-minecraft";
    const backend = fakeBackend({
      prefs: { tourDone: true, selectedPack: "pack-a", useDefaultFolder: true },
      root: ROOT,
      folders: { [ROOT]: { owner: "pack-a", arnoldJars: [], stashes: [] } },
    });
    const flow = flowFor();
    await flow.selectPack("pack-x");
    const out = await flow.setGameDir(nextRoot);
    const runId = await flow.install(out.plan!, out.detection, "pack-x", false, noHooks);
    expect(runId).toBe("run-1");
    expect(backend.installed?.packDir).toBe(nextRoot);
    expect(out.plan?.packDir).toBe(nextRoot);
    expect(out.detection.packDir).toBe(nextRoot);
  });

  it("refused occupied folder never replans or installs", async () => {
    const backend = fakeBackend({
      prefs: { tourDone: true, selectedPack: "pack-x", useDefaultFolder: false },
      root: ROOT,
      folders: {
        [ROOT]: {
          owner: null,
          arnoldJars: ["mods/arnold-snowfall-1.3.3.jar"],
          stashes: [],
        },
      },
    });
    const flow = flowFor();
    await expect(flow.useDefaultFolder(true)).rejects.toMatchObject({ code: "E-FOLDER-OCCUPIED" });
    expect(backend.calls[backend.calls.length - 1]).toBe("set_use_default_folder");
    expect(backend.calls).toEqual(["set_use_default_folder"]);
    expect(backend.prefs.useDefaultFolder).toBe(false);
    expect(backend.folders[ROOT].owner).toBeNull();
    expect(backend.installed).toBeNull();
  });

  it("stale plan from before the change is not installed", async () => {
    const backend = fakeBackend({
      prefs: { tourDone: true, selectedPack: "pack-x", useDefaultFolder: true },
      root: ROOT,
      folders: { [ROOT]: { owner: "pack-x", arnoldJars: [], stashes: [] } },
    });
    const flow = flowFor();
    const detection = detectionOf(backend);
    const onStart = vi.fn();
    const runId = await flow.install(barePlan("/before/the/change"), detection, "pack-x", false, {
      onStart,
      onProgress: () => undefined,
    });
    expect(runId).toBeNull();
    expect(backend.calls).not.toContain("install");
    expect(backend.calls).not.toContain("listen");
    expect(onStart).not.toHaveBeenCalled();
    expect(listen).not.toHaveBeenCalled();
    expect(backend.installed).toBeNull();
  });

  it("progress is live and the screen opens before the install resolves", async () => {
    const backend = fakeBackend({
      prefs: { tourDone: true, selectedPack: "pack-x", useDefaultFolder: true },
      root: ROOT,
      folders: { [ROOT]: { owner: "pack-x", arnoldJars: [], stashes: [] } },
    });
    backend.holdInstall = true;
    const flow = flowFor();
    const onStart = vi.fn();
    const events: ProgressEvent[] = [];
    const pending = flow.install(planOf(backend), detectionOf(backend), "pack-x", false, {
      onStart,
      onProgress: (event) => events.push(event),
    });
    await vi.waitFor(() => expect(backend.calls).toContain("install"));
    expect(backend.calls).toEqual(["listen", "install"]);
    expect(onStart).toHaveBeenCalledOnce();
    expect(backend.pending).not.toBeNull();
    progressHandler?.({
      payload: {
        runId: "run-1",
        step: "download",
        status: "running",
        percent: 30,
        message: "Downloading",
      },
    });
    expect(events).toHaveLength(1);
    expect(backend.pending).not.toBeNull();
    backend.pending?.resolve("run-1");
    await expect(pending).resolves.toBe("run-1");
    expect(unlisten).toHaveBeenCalledOnce();
  });

  it("cancel works while the install is pending", async () => {
    const backend = fakeBackend({
      prefs: { tourDone: true, selectedPack: "pack-x", useDefaultFolder: true },
      root: ROOT,
      folders: { [ROOT]: { owner: "pack-x", arnoldJars: [], stashes: [] } },
    });
    backend.holdInstall = true;
    const flow = flowFor();
    const pending = flow.install(planOf(backend), detectionOf(backend), "pack-x", false, noHooks);
    const rejected = expect(pending).rejects.toMatchObject({ code: "E-CANCELLED" });
    await vi.waitFor(() => expect(backend.calls).toContain("install"));
    await api.cancelInstall();
    expect(backend.calls.indexOf("cancel")).toBeGreaterThan(backend.calls.indexOf("install"));
    await rejected;
    expect(unlisten).toHaveBeenCalledOnce();
  });

  it("cancel carries the same request id as the install", async () => {
    const backend = fakeBackend({
      prefs: { tourDone: true, selectedPack: "pack-x", useDefaultFolder: true },
      root: ROOT,
      folders: { [ROOT]: { owner: "pack-x", arnoldJars: [], stashes: [] } },
    });
    backend.holdInstall = true;
    const flow = flowFor();
    const pending = flow.install(planOf(backend), detectionOf(backend), "pack-x", false, noHooks);
    const rejected = expect(pending).rejects.toMatchObject({ code: "E-CANCELLED" });
    await vi.waitFor(() => expect(backend.calls).toContain("install"));
    await api.cancelInstall();
    await rejected;
    expect(backend.installIds[0]).toMatch(/^req-/);
    expect(backend.cancelIds).toEqual([backend.installIds[0]]);
  });

  it("cancel pressed before the install command is sent is honoured", async () => {
    const backend = fakeBackend({
      prefs: { tourDone: true, selectedPack: "pack-x", useDefaultFolder: true },
      root: ROOT,
      folders: { [ROOT]: { owner: "pack-x", arnoldJars: [], stashes: [] } },
    });
    let openListener: (() => void) | null = null;
    vi.mocked(listen).mockImplementation((async () => {
      backend.calls.push("listen");
      await new Promise<void>((resolve) => {
        openListener = resolve;
      });
      return unlisten;
    }) as unknown as typeof listen);
    const flow = flowFor();
    const pending = flow.install(planOf(backend), detectionOf(backend), "pack-x", false, noHooks);
    const rejected = expect(pending).rejects.toMatchObject({ code: "E-CANCELLED" });
    await vi.waitFor(() => expect(openListener).not.toBeNull());
    await api.cancelInstall();
    expect(backend.cancelIds).toHaveLength(1);
    expect(backend.cancelIds[0]).toMatch(/^req-/);
    openListener!();
    await rejected;
    expect(backend.calls).not.toContain("install");
    expect(backend.installed).toBeNull();
    expect(unlisten).toHaveBeenCalledOnce();
  });

  it("cancel that reaches the backend before its worker registers is honoured", async () => {
    const backend = fakeBackend({
      prefs: { tourDone: true, selectedPack: "pack-x", useDefaultFolder: true },
      root: ROOT,
      folders: { [ROOT]: { owner: "pack-x", arnoldJars: [], stashes: [] } },
    });
    const flow = flowFor();
    // The backend has stored this request's cancel before the worker looked at it.
    const realInvoke = vi.mocked(invoke).getMockImplementation()!;
    vi.mocked(invoke).mockImplementation(((cmd: string, args?: Record<string, unknown>) => {
      if (cmd === "install") backend.cancelledIds.add(String(args?.requestId));
      return realInvoke(cmd, args);
    }) as typeof invoke);
    await expect(
      flow.install(planOf(backend), detectionOf(backend), "pack-x", false, noHooks),
    ).rejects.toMatchObject({ code: "E-CANCELLED" });
    expect(backend.installed).toBeNull();
  });

  it("a cancel from an earlier install never stops the next one", async () => {
    const backend = fakeBackend({
      prefs: { tourDone: true, selectedPack: "pack-x", useDefaultFolder: true },
      root: ROOT,
      folders: { [ROOT]: { owner: "pack-x", arnoldJars: [], stashes: [] } },
    });
    backend.holdInstall = true;
    const flow = flowFor();
    const first = flow.install(planOf(backend), detectionOf(backend), "pack-x", false, noHooks);
    const firstRejected = expect(first).rejects.toMatchObject({ code: "E-CANCELLED" });
    await vi.waitFor(() => expect(backend.installIds).toHaveLength(1));
    await api.cancelInstall();
    await firstRejected;
    // No install is running now: a stray Cancel has no request to stop.
    await api.cancelInstall();
    expect(backend.cancelIds[1]).toBeUndefined();
    backend.holdInstall = false;
    await expect(
      flow.install(planOf(backend), detectionOf(backend), "pack-x", false, noHooks),
    ).resolves.toBe("run-1");
    expect(backend.installIds[1]).not.toEqual(backend.installIds[0]);
    expect(backend.cancelledIds.has(backend.installIds[1]!)).toBe(false);
  });

  it("BA+ refusal offers the import, other packs do not", async () => {
    const backend = fakeBackend({
      prefs: { tourDone: true, selectedPack: "better-adventures-plus", useDefaultFolder: true },
      root: ROOT,
      folders: { [ROOT]: { owner: null, arnoldJars: [], stashes: [] } },
    });
    const flow = flowFor();
    await expect(
      flow.occupiedImportOffer({ code: "E-FOLDER-OCCUPIED" }, "pack-x"),
    ).resolves.toBeNull();
    expect(backend.calls).not.toContain("detect_legacy");
    const offer: ImportOffer = {
      found: true,
      alreadyAdopted: false,
      jars: [
        {
          path: "mods/arnold-snowfall-1.3.2.jar",
          fileName: "arnold-snowfall-1.3.2.jar",
          sha512: "abc",
          knownBuild: false,
          tracked: false,
        },
      ],
      backups: [],
    };
    backend.legacyOffer = offer;
    await expect(
      flow.occupiedImportOffer({ code: "E-FOLDER-OCCUPIED" }, "better-adventures-plus"),
    ).resolves.toEqual(offer);
    await expect(
      flow.occupiedImportOffer({ code: "E-STATE" }, "better-adventures-plus"),
    ).resolves.toBeNull();
    backend.legacyOffer = { ...offer, alreadyAdopted: true };
    await expect(
      flow.occupiedImportOffer({ code: "E-FOLDER-OCCUPIED" }, "better-adventures-plus"),
    ).resolves.toBeNull();
  });

  it("adopting then retrying keep enters the folder", async () => {
    const backend = fakeBackend({
      prefs: { tourDone: true, selectedPack: "better-adventures-plus", useDefaultFolder: false },
      root: ROOT,
      folders: {
        [ROOT]: {
          owner: null,
          arnoldJars: ["mods/arnold-snowfall-1.3.2.jar"],
          stashes: [],
        },
      },
    });
    const flow = flowFor();
    await expect(flow.chooseFolder("keep")).rejects.toMatchObject({ code: "E-FOLDER-OCCUPIED" });
    await flow.adoptThen(() => flow.chooseFolder("keep"));
    const confirm = backend.calls.indexOf("confirm_import");
    const firstChoose = backend.calls.indexOf("choose_pack_folder");
    const secondChoose = backend.calls.indexOf("choose_pack_folder", confirm);
    expect(confirm).toBeGreaterThan(firstChoose);
    expect(secondChoose).toBeGreaterThan(confirm);
    expect(backend.folders[ROOT].owner).toBe("better-adventures-plus");
  });

  it("App routes destination commands through destinationFlow", () => {
    const source = readFileSync("src/App.tsx", "utf8");
    expect(source).toContain("createDestinationFlow(");
    const clause = source.match(/import\s*\{([^}]+)\}\s*from\s*"\.\/lib\/api"/);
    expect(clause).toBeTruthy();
    const names = clause?.[1] ?? "";
    for (const forbidden of [
      "selectPack",
      "setUseDefaultFolder",
      "choosePackFolder",
      "setGameDir",
      "runInstall",
    ]) {
      expect(names).not.toMatch(new RegExp(`\\b${forbidden}\\b`));
    }
    const start = source.indexOf("const startInstall");
    const repair = source.indexOf("const startRepair");
    const slice = source.slice(start, repair);
    expect(slice).toContain("onStart:");
    expect(slice).not.toContain("listenProgress(");
    const onStart = slice.indexOf("onStart:");
    const progress = slice.indexOf('go("progress")');
    const onProgress = slice.indexOf("onProgress:");
    expect(progress).toBeGreaterThan(onStart);
    expect(progress).toBeLessThan(onProgress);
    expect(slice.match(/go\("progress"\)/g)).toEqual(['go("progress")']);
    expect(source).toContain("occupiedImportOffer(");
    expect(source).toContain("adoptThen(");
  });
});
