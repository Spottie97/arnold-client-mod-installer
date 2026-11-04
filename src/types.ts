export type Screen =
  | "welcome"
  | "scan"
  | "kit"
  | "custom"
  | "progress"
  | "done"
  | "error"
  | "manage"
  | "help"
  | "import"
  | "settings";

export type ComponentGroup = "base" | "shaders" | "arnold" | "ocean";
export type ComponentKind = "base" | "download" | "pending";
export type FileDest = "mods" | "shaderpacks";

export interface ComponentFile {
  file: string;
  dest: FileDest;
  match: string;
  url: string;
  sha512: string;
  size: number;
}

export interface SettingSpec {
  type: "bool" | "int" | "number" | "string";
  default: unknown;
  label?: string;
  description?: string;
}

export interface ModConfig {
  file: string;
  format: string;
  header?: string;
  settings: Record<string, SettingSpec>;
  presets?: Record<string, { marker?: string; overrides: Record<string, unknown> }>;
}

export interface Component {
  id: string;
  group: ComponentGroup;
  kind: ComponentKind;
  required?: boolean;
  requires?: string[];
  name: string;
  version: string;
  description: string;
  requiredInAllKits?: boolean;
  minServerShipwright?: string;
  config?: ModConfig;
  jvmFlags?: { flag: string; purpose: string }[];
  pending?: { source: string; project?: string; page?: string };
  legacyMatch?: string[];
  knownSha512?: string[];
  files: ComponentFile[];
}

export interface Kit {
  name: string;
  components: string[];
  snowLite?: boolean;
  description: string;
}

export type QualityLevel = "potato" | "lite" | "normal" | "ultra";

export interface QualityRow {
  label: string;
  description: string;
  skipComponents?: string[];
  recommendOcean?: boolean;
  snow?: { quality: string; fpsTarget?: number; adaptiveFps?: boolean; legacyLite?: boolean };
  ships?: { particles: number; spray?: boolean; wake?: boolean; hdModels?: boolean };
  sodium?: {
    weatherQuality: string;
    leavesQuality: string;
    renderDistance?: number;
    cloudStatus?: string;
  };
  iris?: { enableShaders: boolean };
  complementary?: { profile: string };
}

export interface QualityTable {
  potato: QualityRow;
  lite: QualityRow;
  normal: QualityRow;
  ultra: QualityRow;
}

export interface Manifest {
  schema: number;
  manifestVersion: string;
  minAppVersion: string;
  minecraft: string;
  currentServerShipwright?: string;
  loader: { id: string; url: string; sha512: string };
  components: Component[];
  kits: Record<string, Kit>;
  addons?: Record<
    string,
    { name: string; components: string[]; defaultOn?: Record<string, boolean> }
  >;
  qualityPresets?: QualityTable;
}

export interface ManifestLoad {
  manifest: Manifest;
  source: "remote" | "cache" | "bundled" | "local" | "error" | string;
  offline: boolean;
  message?: string | null;
  localSource?: boolean;
  localSourceDir?: string | null;
}

export interface Detection {
  gameDir?: string | null;
  candidates: string[];
  minecraftVersion: string;
  minecraftVersionInstalled: boolean;
  fabric: { installed: boolean; profileId?: string | null; willInstall: boolean };
  gameRunning: boolean;
  importedFromLegacy: boolean;
}

export interface Hardware {
  cpu: string;
  cores: number;
  ramGb: number;
  gpu: string;
  vramGb: number;
  dedicatedGpu: boolean;
}

export interface Recommendation {
  kit: string;
  snowLite: boolean;
  quality: QualityLevel;
  recommendOcean: boolean;
  reasons: string[];
  hardware: Hardware;
  warnLowRam: boolean;
}

export interface Selection {
  kit: string;
  customComponents: string[];
  ocean: boolean;
  /** Optional add-ons other than Ocean. A missing id stays off. */
  addons?: Record<string, boolean>;
  snowLite: boolean | null;
  quality: QualityLevel | null;
}

export interface PlanFile {
  componentId: string;
  name: string;
  version: string;
  file: string;
  dest: FileDest;
  url: string;
  sha512: string;
  size: number;
}

export interface InstallPlan {
  add: PlanFile[];
  update: PlanFile[];
  remove: PlanFile[];
  configs: { componentId: string; file: string; snowLite: boolean }[];
  loader?: { id: string; url: string; sha512: string } | null;
  snowLite: boolean;
  quality: QualityLevel;
  kit: string;
  components: string[];
  warnings: string[];
  bytes: number;
}

export interface InstalledState {
  schema: number;
  source: string;
  manifestVersion: string;
  kit?: string | null;
  quality?: QualityLevel | null;
  components: { id: string; version: string; name: string }[];
  files: { path: string; sha512: string; componentId: string }[];
  updatedAt: string;
}

export interface SnapshotMeta {
  id: string;
  createdAt: string;
  label: string;
  kit?: string | null;
  files: string[];
}

export interface ErrorPayload {
  code: string;
  title: string;
  hint: string;
  detail: string;
}

export interface ProgressEvent {
  runId: string;
  step: string;
  file?: string;
  status: "pending" | "running" | "done" | "error" | "cancelled" | string;
  percent: number;
  message: string;
}

export interface ImportJar {
  path: string;
  fileName: string;
  componentId?: string | null;
  sha512: string;
  knownBuild: boolean;
  tracked: boolean;
}

export interface ImportOffer {
  found: boolean;
  versionGuess?: string | null;
  jars: ImportJar[];
  backups: { path: string; kind: string }[];
  alreadyAdopted: boolean;
}

export interface Prefs {
  lastGameDir?: string | null;
  tourDone: boolean;
}

export interface UpdateBadge {
  id: string;
  name: string;
  installed: string;
  available: string;
}

export function assertNever(value: never): never {
  throw new Error(`unexpected value: ${String(value)}`);
}
