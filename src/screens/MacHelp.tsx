export function MacHelp() {
  return (
    <div className="flex flex-1 flex-col">
      <h1 className="mt-6 font-heading text-4xl text-gold">Mac: opening for the first time</h1>
      <ol className="panel mt-6 list-decimal space-y-3 pl-6 text-lg text-muted">
        <li>
          Open the <strong className="text-text">.dmg</strong> and drag{" "}
          <strong className="text-text">Arnold Client Mods</strong> into Applications.
        </li>
        <li>
          In Applications, <strong className="text-text">right-click</strong> the app and choose
          Open, then Open again.
        </li>
        <li>
          On macOS 15 or newer, if there is no Open button: System Settings → Privacy &amp; Security
          → Open Anyway.
        </li>
      </ol>
      <p className="mt-4 text-muted">You only need to do this once. Updates install without asking again.</p>
    </div>
  );
}
