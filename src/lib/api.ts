import type { LocalSourceInfo } from "./localSource";
import type {
  CopyReport,
  DestinationEntry,
  Detection,
  ErrorPayload,
  ImportOffer,
  InstallPlan,
  InstalledState,
  PackInstallState,
  LauncherPrefs,
  LauncherReport,
  Manifest,
  ManifestLoad,
  PackIndexLoad,
  Prefs,
  ProgressEvent,
  QualityLevel,
  Recommendation,
  Selection,
  SnapshotMeta,
} from "../types";
import { APP_VERSION } from "./version";
import {
  mockDetection,
  mockInstalled,
  mockManifest,
  mockPackIndex,
  mockPlan,
  mockRecommendation,
  mockSnapshots,
} from "../mock/backend";

function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke: tauriInvoke } = await import("@tauri-apps/api/core");
  return tauriInvoke<T>(cmd, args);
}

export async function getLocalSource(): Promise<LocalSourceInfo> {
  if (!isTauri()) {
    return { enabled: false, dir: null, banner: null, needsFolder: false };
  }
  try {
    return await invoke<LocalSourceInfo>("get_local_source");
  } catch {
    return { enabled: false, dir: null, banner: null, needsFolder: false };
  }
}

export async function pickLocalSource(): Promise<LocalSourceInfo> {
  if (!isTauri()) {
    return { enabled: false, dir: null, banner: null, needsFolder: false };
  }
  return invoke<LocalSourceInfo>("pick_local_source");
}

export async function setLocalSource(dir: string): Promise<LocalSourceInfo> {
  if (!isTauri()) {
    return { enabled: true, dir, banner: "TEST BUILD - local source", needsFolder: false };
  }
  return invoke<LocalSourceInfo>("set_local_source", { dir });
}

export async function getManifest(): Promise<ManifestLoad> {
  if (!isTauri()) {
    return { manifest: mockManifest(), source: "bundled", offline: false, message: null };
  }
  return await invoke<ManifestLoad>("get_manifest");
}

export async function detect(): Promise<Detection> {
  if (!isTauri()) return mockDetection();
  try {
    return await invoke<Detection>("detect");
  } catch {
    return mockDetection();
  }
}

export async function scanHardware(): Promise<Recommendation> {
  if (!isTauri()) return mockRecommendation();
  try {
    return await invoke<Recommendation>("scan_hardware");
  } catch {
    return mockRecommendation();
  }
}

export async function plan(sel: Selection): Promise<InstallPlan> {
  if (!isTauri()) return mockPlan(sel);
  try {
    return await invoke<InstallPlan>("plan", { sel });
  } catch {
    return mockPlan(sel);
  }
}

export async function listInstalled(): Promise<InstalledState | null> {
  if (!isTauri()) return mockInstalled();
  try {
    return await invoke<InstalledState | null>("list_installed");
  } catch {
    return null;
  }
}

export async function listPackStates(): Promise<PackInstallState[]> {
  if (!isTauri()) {
    return mockPackIndex().index.packs.map((pack) => ({
      packId: pack.id,
      installed: pack.id === "better-adventures-plus" ? mockInstalled() : null,
    }));
  }
  try {
    return await invoke<PackInstallState[]>("list_pack_states");
  } catch {
    return [];
  }
}

export async function listSnapshots(): Promise<SnapshotMeta[]> {
  if (!isTauri()) return mockSnapshots();
  try {
    return await invoke<SnapshotMeta[]>("list_snapshots");
  } catch {
    return mockSnapshots();
  }
}

export async function setGameDir(path: string): Promise<Detection> {
  if (!isTauri()) {
    return mockDetection({
      gameDir: path,
      minecraftVersionInstalled: path.length > 0,
    });
  }
  return invoke<Detection>("set_game_dir", { path });
}

/**
 * One id per install or repair the player starts. `install`, `repair` and
 * `cancel` all carry it, so a Cancel pressed before the backend worker has
 * registered its flag is still honoured, and never leaks into the next run.
 */
interface InstallRequest {
  id: string;
  cancelled: boolean;
}

let activeRequest: InstallRequest | null = null;
let requestSeq = 0;

export const CANCELLED_PAYLOAD: ErrorPayload = {
  code: "E-CANCELLED",
  title: "Install cancelled",
  hint: "Nothing was left half-applied. Your game folder is as it was before this run.",
  detail: "Cancelled before the install started.",
};

