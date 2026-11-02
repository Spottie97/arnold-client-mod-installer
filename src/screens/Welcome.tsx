import { StatusChip } from "../components/StatusChip";
import type { LocalSourceInfo } from "../lib/localSource";
import type { Detection, InstalledState, Recommendation, UpdateBadge } from "../types";

export function Welcome({
  detection,
  installed,
  recommendation,
  badges,
  localSource,
  onHelp,
  onInstallRecommended,
  onPickLocalSource,
}: {
  detection: Detection;
  installed: InstalledState | null;
  recommendation: Recommendation;
  badges: UpdateBadge[];
  localSource?: LocalSourceInfo | null;
  onHelp: () => void;
  onInstallRecommended: () => void;
  onPickLocalSource?: () => void;
}) {
  const already = Boolean(installed);
  return (
    <div className="flex flex-1 flex-col">
      <h1 className="mt-6 font-heading text-4xl text-gold md:text-5xl">Welcome, traveller</h1>
      <p className="mt-5 max-w-3xl text-lg leading-relaxed text-muted">
        This installs Arnold&apos;s ship, deck and snow mods plus optional performance and shader
        packs. Vanilla clients can still join. Everything is checked with SHA-512 and backed up
        before anything changes.
      </p>
      {localSource?.enabled ? (
        <section className="panel mt-8 max-w-2xl border border-gold/40">
          <h2 className="font-heading text-2xl text-gold">TEST BUILD - local source</h2>
          {localSource.dir ? (
            <>
              <p className="mt-2 break-all text-text">{localSource.dir}</p>
              <p className="mt-2 text-muted">
                Reading manifest.json, jars and settings from this folder.
              </p>
            </>
          ) : (
            <p className="mt-2 text-muted">
              No mods folder was found next to the app. Choose the folder that has manifest.json
              and manifest.json.sig. An MSI installed under Program Files will not see a folder
              sitting beside the download, so the picker still appears there.
            </p>
          )}
          {onPickLocalSource ? (
            <button type="button" className="btn-primary mt-4" onClick={onPickLocalSource}>
              {localSource.needsFolder ? "Choose mods folder" : "Change folder"}
            </button>
          ) : null}
        </section>
      ) : null}
      <div className="mt-8 flex flex-wrap gap-3">
        <StatusChip
          tone={detection.minecraftVersionInstalled ? "ok" : "warn"}
          label={
            detection.minecraftVersionInstalled
              ? `Minecraft ${detection.minecraftVersion} found`
              : `Minecraft ${detection.minecraftVersion} missing`
          }
        />
        <StatusChip
          tone={detection.fabric.installed ? "ok" : "idle"}
          label={
            detection.fabric.installed
              ? "Fabric installed"
              : "Fabric will install"
          }
        />
        <StatusChip
          tone={detection.gameRunning ? "warn" : "ok"}
          label={detection.gameRunning ? "Game is running" : "Game not running"}
        />
        {badges.map((badge) => (
          <StatusChip
            key={badge.id}
            tone="warn"
            label={`Update available: ${badge.name} ${badge.available}`}
          />
        ))}
      </div>
      {already ? (
        <section className="panel mt-8 max-w-2xl">
          <h2 className="font-heading text-2xl text-text">Already installed</h2>
          <p className="mt-2 text-muted">
            {installed?.source} · {installed?.components.length ?? 0} mods · last change{" "}
            {installed?.updatedAt}
          </p>
        </section>
      ) : (
        <section className="panel mt-8 max-w-2xl">
          <h2 className="font-heading text-2xl text-text">Ready when you are</h2>
          <p className="mt-2 text-muted">
            This PC would get the {recommendation.kit} kit at {recommendation.quality} quality
            {recommendation.snowLite ? " with lighter snow" : ""}.
          </p>
          <button type="button" className="btn-primary mt-4" onClick={onInstallRecommended}>
            Install recommended
          </button>
        </section>
      )}
      <button type="button" className="mt-6 self-start text-sm text-primary" onClick={onHelp}>
        Mac: opening for the first time
      </button>
    </div>
  );
}
