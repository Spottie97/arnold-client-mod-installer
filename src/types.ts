export type Screen =
  | "welcome"
  | "pack"
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
export type FileDest = "mods" | "shaderpacks" | "resourcepacks";

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
  description: string;
}

export type QualityLevel = "potato" | "lite" | "normal" | "ultra";

export interface QualityRow {
  label: string;
  description: string;
  skipComponents?: string[];
  recommendOcean?: boolean;
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
  packId?: string;
  packDir?: string | null;
  launcherRoot?: string;
  folderMode?: string;
  needsFolderChoice?: LegacyInstall | null;
  launchers?: LauncherReport;
  /** False when no selected launcher needs a vanilla jar before install. */
  requireVanilla?: boolean;
}

export interface LegacyInstall {
  root: string;
  manifestVersion?: string | null;
  kit?: string | null;
}

export type LauncherKind = "official" | "tLauncher";

export type FoundLauncher =
  | "windowsStore"
  | "macApp"
  | "linuxNative"
  | "linuxFlatpak"
  | { windowsClassic: string };

export interface TLauncherFound {
  openTarget?: string | null;
  gameDir: string;
  verified: boolean;
}

export interface LauncherReport {
  official?: FoundLauncher | null;
  officialRoot: string;
  tlauncher?: TLauncherFound | null;
  playDefault?: LauncherKind | null;
}

export interface LauncherPrefs {
  writeOfficial?: boolean | null;
  writeTlauncher?: boolean | null;
  play?: LauncherKind | null;
}

export interface PackEntry {
  id: string;
  name: string;
  minecraft: string;
  displayVersion?: string | null;
  manifestUrl: string;
  manifestSigUrl: string;
  manifestSha512?: string | null;
  art?: string | null;
  minAppVersion?: string | null;
}

export interface PackIndexLoad {
  index: {
    kind: string;
    schema: number;
    indexVersion: string;
    minAppVersion: string;
    packs: PackEntry[];
  };
  source: string;
  message?: string | null;
}

export type PackLocation = "ownFolder" | "defaultFolder";

export interface CopyReport {
  item: string;
  copied: string[];
  skipped: string[];
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
  configs: { componentId: string; file: string }[];
  loader?: { id: string; url: string; sha512: string } | null;
  quality: QualityLevel;
  kit: string;
  components: string[];
  /** Present only because an add-on toggle is on. Omitted plans treat every file as required. */
  optionalComponents?: string[];
  warnings: string[];
  bytes: number;
  packId?: string;
  packDir?: string;
}

export interface PackInstallState {
  packId: string;
  installed: InstalledState | null;
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
  packId?: string | null;
}

export type OwnerSource = "marker" | "state" | "legacy-ba-plus" | "empty";

export type FolderOwner =
  | { kind: "empty" }
  | { kind: "marked"; packId: string }
  | { kind: "inferred"; packId: string; source: OwnerSource }
  | { kind: "ambiguous"; reason: string }
  | { kind: "occupied"; files: string[] };

export interface SwitchReport {
  from: string;
  to: string;
  restored: boolean;
  flagged: string[];
  legacy: boolean;
  stashPath: string;
  partialRestore: boolean;
  conflicts: string[];
  savedCopies: string[];
}

export interface DestinationEntry {
  packId: string;
  path: string;
  mode: string;
  ownerBefore?: FolderOwner | null;
  switch?: SwitchReport | null;
  adopted: boolean;
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
  selectedPack?: string | null;
  useDefaultFolder?: boolean;
  packLocations?: Record<string, PackLocation>;
  launchers?: LauncherPrefs;
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
