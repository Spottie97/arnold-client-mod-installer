import { APP_VERSION } from "./version";
import type { InstalledState, PackEntry, PackInstallState } from "../types";

function versionGreater(left: string, right: string): boolean {
  const parts = (value: string) => value.split(".").map((part) => Number.parseInt(part, 10) || 0);
  const a = parts(left);
  const b = parts(right);
  const length = Math.max(a.length, b.length);
  for (let index = 0; index < length; index += 1) {
    const diff = (a[index] ?? 0) - (b[index] ?? 0);
    if (diff > 0) return true;
    if (diff < 0) return false;
  }
  return false;
}

export function packStatus(pack: PackEntry, installed: InstalledState | null): string {
  if (pack.minAppVersion && versionGreater(pack.minAppVersion, APP_VERSION)) {
    return "NEEDS A NEWER INSTALLER";
  }
  if (!installed) return "NOT INSTALLED";
  if (pack.displayVersion && installed.manifestVersion !== pack.displayVersion) {
    return "UPDATE AVAILABLE";
  }
  return "INSTALLED";
}

export function indexPackStates(
  states: PackInstallState[],
): Record<string, InstalledState | null> {
  const out: Record<string, InstalledState | null> = {};
  for (const row of states) out[row.packId] = row.installed;
  return out;
}

export function statusForPack(
  pack: PackEntry,
  installedByPack: Record<string, InstalledState | null | undefined>,
): string {
  return packStatus(pack, installedByPack[pack.id] ?? null);
}
