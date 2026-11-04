import type { AddonRow } from "../lib/addons";

export function AddonToggles({
  addons,
  onToggle,
}: {
  addons: AddonRow[];
  onToggle: (id: string, on: boolean) => void;
}) {
  if (addons.length === 0) return null;
  return (
    <div className="mt-5 grid gap-3 md:grid-cols-2" role="group" aria-label="Optional add-ons">
      {addons.map((addon) => (
        <label key={addon.id} className="panel flex items-start gap-4">
          <span
            className={`relative mt-1 h-7 w-12 shrink-0 rounded-full ${
              addon.checked ? "bg-primary" : "bg-[#3a3548]"
            }`}
          >
            <input
              type="checkbox"
              className="peer sr-only"
              checked={addon.checked}
              aria-label={addon.name}
              onChange={(event) => onToggle(addon.id, event.target.checked)}
            />
            <span
              className={`absolute top-1 h-5 w-5 rounded-full bg-white transition-all ${
                addon.checked ? "left-6" : "left-1"
              }`}
            />
          </span>
          <span className="min-w-0">
            <span className="font-heading text-lg">{addon.name}</span>
            <span className="mt-1 block text-sm text-muted">{addon.detail}</span>
            {addon.note ? <span className="mt-1 block text-sm text-gold">{addon.note}</span> : null}
          </span>
        </label>
      ))}
    </div>
  );
}
