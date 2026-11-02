export function LogExportResult({
  path,
  error,
  onCopy,
  onOpenFolder,
}: {
  path: string | null;
  error: string | null;
  onCopy: () => void;
  onOpenFolder: () => void;
}) {
  if (!path && !error) return null;
  return (
    <section className="panel mt-6" aria-live="polite">
      {error ? (
        <>
          <h2 className="font-heading text-xl text-gold">Log was not saved</h2>
          <p className="mt-2 text-muted">{error}</p>
        </>
      ) : (
        <>
          <h2 className="font-heading text-xl text-gold">Log saved</h2>
          <p className="mt-2 break-all text-text">{path}</p>
          <div className="mt-4 flex flex-wrap gap-3">
            <button type="button" className="btn-secondary" onClick={onCopy}>
              Copy path
            </button>
            <button type="button" className="btn-secondary" onClick={onOpenFolder}>
              Open folder
            </button>
          </div>
        </>
      )}
    </section>
  );
}
