import { describe, expect, it } from "vitest";
import { updateBadges } from "./updates";
import type { InstalledState, Manifest } from "../types";
import { TOUR_TIPS } from "./tour";

const manifest = {
  components: [
    { id: "ships", name: "Arnold Ships", version: "0.10.2", files: [] },
    { id: "snow", name: "Arnold Snow", version: "1.3.3", files: [] },
  ],
} as unknown as Manifest;

describe("update badges", () => {
  it("flags a newer ships build", () => {
    const installed = {
      components: [
        { id: "ships", name: "Arnold Ships", version: "0.10.1" },
        { id: "snow", name: "Arnold Snow", version: "1.3.3" },
      ],
    } as InstalledState;
    const badges = updateBadges(manifest, installed);
    expect(badges).toEqual([
      { id: "ships", name: "Arnold Ships", installed: "0.10.1", available: "0.10.2" },
    ]);
  });

  it("is empty when current", () => {
    const installed = {
      components: [{ id: "snow", name: "Arnold Snow", version: "1.3.3" }],
    } as InstalledState;
    expect(updateBadges(manifest, installed)).toEqual([]);
  });
});

describe("tour", () => {
  it("has three tips", () => {
    expect(TOUR_TIPS).toHaveLength(3);
    expect(TOUR_TIPS[2].title).toContain("Fabric 26.3");
  });
});
