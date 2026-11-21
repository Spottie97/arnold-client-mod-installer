import { QualityPicker } from "../components/QualityPicker";
import type { Detection, QualityLevel, Recommendation } from "../types";

export function Scan({
  recommendation,
  detection,
  quality,
  onQuality,
  onRescan,
}: {
  recommendation: Recommendation;
  detection: Detection;
  quality: QualityLevel;
  onQuality: (level: QualityLevel) => void;
  onRescan: () => void;
}) {
  const { hardware } = recommendation;
  const official = Boolean(detection.launchers?.official);
  const tlauncher = Boolean(detection.launchers?.tlauncher);
  return (
    <div className="flex flex-1 flex-col">
      <h2 className="mt-2 font-heading text-4xl text-gold">This PC</h2>
      <p className="mt-2 max-w-3xl text-base text-muted">
        We scan this machine, suggest a quality, and look for the official Minecraft Launcher and
        TLauncher. Quality chooses how hard effects run. Kit (next) chooses the mods.
      </p>
      <div className="mt-6 grid gap-4 sm:grid-cols-2">
        <Stat label="Computer" value={`${hardware.cpu} · ${hardware.cores} cores`} />
        <Stat label="Memory" value={`${hardware.ramGb} GB`} />
        <Stat label="Graphics" value={hardware.gpu} />
        <Stat
          label="Minecraft"
          value={
            detection.minecraftVersionInstalled
              ? `${detection.minecraftVersion} found · ${detection.gameRunning ? "game is running" : "game not running"}`
              : `${detection.minecraftVersion} not found`
          }
        />
      </div>
      <section className="panel mt-4">
        <h3 className="font-heading text-xl text-gold">Launchers</h3>
        <p className="mt-2 text-sm">
          Official Minecraft Launcher: {official ? "found" : "not found"}
        </p>
        <p className="mt-1 text-sm">TLauncher: {tlauncher ? "found" : "not found"}</p>
        <p className="mt-2 text-sm leading-relaxed text-muted">
          {official && tlauncher
            ? "Both are present, so the pack profile will be written to both. Play will open the official Minecraft Launcher unless you change that in Settings."
            : official
              ? "The pack profile will be written for the official Minecraft Launcher. Play will open it."
              : tlauncher
                ? "The pack profile will be written for TLauncher. Play will open TLauncher."
                : "Neither launcher was found. Use Settings if you install one later. We'll still set up the pack folder and a launcher profile file."}
        </p>
      </section>
      <div className="mt-4">
        <QualityPicker
          selected={quality}
          recommended={recommendation.quality}
          table={undefined}
          onSelect={onQuality}
        />
      </div>
      <button type="button" className="btn-secondary mt-4 self-start" onClick={onRescan}>
        Re-scan
      </button>
    </div>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="panel">
      <div className="text-sm text-muted">{label}</div>
      <div className="mt-2 text-lg">{value}</div>
    </div>
  );
}
