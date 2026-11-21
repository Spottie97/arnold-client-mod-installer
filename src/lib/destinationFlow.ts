import type { Detection, ImportOffer, InstallPlan, ProgressEvent } from "../types";
import * as api from "./api";
import { changeDestination } from "./destination";
import { planMatchesDestination } from "./installGate";

const BA_PLUS_ID = "better-adventures-plus";

export interface DestinationDeps {
  invalidate: () => void;
  detect: () => Promise<Detection>;
  replan: () => Promise<InstallPlan | null>;
}

export interface InstallHooks {
  /** Called after the progress listener is live and before the install command is sent. */
  onStart: () => void;
  onProgress: (event: ProgressEvent) => void;
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
      hooks: InstallHooks,
    ): Promise<string | null> => {
      if (!planMatchesDestination(plan, detection, selectedPack)) return null;
      const stop = await api.listenProgress(hooks.onProgress);
      try {
        hooks.onStart();
        return await api.runInstall(plan, replace);
      } finally {
        stop();
      }
    },
    /** Import offer for a Better Adventures+ refusal on an occupied folder, else null. Read only. */
    occupiedImportOffer: async (
      err: unknown,
      selectedPack: string | null | undefined,
    ): Promise<ImportOffer | null> => {
      const code = (err as { code?: unknown } | null)?.code;
      if (code !== "E-FOLDER-OCCUPIED" || selectedPack !== BA_PLUS_ID) return null;
      const offer = await api.detectLegacy().catch(() => null);
      return offer && offer.found && !offer.alreadyAdopted && offer.jars.length > 0 ? offer : null;
    },
    /** User pressed "Adopt into this installer": write the record, then retry the refused step. */
    adoptThen: async <T>(retry: () => Promise<T>): Promise<T> => {
      deps.invalidate();
      await api.confirmImport();
      return retry();
    },
  };
}
