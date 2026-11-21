import { useState } from "react";
import { PackArt } from "../components/PackArt";
import { openCreditLink } from "../lib/api";
import { packArtFor } from "../lib/packArt";
import type { CopyReport, Detection, InstalledState, PackEntry } from "../types";

export function PackDetails({
  pack,
  detection,
  installed,
  canCopy,
  onChoose,
  onCopy,
}: {
  pack: PackEntry;
  detection: Detection;
  installed: InstalledState | null;
  canCopy: boolean;
  onChoose: (choice: "keep" | "fresh") => void;
  onCopy: (item: "options" | "servers" | "saves") => Promise<CopyReport>;
}) {
  const art = packArtFor(pack.art);
  const choice = detection.needsFolderChoice;
  const [copying, setCopying] = useState<string | null>(null);
  const [reports, setReports] = useState<CopyReport[]>([]);
  const folder = detection.packDir ?? "";
  const own = detection.folderMode !== "default";

  const copy = async (item: "options" | "servers" | "saves") => {
    setCopying(item);
    try {
      const report = await onCopy(item);
      setReports((current) => [...current.filter((row) => row.item !== item), report]);
    } finally {
      setCopying(null);
    }
  };

  return (
    <div className="flex flex-1 flex-col">
      <div className="panel mt-2 flex flex-col gap-6 p-7 md:flex-row md:items-center">
        <div>
          <PackArt art={pack.art} name={pack.name} size={180} />
          {art ? (
            <p className="mt-3 max-w-[180px] text-xs leading-relaxed text-muted">
              Pack art: {art.credit} ·{" "}
              <button
                type="button"
                className="text-primary underline"
                onClick={() => void openCreditLink(art.licenceLink)}
              >
                {art.licence}
              </button>{" "}
              ·{" "}
              <button
                type="button"
                className="text-primary underline"
                onClick={() => void openCreditLink(art.sourceLink)}
              >
                Source on Modrinth
              </button>
              <span className="mt-1 block">({art.note})</span>
            </p>
          ) : null}
        </div>
        <div>
          <span className="rounded-full bg-accent-deep px-3 py-1 text-xs text-text">
            FOR THE ARNOLD SERVER
          </span>
          <h2 className="mt-3 font-heading text-4xl text-gold">{pack.name}</h2>
          <p className="mt-1 text-lg">
            {art ? "Matcha Flavoured · " : ""}
            pack {pack.displayVersion ?? installed?.manifestVersion ?? ""}
          </p>
          <p className="mt-3 max-w-2xl text-base leading-relaxed">
            This is the official client pack for the Arnold server. It installs looks, performance
            and quality of life mods, plus resource packs, into its own {pack.name} folder by
            default. You need this pack — and this installer — to join. Everything is checked with
            SHA-512 and backed up before anything changes.
          </p>
          <p className="mt-3 text-sm text-muted">
            Minecraft {pack.minecraft} · profile “Arnold — {pack.name}”
            {installed ? ` · installed ${installed.manifestVersion}` : ""}
          </p>
          <p className="mt-3 text-sm text-text">
            {own
              ? `Installs into its own folder: ${folder}`
              : `Installs into the Minecraft folder: ${folder}`}
          </p>
        </div>
      </div>
      {choice ? (
        <section className="panel mt-4">
          <h3 className="font-heading text-xl text-gold">This PC already has an install</h3>
          <p className="mt-2 text-sm leading-relaxed text-muted">
            Found an existing install at {choice.root}. Nothing has been moved or deleted. Keep
            using that Minecraft folder, or set up a fresh pack folder.
          </p>
          <div className="mt-4 flex flex-wrap gap-3">
            <button type="button" className="btn-primary" onClick={() => onChoose("keep")}>
              Keep using my current Minecraft folder
            </button>
            <button type="button" className="btn-secondary" onClick={() => onChoose("fresh")}>
              Set up the pack folder fresh
            </button>
          </div>
        </section>
      ) : null}
      {!choice && own && canCopy ? (
        <section className="panel mt-4">
          <h3 className="font-heading text-xl text-gold">Copy from the old folder</h3>
          <p className="mt-2 text-sm text-muted">
            Nothing is copied until you press a button. Existing files in the pack folder are left
            as they are.
          </p>
          <div className="mt-4 flex flex-wrap gap-3">
            <button type="button" className="btn-secondary" disabled={copying !== null} onClick={() => void copy("options")}>
              {copying === "options" ? "Copying…" : "Copy my options"}
            </button>
            <button type="button" className="btn-secondary" disabled={copying !== null} onClick={() => void copy("servers")}>
              {copying === "servers" ? "Copying…" : "Copy my server list"}
            </button>
            <button type="button" className="btn-secondary" disabled={copying !== null} onClick={() => void copy("saves")}>
              {copying === "saves" ? "Copying…" : "Copy my worlds"}
            </button>
          </div>
          {reports.map((report) => (
            <p key={report.item} className="mt-2 text-sm text-muted">
              {report.item}: copied {report.copied.length}, left in place {report.skipped.length}
            </p>
          ))}
        </section>
      ) : null}
    </div>
  );
}
