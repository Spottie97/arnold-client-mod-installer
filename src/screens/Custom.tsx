import type { Component, InstallPlan, Manifest } from "../types";

const GROUPS: { id: Component["group"]; title: string }[] = [
  { id: "arnold", title: "Arnold" },
  { id: "shaders", title: "Performance & Shaders" },
  { id: "ocean", title: "Ocean" },
];

export function Custom({
  manifest,
  selected,
  snowLite,
  plan,
  onToggle,
  onSnowLite,
}: {
  manifest: Manifest;
  selected: string[];
  snowLite: boolean;
  plan: InstallPlan;
  onToggle: (id: string) => void;
  onSnowLite: (on: boolean) => void;
}) {
  return (
    <div className="flex flex-1 flex-col">
      <h1 className="mt-4 font-heading text-4xl text-gold">Custom picker</h1>
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
                            Needed for smooth ship decks on the Arnold server
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
