import { useEffect, useMemo, useRef, useState } from "react";
import { AppShell } from "./components/AppShell";
import { Tour } from "./components/Tour";
import {
  asErrorPayload,
  beginInstallRequest,
  cancelInstall,
  endInstallRequest,
  confirmImport,
  copyDiagnostics,
  copyFromOldFolder,
  detect,
  detectLegacy,
  onFolderSwitched,
  exportLog,
  finishApp,
  getLocalSource,
  getManifest,
  getPrefs,
  listInstalled,
  listPacks,
  listPackStates,
  listSnapshots,
  listenProgress,
  pickLocalSource,
  play,
  revealPath,
  plan,
  repairInstall,
  rollback,
  scanHardware,
  setLauncherPrefs,
  setQuality,
  setTourDone,
} from "./lib/api";
import { addonRows, freshAddonFlags, savedAddonFlags } from "./lib/addons";
import { pickBanner, type LocalSourceInfo } from "./lib/localSource";
import { createDestinationFlow } from "./lib/destinationFlow";
import { folderSwitchNotice } from "./lib/folderSwitch";
import { minecraftBlocksInstall, planMatchesDestination } from "./lib/installGate";
import { indexPackStates } from "./lib/libraryStatus";
import { playPack } from "./lib/playPack";
import { mergeProgress } from "./lib/progress";
import { parseQuality, qualityLabel } from "./lib/quality";
import { packSubtitle } from "./lib/version";
import { TOUR_TIPS } from "./lib/tour";
import { updateBadges } from "./lib/updates";
import { missingMinecraftError } from "./mock/backend";
import { Custom } from "./screens/Custom";
import { Done } from "./screens/Done";
import { Library } from "./screens/Library";
import { PackDetails } from "./screens/PackDetails";
import { ErrorScreen } from "./screens/ErrorScreen";
import { ImportScreen } from "./screens/Import";
import { Kit } from "./screens/Kit";
import { MacHelp } from "./screens/MacHelp";
import { Manage } from "./screens/Manage";
import { Progress } from "./screens/Progress";
import { Scan } from "./screens/Scan";
import { Settings } from "./screens/Settings";
import type {
  Detection,
  ErrorPayload,
  ImportOffer,
  InstallPlan,
  InstalledState,
  LauncherKind,
  Manifest,
  PackIndexLoad,
  Prefs,
  ProgressEvent,
  QualityLevel,
  Recommendation,
  Screen,
  SnapshotMeta,
} from "./types";
import { assertNever } from "./types";

