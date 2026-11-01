import type { ImportOffer } from "../types";

export function ImportScreen({
  offer,
  onConfirm,
  onSkip,
}: {
  offer: ImportOffer;
  onConfirm: () => void;
  onSkip: () => void;
}) {
  return (
    <div className="flex flex-1 flex-col">
      <h1 className="mt-6 font-heading text-4xl text-gold md:text-5xl">Found a 2.3.x install</h1>
      <p className="mt-4 max-w-3xl text-lg text-muted">
        {offer.versionGuess ?? "An earlier Arnold installer"} left jars
        {offer.backups.length > 0 ? " and backups" : ""}. Adopt them into state.json so Repair and
        Uninstall know what is yours?
      </p>
      <section className="panel mt-6" aria-label="Detected Arnold jars">
        <h2 className="font-heading text-xl text-gold">Jars</h2>
        <ul className="mt-3 space-y-2 text-muted">
          {offer.jars.map((jar) => (
            <li key={jar.path}>
              {jar.fileName}
              {jar.componentId ? ` · ${jar.componentId}` : ""}
              {jar.knownBuild ? " · known Arnold build" : " · unknown checksum"}
            </li>
          ))}
        </ul>
      </section>
      {offer.backups.length > 0 ? (
        <section className="panel mt-4" aria-label="Detected backups">
          <h2 className="font-heading text-xl text-gold">Backups</h2>
          <ul className="mt-3 space-y-2 text-muted">
            {offer.backups.map((backup) => (
              <li key={backup.path}>
                {backup.path} · {backup.kind}
              </li>
            ))}
          </ul>
        </section>
      ) : null}
      <div className="mt-6 flex flex-wrap gap-3">
        <button type="button" className="btn-secondary" onClick={onSkip}>
          Leave them alone
        </button>
        <button type="button" className="btn-primary" onClick={onConfirm}>
          Adopt into this installer
        </button>
      </div>
    </div>
  );
}
