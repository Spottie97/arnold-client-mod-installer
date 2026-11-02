import { useEffect, useMemo, useState } from "react";
import { AppShell } from "./components/AppShell";
import { Tour } from "./components/Tour";
import {
  asErrorPayload,
  cancelInstall,
  confirmImport,
  copyDiagnostics,
  detect,
  detectLauncher,
  detectLegacy,
  exportLog,
  finishApp,
  getLocalSource,
  getManifest,
  getPrefs,
  listInstalled,
  listSnapshots,
  listenProgress,
  openLauncher,
  pickLocalSource,
  revealPath,
  plan,
  repairInstall,
  runInstall,
  scanHardware,
  setGameDir,
  setQuality,
  setTourDone,
} from "./lib/api";
import { pickBanner, type LocalSourceInfo } from "./lib/localSource";
import { parseQuality, snowLiteForQuality } from "./lib/quality";
import { TOUR_TIPS } from "./lib/tour";
import { updateBadges } from "./lib/updates";
import { missingMinecraftError } from "./mock/backend";
import { Custom } from "./screens/Custom";
import { Done } from "./screens/Done";
import { ErrorScreen } from "./screens/ErrorScreen";
import { ImportScreen } from "./screens/Import";
import { Kit } from "./screens/Kit";
import { MacHelp } from "./screens/MacHelp";
import { Manage } from "./screens/Manage";
import { Progress } from "./screens/Progress";
import { Scan } from "./screens/Scan";
import { Settings } from "./screens/Settings";
import { Welcome } from "./screens/Welcome";
import type {
  Detection,
  ErrorPayload,
  ImportOffer,
  InstallPlan,
  InstalledState,
  Manifest,
  ProgressEvent,
  QualityLevel,
  Recommendation,
  Screen,
  SnapshotMeta,
} from "./types";
import { assertNever } from "./types";

const SCREENS: Screen[] = [
  "welcome",
  "scan",
  "kit",
  "custom",
  "progress",
  "done",
  "error",
  "manage",
  "help",
  "import",
  "settings",
];

function screenFromHash(): Screen {
  const value = window.location.hash.replace("#", "") as Screen;
  return SCREENS.includes(value) ? value : "welcome";
}

function emptyPlan(): InstallPlan {
  return {
    add: [],
    update: [],
    remove: [],
    configs: [],
    loader: null,
    snowLite: false,
    quality: "normal",
    kit: "recommended",
    components: [],
    warnings: [],
    bytes: 0,
  };
}

