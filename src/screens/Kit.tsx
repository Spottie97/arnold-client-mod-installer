import { AddonToggles } from "../components/AddonToggles";
import { KitCard } from "../components/KitCard";
import { QualityPicker } from "../components/QualityPicker";
import type { AddonRow } from "../lib/addons";
import { kitBlurb, kitSizeLabel } from "../mock/backend";
import type { InstallPlan, QualityLevel, QualityTable, Recommendation } from "../types";

export function Kit({
  selected,
  quality,
  ocean,
  addons,
  recommendation,
  plan,
  table,
  onSelect,
  onQuality,
  onAddon,
}: {
  selected: string;
  quality: QualityLevel;
  ocean: boolean;
  addons: AddonRow[];
  recommendation: Recommendation;
  plan: InstallPlan;
  table?: QualityTable;
  onSelect: (kit: string) => void;
  onQuality: (level: QualityLevel) => void;
  onAddon: (id: string, on: boolean) => void;
}) {
  const extra = Object.fromEntries(
    addons.filter((addon) => addon.id !== "ocean").map((addon) => [addon.id, addon.checked]),
  );
  return (
    <div className="flex flex-1 flex-col">
      <h1 className="mt-4 font-heading text-4xl text-gold md:text-5xl">Choose your kit</h1>
      <div className="mt-6 grid gap-4 md:grid-cols-4" role="radiogroup" aria-label="Kit">
        <KitCard
          title="Recommended"
          body={kitBlurb("recommended", recommendation)}
          meta={`${kitSizeLabel("full", ocean, extra)} · shaders on`}
          selected={selected === "recommended"}
          badge="FOR THIS PC"
          onSelect={() => onSelect("recommended")}
        />
        <KitCard
          title="Basic"
          body={kitBlurb("basic", recommendation)}
          meta={kitSizeLabel("basic", ocean, extra)}
          selected={selected === "basic"}
          onSelect={() => onSelect("basic")}
        />
        <KitCard
          title="Full"
          body={kitBlurb("full", recommendation)}
          meta={kitSizeLabel("full", ocean, extra)}
          selected={selected === "full"}
          onSelect={() => onSelect("full")}
        />
        <KitCard
          title="Custom"
          body={kitBlurb("custom", recommendation)}
          meta="—"
          selected={selected === "custom"}
          onSelect={() => onSelect("custom")}
        />
      </div>
      <div className="mt-6">
        <QualityPicker
          selected={quality}
          recommended={recommendation.quality}
          table={table}
          onSelect={onQuality}
        />
      </div>
      <AddonToggles addons={addons} onToggle={onAddon} />
      <p className="mt-4 text-sm leading-relaxed text-muted">
        Why: dedicated GPU with {recommendation.hardware.vramGb} GB VRAM,{" "}
        {recommendation.hardware.ramGb} GB RAM, {recommendation.hardware.cores} cores →{" "}
        {recommendation.quality} quality. Sodium: yes, it raises FPS on every PC.
      </p>
      {plan.warnings.length > 0 ? (
        <p className="mt-4 rounded-xl border border-gold/40 bg-panel px-4 py-3 text-sm text-gold">
          {plan.warnings[0]}
        </p>
      ) : null}
    </div>
  );
}
