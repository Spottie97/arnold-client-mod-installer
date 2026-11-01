type Tone = "ok" | "warn" | "idle";

export function StatusChip({
  label,
  tone = "ok",
}: {
  label: string;
  tone?: Tone;
}) {
  const mark =
    tone === "ok" ? "✓" : tone === "warn" ? "↻" : "·";
  const markClass =
    tone === "ok" ? "text-success" : tone === "warn" ? "text-gold" : "text-muted";
  return (
    <span className="inline-flex items-center gap-2 rounded-full bg-[#241f2e] px-4 py-2 font-heading text-sm text-gold">
      <span className={markClass}>{mark}</span>
      {label}
    </span>
  );
}
