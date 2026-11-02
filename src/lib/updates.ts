import { versionLess } from "./format";
import type { InstalledState, Manifest, UpdateBadge } from "../types";

export function updateBadges(manifest: Manifest, installed: InstalledState | null): UpdateBadge[] {
  if (!installed) return [];
  const badges: UpdateBadge[] = [];
  for (const have of installed.components) {
    const want = manifest.components.find((component) => component.id === have.id);
    if (!want) continue;
    if (have.version !== want.version && versionLess(have.version, want.version)) {
      badges.push({
        id: have.id,
        name: want.name,
        installed: have.version,
        available: want.version,
      });
    }
  }
  return badges;
}
