import type { Detection, InstallPlan } from "../types";

export function planMatchesDestination(
  plan: InstallPlan | null | undefined,
  detection: Pick<Detection, "packId" | "packDir" | "needsFolderChoice"> | null,
  selectedPack?: string | null,
): boolean {
  if (!plan || !detection) return false;
  if (detection.needsFolderChoice) return false;
  if (!plan.packId || !plan.packDir || !detection.packDir) return false;
  const packId = selectedPack || detection.packId;
  if (!packId || plan.packId !== packId) return false;
  return plan.packDir === detection.packDir;
}

export function minecraftBlocksInstall(detection: Detection): boolean {
  const launcherFound = Boolean(detection.launchers?.official || detection.launchers?.tlauncher);
  if (!launcherFound) return false;
  if (detection.requireVanilla === false) return false;
  return !detection.minecraftVersionInstalled;
}
