import { describe, expect, it } from "vitest";
import type { Detection, InstallPlan } from "../types";
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
});
