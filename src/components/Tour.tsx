import { TOUR_TIPS } from "../lib/tour";

export function Tour({
  step,
  onNext,
  onSkip,
}: {
  step: number;
  onNext: () => void;
  onSkip: () => void;
}) {
  const tip = TOUR_TIPS[step];
  const last = step >= TOUR_TIPS.length - 1;
  return (
    <div
      className="fixed inset-0 z-20 flex items-center justify-center bg-black/60 px-6"
      role="dialog"
      aria-modal="true"
      aria-labelledby="tour-title"
    >
      <section className="panel max-w-lg">
        <p className="text-sm text-muted">Tip {step + 1} of {TOUR_TIPS.length}</p>
        <h2 id="tour-title" className="mt-2 font-heading text-2xl text-gold">
          {tip.title}
        </h2>
        <p className="mt-3 text-muted">{tip.body}</p>
        <div className="mt-5 flex justify-end gap-3">
          <button type="button" className="btn-secondary" onClick={onSkip}>
            Skip tour
          </button>
          <button type="button" className="btn-primary" onClick={onNext}>
            {last ? "Got it" : "Next tip"}
          </button>
        </div>
      </section>
    </div>
  );
}
