import type { InstallPlan, ProgressEvent } from "../types";

const ROWS = [
  "Backup saved (restore point 2026-11-01 20:41)",
  "Fabric loader 0.20.1 + Fabric API 0.165.0 kept",
  "Sodium 0.9.2 SHA-512 verified",
  "Arnold Ships client 0.10.2 SHA-512 verified · replaced 0.10.1",
  "Iris 1.11.7 — verifying SHA-512…",
  "Complementary Reimagined r5.9.3 — 41%",
  "Wakes 0.7.3 · Particle Rain 4.0.2 · Snow config",
];

export function Progress({
  plan,
  event,
}: {
  plan: InstallPlan;
  event: ProgressEvent | null;
}) {
  const percent = event?.percent ?? 8;
  const current = event?.message ?? ROWS[0];
  const reached = Math.max(
    0,
    ROWS.findIndex((row) => current.startsWith(row.slice(0, 12))),
  );
  return (
    <div className="flex flex-1 flex-col">
      <h1 className="mt-6 font-heading text-4xl text-gold md:text-5xl">Installing…</h1>
      <div className="progress-track mt-6">
        <div className="progress-fill" style={{ width: `${percent}%` }} />
      </div>
      <p className="mt-3 text-muted">
        Step 3 of 4 · Mods · {Math.round((percent / 100) * (plan.bytes / (1024 * 1024)))} MB of{" "}
        {Math.round(plan.bytes / (1024 * 1024))} MB
      </p>
      <section className="panel mt-6 space-y-3">
        {ROWS.map((row, index) => {
          const done = index < reached;
          const active = index === reached;
          return (
            <div
              key={row}
              className={`flex gap-3 ${
                active ? "text-glow" : done ? "text-text" : "text-muted"
              }`}
            >
              <span className={done ? "text-success" : active ? "text-glow" : "text-muted"}>
                {done ? "✓" : active ? "↻" : "○"}
              </span>
              <span>{row}</span>
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
