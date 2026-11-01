import type {
  Detection,
  InstallPlan,
  InstalledState,
  Manifest,
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

export async function getManifest(): Promise<Manifest> {
  if (!isTauri()) return mockManifest();
  try {
    return await invoke<Manifest>("get_manifest");
  } catch {
    return mockManifest();
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
      minecraftVersionInstalled: false,
    });
  }
  return invoke<Detection>("set_game_dir", { path });
}
