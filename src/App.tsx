import { useEffect, useMemo, useState } from "react";
import { AppShell } from "./components/AppShell";
import { detect, getManifest, listInstalled, listSnapshots, plan, scanHardware, setGameDir } from "./lib/api";
import { missingMinecraftError, playMockInstall } from "./mock/backend";
import { Custom } from "./screens/Custom";
import { Done } from "./screens/Done";
import { ErrorScreen } from "./screens/ErrorScreen";
import { Kit } from "./screens/Kit";
import { MacHelp } from "./screens/MacHelp";
import { Manage } from "./screens/Manage";
import { Progress } from "./screens/Progress";
import { Scan } from "./screens/Scan";
import { Welcome } from "./screens/Welcome";
import type {
  Detection,
  ErrorPayload,
  InstallPlan,
  InstalledState,
  Manifest,
  ProgressEvent,
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
    kit: "recommended",
    components: [],
    warnings: [],
    bytes: 0,
  };
}

export default function App() {
  const [screen, setScreen] = useState<Screen>(screenFromHash);
  const [manifest, setManifest] = useState<Manifest | null>(null);
  const [detection, setDetection] = useState<Detection | null>(null);
  const [recommendation, setRecommendation] = useState<Recommendation | null>(null);
  const [installed, setInstalled] = useState<InstalledState | null>(null);
  const [snapshots, setSnapshots] = useState<SnapshotMeta[]>([]);
  const [kit, setKit] = useState("recommended");
  const [ocean, setOcean] = useState(true);
  const [customIds, setCustomIds] = useState<string[]>(["deck", "ships", "snow", "sodium", "base"]);
  const [snowLite, setSnowLite] = useState(false);
  const [currentPlan, setCurrentPlan] = useState<InstallPlan>(emptyPlan());
  const [progress, setProgress] = useState<ProgressEvent | null>(null);
  const [error, setError] = useState<ErrorPayload | null>(null);
  const [cancelFlag] = useState({ cancelled: false });

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
    let alive = true;
    void (async () => {
      const [m, d, r, inst, snaps] = await Promise.all([
        getManifest(),
        detect(),
        scanHardware(),
        listInstalled(),
        listSnapshots(),
      ]);
      if (!alive) return;
      setManifest(m);
      setDetection(d);
      setRecommendation(r);
      setInstalled(inst);
      setSnapshots(snaps);
      setKit(r.kit === "full" ? "recommended" : r.kit);
      setSnowLite(r.snowLite);
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
    }),
    [kit, customIds, ocean, snowLite],
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

  const startInstall = async () => {
    if (detection && !detection.minecraftVersionInstalled) {
      setError(missingMinecraftError(detection.gameDir ?? "(no game folder)"));
      go("error");
      return;
    }
    cancelFlag.cancelled = false;
    setProgress({
      runId: "mock",
      step: "mods",
      status: "running",
      percent: 12,
      message: "Backup saved (restore point 2026-11-01 20:41)",
    });
    go("progress");
    const result = await playMockInstall(currentPlan, setProgress, cancelFlag);
    if (result === "cancelled") {
      go("kit");
      return;
    }
    go("done");
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
      cancelFlag.cancelled = true;
      go("kit");
    },
    onExport: () => {
      const blob = new Blob(
        [JSON.stringify({ error, plan: currentPlan, detection }, null, 2)],
        { type: "application/json" },
      );
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = "arnold-install-log.json";
      a.click();
      URL.revokeObjectURL(url);
    },
    onRetry: () => go("welcome"),
    onFolder: () => void chooseFolder(),
    onDoneHome: () => go("welcome"),
    onHelpBack: () => go("welcome"),
  });

  if (!manifest || !detection || !recommendation) {
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
          onHelp={() => go("help")}
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
          ocean={ocean}
          recommendation={recommendation}
          plan={currentPlan}
          onSelect={(next) => {
            setKit(next);
            if (next === "custom") go("custom");
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
      body = <Progress plan={currentPlan} event={progress} />;
      break;
    case "done":
      body = <Done plan={currentPlan} />;
      break;
    case "error":
      body = (
        <ErrorScreen
          error={
            error ??
            missingMinecraftError(detection.gameDir ?? "C:\\Users\\Sam\\AppData\\Roaming\\.minecraft")
          }
        />
      );
      break;
    case "manage":
      body = <Manage installed={installed} snapshots={snapshots} />;
      break;
    case "help":
      body = <MacHelp />;
      break;
    default:
      body = assertNever(screen);
  }

  return (
    <AppShell screen={screen} footer={footer}>
      {body}
    </AppShell>
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
    onRetry: () => void;
    onFolder: () => void;
    onDoneHome: () => void;
    onHelpBack: () => void;
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
          <button type="button" className="btn-primary" onClick={actions.onDoneHome}>
            Open Minecraft Launcher
          </button>
        </>
      );
    case "error":
      return (
        <>
          <button type="button" className="btn-secondary" onClick={actions.onExport}>
            Export log
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
          <button type="button" className="btn-primary" onClick={actions.onUpdate}>
            Update
          </button>
        </>
      );
    case "help":
      return (
        <button type="button" className="btn-primary" onClick={actions.onHelpBack}>
          ← Back
        </button>
      );
    default:
      return assertNever(screen);
  }
}
