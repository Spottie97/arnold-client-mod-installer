export function KitCard({
  title,
  body,
  meta,
  selected,
  badge,
  onSelect,
}: {
  title: string;
  body: string;
  meta: string;
  selected: boolean;
  badge?: string;
  onSelect: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onSelect}
      className={`flex min-h-[220px] flex-col rounded-2xl bg-panel p-5 text-left transition-shadow ${
        selected ? "shadow-selected" : "hover:shadow-card"
      }`}
    >
      <div className="mb-3 min-h-7">
        {badge ? (
          <span className="inline-flex items-center gap-2 font-heading text-sm text-gold">
            <span className="h-2.5 w-2.5 rounded-full bg-gold" />
            {badge}
          </span>
        ) : (
          <span className="text-muted">·</span>
        )}
      </div>
      <h3 className="font-heading text-2xl text-text">{title}</h3>
      <p className="mt-3 flex-1 text-sm leading-relaxed text-muted">{body}</p>
      <p className="mt-4 text-sm text-muted">{meta}</p>
    </button>
  );
}