export default function App() {
  const [screen, setScreen] = useState<Screen>(screenFromHash);
  const [manifest, setManifest] = useState<Manifest | null>(null);
  const [offlineBanner, setOfflineBanner] = useState<string | null>(null);
  const [localSource, setLocalSource] = useState<LocalSourceInfo | null>(null);
  const [detection, setDetection] = useState<Detection | null>(null);
  const [recommendation, setRecommendation] = useState<Recommendation | null>(null);
  const [installed, setInstalled] = useState<InstalledState | null>(null);
  const [snapshots, setSnapshots] = useState<SnapshotMeta[]>([]);
  const [kit, setKit] = useState("recommended");
  const [ocean, setOcean] = useState(true);
  const [customIds, setCustomIds] = useState<string[]>(["deck", "ships", "snow", "sodium", "base"]);
  const [snowLite, setSnowLite] = useState(false);
  const [quality, setQualityLevel] = useState<QualityLevel>("normal");
  const [currentPlan, setCurrentPlan] = useState<InstallPlan>(emptyPlan());
  const [progress, setProgress] = useState<ProgressEvent | null>(null);
  const [progressLog, setProgressLog] = useState<ProgressEvent[]>([]);
  const [error, setError] = useState<ErrorPayload | null>(null);
  const [tourStep, setTourStep] = useState<number | null>(null);
  const [importOffer, setImportOffer] = useState<ImportOffer | null>(null);
  const [replaceCollisions, setReplaceCollisions] = useState(false);
  const [launcher, setLauncher] = useState<{ available: boolean; note: string } | null>(null);
  const [launcherFailed, setLauncherFailed] = useState(false);
  const [logPath, setLogPath] = useState<string | null>(null);
  const [logError, setLogError] = useState<string | null>(null);

  const go = (next: Screen) => {
    setScreen(next);
    window.location.hash = next;
  };

  useEffect(() => {
    const onHash = () => setScreen(screenFromHash());
    window.addEventListener("hashchange", onHash);
    return () => window.removeEventListener("hashchange", onHash);
  }, []);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      if (tourStep !== null) {
        setTourStep(null);
        return;
      }
      if (screen === "progress") return;
      if (screen === "welcome") return;
      go("welcome");
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [screen, tourStep]);

  useEffect(() => {
    if (screen !== "done") return;
    let alive = true;
    void detectLauncher().then((status) => {
      if (alive) setLauncher(status);
    });
    return () => {
      alive = false;
    };
  }, [screen]);

  useEffect(() => {
    let alive = true;
    void (async () => {
      try {
        const [load, d, r, inst, snaps, prefs, legacy, local] = await Promise.all([
          getManifest(),
          detect(),
          scanHardware(),
          listInstalled(),
          listSnapshots(),
          getPrefs(),
          detectLegacy().catch(() => null),
          getLocalSource(),
        ]);
        if (!alive) return;
        setManifest(load.manifest);
        setLocalSource(local);
        setOfflineBanner(load.offline ? load.message ?? "Using the last good mod list (offline)." : null);
        setDetection(d);
        setRecommendation(r);
        setInstalled(inst);
        setSnapshots(snaps);
        setKit(r.kit === "full" ? "recommended" : r.kit);
        const nextQuality = parseQuality(inst?.quality) ?? r.quality;
        setQualityLevel(nextQuality);
        setSnowLite(snowLiteForQuality(nextQuality));
        setOcean(r.recommendOcean);
        if (!prefs.tourDone) setTourStep(0);
        if (legacy?.found && !legacy.alreadyAdopted && !inst) {
          setImportOffer(legacy);
          go("import");
        }
      } catch (err) {
        if (!alive) return;
        setError(asErrorPayload(err));
      }
    })();
    return () => {
      alive = false;
    };
  }, []);

  const selection = useMemo(
    () => ({
      kit,
      customComponents: customIds,
      ocean,
      snowLite,
      quality,
    }),
    [kit, customIds, ocean, snowLite, quality],
  );

  useEffect(() => {
    let alive = true;
    void plan(selection).then((next) => {
      if (alive) setCurrentPlan(next);
    });
    return () => {
      alive = false;
    };
  }, [selection]);

  const badges = useMemo(
    () => (manifest ? updateBadges(manifest, installed) : []),
    [manifest, installed],
  );

  const startInstall = async (nextPlan = currentPlan, replace = replaceCollisions) => {
    if (detection && !detection.minecraftVersionInstalled) {
      setError(missingMinecraftError(detection.gameDir ?? "(no game folder)"));
      go("error");
      return;
    }
    setProgressLog([]);
    setProgress({
      runId: "",
      step: "prepare",
      status: "running",
      percent: 1,
      message: "Starting install",
    });
    go("progress");
    const stop = await listenProgress((event) => {
      setProgress(event);
      setProgressLog((rows) => [...rows, event]);
    });
    try {
      await runInstall(nextPlan, replace);
      const [inst, snaps] = await Promise.all([listInstalled(), listSnapshots()]);
      setInstalled(inst);
      setSnapshots(snaps);
      go("done");
    } catch (err) {
      const payload = asErrorPayload(err);
      if (payload.code === "E-CANCELLED") {
        go("kit");
        return;
      }
      setError(payload);
      setReplaceCollisions(payload.code === "P-COLLISION");
      go("error");
    } finally {
      stop();
    }
  };

  const startRecommended = async () => {
    if (!recommendation) return;
    const next = await plan({
      kit: recommendation.kit,
      customComponents: [],
      ocean: recommendation.recommendOcean,
      snowLite: snowLiteForQuality(recommendation.quality),
      quality: recommendation.quality,
    });
    setCurrentPlan(next);
    setKit(recommendation.kit === "full" ? "recommended" : recommendation.kit);
    setQualityLevel(recommendation.quality);
    setSnowLite(snowLiteForQuality(recommendation.quality));
    setOcean(recommendation.recommendOcean);
    await startInstall(next, false);
  };

  const startRepair = async () => {
    setProgressLog([]);
    go("progress");
    const stop = await listenProgress((event) => {
      setProgress(event);
      setProgressLog((rows) => [...rows, event]);
    });
    try {
      await repairInstall();
      go("done");
    } catch (err) {
      setError(asErrorPayload(err));
      go("error");
    } finally {
      stop();
    }
  };

  const chooseFolder = async () => {
    const path = window.prompt("Game folder", detection?.gameDir ?? "") ?? "";
    if (!path) return;
    const next = await setGameDir(path);
    setDetection(next);
    if (!next.minecraftVersionInstalled) {
      setError(missingMinecraftError(next.gameDir ?? path));
      go("error");
    }
  };

  const copyDiag = async () => {
    const text = await copyDiagnostics();
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      window.prompt("Copy diagnostics", text);
    }
  };

  const doExport = async () => {
    setLogError(null);
    try {
      const path = await exportLog();
      setLogPath(path);
    } catch (err) {
      const payload = asErrorPayload(err);
      setLogPath(null);
      setLogError(`${payload.title} ${payload.hint}`.trim());
    }
  };

  const copyLogPath = async () => {
    if (!logPath) return;
    try {
      await navigator.clipboard.writeText(logPath);
    } catch {
      window.prompt("Copy path", logPath);
    }
  };

  const openLogFolder = async () => {
    if (!logPath) return;
    try {
      await revealPath(logPath);
    } catch (err) {
      const payload = asErrorPayload(err);
      setLogError(`${payload.title} ${payload.hint}`.trim());
    }
  };

  const footer = footerFor(screen, {
    already: Boolean(installed),
    kit,
    onManage: () => go("manage"),
    onUpdate: () => go("scan"),
    onScanNext: () => go("kit"),
    onBackWelcome: () => go("welcome"),
    onBackKit: () => go("kit"),
    onBackScan: () => go("scan"),
    onCustom: () => go("custom"),
    onInstall: () => void startInstall(),
    onCancel: () => {
      void cancelInstall();
    },
    onExport: () => void doExport(),
    onCopy: () => void copyDiag(),
    onRetry: () => {
      if (error?.code === "P-COLLISION") {
        void startInstall(currentPlan, true);
        return;
      }
      go("welcome");
    },
    onFolder: () => void chooseFolder(),
    launcherAvailable: Boolean(launcher?.available),
    onOpenLauncher: () => {
      void openLauncher().then((result) => {
        if (result.opened) {
          void finishApp();
          return;
        }
        setLauncherFailed(true);
        if (result.note) {
          setLauncher((current) =>
            current ? { ...current, note: result.note } : { available: false, note: result.note },
          );
        }
      });
    },
    onFinish: () => {
      void finishApp();
    },
    onHelpBack: () => go("welcome"),
    onSettings: () => go("settings"),
  });

  if (!manifest || !detection || !recommendation) {
    if (error) {
      return (
        <AppShell
          screen="error"
          footer={
            <button type="button" className="btn-primary" onClick={() => window.location.reload()}>
              Try again
            </button>
          }
        >
          <ErrorScreen
            error={error}
            logPath={logPath}
            logError={logError}
            onCopyLogPath={() => void copyLogPath()}
            onOpenLogFolder={() => void openLogFolder()}
          />
        </AppShell>
      );
    }
    return (
      <div className="app-bg flex min-h-screen items-center justify-center font-heading text-gold">
        Loading…
      </div>
    );
  }

  let body: JSX.Element;
  switch (screen) {
    case "welcome":
      body = (
        <Welcome
          detection={detection}
          installed={installed}
          recommendation={recommendation}
          badges={badges}
          localSource={localSource}
          onHelp={() => go("help")}
          onInstallRecommended={() => void startRecommended()}
          onPickLocalSource={() => {
            void pickLocalSource()
              .then(async (info) => {
                setLocalSource(info);
                const load = await getManifest();
                setManifest(load.manifest);
                setOfflineBanner(
                  load.offline ? load.message ?? "Using the last good mod list (offline)." : null,
                );
                setCurrentPlan(await plan(selection));
              })
              .catch((err: unknown) => {
                setError(asErrorPayload(err));
                go("error");
              });
          }}
        />
      );
      break;
    case "scan":
      body = (
        <Scan
          recommendation={recommendation}
          onRescan={() => {
            void scanHardware().then(setRecommendation);
          }}
        />
      );
      break;
    case "kit":
      body = (
        <Kit
          selected={kit}
          quality={quality}
          ocean={ocean}
          recommendation={recommendation}
          plan={currentPlan}
          table={manifest.qualityPresets}
          onSelect={(next) => {
            setKit(next);
            if (next === "custom") go("custom");
          }}
          onQuality={(next) => {
            setQualityLevel(next);
            setSnowLite(snowLiteForQuality(next));
          }}
          onOcean={setOcean}
        />
      );
      break;
    case "custom":
      body = (
        <Custom
          manifest={manifest}
          selected={currentPlan.components}
          snowLite={snowLite}
          plan={currentPlan}
          onToggle={(id) => {
            setCustomIds((ids) =>
              ids.includes(id) ? ids.filter((item) => item !== id) : [...ids, id],
            );
            setKit("custom");
          }}
          onSnowLite={setSnowLite}
        />
      );
      break;
    case "progress":
      body = <Progress plan={currentPlan} event={progress} log={progressLog} />;
      break;
    case "done":
      body = (
        <Done
          plan={currentPlan}
          launcherNote={launcher?.note ?? null}
          showLauncherNote={Boolean(launcher) && (!launcher?.available || launcherFailed)}
          logPath={logPath}
          logError={logError}
          onCopyLogPath={() => void copyLogPath()}
          onOpenLogFolder={() => void openLogFolder()}
        />
      );
      break;
    case "error":
      body = (
        <ErrorScreen
          error={
            error ??
            missingMinecraftError(detection.gameDir ?? "C:\\Users\\Sam\\AppData\\Roaming\\.minecraft")
          }
          logPath={logPath}
          logError={logError}
          onCopyLogPath={() => void copyLogPath()}
          onOpenLogFolder={() => void openLogFolder()}
        />
      );
      break;
    case "manage":
      body = (
        <Manage
          installed={installed}
          snapshots={snapshots}
          badges={badges}
          onRepair={() => void startRepair()}
          onSettings={() => go("settings")}
        />
      );
      break;
    case "help":
      body = <MacHelp />;
      break;
    case "import":
      body = (
        <ImportScreen
          offer={
            importOffer ?? {
              found: false,
              jars: [],
              backups: [],
              alreadyAdopted: false,
            }
          }
          onConfirm={() => {
            void confirmImport().then((state) => {
              setInstalled(state);
              go("welcome");
            });
          }}
          onSkip={() => go("welcome")}
        />
      );
      break;
    case "settings":
      body = (
        <Settings
          manifest={manifest}
          quality={quality}
          recommended={recommendation.quality}
          onSnowLite={setSnowLite}
          onQuality={(next) => {
            setQualityLevel(next);
            setSnowLite(snowLiteForQuality(next));
            void setQuality(next)
              .then((state) => {
                if (state) setInstalled(state);
              })
              .catch(() => undefined);
          }}
        />
      );
      break;
    default:
      body = assertNever(screen);
  }

  return (
    <>
      {tourStep !== null ? (
        <Tour
          step={tourStep}
          onNext={() => {
            if (tourStep >= TOUR_TIPS.length - 1) {
              setTourStep(null);
              void setTourDone();
            } else {
              setTourStep(tourStep + 1);
            }
          }}
          onSkip={() => {
            setTourStep(null);
            void setTourDone();
          }}
        />
      ) : null}
      <AppShell
        screen={screen}
        footer={footer}
        banner={pickBanner(localSource, offlineBanner)}
        bannerDetail={localSource?.enabled && localSource.dir ? localSource.dir : null}
      >
        {body}
      </AppShell>
    </>
  );
}

