import type { Detection, InstallPlan } from "../types";
import * as api from "./api";
import { changeDestination } from "./destination";
import { planMatchesDestination } from "./installGate";

export interface DestinationDeps {
  invalidate: () => void;
  detect: () => Promise<Detection>;
  replan: () => Promise<InstallPlan | null>;
}

export function createDestinationFlow(deps: DestinationDeps) {
  return {
    selectPack: async (id: string) => {
      deps.invalidate();
      const result = await api.selectPack(id);
      const detection = await deps.detect();
      return { result, detection };
    },
    useDefaultFolder: (enabled: boolean) =>
      changeDestination(() => api.setUseDefaultFolder(enabled), deps),
    chooseFolder: (choice: "keep" | "fresh") =>
      changeDestination(() => api.choosePackFolder(choice), deps),
    setGameDir: (path: string) => changeDestination(() => api.setGameDir(path), deps),
    install: async (
      plan: InstallPlan,
      detection: Detection | null,
      selectedPack: string | null | undefined,
      replace: boolean,
    ): Promise<string | null> =>
      planMatchesDestination(plan, detection, selectedPack) ? api.runInstall(plan, replace) : null,
  };
}
