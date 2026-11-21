import { PackArt } from "./PackArt";
import type { PackEntry } from "../types";

export function PackCard({
  pack,
  status,
  blurb,
  onDetails,
  onInstall,
  onPlay,
}: {
  pack: PackEntry;
  status: string;
  blurb: string;
  onDetails: () => void;
  onInstall: () => void;
  onPlay?: () => void;
}) {
  return (
    <article className="card flex items-center gap-4 rounded-2xl bg-panel p-5">
      <PackArt art={pack.art} name={pack.name} size={112} />
      <div className="min-w-0 flex-1">
        <div className="flex flex-wrap gap-2">
          <span className="rounded-full bg-accent-deep px-3 py-1 text-xs text-text">
            FOR THE ARNOLD SERVER
          </span>
          <span className="rounded-full bg-[#3a2a12] px-3 py-1 text-xs text-gold">{status}</span>
        </div>
        <h3 className="mt-2 font-heading text-2xl text-gold">{pack.name}</h3>
        <p className="mt-1 text-sm text-muted">
          {pack.displayVersion ? `pack ${pack.displayVersion} · ` : ""}
          Minecraft {pack.minecraft}
        </p>
        <p className="mt-2 text-sm leading-relaxed text-text">{blurb}</p>
        <div className="mt-3 flex flex-wrap gap-2">
          {onPlay ? (
            <button type="button" className="btn-secondary" onClick={onPlay}>
              Play
            </button>
          ) : null}
          <button type="button" className="btn-primary" onClick={onInstall}>
            {status === "UPDATE AVAILABLE" ? "Update" : "Install"}
          </button>
          <button type="button" className="btn-secondary" onClick={onDetails}>
            Details
          </button>
        </div>
      </div>
    </article>
  );
}