const SCREENS: Screen[] = [
  "welcome",
  "pack",
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
    quality: "normal",
    kit: "recommended",
    components: [],
    optionalComponents: [],
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
  const [addonFlags, setAddonFlags] = useState<Record<string, boolean>>({});
  const [customIds, setCustomIds] = useState<string[]>(["deck", "ships", "sodium", "base"]);
  const [quality, setQualityLevel] = useState<QualityLevel>("normal");
  const [currentPlan, setCurrentPlan] = useState<InstallPlan>(emptyPlan());
  const [installReady, setInstallReady] = useState(false);
  const [installedByPack, setInstalledByPack] = useState<Record<string, InstalledState | null>>({});
  const [progress, setProgress] = useState<ProgressEvent | null>(null);
  const [progressLog, setProgressLog] = useState<ProgressEvent[]>([]);
  const [error, setError] = useState<ErrorPayload | null>(null);
  const [tourStep, setTourStep] = useState<number | null>(null);
  const [importOffer, setImportOffer] = useState<ImportOffer | null>(null);
  const [importRetry, setImportRetry] = useState<(() => Promise<void>) | null>(null);
  const [replaceCollisions, setReplaceCollisions] = useState(false);
  const [library, setLibrary] = useState<PackIndexLoad | null>(null);
  const [prefs, setPrefs] = useState<Prefs>({ tourDone: false, useDefaultFolder: false });
  const [logPath, setLogPath] = useState<string | null>(null);
  const [logError, setLogError] = useState<string | null>(null);
  const [folderNotice, setFolderNotice] = useState<string | null>(null);

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
    let alive = true;
    void (async () => {
      try {
        const packs = await listPacks().catch(() => null);
        const [load, d, r, inst, snaps, storedPrefs, legacy, local] = await Promise.all([
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
        if (packs) setLibrary(packs);
        setPrefs(storedPrefs);
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
        setOcean(r.recommendOcean);
        setAddonFlags(
          freshAddonFlags(
            load.manifest,
            savedAddonFlags(load.manifest, inst ? inst.components.map((item) => item.id) : null),
          ),
        );
        if (!storedPrefs.tourDone) setTourStep(0);
        if (d.needsFolderChoice) {
          go("pack");
        } else if (
          legacy?.found &&
          !legacy.alreadyAdopted &&
          !inst &&
          d.folderMode === "default"
        ) {
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
      addons: addonFlags,
      quality,
    }),
    [kit, customIds, ocean, addonFlags, quality],
  );

  const installContext = useMemo(
    () =>
      JSON.stringify({
        pack: prefs.selectedPack ?? null,
        useDefault: Boolean(prefs.useDefaultFolder),
        locations: prefs.packLocations ?? {},
        lastGameDir: prefs.lastGameDir ?? null,
        writeOfficial: prefs.launchers?.writeOfficial ?? null,
        writeTlauncher: prefs.launchers?.writeTlauncher ?? null,
      }),
    [prefs],
  );
  const requestKey = `${installContext}|${JSON.stringify(selection)}`;
  const [loadedKey, setLoadedKey] = useState("");
  const [reloadToken, setReloadToken] = useState(0);
  const loadGeneration = useRef(0);
  const seenContext = useRef(installContext);
  const installAllowed = installReady && loadedKey === requestKey;

  useEffect(() => {
    const generation = ++loadGeneration.current;
    const key = requestKey;
    const contextChanged = seenContext.current !== installContext;
    seenContext.current = installContext;
    setInstallReady(false);
    if (contextChanged) {
      setCurrentPlan(emptyPlan());
      setInstalled(null);
      setInstalledByPack({});
      setSnapshots([]);
    }
    void (async () => {
      try {
        const [load, freshDetection, inst, snaps, states] = await Promise.all([
          getManifest(),
          detect(),
          listInstalled(),
          listSnapshots(),
          listPackStates(),
        ]);
        if (generation !== loadGeneration.current) return;
        setManifest(load.manifest);
        setOfflineBanner(
          load.offline ? load.message ?? "Using the last good mod list (offline)." : null,
        );
        setDetection(freshDetection);
        setInstalled(inst);
        setSnapshots(snaps);
        setInstalledByPack(indexPackStates(states));
        const next = await plan(selection);
        if (generation !== loadGeneration.current) return;
        setCurrentPlan(next);
        const matches = planMatchesDestination(
          next,
          freshDetection,
          prefs.selectedPack ?? freshDetection.packId,
        );
        setInstallReady(matches);
        setLoadedKey(matches ? key : "");
      } catch (err) {
        if (generation !== loadGeneration.current) return;
        setInstallReady(false);
        setLoadedKey("");
        setError(asErrorPayload(err));
      }
    })();
  }, [requestKey, installContext, selection, prefs.selectedPack, reloadToken]);

  useEffect(() => {
    let stop: (() => void) | undefined;
    let alive = true;
    void onFolderSwitched((entry) => {
      if (!alive) return;
      const notice = folderSwitchNotice(entry, (id) => {
        if (id === "better-adventures-plus") return "Better Adventures+";
        return library?.index.packs.find((pack) => pack.id === id)?.name ?? id;
      });
      if (notice) setFolderNotice(notice);
      void listPackStates().then((states) => {
        if (alive) setInstalledByPack(indexPackStates(states));
      });
    }).then((unlisten) => {
      if (!alive) {
        unlisten();
        return;
      }
      stop = unlisten;
    });
    return () => {
      alive = false;
      stop?.();
    };
  }, [library]);

  const badges = useMemo(
    () => (manifest ? updateBadges(manifest, installed) : []),
    [manifest, installed],
  );

  const invalidateInstall = () => {
    loadGeneration.current += 1;
    setInstallReady(false);
    setLoadedKey("");
    setCurrentPlan(emptyPlan());
    setReloadToken((token) => token + 1);
  };

  const flow = createDestinationFlow({
    invalidate: invalidateInstall,
    detect: () => detect(),
    replan: async () => {
      loadGeneration.current += 1;
      setInstallReady(false);
      setLoadedKey("");
      const next = await plan(selection);
      setCurrentPlan(next);
      return next;
    },
  });

  const routeOccupied = async (err: unknown, retry: () => Promise<void>): Promise<boolean> => {
    const offer = await flow.occupiedImportOffer(err, prefs.selectedPack ?? detection?.packId);
    if (!offer) return false;
    setImportOffer(offer);
    setImportRetry(() => retry);
    go("import");
    return true;
  };

  const startInstall = async (nextPlan = currentPlan, replace = replaceCollisions) => {
    if (detection?.needsFolderChoice) {
      go("pack");
      return;
    }
    if (detection && minecraftBlocksInstall(detection)) {
      setError(missingMinecraftError(detection.gameDir ?? "(no game folder)"));
      go("error");
      return;
    }
    let runId: string | null;
    try {
      runId = await flow.install(nextPlan, detection, prefs.selectedPack, replace, {
        onStart: () => {
          setProgressLog([]);
          setProgress({
            runId: "",
            step: "prepare",
            status: "running",
            percent: 1,
            message: "Starting install",
          });
          go("progress");
        },
        onProgress: (event) => {
          setProgress(event);
          setProgressLog((rows) => mergeProgress(rows, event));
        },
      });
    } catch (err) {
      const payload = asErrorPayload(err);
      if (payload.code === "E-CANCELLED") {
        go("kit");
        return;
      }
      if (
        await routeOccupied(err, async () => {
          invalidateInstall();
          go("kit");
        })
      ) {
        return;
      }
      setError(payload);
      setReplaceCollisions(payload.code === "P-COLLISION");
      go("error");
      return;
    }
    if (runId === null) {
      go("kit");
      return;
    }
    try {
      setProgress((current) =>
        current
          ? { ...current, percent: 100, status: "done" }
          : {
              runId,
              step: "done",
              status: "done",
              percent: 100,
              message: "Install finished",
            },
      );
      const [inst, snaps, states] = await Promise.all([
        listInstalled(),
        listSnapshots(),
        listPackStates(),
      ]);
      setInstalled(inst);
      setSnapshots(snaps);
      setInstalledByPack(indexPackStates(states));
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
    }
  };

  const startRepair = async () => {
    const requestId = beginInstallRequest();
    setProgressLog([]);
    go("progress");
    try {
      const stop = await listenProgress((event) => {
        setProgress(event);
        setProgressLog((rows) => mergeProgress(rows, event));
      });
      try {
        await repairInstall(requestId);
        go("done");
      } catch (err) {
        setError(asErrorPayload(err));
        go("error");
      } finally {
        stop();
      }
    } finally {
      endInstallRequest(requestId);
    }
  };

  const chooseFolder = async () => {
    const path = window.prompt("Game folder", detection?.gameDir ?? "") ?? "";
    if (!path) return;
    try {
      const { detection: next } = await flow.setGameDir(path);
      setDetection(next);
      setPrefs(await getPrefs());
      if (minecraftBlocksInstall(next)) {
        setError(missingMinecraftError(next.gameDir ?? path));
        go("error");
      }
    } catch (err) {
      setError(asErrorPayload(err));
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
    onCustom: () => go("pack"),
    installReady: installAllowed,
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
    launcherAvailable: detection ? playOpenable(detection, prefs) : false,
    playLabel: detection ? playLabelFor(detection, prefs) : "Minecraft Launcher",
    qualityName: qualityLabel(quality),
    onOpenLauncher: () => {
      void play();
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

  const rows = addonRows(manifest, ocean, addonFlags);
  const onAddon = (id: string, on: boolean) => {
    if (id === "ocean") {
      setOcean(on);
      return;
    }
    setAddonFlags((current) => ({ ...current, [id]: on }));
  };

  let body: JSX.Element;
  switch (screen) {
    case "welcome":
      body = (
        <Library
          library={library}
          detection={detection}
          installedByPack={installedByPack}
          localSource={localSource}
          onOpen={(id) => {
            void (async () => {
              try {
                const { result, detection: next } = await flow.selectPack(id);
                setPrefs(result);
                setDetection(next);
                go("pack");
              } catch (err) {
                setError(asErrorPayload(err));
                go("error");
              }
            })();
          }}
          onInstall={(id) => {
            void (async () => {
              try {
                const { result, detection: fresh } = await flow.selectPack(id);
                setPrefs(result);
                setDetection(fresh);
                go(fresh.needsFolderChoice ? "pack" : "scan");
              } catch (err) {
                setError(asErrorPayload(err));
                go("error");
              }
            })();
          }}
          onPlay={(id) => {
            void (async () => {
              try {
                const next = await playPack(id, {
                  selectPack: (packId) => flow.selectPack(packId).then((opened) => opened.result),
                  play,
                });
                setPrefs(next);
              } catch (err) {
                setError(asErrorPayload(err));
                go("error");
              }
            })();
          }}
          onHelp={() => go("help")}
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
    case "pack":
      body = (
        <PackDetails
          pack={
            library?.index.packs.find((item) => item.id === (prefs.selectedPack ?? detection.packId)) ??
            library?.index.packs[0] ?? {
              id: "better-adventures-plus",
              name: "Better Adventures+",
              minecraft: "26.3",
              displayVersion: manifest.manifestVersion,
              manifestUrl: "",
              manifestSigUrl: "",
              art: "matcha-flavoured",
            }
          }
          detection={detection}
          installed={installed}
          canCopy={Boolean(prefs.lastGameDir) && detection.folderMode === "own"}
          onChoose={(choice) => {
            const apply = () =>
              flow.chooseFolder(choice).then(({ result, detection: next }) => {
                setPrefs(result);
                setDetection(next);
              });
            void apply().catch(async (err: unknown) => {
              if (await routeOccupied(err, apply)) return;
              setError(asErrorPayload(err));
              go("error");
            });
          }}
          onCopy={(item) => copyFromOldFolder(item)}
        />
      );
      break;
    case "scan":
      body = (
        <Scan
          recommendation={recommendation}
          detection={detection}
          quality={quality}
          onQuality={setQualityLevel}
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
          addons={rows}
          recommendation={recommendation}
          plan={currentPlan}
          table={manifest.qualityPresets}
          onSelect={setKit}
          onQuality={setQualityLevel}
          onAddon={onAddon}
          onOptions={() => {
            setKit(kit === "custom" ? "custom" : kit);
            go("custom");
          }}
        />
      );
      break;
    case "custom":
      body = (
        <Custom
          manifest={manifest}
          selected={currentPlan.components}
          plan={currentPlan}
          quality={quality}
          addons={rows}
          onToggle={(id) => {
            setCustomIds((ids) =>
              ids.includes(id) ? ids.filter((item) => item !== id) : [...ids, id],
            );
            setKit("custom");
          }}
          onAddon={onAddon}
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
          detection={detection}
          packName={
            library?.index.packs.find((item) => item.id === detection.packId)?.name ??
            "Better Adventures+"
          }
          playLabel={playLabelFor(detection, prefs)}
          profileNote={
            prefs.packLocations?.[detection.packId ?? ""] === "ownFolder" && prefs.lastGameDir
              ? "The previous profile is still in the launcher."
              : null
          }
          logPath={logPath}
          logError={logError}
          skipped={progressLog.filter((entry) => entry.status === "skipped")}
          onOpenFolder={() => {
            if (detection.packDir) void revealPath(detection.packDir);
          }}
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
            missingMinecraftError(detection.gameDir ?? "(no game folder)")
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
            void (async () => {
              try {
                const retry = importRetry;
                setImportRetry(null);
                if (retry) {
                  await flow.adoptThen(retry);
                } else {
                  setInstalled(await confirmImport());
                  go("welcome");
                }
                setInstalled(await listInstalled());
              } catch (err) {
                setError(asErrorPayload(err));
                go("error");
              }
            })();
          }}
          onSkip={() => {
            setImportRetry(null);
            go("welcome");
          }}
        />
      );
      break;
    case "settings":
      body = (
        <Settings
          manifest={manifest}
          quality={quality}
          recommended={recommendation.quality}
          detection={detection}
          prefs={prefs}
          snapshots={snapshots}
          onUseDefault={(enabled) => {
            const apply = () =>
              flow.useDefaultFolder(enabled).then(({ result, detection: next }) => {
                setPrefs(result);
                setDetection(next);
              });
            void apply().catch(async (err: unknown) => {
              if (await routeOccupied(err, apply)) return;
              setError(asErrorPayload(err));
              go("error");
            });
          }}
          onChooseFolder={(choice) => {
            const apply = () =>
              flow.chooseFolder(choice).then(({ result, detection: next }) => {
                setPrefs(result);
                setDetection(next);
              });
            void apply().catch(async (err: unknown) => {
              if (await routeOccupied(err, apply)) return;
              setError(asErrorPayload(err));
              go("error");
            });
          }}
          onPickGameDir={() => void chooseFolder()}
          onLauncherWrite={(which, enabled) => {
            invalidateInstall();
            const next = {
              ...prefs.launchers,
              writeOfficial: which === "official" ? enabled : prefs.launchers?.writeOfficial,
              writeTlauncher: which === "tLauncher" ? enabled : prefs.launchers?.writeTlauncher,
            };
            void setLauncherPrefs(next).then(setPrefs);
          }}
          onPlayTarget={(kind: LauncherKind) => {
            void setLauncherPrefs({ ...prefs.launchers, play: kind }).then(setPrefs);
          }}
          onRestore={(id) => {
            void rollback(id).then(async (state) => {
              setInstalled(state);
              setSnapshots(await listSnapshots());
            });
          }}
          onOpenFolder={() => {
            if (detection.packDir) void revealPath(detection.packDir);
          }}
          onQuality={(next) => {
            setQualityLevel(next);
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
        banner={folderNotice ?? pickBanner(localSource, offlineBanner)}
        bannerDetail={localSource?.enabled && localSource.dir ? localSource.dir : null}
        subtitle={screenSubtitle(screen, quality, kit, manifest.manifestVersion)}
      >
        {folderNotice ? (
          <div className="mb-3 flex justify-end">
            <button type="button" className="btn-secondary" onClick={() => setFolderNotice(null)}>
              Dismiss
            </button>
          </div>
        ) : null}
        {body}
      </AppShell>
    </>
  );
}

function playOpenable(detection: Detection, prefs: Prefs): boolean {
  const official = Boolean(detection.launchers?.official);
  const tlauncher = Boolean(detection.launchers?.tlauncher?.openTarget);
  if (prefs.launchers?.play === "tLauncher") return tlauncher;
  if (prefs.launchers?.play === "official") return official;
  return official || tlauncher;
}

function playLabelFor(detection: Detection, prefs: Prefs): string {
  const official = Boolean(detection.launchers?.official);
  const tlauncher = Boolean(detection.launchers?.tlauncher?.openTarget);
  if (prefs.launchers?.play === "tLauncher" && tlauncher) return "TLauncher";
  if (official) return "Minecraft Launcher";
  if (tlauncher) return "TLauncher";
  return "Minecraft Launcher";
}

function kitTitle(kit: string): string {
  if (kit === "basic") return "Basic";
  if (kit === "full") return "Full";
  if (kit === "recommended") return "Recommended";
  return "this setup";
}

function screenSubtitle(
  screen: Screen,
  quality: QualityLevel,
  kit: string,
  manifestVersion: string,
): string {
  const pack = "Better Adventures+";
  const version = manifestVersion || "2026.11.7";
  if (screen === "welcome" || screen === "settings" || screen === "manage" || screen === "help" || screen === "import") {
    return packSubtitle("For the Arnold server", "Minecraft 26.3");
  }
  if (screen === "kit") return packSubtitle(pack, `${qualityLabel(quality)} quality`);
  if (screen === "custom") return packSubtitle(pack, `${kitTitle(kit)} · ${qualityLabel(quality)} quality`);
  if (screen === "progress" || screen === "error") return packSubtitle(pack, kitTitle(kit));
  return packSubtitle(pack, version);
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
    installReady: boolean;
    onInstall: () => void;
    onCancel: () => void;
    onExport: () => void;
    onCopy: () => void;
    onRetry: () => void;
    onFolder: () => void;
    launcherAvailable: boolean;
    playLabel: string;
    qualityName: string;
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
          <button type="button" className="btn-secondary" onClick={actions.onSettings}>
            Settings
          </button>
          {actions.already && actions.launcherAvailable ? (
            <button type="button" className="btn-secondary" onClick={actions.onOpenLauncher}>
              Play
            </button>
          ) : null}
          <button type="button" className="btn-primary" onClick={actions.onCustom}>
            Open Better Adventures+
          </button>
        </>
      );
    case "pack":
      return (
        <>
          <button type="button" className="btn-secondary" onClick={actions.onBackWelcome}>
            All packs
          </button>
          <button type="button" className="btn-primary" onClick={actions.onUpdate}>
            Continue to This PC
          </button>
        </>
      );
    case "scan":
      return (
        <>
          <button type="button" className="btn-secondary" onClick={() => actions.onCustom()}>
            Pack details
          </button>
          <button type="button" className="btn-primary" onClick={actions.onScanNext}>
            Use {actions.qualityName} quality
          </button>
        </>
      );
    case "kit":
      return (
        <>
          <button type="button" className="btn-secondary" onClick={actions.onBackScan}>
            This PC
          </button>
          <button
            type="button"
            className="btn-primary"
            onClick={actions.onInstall}
            disabled={!actions.installReady}
          >
            Install {kitTitle(actions.kit)}
          </button>
        </>
      );
    case "custom":
      return (
        <>
          <button type="button" className="btn-secondary" onClick={actions.onBackKit}>
            Kit
          </button>
          <button
            type="button"
            className="btn-primary"
            onClick={actions.onInstall}
            disabled={!actions.installReady}
          >
            Install this setup
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
          <button type="button" className="btn-secondary" onClick={actions.onSettings}>
            Settings
          </button>
          <button type="button" className="btn-secondary" onClick={actions.onBackWelcome}>
            All packs
          </button>
          <button type="button" className="btn-secondary" onClick={actions.onFinish}>
            Finish
          </button>
          {actions.launcherAvailable ? (
            <button type="button" className="btn-primary" onClick={actions.onOpenLauncher}>
              Play in {actions.playLabel}
            </button>
          ) : null}
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
