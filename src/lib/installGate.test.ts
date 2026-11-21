import { describe, expect, it } from "vitest";
import type { Detection, InstallPlan } from "../types";
import { minecraftBlocksInstall, planMatchesDestination } from "./installGate";

function detection(overrides: Partial<Detection> = {}): Detection {
  return {
    candidates: [],
    minecraftVersion: "26.3",
    minecraftVersionInstalled: true,
    fabric: { installed: true, willInstall: false },
    gameRunning: false,
    importedFromLegacy: false,
    packId: "better-adventures-plus",
    packDir: "/packs/better-adventures-plus",
    requireVanilla: true,
    launchers: {
      official: "windowsStore",
      officialRoot: "/official",
      tlauncher: null,
    },
    ...overrides,
  };
}

function plan(overrides: Partial<InstallPlan> = {}): InstallPlan {
  return {
    add: [],
    update: [],
    remove: [],
    configs: [],
    loader: null,
    quality: "normal",
    kit: "recommended",
    components: [],
    warnings: [],
    bytes: 0,
    packId: "better-adventures-plus",
    packDir: "/packs/better-adventures-plus",
    ...overrides,
  };
}

describe("plan binding", () => {
  it("rejects a plan built for another folder or pack", () => {
    const here = detection();
    expect(planMatchesDestination(plan(), here, here.packId)).toBe(true);
    expect(
      planMatchesDestination(plan({ packDir: "/old/install" }), detection({ packDir: "/fresh" }), here.packId),
    ).toBe(false);
    expect(
      planMatchesDestination(plan({ packId: "other-pack" }), here, "better-adventures-plus"),
    ).toBe(false);
    expect(planMatchesDestination(plan({ packId: "", packDir: "" }), here, here.packId)).toBe(false);
    expect(
      planMatchesDestination(
        plan(),
        detection({ needsFolderChoice: { root: "/legacy" } }),
        here.packId,
      ),
    ).toBe(false);
  });
});

describe("vanilla prerequisite", () => {
  it("uses the selected launcher and ignores a disabled one", () => {
    expect(minecraftBlocksInstall(detection({ minecraftVersionInstalled: true }))).toBe(false);
    expect(
      minecraftBlocksInstall(
        detection({
          minecraftVersionInstalled: false,
          launchers: {
            official: null,
            officialRoot: "/official",
            tlauncher: { gameDir: "/custom", verified: false, openTarget: "/apps/TLauncher" },
          },
        }),
      ),
    ).toBe(true);
    expect(
      minecraftBlocksInstall(
        detection({
          minecraftVersionInstalled: true,
          requireVanilla: true,
          launchers: {
            official: null,
            officialRoot: "/official",
            tlauncher: { gameDir: "/custom", verified: false, openTarget: "/apps/TLauncher" },
          },
        }),
      ),
    ).toBe(false);
    expect(
      minecraftBlocksInstall(
        detection({
          minecraftVersionInstalled: false,
          requireVanilla: false,
        }),
      ),
    ).toBe(false);
  });
});
