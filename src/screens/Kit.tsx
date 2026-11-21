import { AddonToggles } from "../components/AddonToggles";
import { KitCard } from "../components/KitCard";
import { QualityPicker } from "../components/QualityPicker";
import type { AddonRow } from "../lib/addons";
import { qualityLabel } from "../lib/quality";
import type { InstallPlan, QualityLevel, QualityTable, Recommendation } from "../types";

export function Kit({
  selected,
  quality,
  addons,
  recommendation,
  plan,
  table,
  onSelect,
  onQuality,
  onAddon,
  onOptions,
}: {
  selected: string;
  quality: QualityLevel;
  addons: AddonRow[];
  recommendation: Recommendation;
  plan: InstallPlan;
  table?: QualityTable;
  onSelect: (kit: string) => void;
  onQuality: (level: QualityLevel) => void;
  onAddon: (id: string, on: boolean) => void;
  onOptions?: () => void;
}) {
  const label = qualityLabel(quality);
  return (
    <div className="flex flex-1 flex-col">
      <h2 className="mt-2 font-heading text-4xl text-gold">Choose your kit</h2>
      <p className="mt-2 max-w-3xl text-base text-muted">
        Kit chooses the mods. Quality is already set to {label} from the scan — you can change it
        later in Options. The lightest kit is Basic. Lite is a quality level only.
      </p>
      <div className="mt-6 grid gap-4 md:grid-cols-3" role="radiogroup" aria-label="Kit">
        <KitCard
          title="Basic"
          body="The lighter set. Server-required mods and performance, sized to run on almost any PC."
          meta="Fabric API, Sodium, Arnold Deck · Lighter snow · smaller download"
          selected={selected === "basic"}
          onSelect={() => onSelect("basic")}
        />
        <KitCard
          title="Recommended"
          body="The usual Better Adventures+ list for this machine, including resource packs."
          meta="Everything in Basic, plus the full client list · Flashback 0.43.6 · Iris + Complementary"
          selected={selected === "recommended"}
          badge="FOR THIS PACK"
          onSelect={() => onSelect("recommended")}
        />
        <KitCard
          title="Full"
          body="The complete list: every client mod and resource pack in this pack."
          meta="Same mods as Recommended · Flashback 0.43.6 · All resource packs"
          selected={selected === "full"}
          onSelect={() => onSelect("full")}
        />
      </div>
      <button type="button" className="mt-4 self-start text-sm text-primary" onClick={onOptions ?? (() => undefined)}>
        Change options
      </button>
      <AddonToggles addons={addons} onToggle={onAddon} />
      <div className="mt-6">
        <QualityPicker
          selected={quality}
          recommended={recommendation.quality}
          table={table}
          onSelect={onQuality}
        />
      </div>
      {plan.warnings.length > 0 ? (
        <p className="mt-4 rounded-xl border border-gold/40 bg-panel px-4 py-3 text-sm text-gold">
          {plan.warnings[0]}
        </p>
      ) : null}
    </div>
  );
}
