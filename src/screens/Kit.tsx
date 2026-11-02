import { KitCard } from "../components/KitCard";
import { QualityPicker } from "../components/QualityPicker";
import { kitBlurb, kitSizeLabel } from "../mock/backend";
import type { InstallPlan, QualityLevel, QualityTable, Recommendation } from "../types";

export function Kit({
  selected,
  quality,
  ocean,
  recommendation,
  plan,
  table,
  onSelect,
  onQuality,
  onOcean,
}: {
  selected: string;
  quality: QualityLevel;
  ocean: boolean;
  recommendation: Recommendation;
  plan: InstallPlan;
  table?: QualityTable;
  onSelect: (kit: string) => void;
  onQuality: (level: QualityLevel) => void;
  onOcean: (on: boolean) => void;
}) {
  return (
    <div className="flex flex-1 flex-col">
      <h1 className="mt-4 font-heading text-4xl text-gold md:text-5xl">Choose your kit</h1>
      <div className="mt-6 grid gap-4 md:grid-cols-4" role="radiogroup" aria-label="Kit">
        <KitCard
          title="Recommended"
          body={kitBlurb("recommended", recommendation)}
          meta={`${kitSizeLabel("full", ocean)} · shaders on`}
          selected={selected === "recommended"}
          badge="FOR THIS PC"
          onSelect={() => onSelect("recommended")}
        />
        <KitCard
          title="Basic"
          body={kitBlurb("basic", recommendation)}
          meta={kitSizeLabel("basic", ocean)}
          selected={selected === "basic"}
          onSelect={() => onSelect("basic")}
        />
        <KitCard
          title="Full"
          body={kitBlurb("full", recommendation)}
          meta={kitSizeLabel("full", ocean)}
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
      <label className="panel mt-5 flex items-center gap-4">
        <span
          className={`relative h-7 w-12 rounded-full ${ocean ? "bg-primary" : "bg-[#3a3548]"}`}
        >
          <input
            type="checkbox"
            className="peer sr-only"
            checked={ocean}
            onChange={(event) => onOcean(event.target.checked)}
          />
          <span
            className={`absolute top-1 h-5 w-5 rounded-full bg-white transition-all ${
              ocean ? "left-6" : "left-1"
            }`}
          />
        </span>
        <span>
          <span className="font-heading text-lg">Ocean pack</span>
          <span className="ml-3 text-muted">
            Particle Rain · Wakes · Realistic Waves (coming soon)
          </span>
        </span>
      </label>
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
