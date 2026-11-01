import type { Recommendation } from "../types";

export function Scan({
  recommendation,
  onRescan,
}: {
  recommendation: Recommendation;
  onRescan: () => void;
}) {
  const { hardware } = recommendation;
  return (
    <div className="flex flex-1 flex-col">
      <h1 className="mt-6 font-heading text-4xl text-gold md:text-5xl">Your PC</h1>
      <div className="mt-8 grid gap-4 sm:grid-cols-2">
        <Stat label="CPU" value={`${hardware.cpu} · ${hardware.cores} cores`} />
        <Stat label="RAM" value={`${hardware.ramGb} GB`} />
        <Stat label="GPU" value={hardware.gpu} />
        <Stat label="VRAM" value={`${hardware.vramGb} GB`} />
      </div>
      <section className="panel mt-6">
        <p className="font-heading text-gold">Recommended kit: {recommendation.kit}</p>
        <ul className="mt-3 space-y-1 text-muted">
          {recommendation.reasons.map((reason) => (
            <li key={reason}>{reason}</li>
          ))}
        </ul>
        {recommendation.warnLowRam ? (
          <p className="mt-3 text-gold">This PC is short on RAM. Lighter snow is safer.</p>
        ) : null}
      </section>
      <button type="button" className="btn-secondary mt-6 self-start" onClick={onRescan}>
        Re-scan
      </button>
    </div>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="panel">
      <div className="text-sm text-muted">{label}</div>
      <div className="mt-2 font-heading text-xl">{value}</div>
    </div>
  );
}
