import type { Detection, InstallPlan } from "../types";

export async function changeDestination<T>(
  change: () => Promise<T>,
  deps: {
    invalidate: () => void;
    detect: () => Promise<Detection>;
    replan: () => Promise<InstallPlan | null>;
  },
): Promise<{ result: T; detection: Detection; plan: InstallPlan | null }> {
  deps.invalidate();
  const result = await change();
  const detection = await deps.detect();
  const plan = await deps.replan();
  return { result, detection, plan };
}
