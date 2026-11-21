import { describe, expect, it } from "vitest";
import fixture from "@fixtures/manifest.v3.json";
import { parseQuality, qualityRow, fallbackQualityTable } from "./quality";
import type { Manifest } from "../types";

describe("quality", () => {
  it("accepts low and high aliases", () => {
    expect(parseQuality("low")).toBe("lite");
    expect(parseQuality("high")).toBe("ultra");
    expect(parseQuality("potato")).toBe("potato");
  });

  it("ships no snowfall component and no kit includes snow", () => {
    const manifest = fixture as unknown as Manifest;
    expect(manifest.components.map((component) => component.id)).not.toContain("snow");
    expect(
      manifest.components.some((component) =>
        component.files.some((file) => file.file.includes("snowfall")),
      ),
    ).toBe(false);
    for (const kit of Object.values(manifest.kits)) {
      expect(kit.components).not.toContain("snow");
    }
    expect(manifest.components.some((component) => component.id === "bettersnowcoverage")).toBe(
      true,
    );
    expect(manifest.components.some((component) => component.id === "weatherglass")).toBe(true);
    for (const level of ["potato", "lite", "normal", "ultra"] as const) {
      expect(manifest.qualityPresets?.[level]).not.toHaveProperty("snow");
    }
  });

  it("keeps ship-heavy ocean off potato", () => {
    const row = qualityRow(fallbackQualityTable(), "potato");
    expect(row.skipComponents).toContain("wakes");
    expect(row.skipComponents).toContain("waves");
    expect(row.description.length).toBeGreaterThan(20);
  });
});
