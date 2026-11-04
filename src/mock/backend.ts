import fixture from "@fixtures/manifest.v3.json";
import type {
  Detection,
  ErrorPayload,
  InstallPlan,
  InstalledState,
  Manifest,
  ProgressEvent,
  QualityLevel,
  Recommendation,
  Selection,
  SnapshotMeta,
} from "../types";
import { snowLiteForQuality } from "../lib/quality";
import { formatMb } from "../lib/format";

const manifest = fixture as unknown as Manifest;

const mockHardware = {
  cpu: "AMD Ryzen 7 5800X",
  cores: 8,
  ramGb: 32,
  gpu: "NVIDIA GeForce RTX 3060",
  vramGb: 12,
  dedicatedGpu: true,
};

function kitBytes(ids: string[]): number {
  return manifest.components
    .filter((c) => ids.includes(c.id))
    .flatMap((c) => c.files)
    .reduce((sum, file) => sum + file.size, 0);
}

function resolveSelection(sel: Selection): string[] {
  const kit = sel.kit === "custom" ? null : manifest.kits[sel.kit];
  const selected = new Set(sel.kit === "custom" ? sel.customComponents : (kit?.components ?? []));
  for (const component of manifest.components) {
    if (component.requiredInAllKits) selected.add(component.id);
  }
  const quality: QualityLevel = sel.quality ?? (sel.kit === "basic" ? "lite" : "normal");
  const skip = new Set(manifest.qualityPresets?.[quality]?.skipComponents ?? []);
  const extras = sel.addons ?? {};
  for (const [addonId, addon] of Object.entries(manifest.addons ?? {})) {
    const enabled = addonId === "ocean" ? sel.ocean : extras[addonId] === true;
    if (!enabled) continue;
    for (const id of addon.components) {
      if (skip.has(id)) continue;
      const component = manifest.components.find((c) => c.id === id);
      if (component && component.kind !== "pending") selected.add(id);
    }
  }
  for (const id of skip) selected.delete(id);
  const byId = new Map(manifest.components.map((c) => [c.id, c]));
  const out: string[] = [];
  const seen = new Set<string>();
  const walk = (id: string) => {
    if (seen.has(id)) return;
    seen.add(id);
    const component = byId.get(id);
    if (!component || component.kind === "pending") return;
    for (const req of component.requires ?? []) walk(req);
    out.push(id);
  };
  for (const id of selected) walk(id);
  return out;
}

export function mockManifest(): Manifest {
  return manifest;
}

export function mockDetection(overrides: Partial<Detection> = {}): Detection {
  return {
    gameDir: "C:\\Users\\Sam\\AppData\\Roaming\\.minecraft",
    candidates: ["C:\\Users\\Sam\\AppData\\Roaming\\.minecraft"],
    minecraftVersion: manifest.minecraft,
    minecraftVersionInstalled: true,
    fabric: {
      installed: true,
      profileId: manifest.loader.id,
      willInstall: false,
    },
    gameRunning: false,
    importedFromLegacy: true,
    ...overrides,
  };
}

export function mockRecommendation(): Recommendation {
  return {
    kit: "full",
    snowLite: false,
    quality: "ultra",
    recommendOcean: true,
    reasons: [
      "Dedicated GPU with 12 GB VRAM",
      "32 GB RAM",
      "8 cores",
      "Shaders: yes",
      "Quality: ultra",
      "Sodium: yes — it raises FPS on every PC",
    ],
    hardware: mockHardware,
    warnLowRam: false,
  };
}

export function mockInstalled(): InstalledState {
  return {
    schema: 1,
    source: "Imported from 2.3.x",
    manifestVersion: "2.3.4",
    kit: "recommended",
    components: [
      { id: "base", version: "0.165.0", name: "Fabric API" },
      { id: "sodium", version: "0.9.2", name: "Sodium" },
      { id: "deck", version: "0.11.1", name: "Arnold Deck" },
      { id: "ships", version: "0.10.1", name: "Arnold Ships" },
      { id: "snow", version: "0.8.0", name: "Arnold Snow" },
    ],
    files: [],
    updatedAt: "1 November 2026 18:40",
  };
}

export function mockSnapshots(): SnapshotMeta[] {
  return [
    {
      id: "snap-20261101T204100",
      createdAt: "2026-11-01 20:41",
      label: "Before update",
      kit: "recommended",
      files: ["mods/arnold-ships-0.10.1.jar"],
    },
  ];
}

