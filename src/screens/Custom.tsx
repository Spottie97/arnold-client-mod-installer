import { AddonToggles } from "../components/AddonToggles";
import type { AddonRow } from "../lib/addons";
import { qualityLabel } from "../lib/quality";
import type { Component, InstallPlan, Manifest, QualityLevel } from "../types";

const GROUPS: { id: Component["group"]; title: string }[] = [
  { id: "base", title: "Mods" },
  { id: "arnold", title: "Arnold" },
  { id: "shaders", title: "Shaders & add-ons" },
  { id: "ocean", title: "Ocean" },
];

export function Custom({
  manifest,
  selected,
  snowLite,
  plan,
  quality,
  addons,
  onToggle,
  onAddon,
  onSnowLite,
}: {
  manifest: Manifest;
  selected: string[];
  snowLite: boolean;
  plan: InstallPlan;
  quality: QualityLevel;
  addons: AddonRow[];
  onToggle: (id: string) => void;
  onAddon: (id: string, on: boolean) => void;
  onSnowLite: (on: boolean) => void;
}) {
  return (
    <div className="flex flex-1 flex-col">
      <h2 className="mt-2 font-heading text-4xl text-gold">Options</h2>
      <p className="mt-2 text-base text-muted">
        {plan.kit === "basic" ? "Basic" : plan.kit === "full" ? "Full" : "Recommended"} kit ·{" "}
        {qualityLabel(quality)} quality. Locked items are required to join, or required for this
        kit, and cannot be turned off.
      </p>
      <div className="mt-6 space-y-5">
        {GROUPS.map((group) => (
          <section key={group.id} className="panel">
            <h2 className="font-heading text-xl text-gold">{group.title}</h2>
            <div className="mt-3 space-y-2">
              {manifest.components
                .filter((component) => component.group === group.id)
                .map((component) => {
                  const locked = Boolean(component.requiredInAllKits);
                  const pending = component.kind === "pending";
                  const checked = selected.includes(component.id) || locked;
                  return (
                    <label
                      key={component.id}
                      className={`flex items-start gap-3 rounded-xl px-2 py-2 ${
                        pending ? "opacity-50" : ""
                      }`}
                    >
                      <input
                        type="checkbox"
                        className="mt-1"
                        checked={checked}
                        disabled={locked || pending}
                        onChange={() => onToggle(component.id)}
                      />
                      <span>
                        <span className="font-heading text-lg">
                          {component.name}{" "}
                          <span className="text-sm text-muted">{component.version}</span>
                        </span>
                        <span className="block text-sm text-muted">{component.description}</span>
                        {locked ? (
                          <span className="block text-sm text-gold">
                            {component.id === "deck"
                              ? "Needed for smooth ship decks on the Arnold server"
                              : "Needed to join the Arnold server"}
                          </span>
                        ) : null}
                      </span>
                    </label>
                  );
                })}
            </div>
          </section>
        ))}
      </div>
      <AddonToggles addons={addons} onToggle={onAddon} />
      <label className="panel mt-5 flex items-center gap-3">
        <input
          type="checkbox"
          checked={snowLite}
          onChange={(event) => onSnowLite(event.target.checked)}
        />
        <span className="font-heading">Lighter snow</span>
      </label>
      <p className="mt-4 text-sm text-muted">
        Plan: add {plan.add.length}, update {plan.update.length}, remove {plan.remove.length}.
      </p>
    </div>
  );
}
