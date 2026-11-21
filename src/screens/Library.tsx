import { LauncherChips } from "../components/LauncherChips";
import { PackCard } from "../components/PackCard";
import type { LocalSourceInfo } from "../lib/localSource";
import { statusForPack } from "../lib/libraryStatus";
import type { Detection, InstalledState, PackIndexLoad } from "../types";

export function Library({
  library,
  detection,
  installedByPack,
  localSource,
  onOpen,
  onInstall,
  onPlay,
  onHelp,
  onPickLocalSource,
}: {
  library: PackIndexLoad | null;
  detection: Detection;
  installedByPack: Record<string, InstalledState | null>;
  localSource?: LocalSourceInfo | null;
  onOpen: (id: string) => void;
  onInstall: (id: string) => void;
  onPlay: (id: string) => void;
  onHelp: () => void;
  onPickLocalSource?: () => void;
}) {
  const packs = library?.index.packs ?? [];
  return (
    <div className="flex flex-1 flex-col">
      <h2 className="mt-2 font-heading text-4xl text-gold">Welcome, traveller</h2>
      <p className="mt-3 max-w-3xl text-base leading-relaxed text-muted">
        You need this installer to join the Arnold server. Each pack is signed and kept in its own
        folder by default, so packs do not overwrite each other.
      </p>
      {localSource?.enabled ? (
        <section className="panel mt-6 max-w-2xl border border-gold/40">
          <h3 className="font-heading text-2xl text-gold">TEST BUILD - local source</h3>
          {localSource.dir ? (
            <>
              <p className="mt-2 break-all text-text">{localSource.dir}</p>
              <p className="mt-2 text-muted">
                Reading packs.json or manifest.json, jars and settings from this folder.
              </p>
            </>
          ) : (
            <p className="mt-2 text-muted">
              No mods folder was found next to the app. Choose the folder that has manifest.json
              and manifest.json.sig, or packs.json and packs.json.sig.
            </p>
          )}
          {onPickLocalSource ? (
            <button type="button" className="btn-primary mt-4" onClick={onPickLocalSource}>
              {localSource.needsFolder ? "Choose mods folder" : "Change folder"}
            </button>
          ) : null}
        </section>
      ) : null}
      <div className="mt-6 grid gap-4 lg:grid-cols-[1.35fr_1fr]">
        <div className="space-y-4">
          {packs.map((pack) => {
            const status = statusForPack(pack, installedByPack);
            const installedHere = status === "INSTALLED" || status === "UPDATE AVAILABLE";
            return (
              <PackCard
                key={pack.id}
                pack={pack}
                status={status}
                blurb="The official client pack for the Arnold server: looks, performance, quality of life and resource packs."
                onDetails={() => onOpen(pack.id)}
                onInstall={() => onInstall(pack.id)}
                onPlay={installedHere ? () => onPlay(pack.id) : undefined}
              />
            );
          })}
        </div>
        <aside className="flex items-center justify-center rounded-2xl border border-dashed border-[#472a71] p-6 text-center text-sm leading-relaxed text-muted">
          More packs will appear here when they are published and signed. You will not need a new
          installer.
        </aside>
      </div>
      <LauncherChips detection={detection} />
      <button type="button" className="mt-6 self-start text-sm text-primary" onClick={onHelp}>
        Mac: opening for the first time
      </button>
    </div>
  );
}