function newRequestId(): string {
  requestSeq += 1;
  const random =
    globalThis.crypto?.randomUUID?.() ??
    `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
  return `req-${requestSeq}-${random}`;
}

/** Call when the player starts an install or repair, before anything async. */
export function beginInstallRequest(): string {
  const id = newRequestId();
  activeRequest = { id, cancelled: false };
  return id;
}

/** Call when that install or repair has settled. */
export function endInstallRequest(id: string): void {
  if (activeRequest?.id === id) activeRequest = null;
}

function cancelledLocally(id: string | undefined): boolean {
  return id !== undefined && activeRequest?.id === id && activeRequest.cancelled;
}

export async function runInstall(
  nextPlan: InstallPlan,
  replaceCollisions = false,
  requestId?: string,
): Promise<string> {
  if (cancelledLocally(requestId)) throw CANCELLED_PAYLOAD;
  if (!isTauri()) return "mock-run";
  return invoke<string>("install", { plan: nextPlan, replaceCollisions, requestId });
}

export async function cancelInstall(): Promise<void> {
  const request = activeRequest;
  if (request) request.cancelled = true;
  if (!isTauri()) return;
  // With an id the backend keeps the cancel even if its worker has not started yet.
  await invoke("cancel", request ? { requestId: request.id } : {});
}

export async function repairInstall(requestId?: string): Promise<string> {
  if (cancelledLocally(requestId)) throw CANCELLED_PAYLOAD;
  if (!isTauri()) return "mock-repair";
  return invoke<string>("repair", { requestId });
}

export async function listenProgress(onEvent: (event: ProgressEvent) => void): Promise<() => void> {
  if (!isTauri()) return () => undefined;
  const { listen } = await import("@tauri-apps/api/event");
  const unlisten = await listen<ProgressEvent>("install://progress", (event) => {
    onEvent(event.payload);
  });
  return unlisten;
}

export async function onFolderSwitched(
  cb: (entry: DestinationEntry) => void,
): Promise<() => void> {
  if (!isTauri()) return () => undefined;
  const { listen } = await import("@tauri-apps/api/event");
  const unlisten = await listen<DestinationEntry>("pack://folder-switched", (event) => {
    cb(event.payload);
  });
  return unlisten;
}

export async function exportLog(): Promise<string> {
  if (!isTauri()) {
    return "C:\\Users\\Sam\\Documents\\Arnold Installer\\logs\\Arnold-install-log-2026-11-02-1915.txt";
  }
  const result = await invoke<{ path: string; revealed: boolean }>("export_log");
  return result.path;
}

export async function revealPath(path: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("reveal_path", { path });
}

export async function copyDiagnostics(): Promise<string> {
  if (!isTauri()) {
    return `Arnold Client Mods diagnostics\napp: ${APP_VERSION}\n`;
  }
  return invoke<string>("copy_diagnostics");
}

export function launcherNote(packName: string, launcherLabel: string): string {
  return `Open ${launcherLabel} and choose the 'Arnold — ${packName}' profile.`;
}

export const LAUNCHER_NOTE = launcherNote("Better Adventures+", "Minecraft Launcher");

export interface LauncherAvailability {
  available: boolean;
  note: string;
}

export interface LauncherOpenResult {
  opened: boolean;
  note: string;
}

export async function detectLauncher(): Promise<LauncherAvailability> {
  if (!isTauri()) return { available: false, note: LAUNCHER_NOTE };
  return invoke<LauncherAvailability>("detect_launcher");
}

export async function openLauncher(): Promise<LauncherOpenResult> {
  if (!isTauri()) return { opened: false, note: LAUNCHER_NOTE };
  return invoke<LauncherOpenResult>("open_launcher");
}

export async function finishApp(): Promise<void> {
  if (!isTauri()) {
    window.close();
    return;
  }
  await invoke("finish");
}

export async function detectLegacy(): Promise<ImportOffer> {
  if (!isTauri()) {
    return {
      found: true,
      versionGuess: "2.3.x / 1.3.x snowfall",
      alreadyAdopted: false,
      jars: [
        {
          path: "mods/arnold-snowfall-1.3.2.jar",
          fileName: "arnold-snowfall-1.3.2.jar",
          componentId: "snow",
          sha512: "abc",
          knownBuild: true,
          tracked: false,
        },
      ],
      backups: [{ path: "arnold-backups", kind: "folder" }],
    };
  }
  return invoke<ImportOffer>("detect_legacy");
}

export async function confirmImport(): Promise<InstalledState> {
  if (!isTauri()) return mockInstalled();
  return invoke<InstalledState>("confirm_import");
}

export async function listPacks(): Promise<PackIndexLoad> {
  if (!isTauri()) return mockPackIndex();
  return invoke<PackIndexLoad>("list_packs");
}

export async function selectPack(id: string): Promise<Prefs> {
  if (!isTauri()) return { lastGameDir: null, tourDone: true, selectedPack: id };
  return invoke<Prefs>("select_pack", { id });
}

export async function choosePackFolder(choice: "keep" | "fresh"): Promise<Prefs> {
  if (!isTauri()) {
    return {
      lastGameDir: null,
      tourDone: true,
      packLocations: { "better-adventures-plus": choice === "keep" ? "defaultFolder" : "ownFolder" },
    };
  }
  return invoke<Prefs>("choose_pack_folder", { choice });
}

export async function copyFromOldFolder(item: "options" | "servers" | "saves"): Promise<CopyReport> {
  if (!isTauri()) return { item, copied: [item], skipped: [] };
  return invoke<CopyReport>("copy_from_old_folder", { item });
}

export async function setUseDefaultFolder(enabled: boolean): Promise<Prefs> {
  if (!isTauri()) return { lastGameDir: null, tourDone: true, useDefaultFolder: enabled };
  return invoke<Prefs>("set_use_default_folder", { enabled });
}

export async function setLauncherPrefs(launchers: LauncherPrefs): Promise<Prefs> {
  if (!isTauri()) return { lastGameDir: null, tourDone: true, launchers };
  return invoke<Prefs>("set_launcher_prefs", { prefs: launchers });
}

export async function detectLaunchers(): Promise<LauncherReport> {
  if (!isTauri()) return mockDetection().launchers as LauncherReport;
  return invoke<LauncherReport>("detect_launchers");
}

export async function play(): Promise<LauncherOpenResult> {
  if (!isTauri()) return { opened: false, note: LAUNCHER_NOTE };
  return invoke<LauncherOpenResult>("play");
}

export async function openCreditLink(which: "matcha-source" | "cc-by-nc-sa-4"): Promise<void> {
  if (!isTauri()) return;
  await invoke("open_credit_link", { which });
}

export async function rollback(snapshotId: string): Promise<InstalledState> {
  if (!isTauri()) return mockInstalled();
  return invoke<InstalledState>("rollback", { snapshotId });
}

export async function getPrefs(): Promise<Prefs> {
  if (!isTauri()) return { lastGameDir: null, tourDone: false, useDefaultFolder: false };
  try {
    return await invoke<Prefs>("get_prefs");
  } catch {
    return { lastGameDir: null, tourDone: false };
  }
}

export async function setTourDone(): Promise<void> {
  if (!isTauri()) return;
  await invoke("set_tour_done");
}

export async function getConfig(id: string): Promise<{
  id: string;
  hasConfig: boolean;
  file?: string;
  text?: string;
  liteDetected?: boolean;
  settings?: Record<string, { type: string; default: unknown }>;
}> {
  if (!isTauri()) {
    return { id, hasConfig: true, text: "", liteDetected: false, settings: {} };
  }
  return invoke("get_config", { id });
}

export async function setConfig(id: string, lite: boolean): Promise<void> {
  if (!isTauri()) return;
  await invoke("set_config", { id, lite });
}

export async function setConfigKeys(id: string, values: Record<string, string>): Promise<void> {
  if (!isTauri()) return;
  await invoke("set_config_keys", { update: { id, values } });
}

export async function setQuality(level: QualityLevel): Promise<InstalledState | null> {
  if (!isTauri()) return mockInstalled();
  return invoke<InstalledState>("set_quality", { level });
}

export async function checkUpdate(): Promise<{ available: boolean; reason: string; enabled?: boolean }> {
  if (!isTauri()) return { available: false, reason: "Preview build", enabled: false };
  return invoke("check_update");
}

function payloadFrom(error: unknown): ErrorPayload | null {
  if (typeof error === "string") {
    try {
      return payloadFrom(JSON.parse(error) as unknown);
    } catch {
      return null;
    }
  }
  if (error && typeof error === "object" && "code" in error && "title" in error) {
    return error as ErrorPayload;
  }
  return null;
}

export function asErrorPayload(error: unknown): ErrorPayload {
  const payload = payloadFrom(error);
  if (payload) return payload;
  if (error && typeof error === "object" && "message" in error) {
    const message = String((error as { message: unknown }).message);
    const nested = payloadFrom(message);
    if (nested) return nested;
    return {
      code: "E-IO",
      title: "Something went wrong",
      hint: "Try again. If it keeps happening, use Copy diagnostics.",
      detail: message,
    };
  }
  return {
    code: "E-IO",
    title: "Something went wrong",
    hint: "Try again. If it keeps happening, use Copy diagnostics.",
    detail: String(error),
  };
}

export function bundleManifest(load: ManifestLoad): Manifest {
  return load.manifest;
}
