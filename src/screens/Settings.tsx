import { useEffect, useState } from "react";
import { QualityPicker } from "../components/QualityPicker";
import { getConfig, setConfig, setConfigKeys } from "../lib/api";
import type { Manifest, QualityLevel } from "../types";

export function Settings({
  manifest,
  quality,
  recommended,
  onSnowLite,
  onQuality,
}: {
  manifest: Manifest;
  quality: QualityLevel;
  recommended?: QualityLevel;
  onSnowLite: (lite: boolean) => void;
  onQuality: (level: QualityLevel) => void;
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

  return (
    <div className="flex flex-1 flex-col">
      <h1 className="mt-6 font-heading text-4xl text-gold">Ships &amp; snow</h1>
      <p className="mt-3 text-muted">
        Quality writes Sodium, Iris, Complementary, ships and snow keys. Missing keys only —
        values you already set stay put, except ship waterMask, rigidRoot and calmWhenAboard stay
        on. Snowfall 1.3 writes the quality key directly. The snow.lite file swap is
        only for migrating old 1.2 installs.
      </p>
      <section className="panel mt-6">
        <QualityPicker
          selected={quality}
          recommended={recommended}
          table={manifest.qualityPresets}
          onSelect={onQuality}
        />
      </section>
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
    </div>
  );
}
