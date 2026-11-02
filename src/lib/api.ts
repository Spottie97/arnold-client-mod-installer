import type { LocalSourceInfo } from "./localSource";
import type {
  Detection,
  ErrorPayload,
  ImportOffer,
  InstallPlan,
  InstalledState,
  Manifest,
  ManifestLoad,
  Prefs,
  ProgressEvent,
  QualityLevel,
  Recommendation,
  Selection,
  SnapshotMeta,
} from "../types";
import {
  mockDetection,
  mockInstalled,
  mockManifest,
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
  try {
    return await invoke<ManifestLoad>("get_manifest");
  } catch {
    return { manifest: mockManifest(), source: "bundled", offline: true, message: "Using the bundled list." };
  }
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
    return mockInstalled();
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

export async function runInstall(
  nextPlan: InstallPlan,
  replaceCollisions = false,
): Promise<string> {
  if (!isTauri()) return "mock-run";
  return invoke<string>("install", { plan: nextPlan, replaceCollisions });
}

export async function cancelInstall(): Promise<void> {
  if (!isTauri()) return;
  await invoke("cancel");
}

export async function repairInstall(): Promise<string> {
  if (!isTauri()) return "mock-repair";
  return invoke<string>("repair");
}

export async function listenProgress(onEvent: (event: ProgressEvent) => void): Promise<() => void> {
  if (!isTauri()) return () => undefined;
  const { listen } = await import("@tauri-apps/api/event");
  const unlisten = await listen<ProgressEvent>("install://progress", (event) => {
    onEvent(event.payload);
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
    return "Arnold Client Mods diagnostics\napp: 3.0.0\n";
  }
  return invoke<string>("copy_diagnostics");
}

export const LAUNCHER_NOTE =
  "Open Minecraft and choose the 'Arnold – Fabric 26.3' profile.";

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

export async function getPrefs(): Promise<Prefs> {
  if (!isTauri()) return { lastGameDir: null, tourDone: false };
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

export function asErrorPayload(error: unknown): ErrorPayload {
  if (error && typeof error === "object" && "code" in error && "title" in error) {
    return error as ErrorPayload;
  }
  if (error && typeof error === "object" && "message" in error) {
    const message = String((error as { message: unknown }).message);
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
