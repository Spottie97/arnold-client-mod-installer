import { LogExportResult } from "../components/LogExport";
import { qualityLabel } from "../lib/quality";
import type { Detection, InstallPlan, ProgressEvent } from "../types";

export function Done({
  plan,
  detection,
  packName,
  playLabel,
  profileNote,
  logPath,
  logError,
  skipped,
  onOpenFolder,
  onCopyLogPath,
  onOpenLogFolder,
}: {
  plan: InstallPlan;
  detection: Detection;
  packName: string;
  playLabel: string;
  profileNote?: string | null;
  logPath: string | null;
  logError: string | null;
  skipped: ProgressEvent[];
  onOpenFolder: () => void;
  onCopyLogPath: () => void;
  onOpenLogFolder: () => void;
}) {
  const official = Boolean(detection.launchers?.official);
  const tlauncher = Boolean(detection.launchers?.tlauncher);
  const profileSkipped = skipped.find((row) => row.step === "profile");
  return (
    <div className="flex flex-1 flex-col">
      <h2 className="mt-2 font-heading text-4xl text-gold">{packName} is ready</h2>
      <p className="mt-3 max-w-3xl text-base leading-relaxed">
        The pack is in {detection.folderMode === "default" ? "the Minecraft folder" : "its own folder"}. The profile “Arnold — {packName}” was written for the
        official Minecraft Launcher{tlauncher ? " and for TLauncher" : ""}. Play opens {playLabel} —
        it does not start the game inside this app. You need this pack to join the
        Arnold server.
      </p>
      <ol className="mt-4 list-decimal space-y-1 pl-5 text-sm leading-relaxed">
        <li>Press Play, or open the official Minecraft Launcher yourself.</li>
        <li>Choose the profile “Arnold — {packName}” (or the same version in TLauncher).</li>
        <li>Start the game, then join the Arnold server from Multiplayer.</li>
      </ol>
      <p className="mt-3 text-sm text-muted">
        Change the Play target in Settings if you would rather open TLauncher.
      </p>
      <section className="panel mt-4 text-sm">
        <p>
          <b>Kit</b> {plan.kit} · {qualityLabel(plan.quality)} quality
        </p>
        <p className="mt-2 break-all">
          <b>Folder</b> {detection.packDir}
        </p>
        <button type="button" className="btn-secondary mt-3" onClick={onOpenFolder}>
          Open folder
        </button>
        <p className="mt-3">
          <b>Profiles</b>{" "}
          {official && tlauncher
            ? "Written for official and TLauncher"
            : official
              ? "Written for the official Minecraft Launcher"
              : tlauncher
                ? "Written for TLauncher"
                : "An official-style profile file was written"}
        </p>
        {profileSkipped ? <p className="mt-2 text-gold">{profileSkipped.message}</p> : null}
        {profileNote ? <p className="mt-2 text-muted">{profileNote}</p> : null}
        {detection.folderMode === "own" ? (
          <p className="mt-2 text-muted">
            If a Flatpak launcher cannot see this folder, use the default Minecraft folder option
            in Settings.
          </p>
        ) : null}
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
