import { describe, expect, it } from "vitest";
import type { Detection, InstallPlan, Prefs } from "../types";
import { planMatchesDestination } from "./installGate";
import { changeDestination } from "./destination";

const launcherRoot = "/home/sam/.minecraft";

function detection(packDir: string): Pick<Detection, "packId" | "packDir" | "needsFolderChoice"> {
  return { packId: "pack-x", packDir, needsFolderChoice: null };
}

function plan(packDir: string): InstallPlan {
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

describe("changeDestination", () => {
  it("changes destination before detecting and replanning", async () => {
    const order: string[] = [];
    const nextPlan = plan(launcherRoot);
    const outcome = await changeDestination(
      async () => {
        order.push("change");
        return "ok";
      },
      {
        invalidate: () => order.push("invalidate"),
        detect: async () => {
          order.push("detect");
          return detection(launcherRoot) as Detection;
        },
        replan: async () => {
          order.push("replan");
          return nextPlan;
        },
      },
    );
    expect(order).toEqual(["invalidate", "change", "detect", "replan"]);
    expect(outcome.plan).toBe(nextPlan);
    expect(outcome.result).toBe("ok");
  });

  it("does not replan when the destination change fails", async () => {
    const order: string[] = [];
    const payload = {
      code: "E-FOLDER-OWNER",
      title: "We can't tell which pack this Minecraft folder belongs to",
      hint: "Nothing was moved or changed.",
      detail: "state.json can't be read",
    };
    await expect(
      changeDestination(
        async () => {
          order.push("change");
          throw payload;
        },
        {
          invalidate: () => order.push("invalidate"),
          detect: async () => {
            order.push("detect");
            return detection(launcherRoot) as Detection;
          },
          replan: async () => {
            order.push("replan");
            return plan(launcherRoot);
          },
        },
      ),
    ).rejects.toBe(payload);
    expect(order).toEqual(["invalidate", "change"]);
  });

  it("selection then default-folder toggle then install uses the new plan", async () => {
    const calls: string[] = [];
    const stale = plan("/own/pack-x");
    const fresh = plan(launcherRoot);
    const seen = detection(launcherRoot);

    async function selectPack(id: string): Promise<Prefs> {
      calls.push(`selectPack:${id}`);
      return { tourDone: true, selectedPack: id, useDefaultFolder: false };
    }
    async function setUseDefaultFolder(enabled: boolean): Promise<Prefs> {
      calls.push(`setUseDefaultFolder:${enabled}`);
      return { tourDone: true, selectedPack: "pack-x", useDefaultFolder: enabled };
    }

    await selectPack("pack-x");
    const { plan: next, detection: nextDetection } = await changeDestination(
      () => setUseDefaultFolder(true),
      {
        invalidate: () => calls.push("invalidate"),
        detect: async () => {
          calls.push("detect");
          return seen as Detection;
        },
        replan: async () => {
          calls.push("replan");
          return fresh;
        },
      },
    );
    expect(calls).toEqual([
      "selectPack:pack-x",
      "invalidate",
      "setUseDefaultFolder:true",
      "detect",
      "replan",
    ]);
    expect(next?.packId).toBe("pack-x");
    expect(next?.packDir).toBe(launcherRoot);
    expect(planMatchesDestination(next, nextDetection, "pack-x")).toBe(true);
    expect(planMatchesDestination(stale, nextDetection, "pack-x")).toBe(false);
  });

  it("selection then keep choice then install uses the new plan", async () => {
    const calls: string[] = [];
    const stale = plan("/own/pack-x");
    const fresh = plan(launcherRoot);
    const seen = detection(launcherRoot);

    async function selectPack(id: string): Promise<Prefs> {
      calls.push(`selectPack:${id}`);
      return { tourDone: true, selectedPack: id };
    }
    async function choosePackFolder(choice: "keep" | "fresh"): Promise<Prefs> {
      calls.push(`choosePackFolder:${choice}`);
      return {
        tourDone: true,
        selectedPack: "pack-x",
        packLocations: { "pack-x": choice === "keep" ? "defaultFolder" : "ownFolder" },
      };
    }

    await selectPack("pack-x");
    const { plan: next, detection: nextDetection } = await changeDestination(
      () => choosePackFolder("keep"),
      {
        invalidate: () => calls.push("invalidate"),
        detect: async () => {
          calls.push("detect");
          return seen as Detection;
        },
        replan: async () => {
          calls.push("replan");
          return fresh;
        },
      },
    );
    expect(calls).toEqual([
      "selectPack:pack-x",
      "invalidate",
      "choosePackFolder:keep",
      "detect",
      "replan",
    ]);
    expect(next?.packId).toBe("pack-x");
    expect(next?.packDir).toBe(launcherRoot);
    expect(planMatchesDestination(next, nextDetection, "pack-x")).toBe(true);
    expect(planMatchesDestination(stale, nextDetection, "pack-x")).toBe(false);
  });
});