export function mockPlan(sel: Selection): InstallPlan {
  const ids = resolveSelection(sel);
  const quality: QualityLevel =
    sel.quality ?? (sel.snowLite || sel.kit === "basic" ? "lite" : "normal");
  const snowLite = snowLiteForQuality(quality);
  const add = ids.flatMap((id) => {
    const component = manifest.components.find((c) => c.id === id);
    if (!component) return [];
    return component.files.map((file) => ({
      componentId: id,
      name: component.name,
      version: component.version,
      file: file.file,
      dest: file.dest,
      url: file.url,
      sha512: file.sha512,
      size: file.size,
    }));
  });
  const warnings: string[] = [];
  const have = manifest.currentServerShipwright;
  if (have) {
    for (const id of ids) {
      const component = manifest.components.find((c) => c.id === id);
      if (component?.minServerShipwright && have < component.minServerShipwright) {
        warnings.push(
          `${component.name} ${component.version} needs server Shipwright ${component.minServerShipwright}; the server is on ${have} until its next update. It will still install, but decks may misbehave until then.`,
        );
      }
    }
  }
  return {
    add,
    update: [
      {
        componentId: "ships",
        name: "Arnold Ships",
        version: "0.10.3",
        file: "arnold-ships-client-0.10.3.jar",
        dest: "mods",
        url: add.find((f) => f.componentId === "ships")?.url ?? "",
        sha512: add.find((f) => f.componentId === "ships")?.sha512 ?? "",
        size: add.find((f) => f.componentId === "ships")?.size ?? 0,
      },
    ],
    remove: [],
    configs: ids.includes("snow")
      ? [{ componentId: "snow", file: "config/arnold-snow-client.properties", snowLite }]
      : [],
    loader: null,
    snowLite,
    quality,
    kit: sel.kit,
    components: ids,
    warnings,
    bytes: kitBytes(ids),
  };
}

export function missingMinecraftError(gameDir: string): ErrorPayload {
  return {
    code: "E-MC-MISSING",
    title: "Minecraft 26.3 isn't installed yet",
    hint: "Open the Minecraft Launcher, press Play once on Latest release (26.3), close the game, then press Try again.",
    detail: `detected: versions/26.3/26.3.jar not found in ${gameDir}\nmanifest 2026.11.04-2 (signature ok) · app 3.0.0 · Windows 11 23H2`,
  };
}

export function kitBlurb(kit: string, rec: Recommendation): string {
  switch (kit) {
    case "recommended":
      return `Full kit: ${rec.hardware.gpu} · ${rec.hardware.vramGb} GB VRAM · ${rec.hardware.ramGb} GB RAM · ${rec.hardware.cores} cores`;
    case "basic":
      return "Ships, smooth decks, lighter snow, Sodium, and client performance mods. Runs on almost any PC.";
    case "full":
      return "Everything: Iris + Complementary shaders, client performance mods, full snow, ships.";
    case "custom":
      return "Pick each mod yourself. Requirements are added for you.";
    default:
      return "";
  }
}

export function kitSizeLabel(
  kit: string,
  ocean: boolean,
  addons: Record<string, boolean> = {},
): string {
  const sel: Selection = {
    kit,
    customComponents: [],
    ocean,
    addons,
    snowLite: kit === "basic",
    quality: kit === "basic" ? "lite" : "normal",
  };
  if (kit === "custom") return "—";
  return `${formatMb(mockPlan(sel).bytes)}`;
}

export async function playMockInstall(
  plan: InstallPlan,
  onEvent: (event: ProgressEvent) => void,
  signal: { cancelled: boolean },
): Promise<"done" | "cancelled"> {
  const rows = [
    { message: "Backup saved (restore point 2026-11-01 20:41)", ms: 400 },
    { message: "Fabric loader 0.20.1 + Fabric API 0.165.0 kept", ms: 350 },
    { message: "Sodium 0.9.2 SHA-512 verified", ms: 400 },
    { message: "Arnold Ships client 0.10.3 SHA-512 verified · replaced 0.10.2", ms: 450 },
    { message: "Iris 1.11.7 — verifying SHA-512…", ms: 700 },
    { message: "Complementary Reimagined r5.9.3 — 41%", ms: 800 },
    { message: "Wakes 0.7.3 · Particle Rain 4.0.2 · Snow config", ms: 500 },
  ];
  for (let i = 0; i < rows.length; i += 1) {
    if (signal.cancelled) return "cancelled";
    onEvent({
      runId: "mock",
      step: "mods",
      status: "running",
      percent: Math.round(((i + 1) / rows.length) * 100),
      message: rows[i].message,
    });
    await new Promise((resolve) => setTimeout(resolve, rows[i].ms));
  }
  const _plan = plan;
  void _plan;
  return "done";
}
