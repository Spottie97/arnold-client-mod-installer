import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import fixture from "@fixtures/manifest.v3.json";
import { Kit } from "../screens/Kit";
import { AddonToggles } from "../components/AddonToggles";
import { addonRows, freshAddonFlags, savedAddonFlags } from "./addons";
import { mockPlan, mockRecommendation } from "../mock/backend";
import type { Manifest, Selection } from "../types";

const manifest = fixture as unknown as Manifest;

function selection(overrides: Partial<Selection> = {}): Selection {
  return {
    kit: "full",
    customComponents: [],
    ocean: true,
    snowLite: false,
    quality: "normal",
    ...overrides,
  };
}

describe("The Architect addon stays off", () => {
  it("is default-off for every kit and absent from kits and presets", () => {
    const architect = manifest.addons?.architect;
    expect(architect?.name).toBe("The Architect (AI build helper)");
    expect(architect?.components).toEqual(["architect", "litematica", "malilib"]);
    expect(manifest.addons?.sao).toBeUndefined();
    for (const kit of Object.keys(manifest.kits)) {
      expect(architect?.defaultOn?.[kit]).toBe(false);
      expect(manifest.kits[kit].components).not.toContain("architect");
      expect(manifest.kits[kit].components).not.toContain("litematica");
      expect(manifest.kits[kit].components).not.toContain("malilib");
      expect(manifest.kits[kit].components).not.toContain("saoaincrad");
      expect(manifest.kits[kit].components).not.toContain("geckolib");
    }
    for (const level of ["potato", "lite", "normal", "ultra"] as const) {
      const row = manifest.qualityPresets?.[level];
      expect(row?.skipComponents ?? []).not.toContain("architect");
      expect(row?.recommendOcean).not.toBeUndefined();
    }
  });

  it("does not apply defaultOn onto a saved selection that omits architect", () => {
    const flipped = structuredClone(manifest);
    flipped.addons!.architect!.defaultOn = { basic: true, full: true, recommended: true };
    expect(freshAddonFlags(flipped, {}).architect).toBe(false);
    expect(freshAddonFlags(flipped, null).architect).toBe(true);
    expect(freshAddonFlags(manifest, null).architect).toBe(false);
    const installed = savedAddonFlags(manifest, ["base", "sodium", "deck", "ships", "snow"]);
    expect(freshAddonFlags(manifest, installed).architect).toBe(false);
    const optedIn = savedAddonFlags(manifest, ["base", "architect", "litematica", "malilib"]);
    expect(freshAddonFlags(manifest, optedIn).architect).toBe(true);
    expect(savedAddonFlags(manifest, ["litematica", "malilib"])?.architect).toBe(false);
  });

  it("leaves architect out of a normal plan and adds it only when ticked", () => {
    const off = mockPlan(selection());
    expect(off.components).toContain("particlerain");
    expect(off.components).not.toContain("architect");
    expect(off.components).not.toContain("litematica");
    expect(off.components).not.toContain("malilib");
    const on = mockPlan(selection({ addons: { architect: true } }));
    expect(on.components).toContain("architect");
    expect(on.components).toContain("litematica");
    expect(on.components).toContain("malilib");
    expect(on.components).toContain("base");
    expect(on.components.indexOf("malilib")).toBeLessThan(on.components.indexOf("litematica"));
    expect(on.components.indexOf("litematica")).toBeLessThan(on.components.indexOf("architect"));
  });
});

describe("addon toggles", () => {
  it("renders Ocean and The Architect, with Architect unchecked", () => {
    const rows = addonRows(manifest, true, { architect: false });
    expect(rows.map((row) => row.id)).toEqual(["ocean", "architect"]);
    expect(rows[0]?.checked).toBe(true);
    expect(rows[1]?.checked).toBe(false);
    expect(rows[0]?.detail).toContain("Realistic Waves (coming soon)");
    expect(rows[1]?.detail).toBe("The Architect · Litematica · MaLiLib");
    expect(rows[1]?.note).toContain("own Anthropic API key");
    expect(rows[1]?.note).toContain("press B");

    const html = renderToStaticMarkup(
      createElement(AddonToggles, { addons: rows, onToggle: () => undefined }),
    );
    expect(html).toContain('aria-label="Optional add-ons"');
    expect(html).toContain('aria-label="Ocean pack"');
    expect(html).toContain('aria-label="The Architect (AI build helper)"');
    expect(html).toContain("Particle Rain");
    expect(html).toContain("Litematica");
    expect(html).toContain("MaLiLib");
    const inputTag = (label: string) =>
      html.match(new RegExp(`<input\\b[^>]*aria-label="${label}"[^>]*>`))?.[0] ?? "";
    const oceanInput = inputTag("Ocean pack");
    const architectInput = inputTag("The Architect \\(AI build helper\\)");
    expect(oceanInput).toContain("checked");
    expect(architectInput).not.toContain("checked");
    expect(html).toContain("bg-primary");
    expect(html).toContain("bg-[#3a3548]");

    const kit = renderToStaticMarkup(
      createElement(Kit, {
        selected: "recommended",
        quality: "ultra",
        ocean: true,
        addons: rows,
        recommendation: mockRecommendation(),
        plan: mockPlan(selection()),
        table: manifest.qualityPresets,
        onSelect: () => undefined,
        onQuality: () => undefined,
        onAddon: () => undefined,
      }),
    );
    expect(kit.match(/aria-label="Ocean pack"/g)).toHaveLength(1);
    expect(kit.match(/aria-label="The Architect \(AI build helper\)"/g)).toHaveLength(1);
    expect(kit.indexOf("Ocean pack")).toBeLessThan(kit.indexOf("The Architect (AI build helper)"));
  });
});
