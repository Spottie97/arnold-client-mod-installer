import { readFileSync } from "node:fs";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import type { Detection, InstallPlan, PackLocation, Prefs, Selection } from "../types";
import * as api from "./api";
import { createDestinationFlow } from "./destinationFlow";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const ROOT = "/home/sam/.minecraft";
const SELECTION: Selection = {
  kit: "basic",
  customComponents: [],
  ocean: false,
  addons: {},
  snowLite: true,
  quality: null,
};

interface Folder {
  owner: string | null;
  arnoldJars: string[];
  stashes: string[];
}

interface Backend {
  prefs: Prefs;
  root: string;
  folders: Record<string, Folder>;
  calls: string[];
  installed: InstallPlan | null;
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
    snowLite: false,
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
  };
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
      backend.installed = plan;
      return "run-1";
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

function barePlan(packDir: string): InstallPlan {
  return {
    add: [],
    update: [],
    remove: [],
    configs: [],
    loader: null,
    snowLite: false,
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
    const runId = await flow.install(out.plan!, out.detection, "pack-x", false);
    expect(runId).toBe("run-1");
    expect(backend.calls).toEqual([
      "select_pack",
      "detect",
      "set_use_default_folder",
      "detect",
      "plan",
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
    const runId = await flow.install(out.plan!, out.detection, "pack-x", false);
    expect(runId).toBe("run-1");
    expect(backend.calls).toEqual([
      "select_pack",
      "detect",
      "choose_pack_folder",
      "detect",
      "plan",
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
    const runId = await flow.install(out.plan!, out.detection, "pack-x", false);
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
    const runId = await flow.install(barePlan("/before/the/change"), detection, "pack-x", false);
    expect(runId).toBeNull();
    expect(backend.calls).not.toContain("install");
    expect(backend.installed).toBeNull();
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
  });
});
