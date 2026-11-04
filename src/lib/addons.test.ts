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

describe("SAO addon stays off", () => {
  it("is default-off for every kit and absent from kits and presets", () => {
    const sao = manifest.addons?.sao;
    expect(sao?.name).toBe("SAO: Aincrad (staging test)");
    expect(sao?.components).toEqual(["sao_aincrad", "geckolib"]);
    for (const kit of Object.keys(manifest.kits)) {
      expect(sao?.defaultOn?.[kit]).toBe(false);
      expect(manifest.kits[kit].components).not.toContain("sao_aincrad");
      expect(manifest.kits[kit].components).not.toContain("geckolib");
    }
    for (const level of ["potato", "lite", "normal", "ultra"] as const) {
      const row = manifest.qualityPresets?.[level];
      expect(row?.skipComponents ?? []).not.toContain("sao_aincrad");
      expect(row?.recommendOcean).not.toBeUndefined();
    }
  });

  it("does not apply defaultOn onto a saved selection that omits sao", () => {
    const flipped = structuredClone(manifest);
    flipped.addons!.sao!.defaultOn = { basic: true, full: true, recommended: true };
    expect(freshAddonFlags(flipped, {}).sao).toBe(false);
    expect(freshAddonFlags(flipped, null).sao).toBe(true);
    expect(freshAddonFlags(manifest, null).sao).toBe(false);
    const installed = savedAddonFlags(manifest, ["base", "sodium", "deck", "ships", "snow"]);
    expect(freshAddonFlags(manifest, installed).sao).toBe(false);
    const optedIn = savedAddonFlags(manifest, ["base", "sao_aincrad", "geckolib"]);
    expect(freshAddonFlags(manifest, optedIn).sao).toBe(true);
    expect(savedAddonFlags(manifest, ["geckolib"])?.sao).toBe(false);
  });

  it("leaves sao out of a normal plan and adds it only when ticked", () => {
    const off = mockPlan(selection());
    expect(off.components).toContain("particlerain");
    expect(off.components).not.toContain("sao_aincrad");
    expect(off.components).not.toContain("geckolib");
    const on = mockPlan(selection({ addons: { sao: true } }));
    expect(on.components).toContain("sao_aincrad");
    expect(on.components).toContain("geckolib");
    expect(on.components).toContain("base");
    expect(on.components.indexOf("geckolib")).toBeLessThan(on.components.indexOf("sao_aincrad"));
  });
});

describe("addon toggles", () => {
  it("renders Ocean and the SAO staging test, with SAO unchecked", () => {
    const rows = addonRows(manifest, true, { sao: false });
    expect(rows.map((row) => row.id)).toEqual(["ocean", "sao"]);
    expect(rows[0]?.checked).toBe(true);
    expect(rows[1]?.checked).toBe(false);
    expect(rows[0]?.detail).toContain("Realistic Waves (coming soon)");
    expect(rows[1]?.detail).toBe("GeckoLib");
    expect(rows[1]?.note).toContain("Staging test only");
    expect(rows[1]?.note).toContain("Java 25");

    const html = renderToStaticMarkup(
      createElement(AddonToggles, { addons: rows, onToggle: () => undefined }),
    );
    expect(html).toContain('aria-label="Optional add-ons"');
    expect(html).toContain('aria-label="Ocean pack"');
    expect(html).toContain('aria-label="SAO: Aincrad (staging test)"');
    expect(html).toContain("Particle Rain");
    expect(html).toContain("GeckoLib");
    expect(html).toContain("Staging test only");
    const inputTag = (label: string) =>
      html.match(new RegExp(`<input\\b[^>]*aria-label="${label}"[^>]*>`))?.[0] ?? "";
    const oceanInput = inputTag("Ocean pack");
    const saoInput = inputTag("SAO: Aincrad \\(staging test\\)");
    expect(oceanInput).toContain("checked");
    expect(saoInput).not.toContain("checked");
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
    expect(kit.match(/aria-label="SAO: Aincrad \(staging test\)"/g)).toHaveLength(1);
    expect(kit.indexOf("Ocean pack")).toBeLessThan(kit.indexOf("SAO: Aincrad (staging test)"));
  });
});
