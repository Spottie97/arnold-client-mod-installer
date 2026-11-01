import { QUALITY_ORDER, qualityRow } from "../lib/quality";
import type { QualityLevel, QualityTable } from "../types";

export function QualityPicker({
  selected,
  recommended,
  table,
  onSelect,
}: {
  selected: QualityLevel;
  recommended?: QualityLevel;
  table?: QualityTable;
  onSelect: (level: QualityLevel) => void;
}) {
  const source = table;
  return (
    <div>
      <h2 className="font-heading text-xl text-gold">Quality</h2>
      <p className="mt-1 text-sm text-muted">
        Kit chooses the mods. Quality chooses how hard they run.
      </p>
      <div className="mt-3 grid gap-3 sm:grid-cols-2 lg:grid-cols-4" role="radiogroup" aria-label="Quality">
        {QUALITY_ORDER.map((level) => {
          const row = qualityRow(source, level);
          const active = selected === level;
          return (
            <button
              key={level}
              type="button"
              role="radio"
              aria-checked={active}
              onClick={() => onSelect(level)}
              className={`flex min-h-[140px] flex-col rounded-2xl bg-panel p-4 text-left ${
                active ? "shadow-selected" : "hover:shadow-card"
              }`}
            >
              <div className="mb-2 min-h-6">
                {recommended === level ? (
                  <span className="font-heading text-sm text-gold">FOR THIS PC</span>
                ) : (
                  <span className="text-muted">·</span>
                )}
              </div>
              <h3 className="font-heading text-xl text-text">{row.label}</h3>
              <p className="mt-2 flex-1 text-sm leading-relaxed text-muted">{row.description}</p>
            </button>
          );
        })}
      </div>
    </div>
  );
}
