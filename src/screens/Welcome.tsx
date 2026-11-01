import { StatusChip } from "../components/StatusChip";
import type { Detection, InstalledState, Recommendation } from "../types";

export function Welcome({
  detection,
  installed,
  recommendation,
  onHelp,
}: {
  detection: Detection;
  installed: InstalledState | null;
  recommendation: Recommendation;
  onHelp: () => void;
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
              ? "Fabric 0.20.1 installed"
              : "Fabric will install"
          }
        />
        <StatusChip
          tone={detection.gameRunning ? "warn" : "ok"}
          label={detection.gameRunning ? "Game is running" : "Game not running"}
        />
        <StatusChip tone="warn" label="Update available: Arnold Ships 0.10.2" />
      </div>
      {already ? (
        <section className="panel mt-8 max-w-2xl">
          <h2 className="font-heading text-2xl text-text">Already installed (from 2.3.4)</h2>
          <p className="mt-2 text-muted">
            Recommended kit · 7 mods · last change {installed?.updatedAt}
          </p>
        </section>
      ) : (
        <section className="panel mt-8 max-w-2xl">
          <h2 className="font-heading text-2xl text-text">Ready when you are</h2>
          <p className="mt-2 text-muted">
            Scan this PC next. Right now that would pick the {recommendation.kit} kit.
          </p>
        </section>
      )}
      <button type="button" className="mt-6 self-start text-sm text-primary" onClick={onHelp}>
        Mac: opening for the first time
      </button>
    </div>
  );
}
