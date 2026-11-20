import { describe, expect, it } from "vitest";
import fixture from "@fixtures/manifest.v3.json";
import type { Manifest } from "../types";

const manifest = fixture as unknown as Manifest;

const PACKS = [
  "freshanimations",
  "mfrp",
  "matchablockoverlays",
  "bushypetals",
  "rainbowsfoliage",
  "grassrise",
];

describe("BA+ resource packs", () => {
  it("lists six Modrinth packs for Full and Recommended", () => {
    const packs = manifest.components.filter((component) =>
      component.files.some((file) => file.dest === "resourcepacks"),
    );
    expect(packs.map((component) => component.id)).toEqual(PACKS);
    expect(packs).toHaveLength(6);
    for (const component of packs) {
      expect(component.requiredInAllKits).not.toBe(true);
      for (const file of component.files) {
        expect(file.url.startsWith("https://cdn.modrinth.com/")).toBe(true);
      }
      expect(manifest.kits.full.components).toContain(component.id);
      expect(manifest.kits.recommended.components).toContain(component.id);
      expect(manifest.kits.basic.components).not.toContain(component.id);
    }
  });

  it("needs installer 3.2.0", () => {
    expect(manifest.minAppVersion).toBe("3.2.0");
  });
});
