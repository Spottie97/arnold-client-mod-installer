import { LogExportResult } from "../components/LogExport";
import type { InstallPlan } from "../types";

export function Done({
  plan,
  launcherNote,
  showLauncherNote,
  logPath,
  logError,
  onCopyLogPath,
  onOpenLogFolder,
}: {
  plan: InstallPlan;
  launcherNote: string | null;
  showLauncherNote: boolean;
  logPath: string | null;
  logError: string | null;
  onCopyLogPath: () => void;
  onOpenLogFolder: () => void;
}) {
  return (
    <div className="flex flex-1 flex-col">
      <h1 className="mt-6 font-heading text-4xl text-gold md:text-5xl">You&apos;re set</h1>
      <p className="mt-4 max-w-2xl text-lg text-muted">
        Vanilla still works if a friend joins without mods.
      </p>
      {showLauncherNote && launcherNote ? (
        <p className="mt-3 max-w-2xl text-lg text-text">{launcherNote}</p>
      ) : null}
      <section className="panel mt-6">
        <h2 className="font-heading text-xl text-gold">What changed</h2>
        <ul className="mt-3 space-y-2 text-muted">
          <li>Added {plan.add.length} files ({Math.round(plan.bytes / (1024 * 1024))} MB)</li>
          <li>Updated {plan.update.length} files</li>
          <li>Removed {plan.remove.length} files</li>
          <li>
            Quality {plan.quality}: snow {plan.snowLite ? "lite fallback" : "normal"}, Sodium /
            Iris / Complementary follow the table
          </li>
        </ul>
      </section>
      <LogExportResult
        path={logPath}
        error={logError}
        onCopy={onCopyLogPath}
        onOpenFolder={onOpenLogFolder}
      />
    </div>
  );
}
