import { useEffect, useState } from "react";
import { QualityPicker } from "../components/QualityPicker";
import { getConfig, openCreditLink, setConfig, setConfigKeys } from "../lib/api";
import { APP_VERSION } from "../lib/version";
import type {
  Detection,
  LauncherKind,
  Manifest,
  Prefs,
  QualityLevel,
  SnapshotMeta,
} from "../types";

export function Settings({
  manifest,
  quality,
  recommended,
  detection,
  prefs,
  snapshots,
  onSnowLite,
  onQuality,
  onUseDefault,
  onChooseFolder,
  onPickGameDir,
  onLauncherWrite,
  onPlayTarget,
  onRestore,
  onOpenFolder,
}: {
  manifest: Manifest;
  quality: QualityLevel;
  recommended?: QualityLevel;
  detection: Detection;
  prefs: Prefs;
  snapshots: SnapshotMeta[];
  onSnowLite: (lite: boolean) => void;
  onQuality: (level: QualityLevel) => void;
  onUseDefault: (enabled: boolean) => void;
  onChooseFolder: (choice: "keep" | "fresh") => void;
  onPickGameDir: () => void;
  onLauncherWrite: (which: "official" | "tLauncher", enabled: boolean) => void;
  onPlayTarget: (kind: LauncherKind) => void;
  onRestore: (id: string) => void;
  onOpenFolder: () => void;
}) {
  const ships = manifest.components.find((c) => c.id === "ships");
  const snow = manifest.components.find((c) => c.id === "snow");
  const [lite, setLite] = useState(false);
  const [message, setMessage] = useState("");

  useEffect(() => {
    void getConfig("snow").then((cfg) => setLite(Boolean(cfg.liteDetected)));
  }, []);

  const toggle = async (id: string, key: string, value: boolean) => {
    await setConfigKeys(id, { [key]: value ? "true" : "false" });
    setMessage(`Saved ${key}`);
  };

  const official = Boolean(detection.launchers?.official);
  const tlauncher = Boolean(detection.launchers?.tlauncher?.openTarget || detection.launchers?.tlauncher);
  const writeOfficial = prefs.launchers?.writeOfficial !== false;
  const writeTl = prefs.launchers?.writeTlauncher !== false;
  const play = prefs.launchers?.play ?? detection.launchers?.playDefault ?? "official";

  return (
    <div className="flex flex-1 flex-col">
      <h2 className="mt-2 font-heading text-4xl text-gold">Settings</h2>
      <section className="panel mt-4">
        <h3 className="font-heading text-xl text-gold">Pack folder</h3>
        <p className="mt-2 break-all text-sm">{detection.packDir}</p>
        <label className="mt-3 flex items-center gap-3 text-sm">
          <input
            type="checkbox"
            checked={Boolean(prefs.useDefaultFolder)}
            onChange={(event) => {
              if (!event.target.checked) {
                onUseDefault(false);
                return;
              }
              const ok = window.confirm(
                "Packs will share the Minecraft folder, one pack at a time. Existing files are not moved.",
              );
              if (ok) onUseDefault(true);
            }}
          />
          Install into the default Minecraft folder instead
        </label>
        <p className="mt-2 text-sm text-muted">
          Per-pack folders are the default, so packs do not overwrite each other. This option is
          off until you turn it on. Existing files are never moved or deleted without asking.
        </p>
        <div className="mt-3 flex flex-wrap gap-2">
          <button type="button" className="btn-secondary" onClick={() => onChooseFolder("keep")}>
            Keep my current Minecraft folder
          </button>
          <button type="button" className="btn-secondary" onClick={() => onChooseFolder("fresh")}>
            Use a fresh pack folder
          </button>
          <button type="button" className="btn-secondary" onClick={onPickGameDir}>
            Choose Minecraft folder
          </button>
          <button type="button" className="btn-secondary" onClick={onOpenFolder}>
            Open pack folder
          </button>
        </div>
      </section>
      <section className="panel mt-4">
        <h3 className="font-heading text-xl text-gold">Launchers</h3>
        {official ? (
          <label className="mt-2 flex items-center gap-3 text-sm">
            <input
              type="checkbox"
              checked={writeOfficial}
              onChange={(event) => onLauncherWrite("official", event.target.checked)}
            />
            Write profile to the official Minecraft Launcher
          </label>
        ) : null}
        {tlauncher ? (
          <label className="mt-2 flex items-center gap-3 text-sm">
            <input
              type="checkbox"
              checked={writeTl}
              onChange={(event) => onLauncherWrite("tLauncher", event.target.checked)}
            />
            Write profile to TLauncher
          </label>
        ) : null}
        {official && tlauncher ? (
          <div className="mt-3 text-sm">
            <p>Play opens</p>
            <label className="mt-1 flex items-center gap-2">
              <input
                type="radio"
                name="play-target"
                checked={play !== "tLauncher"}
                onChange={() => onPlayTarget("official")}
              />
              Official Minecraft Launcher
            </label>
            <label className="mt-1 flex items-center gap-2">
              <input
                type="radio"
                name="play-target"
                checked={play === "tLauncher"}
                onChange={() => onPlayTarget("tLauncher")}
              />
              TLauncher
            </label>
          </div>
        ) : null}
        <p className="mt-2 text-sm text-muted">
          If neither is found, the pack folder and an official-style profile file are still written
          so the official launcher can see it later. This app never starts the game itself.
        </p>
      </section>
      <section className="panel mt-4">
        <h3 className="font-heading text-xl text-gold">Quality</h3>
        <QualityPicker
          selected={quality}
          recommended={recommended}
          table={manifest.qualityPresets}
          onSelect={onQuality}
        />
      </section>
      <h3 className="mt-6 font-heading text-2xl text-gold">Ships &amp; snow</h3>
      <p className="mt-3 text-muted">
        Quality writes Sodium, Iris, Complementary, ships and snow keys. Missing keys only —
        values you already set stay put, except ship waterMask, rigidRoot and calmWhenAboard stay
        on. Snowfall 1.3 writes the quality key directly. The snow.lite file swap is
        only for migrating old 1.2 installs.
      </p>
      <section className="panel mt-4">
        <h2 className="font-heading text-xl text-gold">Snow profile</h2>
        <label className="mt-3 flex items-center gap-3">
          <input
            type="checkbox"
            checked={lite}
            onChange={(event) => {
              const next = event.target.checked;
              setLite(next);
              onSnowLite(next);
              void setConfig("snow", next).then(() => setMessage(next ? "snow.lite on" : "normal snow"));
            }}
          />
          <span>Use snow.lite only when migrating an old 1.2 file (# arnold-installer-lite)</span>
        </label>
      </section>
      <section className="panel mt-4">
        <h2 className="font-heading text-xl text-gold">Arnold Ships</h2>
        <p className="mt-1 text-sm text-muted">
          waterMask, rigidRoot and calmWhenAboard stay on.
        </p>
        <div className="mt-3 grid gap-2 sm:grid-cols-2">
          {Object.entries(ships?.config?.settings ?? {})
            .filter(([, spec]) => spec.type === "bool")
            .filter(([key]) => !["waterMask", "rigidRoot", "calmWhenAboard"].includes(key))
            .map(([key, spec]) => (
              <label key={key} className="flex items-center gap-2 text-muted">
                <input
                  type="checkbox"
                  defaultChecked={Boolean(spec.default)}
                  onChange={(event) => void toggle("ships", key, event.target.checked)}
                />
                {key}
              </label>
            ))}
        </div>
      </section>
      <section className="panel mt-4">
        <h2 className="font-heading text-xl text-gold">Arnold Snow</h2>
        <div className="mt-3 grid gap-2 sm:grid-cols-2">
          {Object.entries(snow?.config?.settings ?? {})
            .filter(([, spec]) => spec.type === "bool")
            .map(([key, spec]) => (
              <label key={key} className="flex items-center gap-2 text-muted">
                <input
                  type="checkbox"
                  defaultChecked={Boolean(spec.default)}
                  onChange={(event) => void toggle("snow", key, event.target.checked)}
                />
                {key}
              </label>
            ))}
        </div>
      </section>
      {message ? <p className="mt-4 text-sm text-gold">{message}</p> : null}
      <section className="panel mt-4">
        <h3 className="font-heading text-xl text-gold">Restore</h3>
        {snapshots.length === 0 ? <p className="mt-2 text-sm text-muted">No restore points yet.</p> : null}
        <ul className="mt-2 space-y-2">
          {snapshots.map((snapshot) => (
            <li key={snapshot.id} className="flex items-center justify-between gap-3 text-sm">
              <span>
                {snapshot.label} · {snapshot.createdAt}
              </span>
              <button type="button" className="btn-secondary" onClick={() => onRestore(snapshot.id)}>
                Restore
              </button>
            </li>
          ))}
        </ul>
        <p className="mt-2 text-sm text-muted">
          Restore puts that snapshot back. Later files stay in a new backup first.
        </p>
      </section>
      <section className="panel mt-4">
        <h3 className="font-heading text-xl text-gold">About &amp; credits</h3>
        <p className="mt-2 text-sm">Arnold Client Mods v{APP_VERSION}</p>
        <p className="mt-2 text-sm">
          Pack art: Matcha Flavoured icon by Klei Wright, used unmodified under CC BY-NC-SA 4.0.{" "}
          <button type="button" className="text-primary underline" onClick={() => void openCreditLink("matcha-source")}>
            Source on Modrinth
          </button>{" "}
          <button type="button" className="text-primary underline" onClick={() => void openCreditLink("cc-by-nc-sa-4")}>
            Licence
          </button>
        </p>
        <p className="mt-2 text-sm">Not affiliated with Mojang or Microsoft.</p>
      </section>
    </div>
  );
}