function footerFor(
  screen: Screen,
  actions: {
    already: boolean;
    kit: string;
    onManage: () => void;
    onUpdate: () => void;
    onScanNext: () => void;
    onBackWelcome: () => void;
    onBackKit: () => void;
    onBackScan: () => void;
    onCustom: () => void;
    onInstall: () => void;
    onCancel: () => void;
    onExport: () => void;
    onCopy: () => void;
    onRetry: () => void;
    onFolder: () => void;
    launcherAvailable: boolean;
    onOpenLauncher: () => void;
    onFinish: () => void;
    onHelpBack: () => void;
    onSettings: () => void;
  },
) {
  switch (screen) {
    case "welcome":
      return (
        <>
          <button type="button" className="btn-secondary" onClick={actions.onManage}>
            Manage / Uninstall
          </button>
          <button type="button" className="btn-primary" onClick={actions.onUpdate}>
            {actions.already ? "Update mods →" : "Scan this PC →"}
          </button>
        </>
      );
    case "scan":
      return (
        <>
          <button type="button" className="btn-secondary" onClick={actions.onBackWelcome}>
            ← Back
          </button>
          <button type="button" className="btn-primary" onClick={actions.onScanNext}>
            Choose a kit →
          </button>
        </>
      );
    case "kit":
      return (
        <>
          <button type="button" className="btn-secondary" onClick={actions.onBackScan}>
            ← Back
          </button>
          {actions.kit === "custom" ? (
            <button type="button" className="btn-primary" onClick={actions.onCustom}>
              Customise →
            </button>
          ) : (
            <button type="button" className="btn-primary" onClick={actions.onInstall}>
              Install {actions.kit === "recommended" ? "Recommended" : actions.kit} →
            </button>
          )}
        </>
      );
    case "custom":
      return (
        <>
          <button type="button" className="btn-secondary" onClick={actions.onBackKit}>
            ← Back
          </button>
          <button type="button" className="btn-primary" onClick={actions.onInstall}>
            Install custom →
          </button>
        </>
      );
    case "progress":
      return (
        <button type="button" className="btn-secondary" onClick={actions.onCancel}>
          Cancel safely
        </button>
      );
    case "done":
      return (
        <>
          <button type="button" className="btn-secondary" onClick={actions.onExport}>
            Export log
          </button>
          <button type="button" className="btn-secondary" onClick={actions.onCopy}>
            Copy diagnostics
          </button>
          {actions.launcherAvailable ? (
            <button type="button" className="btn-secondary" onClick={actions.onOpenLauncher}>
              Open Minecraft Launcher
            </button>
          ) : null}
          <button type="button" className="btn-primary" onClick={actions.onFinish}>
            Finish
          </button>
        </>
      );
    case "error":
      return (
        <>
          <button type="button" className="btn-secondary" onClick={actions.onExport}>
            Export log
          </button>
          <button type="button" className="btn-secondary" onClick={actions.onCopy}>
            Copy diagnostics
          </button>
          <button type="button" className="btn-secondary" onClick={actions.onFolder}>
            Choose game folder
          </button>
          <button type="button" className="btn-primary" onClick={actions.onRetry}>
            Try again
          </button>
        </>
      );
    case "manage":
      return (
        <>
          <button type="button" className="btn-secondary" onClick={actions.onBackWelcome}>
            ← Back
          </button>
          <button type="button" className="btn-secondary" onClick={actions.onSettings}>
            Settings
          </button>
          <button type="button" className="btn-primary" onClick={actions.onUpdate}>
            Update
          </button>
        </>
      );
    case "help":
    case "import":
    case "settings":
      return (
        <button type="button" className="btn-primary" onClick={actions.onHelpBack}>
          ← Back
        </button>
      );
    default:
      return assertNever(screen);
  }
}
