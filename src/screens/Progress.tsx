import type { InstallPlan, ProgressEvent } from "../types";

export function Progress({
  plan,
  event,
  log,
}: {
  plan: InstallPlan;
  event: ProgressEvent | null;
  log: ProgressEvent[];
}) {
  const percent = event?.percent ?? 0;
  const rows = log.length > 0 ? log : event ? [event] : [];
  return (
    <div className="flex flex-1 flex-col">
      <h1 className="mt-6 font-heading text-4xl text-gold md:text-5xl">Installing…</h1>
      <div
        className="progress-track mt-6"
        role="progressbar"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={percent}
        aria-label="Install progress"
      >
        <div className="progress-fill" style={{ width: `${percent}%` }} />
      </div>
      <p className="mt-3 text-muted" aria-live="polite">
        {event?.step ?? "prepare"} · {percent}% · {event?.message ?? "Starting"}
      </p>
      <p className="mt-1 text-sm text-muted">
        {Math.round(plan.bytes / (1024 * 1024))} MB planned
      </p>
      <section className="panel mt-6 space-y-3" aria-label="Install log">
        {rows.map((row, index) => {
          const done = row.status === "done" || index < rows.length - 1;
          const active = index === rows.length - 1 && row.status !== "done";
          return (
            <div
              key={`${row.step}-${row.message}-${index}`}
              className={`flex gap-3 ${
                active ? "text-glow" : done ? "text-text" : "text-muted"
              }`}
            >
              <span className={done ? "text-success" : active ? "text-glow" : "text-muted"}>
                {done ? "✓" : active ? "↻" : "○"}
              </span>
              <span>{row.message}</span>
            </div>
          );
        })}
      </section>
      {plan.warnings[0] ? (
        <p className="mt-4 text-sm text-gold">{plan.warnings[0]}</p>
      ) : null}
    </div>
  );
}
