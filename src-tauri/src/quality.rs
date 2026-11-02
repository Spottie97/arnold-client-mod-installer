use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::backup::create_snapshot;
use crate::config::{
    apply_properties, format_setting_value, update_property_keys, ConfigSwitch, LITE_MARKER,
    MANAGED_HEADER,
};
use crate::download::sha512_hex;
use crate::errors::AppError;
use crate::manifest::{component_by_id, lite_preset, FileDest, Manifest};
use crate::scan::Hardware;
use crate::state::{hash_file, record_file, save_state, InstalledState};

/// Installer-wide quality. Kit chooses which mods are present; Quality chooses how hard they run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QualityLevel {
    Potato,
    Lite,
    Normal,
    Ultra,
}

impl QualityLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Potato => "potato",
            Self::Lite => "lite",
            Self::Normal => "normal",
            Self::Ultra => "ultra",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "potato" => Some(Self::Potato),
            "lite" | "low" => Some(Self::Lite),
            "normal" => Some(Self::Normal),
            "ultra" | "high" => Some(Self::Ultra),
            _ => None,
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Potato => {
                "Fastest. Shaders stay off. Sodium uses FAST leaves and weather, a short view distance, and simple clouds. Ships hide spray and foam."
            }
            Self::Lite => {
                "Light. Complementary on its Low profile. Sodium stays modest. Ships run at half particles."
            }
            Self::Normal => {
                "Balanced. Complementary on its Medium profile. Ocean extras are welcome. Ships use the defaults Arnold ships."
            }
            Self::Ultra => {
                "Heaviest. Complementary on its High profile. Extra ship particles when Arnold sends those values."
            }
        }
    }

    pub fn snow_lite(self) -> bool {
        matches!(self, Self::Potato | Self::Lite)
    }

    pub fn recommend_ocean(self) -> bool {
        matches!(self, Self::Normal | Self::Ultra)
    }

    pub fn skip_heavy_ocean(self) -> bool {
        matches!(self, Self::Potato)
    }
}

impl Default for QualityLevel {
    fn default() -> Self {
        Self::Normal
    }
}

/// GPU → default Quality. Player can override in the kit flow or Settings.
/// Integrated / low VRAM → Potato or Lite; mid-range discrete → Normal; strong discrete → Ultra.
pub fn recommend_quality(hw: &Hardware) -> QualityLevel {
    if !hw.dedicated_gpu {
        return if hw.vram_gb + f64::EPSILON < 1.5 {
            QualityLevel::Potato
        } else {
            QualityLevel::Lite
        };
    }
    if hw.vram_gb + f64::EPSILON < 2.0 {
        return QualityLevel::Lite;
    }
    if hw.vram_gb + f64::EPSILON >= 4.0 && hw.ram_gb + f64::EPSILON >= 16.0 {
        return QualityLevel::Ultra;
    }
    QualityLevel::Normal
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QualityTable {
    pub potato: QualityRow,
    pub lite: QualityRow,
    pub normal: QualityRow,
    pub ultra: QualityRow,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QualityRow {
    pub label: String,
    pub description: String,
    #[serde(default)]
    pub skip_components: Vec<String>,
    #[serde(default)]
    pub recommend_ocean: bool,
    #[serde(default)]
    pub snow: SnowQuality,
    #[serde(default)]
    pub ships: ShipsQuality,
    #[serde(default)]
    pub sodium: SodiumQuality,
    #[serde(default)]
    pub iris: IrisQuality,
    #[serde(default)]
    pub complementary: ComplementaryQuality,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SnowQuality {
    #[serde(default = "default_snow_quality")]
    pub quality: String,
    #[serde(default = "default_fps")]
    pub fps_target: u32,
    #[serde(default = "default_adaptive_fps")]
    pub adaptive_fps: bool,
    #[serde(default)]
    pub legacy_lite: bool,
}

impl Default for SnowQuality {
    fn default() -> Self {
        Self {
            quality: "normal".into(),
            fps_target: 60,
            adaptive_fps: true,
            legacy_lite: false,
        }
    }
}

fn default_snow_quality() -> String {
    "normal".into()
}
fn default_fps() -> u32 {
    60
}

fn default_adaptive_fps() -> bool {
    true
}

/// Current Snowfall jar. Quality writes the `quality` key on this build.
/// The lite-file swap stays only for migrating 1.2 / pre-1.3.3 installs.
pub const SNOW_V13_JAR: &str = "arnold-snowfall-1.3.3.jar";

/// Arnold: these ship keys stay on. Quality must never write them false.
pub const SHIPS_ALWAYS_TRUE: [&str; 3] = ["waterMask", "rigidRoot", "calmWhenAboard"];

/// Ship client knobs. Values live in the remote manifest `qualityPresets` table.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ShipsQuality {
    #[serde(default = "default_particles")]
    pub particles: f64,
    #[serde(default = "default_true")]
    pub spray: bool,
    #[serde(default = "default_true")]
    pub wake: bool,
    #[serde(default = "default_true")]
    pub hd_models: bool,
}

impl Default for ShipsQuality {
    fn default() -> Self {
        Self {
            particles: 1.0,
            spray: true,
            wake: true,
            hd_models: true,
        }
    }
}

fn default_particles() -> f64 {
    1.0
}
fn default_true() -> bool {
    true
}

/// Keys from CaffeineMC/sodium SodiumGameOptions.QualitySettings
/// (Gson LOWER_CASE_WITH_UNDERSCORES → weather_quality, leaves_quality).
/// Render distance and clouds are vanilla options.txt bindings in SodiumGameOptionPages.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SodiumQuality {
    #[serde(default = "default_weather")]
    pub weather_quality: String,
    #[serde(default = "default_leaves")]
    pub leaves_quality: String,
    #[serde(default = "default_rd")]
    pub render_distance: u32,
    #[serde(default = "default_clouds")]
    pub cloud_status: String,
}

impl Default for SodiumQuality {
    fn default() -> Self {
        Self {
            weather_quality: "DEFAULT".into(),
            leaves_quality: "DEFAULT".into(),
            render_distance: 12,
            cloud_status: "true".into(),
        }
    }
}

fn default_weather() -> String {
    "DEFAULT".into()
}
fn default_leaves() -> String {
    "DEFAULT".into()
}
fn default_rd() -> u32 {
    12
}
fn default_clouds() -> String {
    "true".into()
}

/// Keys from IrisShaders/Iris IrisConfig saved to config/iris.properties:
/// enableShaders, shaderPack.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IrisQuality {
    #[serde(default)]
    pub enable_shaders: bool,
}

impl Default for IrisQuality {
    fn default() -> Self {
        Self {
            enable_shaders: true,
        }
    }
}

/// Complementary shaders.properties defines profile.LOW / MEDIUM / HIGH.
/// Iris/OptiFine persist the chosen profile as `profile=<NAME>` in shaderpacks/<pack>.txt.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ComplementaryQuality {
    #[serde(default = "default_profile")]
    pub profile: String,
}

impl Default for ComplementaryQuality {
    fn default() -> Self {
        Self {
            profile: "MEDIUM".into(),
        }
    }
}

fn default_profile() -> String {
    "MEDIUM".into()
}

pub fn builtin_quality_table() -> QualityTable {
    QualityTable {
        potato: QualityRow {
            label: "Potato".into(),
            description: QualityLevel::Potato.description().into(),
            skip_components: vec!["wakes".into(), "waves".into()],
            recommend_ocean: false,
            snow: SnowQuality {
                quality: "potato".into(),
                fps_target: 60,
                adaptive_fps: true,
                legacy_lite: true,
            },
            ships: ShipsQuality {
                particles: 0.3,
                spray: false,
                wake: false,
                hd_models: false,
            },
            sodium: SodiumQuality {
                weather_quality: "FAST".into(),
                leaves_quality: "FAST".into(),
                render_distance: 6,
                cloud_status: "fast".into(),
            },
            iris: IrisQuality {
                enable_shaders: false,
            },
            complementary: ComplementaryQuality {
                profile: "LOW".into(),
            },
        },
        lite: QualityRow {
            label: "Lite".into(),
            description: QualityLevel::Lite.description().into(),
            skip_components: Vec::new(),
            recommend_ocean: false,
            snow: SnowQuality {
                quality: "lite".into(),
                fps_target: 60,
                adaptive_fps: true,
                legacy_lite: true,
            },
            ships: ShipsQuality {
                particles: 0.5,
                spray: true,
                wake: true,
                hd_models: false,
            },
            sodium: SodiumQuality {
                weather_quality: "FAST".into(),
                leaves_quality: "DEFAULT".into(),
                render_distance: 8,
                cloud_status: "fast".into(),
            },
            iris: IrisQuality {
                enable_shaders: true,
            },
            complementary: ComplementaryQuality {
                profile: "LOW".into(),
            },
        },
        normal: QualityRow {
            label: "Normal".into(),
            description: QualityLevel::Normal.description().into(),
            skip_components: Vec::new(),
            recommend_ocean: true,
            snow: SnowQuality {
                quality: "normal".into(),
                fps_target: 60,
                adaptive_fps: true,
                legacy_lite: false,
            },
            ships: ShipsQuality::default(),
            sodium: SodiumQuality::default(),
            iris: IrisQuality {
                enable_shaders: true,
            },
            complementary: ComplementaryQuality {
                profile: "MEDIUM".into(),
            },
        },
        ultra: QualityRow {
            label: "Ultra".into(),
            description: QualityLevel::Ultra.description().into(),
            skip_components: Vec::new(),
            recommend_ocean: true,
            snow: SnowQuality {
                quality: "ultra".into(),
                fps_target: 60,
                adaptive_fps: true,
                legacy_lite: false,
            },
            ships: ShipsQuality {
                particles: 1.6,
                spray: true,
                wake: true,
                hd_models: true,
            },
            sodium: SodiumQuality {
                weather_quality: "FANCY".into(),
                leaves_quality: "FANCY".into(),
                render_distance: 16,
                cloud_status: "true".into(),
            },
            iris: IrisQuality {
                enable_shaders: true,
            },
            complementary: ComplementaryQuality {
                profile: "HIGH".into(),
            },
        },
    }
}

pub fn table_from_manifest(manifest: &Manifest) -> QualityTable {
    manifest
        .quality_presets
        .clone()
        .unwrap_or_else(builtin_quality_table)
}

pub fn row_for(table: &QualityTable, level: QualityLevel) -> &QualityRow {
    match level {
        QualityLevel::Potato => &table.potato,
        QualityLevel::Lite => &table.lite,
        QualityLevel::Normal => &table.normal,
        QualityLevel::Ultra => &table.ultra,
    }
}

fn first_line(existing: &str) -> &str {
    existing
        .lines()
        .next()
        .unwrap_or("")
        .trim_end_matches('\r')
        .trim()
}

fn we_own(game_dir: &Path, rel: &str, existing: &str, state: Option<&InstalledState>) -> bool {
    let first = first_line(existing);
    if first == MANAGED_HEADER || first == LITE_MARKER {
        return true;
    }
    let Some(state) = state else {
        return false;
    };
    let Some(tracked) = state.file_by_path(rel) else {
        return false;
    };
    hash_file(&game_dir.join(rel))
        .ok()
        .is_some_and(|actual| actual == tracked.sha512)
}

/// Marker, matching tracked hash, or a tracked path once the player opted into Quality.
/// Iris/Sodium rewrite their files on launch (timestamp comments, pretty-print), so hash
/// drift alone must not block a later Quality change or Repair.
fn may_update(game_dir: &Path, rel: &str, existing: &str, state: Option<&InstalledState>) -> bool {
    if existing.is_empty() || we_own(game_dir, rel, existing, state) {
        return true;
    }
    state.is_some_and(|s| s.quality.is_some() && s.file_by_path(rel).is_some())
}

fn has_id(installed_ids: &[String], id: &str) -> bool {
    installed_ids.iter().any(|x| x == id)
}

/// Config files Quality writes and records in state.json (not options.txt).
pub fn quality_managed_paths(manifest: &Manifest, installed_ids: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    if has_id(installed_ids, "snow") {
        if let Some(file) = component_by_id(manifest, "snow").and_then(|c| c.config.as_ref()) {
            out.push(file.file.clone());
        }
    }
    if has_id(installed_ids, "ships") {
        if let Some(file) = component_by_id(manifest, "ships").and_then(|c| c.config.as_ref()) {
            out.push(file.file.clone());
        }
    }
    if has_id(installed_ids, "sodium") {
        out.push("config/sodium-options.json".into());
    }
    if has_id(installed_ids, "shaders") {
        out.push("config/iris.properties".into());
        if let Some(rel) = complementary_txt_rel(manifest) {
            out.push(rel);
        }
    }
    out
}

/// Snapshot these before Quality writes so rollback can restore them.
/// `options.txt` is never owned or recorded in state — only snapshotted if it already exists.
pub fn quality_snapshot_paths(manifest: &Manifest, installed_ids: &[String]) -> Vec<String> {
    let mut out = quality_managed_paths(manifest, installed_ids);
    if has_id(installed_ids, "sodium") {
        out.push("options.txt".into());
    }
    out
}

fn write_owned(game_dir: &Path, rel: &str, content: &str) -> Result<(), AppError> {
    let path = game_dir.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content.as_bytes())?;
    Ok(())
}

/// Recursively add missing object keys. Never overwrites a key the player already set.
pub fn merge_json_add_missing(existing: &Value, incoming: &Value) -> Value {
    match (existing, incoming) {
        (Value::Object(have), Value::Object(want)) => {
            let mut out = have.clone();
            for (key, value) in want {
                match out.get(key) {
                    Some(prev) => {
                        out.insert(key.clone(), merge_json_add_missing(prev, value));
                    }
                    None => {
                        out.insert(key.clone(), value.clone());
                    }
                }
            }
            Value::Object(out)
        }
        (prev, _) => prev.clone(),
    }
}

/// Replace listed top-level-or-nested keys, keep everything else.
pub fn merge_json_replace_paths(existing: &Value, incoming: &Value) -> Value {
    match (existing, incoming) {
        (Value::Object(have), Value::Object(want)) => {
            let mut out = have.clone();
            for (key, value) in want {
                match (out.get(key), value) {
                    (Some(Value::Object(_)), Value::Object(_)) => {
                        out.insert(key.clone(), merge_json_replace_paths(&out[key], value));
                    }
                    _ => {
                        out.insert(key.clone(), value.clone());
                    }
                }
            }
            Value::Object(out)
        }
        (_, next) => next.clone(),
    }
}

fn merge_colon_options(existing: &str, updates: &BTreeMap<String, String>, owned: bool) -> String {
    if existing.is_empty() {
        let mut out = String::new();
        for (key, value) in updates {
            out.push_str(key);
            out.push(':');
            out.push_str(value);
            out.push('\n');
        }
        return out;
    }
    let ending = if existing.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut present = BTreeMap::new();
    let mut lines: Vec<String> = Vec::new();
    for line in existing.split_inclusive(['\n']) {
        let raw = line.trim_end_matches(['\n', '\r']);
        if let Some((key, _)) = raw.split_once(':') {
            if !key.starts_with('#') {
                present.insert(key.to_string(), raw.to_string());
            }
        }
        lines.push(raw.to_string());
    }
    let mut out = String::new();
    for raw in &lines {
        if let Some((key, _)) = raw.split_once(':') {
            if owned {
                if let Some(value) = updates.get(key) {
                    out.push_str(key);
                    out.push(':');
                    out.push_str(value);
                    out.push_str(ending);
                    continue;
                }
            }
        }
        out.push_str(raw);
        out.push_str(ending);
    }
    for (key, value) in updates {
        if present.contains_key(key) {
            continue;
        }
        out.push_str(key);
        out.push(':');
        out.push_str(value);
        out.push_str(ending);
    }
    out
}

fn complementary_txt_rel(manifest: &Manifest) -> Option<String> {
    let shaders = component_by_id(manifest, "shaders")?;
    let pack = shaders
        .files
        .iter()
        .find(|f| f.dest == FileDest::Shaderpacks && f.file.ends_with(".zip"))?;
    Some(format!("shaderpacks/{}.txt", pack.file))
}

pub fn apply_quality(
    game_dir: &Path,
    manifest: &Manifest,
    level: QualityLevel,
    installed_ids: &[String],
    state: Option<&InstalledState>,
) -> Result<Vec<(String, String, String)>, AppError> {
    let table = table_from_manifest(manifest);
    let row = row_for(&table, level);
    let mut written = Vec::new();
    let has = |id: &str| installed_ids.iter().any(|x| x == id);

    if has("snow") {
        written.extend(apply_snow(game_dir, manifest, row, state)?);
    }
    if has("ships") {
        written.extend(apply_ships(game_dir, manifest, row, state)?);
    }
    if has("sodium") {
        written.extend(apply_sodium(game_dir, row, state)?);
    }
    if has("shaders") {
        written.extend(apply_iris(game_dir, manifest, row, state)?);
        written.extend(apply_complementary(game_dir, manifest, row, state)?);
    }
    Ok(written)
}

fn apply_snow(
    game_dir: &Path,
    manifest: &Manifest,
    row: &QualityRow,
    state: Option<&InstalledState>,
) -> Result<Vec<(String, String, String)>, AppError> {
    let Some(component) = component_by_id(manifest, "snow") else {
        return Ok(Vec::new());
    };
    let Some(config) = &component.config else {
        return Ok(Vec::new());
    };
    let rel = config.file.clone();
    let path = game_dir.join(&rel);
    let existing = fs::read_to_string(&path).unwrap_or_default();
    let mut defaults = BTreeMap::new();
    for (key, spec) in &config.settings {
        if matches!(key.as_str(), "quality" | "adaptiveFps" | "fpsTarget") {
            continue;
        }
        defaults.insert(key.clone(), format_setting_value(&spec.default));
    }
    let mut lite_over = BTreeMap::new();
    if let Some(preset) = lite_preset(config) {
        for (key, value) in &preset.overrides {
            if matches!(key.as_str(), "quality" | "adaptiveFps" | "fpsTarget") {
                continue;
            }
            lite_over.insert(key.clone(), format_setting_value(value));
        }
    }
    let header = config.header.as_deref().unwrap_or(MANAGED_HEADER);
    let marker = lite_preset(config)
        .and_then(|p| p.marker.as_deref())
        .unwrap_or(LITE_MARKER);
    let owned = existing.is_empty() || may_update(game_dir, &rel, &existing, state);
    let lite = if lite_over.is_empty() {
        None
    } else {
        Some(&lite_over)
    };
    // 1.3 writes `quality` and only fills adaptiveFps / fpsTarget when missing.
    // Per-effect switches stay with the player. The lite-file swap is only for
    // migrating old installs that do not already have 1.3 keys.
    let migrating_old = !existing.is_empty() && !looks_like_snow_v13(&existing);
    let use_v12_fallback = row.snow.legacy_lite && owned && migrating_old;
    let base = if use_v12_fallback {
        apply_properties(None, &defaults, lite, header, marker, ConfigSwitch::ToLite).content
    } else if existing.is_empty() {
        apply_properties(None, &defaults, None, header, marker, ConfigSwitch::None).content
    } else {
        existing
    };
    let mut quality_keys = BTreeMap::new();
    quality_keys.insert("quality".into(), row.snow.quality.clone());
    let with_quality = if base.is_empty() {
        apply_properties(
            None,
            &quality_keys,
            None,
            header,
            marker,
            ConfigSwitch::None,
        )
        .content
    } else {
        update_property_keys(&base, &quality_keys)
    };
    let mut fps_if_missing = BTreeMap::new();
    fps_if_missing.insert("adaptiveFps".into(), bool_str(row.snow.adaptive_fps));
    fps_if_missing.insert("fpsTarget".into(), row.snow.fps_target.to_string());
    let content = apply_properties(
        Some(with_quality.as_str()),
        &fps_if_missing,
        None,
        header,
        marker,
        ConfigSwitch::None,
    )
    .content;
    write_owned(game_dir, &rel, &content)?;
    Ok(vec![(rel, sha512_hex(content.as_bytes()), "snow".into())])
}

fn apply_ships(
    game_dir: &Path,
    manifest: &Manifest,
    row: &QualityRow,
    state: Option<&InstalledState>,
) -> Result<Vec<(String, String, String)>, AppError> {
    let Some(component) = component_by_id(manifest, "ships") else {
        return Ok(Vec::new());
    };
    let Some(config) = &component.config else {
        return Ok(Vec::new());
    };
    let rel = config.file.clone();
    let existing = fs::read_to_string(game_dir.join(&rel)).unwrap_or_default();
    let owned = existing.is_empty() || may_update(game_dir, &rel, &existing, state);
    let mut defaults = BTreeMap::new();
    for (key, spec) in &config.settings {
        defaults.insert(key.clone(), format_setting_value(&spec.default));
    }
    defaults.insert("particles".into(), format_particles(row.ships.particles));
    defaults.insert("spray".into(), bool_str(row.ships.spray));
    defaults.insert("wake".into(), bool_str(row.ships.wake));
    defaults.insert("hdModels".into(), bool_str(row.ships.hd_models));
    for key in SHIPS_ALWAYS_TRUE {
        defaults.insert(key.to_string(), "true".into());
    }
    let header = config.header.as_deref().unwrap_or(MANAGED_HEADER);
    let mut quality_keys = BTreeMap::new();
    quality_keys.insert("particles".into(), format_particles(row.ships.particles));
    quality_keys.insert("spray".into(), bool_str(row.ships.spray));
    quality_keys.insert("wake".into(), bool_str(row.ships.wake));
    quality_keys.insert("hdModels".into(), bool_str(row.ships.hd_models));
    for key in SHIPS_ALWAYS_TRUE {
        quality_keys.insert(key.to_string(), "true".into());
    }
    let content = if owned && existing.is_empty() {
        apply_properties(
            None,
            &defaults,
            None,
            header,
            LITE_MARKER,
            ConfigSwitch::None,
        )
        .content
    } else if owned {
        update_property_keys(&existing, &quality_keys)
    } else {
        let added = apply_properties(
            Some(existing.as_str()),
            &defaults,
            None,
            header,
            LITE_MARKER,
            ConfigSwitch::None,
        )
        .content;
        let mut force_true = BTreeMap::new();
        for key in SHIPS_ALWAYS_TRUE {
            force_true.insert(key.to_string(), "true".into());
        }
        update_property_keys(&added, &force_true)
    };
    write_owned(game_dir, &rel, &content)?;
    Ok(vec![(rel, sha512_hex(content.as_bytes()), "ships".into())])
}

fn format_particles(n: f64) -> String {
    format!("{n:.1}")
}

fn looks_like_snow_v13(existing: &str) -> bool {
    existing.lines().any(|line| {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') || trimmed.starts_with('!') {
            return false;
        }
        matches!(
            trimmed.split_once('=').map(|(k, _)| k.trim()),
            Some("quality") | Some("adaptiveFps") | Some("fpsTarget")
        )
    })
}

fn bool_str(v: bool) -> String {
    if v {
        "true".into()
    } else {
        "false".into()
    }
}

fn apply_sodium(
    game_dir: &Path,
    row: &QualityRow,
    state: Option<&InstalledState>,
) -> Result<Vec<(String, String, String)>, AppError> {
    let rel = "config/sodium-options.json";
    let incoming = json!({
        "quality": {
            "weather_quality": row.sodium.weather_quality,
            "leaves_quality": row.sodium.leaves_quality,
        }
    });
    let existing_text = fs::read_to_string(game_dir.join(rel)).ok();
    let owned = existing_text.as_deref().is_none()
        || existing_text
            .as_deref()
            .is_some_and(|text| may_update(game_dir, rel, text, state));
    let merged = match existing_text
        .as_deref()
        .and_then(|t| serde_json::from_str::<Value>(t).ok())
    {
        Some(prev) if owned => merge_json_replace_paths(&prev, &incoming),
        Some(prev) => merge_json_add_missing(&prev, &incoming),
        None => incoming,
    };
    let content = serde_json::to_string_pretty(&merged)?;
    write_owned(game_dir, rel, &content)?;

    let opt_rel = "options.txt";
    let mut vanilla = BTreeMap::new();
    vanilla.insert(
        "renderDistance".into(),
        row.sodium.render_distance.to_string(),
    );
    vanilla.insert("cloudStatus".into(), row.sodium.cloud_status.clone());
    let opt_existing = fs::read_to_string(game_dir.join(opt_rel)).unwrap_or_default();
    // options.txt is the player's vanilla file — never claim ownership; only add missing keys.
    let opt_next = merge_colon_options(&opt_existing, &vanilla, false);
    if opt_next != opt_existing {
        write_owned(game_dir, opt_rel, &opt_next)?;
    }

    Ok(vec![(
        rel.to_string(),
        sha512_hex(content.as_bytes()),
        "sodium".into(),
    )])
}

fn apply_iris(
    game_dir: &Path,
    manifest: &Manifest,
    row: &QualityRow,
    state: Option<&InstalledState>,
) -> Result<Vec<(String, String, String)>, AppError> {
    let rel = "config/iris.properties";
    let pack = complementary_txt_rel(manifest)
        .and_then(|p| {
            p.rsplit('/')
                .next()
                .map(|n| n.trim_end_matches(".txt").to_string())
        })
        .unwrap_or_else(|| "ComplementaryReimagined_r5.9.3.zip".into());
    let existing = fs::read_to_string(game_dir.join(rel)).unwrap_or_default();
    let owned = existing.is_empty() || may_update(game_dir, rel, &existing, state);
    let mut defaults = BTreeMap::new();
    defaults.insert("enableShaders".into(), bool_str(row.iris.enable_shaders));
    defaults.insert("shaderPack".into(), pack);
    let content = if owned && existing.is_empty() {
        apply_properties(
            None,
            &defaults,
            None,
            MANAGED_HEADER,
            LITE_MARKER,
            ConfigSwitch::None,
        )
        .content
    } else if owned {
        update_property_keys(&existing, &defaults)
    } else {
        apply_properties(
            Some(existing.as_str()),
            &defaults,
            None,
            MANAGED_HEADER,
            LITE_MARKER,
            ConfigSwitch::None,
        )
        .content
    };
    write_owned(game_dir, rel, &content)?;
    Ok(vec![(
        rel.into(),
        sha512_hex(content.as_bytes()),
        "shaders".into(),
    )])
}

fn apply_complementary(
    game_dir: &Path,
    manifest: &Manifest,
    row: &QualityRow,
    state: Option<&InstalledState>,
) -> Result<Vec<(String, String, String)>, AppError> {
    let Some(rel) = complementary_txt_rel(manifest) else {
        return Ok(Vec::new());
    };
    if !row.iris.enable_shaders {
        return Ok(Vec::new());
    }
    let existing = fs::read_to_string(game_dir.join(&rel)).unwrap_or_default();
    let owned = existing.is_empty() || may_update(game_dir, &rel, &existing, state);
    let mut defaults = BTreeMap::new();
    defaults.insert("profile".into(), row.complementary.profile.clone());
    let content = if owned && existing.is_empty() {
        apply_properties(
            None,
            &defaults,
            None,
            MANAGED_HEADER,
            LITE_MARKER,
            ConfigSwitch::None,
        )
        .content
    } else if owned {
        update_property_keys(&existing, &defaults)
    } else {
        apply_properties(
            Some(existing.as_str()),
            &defaults,
            None,
            MANAGED_HEADER,
            LITE_MARKER,
            ConfigSwitch::None,
        )
        .content
    };
    write_owned(game_dir, &rel, &content)?;
    Ok(vec![(
        rel,
        sha512_hex(content.as_bytes()),
        "shaders".into(),
    )])
}

pub fn record_quality_files(
    state: &mut InstalledState,
    files: Vec<(String, String, String)>,
    level: QualityLevel,
) {
    for (path, sha, id) in files {
        record_file(state, path, sha, id);
    }
    state.quality = Some(level);
}

/// Settings / Repair path: snapshot, write Quality keys, record hashes.
pub fn apply_quality_change(
    game_dir: &Path,
    manifest: &Manifest,
    level: QualityLevel,
    state: &InstalledState,
) -> Result<InstalledState, AppError> {
    let ids: Vec<String> = state.components.iter().map(|c| c.id.clone()).collect();
    let mut snap = state.clone();
    for rel in quality_snapshot_paths(manifest, &ids) {
        let path = game_dir.join(&rel);
        if path.is_file() && snap.file_by_path(&rel).is_none() {
            record_file(&mut snap, &rel, hash_file(&path)?, "pre-existing");
        }
    }
    create_snapshot(
        game_dir,
        &snap,
        &format!("before-quality-{}", level.as_str()),
    )?;
    let written = apply_quality(game_dir, manifest, level, &ids, Some(state))?;
    let mut next = state.clone();
    record_quality_files(&mut next, written, level);
    save_state(game_dir, &next)?;
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::Hardware;

    fn hw(dedicated: bool, vram: f64, ram: f64) -> Hardware {
        Hardware {
            cpu: "test".into(),
            cores: 8,
            ram_gb: ram,
            gpu: "test".into(),
            vram_gb: vram,
            dedicated_gpu: dedicated,
        }
    }

    #[test]
    fn maps_gpu_to_quality() {
        assert_eq!(
            recommend_quality(&hw(false, 0.0, 8.0)),
            QualityLevel::Potato
        );
        assert_eq!(recommend_quality(&hw(false, 2.0, 16.0)), QualityLevel::Lite);
        assert_eq!(recommend_quality(&hw(true, 1.0, 8.0)), QualityLevel::Lite);
        assert_eq!(
            recommend_quality(&hw(true, 3.0, 16.0)),
            QualityLevel::Normal
        );
        assert_eq!(recommend_quality(&hw(true, 8.0, 32.0)), QualityLevel::Ultra);
    }

    #[test]
    fn descriptions_are_plain_language() {
        for level in [
            QualityLevel::Potato,
            QualityLevel::Lite,
            QualityLevel::Normal,
            QualityLevel::Ultra,
        ] {
            assert!(level.description().len() > 20);
            assert!(!level.description().contains('_'));
        }
    }

    #[test]
    fn json_add_missing_never_overwrites_player() {
        let existing = json!({
            "quality": { "weather_quality": "FANCY", "leaves_quality": "FANCY" },
            "performance": { "use_entity_culling": false }
        });
        let incoming = json!({
            "quality": { "weather_quality": "FAST", "leaves_quality": "FAST" }
        });
        let merged = merge_json_add_missing(&existing, &incoming);
        assert_eq!(merged["quality"]["weather_quality"], "FANCY");
        assert_eq!(merged["quality"]["leaves_quality"], "FANCY");
        assert_eq!(merged["performance"]["use_entity_culling"], false);
    }

    #[test]
    fn owned_json_updates_quality_keeps_other_sections() {
        let existing = json!({
            "quality": { "weather_quality": "FANCY", "enable_vignette": false },
            "performance": { "use_entity_culling": false }
        });
        let incoming = json!({
            "quality": { "weather_quality": "FAST", "leaves_quality": "FAST" }
        });
        let merged = merge_json_replace_paths(&existing, &incoming);
        assert_eq!(merged["quality"]["weather_quality"], "FAST");
        assert_eq!(merged["quality"]["leaves_quality"], "FAST");
        assert_eq!(merged["quality"]["enable_vignette"], false);
        assert_eq!(merged["performance"]["use_entity_culling"], false);
    }

    #[test]
    fn options_txt_never_overwrites_player_render_distance() {
        let existing = "renderDistance:32\ncloudStatus:true\n";
        let mut updates = BTreeMap::new();
        updates.insert("renderDistance".into(), "6".into());
        updates.insert("cloudStatus".into(), "fast".into());
        let next = merge_colon_options(existing, &updates, false);
        assert!(next.contains("renderDistance:32"));
        assert!(next.contains("cloudStatus:true"));
    }

    #[test]
    fn potato_skips_heavy_ocean_and_disables_shaders() {
        let table = builtin_quality_table();
        assert!(table.potato.skip_components.contains(&"wakes".into()));
        assert!(!table.potato.iris.enable_shaders);
        assert_eq!(table.lite.complementary.profile, "LOW");
        assert_eq!(table.normal.complementary.profile, "MEDIUM");
        assert_eq!(table.ultra.complementary.profile, "HIGH");
        assert_eq!(table.potato.ships.particles, 0.3);
        assert!(!table.potato.ships.spray);
        assert!(!table.potato.ships.hd_models);
        assert_eq!(table.lite.ships.particles, 0.5);
        assert!(!table.lite.ships.hd_models);
        assert_eq!(table.ultra.ships.particles, 1.6);
        assert_eq!(table.normal.snow.fps_target, 60);
        assert!(table.normal.snow.adaptive_fps);
    }

    #[test]
    fn apply_writes_sodium_iris_and_does_not_clobber_player_ships() {
        use crate::manifest::load_fixture_manifest;
        use tempfile::tempdir;

        let tmp = tempdir().unwrap();
        let game = tmp.path();
        fs::create_dir_all(game.join("config")).unwrap();
        fs::write(
            game.join("config/arnold-ships-client.properties"),
            "particles=9\nspray=true\n",
        )
        .unwrap();
        fs::write(
            game.join("options.txt"),
            "renderDistance:32\ncloudStatus:true\n",
        )
        .unwrap();
        let manifest = load_fixture_manifest().expect("signed bundled manifest");
        apply_quality(
            game,
            &manifest,
            QualityLevel::Potato,
            &[
                "ships".into(),
                "sodium".into(),
                "shaders".into(),
                "snow".into(),
            ],
            None,
        )
        .unwrap();
        let ships = fs::read_to_string(game.join("config/arnold-ships-client.properties")).unwrap();
        assert!(
            ships.contains("particles=9"),
            "player particles must stay: {ships}"
        );
        assert!(
            ships.contains("wake=false"),
            "missing ship keys may be added: {ships}"
        );
        assert!(ships.contains("hdModels=false"), "{ships}");
        assert!(ships.contains("waterMask=true"), "{ships}");
        assert!(ships.contains("rigidRoot=true"), "{ships}");
        assert!(ships.contains("calmWhenAboard=true"), "{ships}");
        assert!(!ships.contains("waterMask=false"));
        let sodium: Value = serde_json::from_str(
            &fs::read_to_string(game.join("config/sodium-options.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(sodium["quality"]["weather_quality"], "FAST");
        assert_eq!(sodium["quality"]["leaves_quality"], "FAST");
        let iris = fs::read_to_string(game.join("config/iris.properties")).unwrap();
        assert!(iris.contains("enableShaders=false"));
        assert!(iris.contains("shaderPack=ComplementaryReimagined_r5.9.3.zip"));
        let options = fs::read_to_string(game.join("options.txt")).unwrap();
        assert!(options.contains("renderDistance:32"));
        assert!(options.contains("cloudStatus:true"));
        assert!(!game
            .join("shaderpacks/ComplementaryReimagined_r5.9.3.zip.txt")
            .exists());
    }

    #[test]
    fn owned_ships_and_complementary_follow_the_table() {
        use crate::config::MANAGED_HEADER;
        use crate::manifest::load_fixture_manifest;
        use crate::state::InstalledState;
        use tempfile::tempdir;

        let tmp = tempdir().unwrap();
        let game = tmp.path();
        fs::create_dir_all(game.join("config")).unwrap();
        fs::create_dir_all(game.join("shaderpacks")).unwrap();
        fs::write(
            game.join("config/arnold-ships-client.properties"),
            format!("{MANAGED_HEADER}\nparticles=1\nspray=true\nwake=true\n"),
        )
        .unwrap();
        let manifest = load_fixture_manifest().unwrap();
        let mut state = InstalledState::new("installer", "test");
        state.quality = Some(QualityLevel::Normal);
        record_file(
            &mut state,
            "config/arnold-ships-client.properties",
            hash_file(&game.join("config/arnold-ships-client.properties")).unwrap(),
            "ships",
        );
        apply_quality(
            game,
            &manifest,
            QualityLevel::Lite,
            &["ships".into(), "shaders".into(), "sodium".into()],
            Some(&state),
        )
        .unwrap();
        let ships = fs::read_to_string(game.join("config/arnold-ships-client.properties")).unwrap();
        assert!(ships.contains("particles=0.5"), "{ships}");
        assert!(ships.contains("hdModels=false"), "{ships}");
        assert!(ships.contains("waterMask=true"), "{ships}");
        let iris = fs::read_to_string(game.join("config/iris.properties")).unwrap();
        assert!(iris.contains("enableShaders=true"));
        let pack =
            fs::read_to_string(game.join("shaderpacks/ComplementaryReimagined_r5.9.3.zip.txt"))
                .unwrap();
        assert!(pack.contains("profile=LOW"), "{pack}");
    }

    #[test]
    fn quality_change_snapshots_and_updates_iris() {
        use crate::backup::list_snapshots;
        use crate::manifest::load_fixture_manifest;
        use crate::state::InstalledComponent;
        use tempfile::tempdir;

        let tmp = tempdir().unwrap();
        let game = tmp.path();
        fs::create_dir_all(game.join("config")).unwrap();
        fs::write(
            game.join("config/iris.properties"),
            "enableShaders=true\nshaderPack=ComplementaryReimagined_r5.9.3.zip\n",
        )
        .unwrap();
        let manifest = load_fixture_manifest().unwrap();
        let mut state = InstalledState::new("installer", "test");
        state.quality = Some(QualityLevel::Normal);
        state.components.push(InstalledComponent {
            id: "shaders".into(),
            version: "1".into(),
            name: "Iris".into(),
        });
        record_file(
            &mut state,
            "config/iris.properties",
            hash_file(&game.join("config/iris.properties")).unwrap(),
            "shaders",
        );
        apply_quality_change(game, &manifest, QualityLevel::Potato, &state).unwrap();
        let iris = fs::read_to_string(game.join("config/iris.properties")).unwrap();
        assert!(iris.contains("enableShaders=false"), "{iris}");
        assert!(!list_snapshots(game).unwrap().is_empty());
    }

    #[test]
    fn manifest_table_is_easy_to_edit() {
        use crate::manifest::load_fixture_manifest;
        let manifest = load_fixture_manifest().unwrap();
        let table = table_from_manifest(&manifest);
        assert!(!table.potato.ships.spray);
        assert!(!table.potato.ships.hd_models);
        assert_eq!(table.potato.ships.particles, 0.3);
        assert_eq!(table.lite.ships.particles, 0.5);
        assert!(!table.lite.ships.hd_models);
        assert_eq!(table.normal.ships.particles, 1.0);
        assert_eq!(table.ultra.ships.particles, 1.6);
        assert_eq!(table.normal.complementary.profile, "MEDIUM");
        assert_eq!(table.ultra.complementary.profile, "HIGH");
        assert_eq!(table.potato.sodium.weather_quality, "FAST");
        assert!(!table.potato.iris.enable_shaders);
        assert_eq!(SNOW_V13_JAR, "arnold-snowfall-1.3.3.jar");
        let snow = crate::manifest::component_by_id(&manifest, "snow").unwrap();
        assert_eq!(snow.files[0].file, "arnold-snowfall-1.3.3.jar");
        assert_eq!(snow.version, "1.3.3");
        assert_eq!(snow.files[0].size, 152127);
        assert!(snow
            .legacy_match
            .iter()
            .any(|p| p == "arnold-snowfall*.jar"));
        assert!(snow
            .legacy_match
            .iter()
            .any(|p| p == "arnold-snow-client*.jar"));
        assert!(snow.known_sha512.iter().any(|h| h
            == "f281c3aa82bc83cf68cc95e6959a5135b32ebab9b850435b17e0d7691c4eac2e3c193a2ed87c14c24bb758adf0a01a0fd744d6ca36c0bfc4d52a38feb56d1285"));
    }

    #[test]
    fn snow_quality_aliases_low_and_high() {
        assert_eq!(QualityLevel::parse("low"), Some(QualityLevel::Lite));
        assert_eq!(QualityLevel::parse("high"), Some(QualityLevel::Ultra));
        assert_eq!(QualityLevel::parse("LITE"), Some(QualityLevel::Lite));
    }

    #[test]
    fn snow_v13_writes_quality_and_only_fills_missing_fps_keys() {
        use crate::manifest::load_fixture_manifest;
        use tempfile::tempdir;

        let tmp = tempdir().unwrap();
        let game = tmp.path();
        fs::create_dir_all(game.join("config")).unwrap();
        fs::write(
            game.join("config/arnold-snow-client.properties"),
            "quality=normal\nadaptiveFps=false\nfpsTarget=90\ndenserSnow=false\nbreath=false\n",
        )
        .unwrap();
        let manifest = load_fixture_manifest().unwrap();
        apply_quality(
            game,
            &manifest,
            QualityLevel::Potato,
            &["snow".into()],
            None,
        )
        .unwrap();
        let snow = fs::read_to_string(game.join("config/arnold-snow-client.properties")).unwrap();
        assert!(snow.contains("quality=potato"), "{snow}");
        assert!(
            snow.contains("adaptiveFps=false"),
            "player adaptiveFps must stay: {snow}"
        );
        assert!(
            snow.contains("fpsTarget=90"),
            "player fpsTarget must stay: {snow}"
        );
        assert!(
            snow.contains("denserSnow=false"),
            "effect switches stay: {snow}"
        );
        assert!(snow.contains("breath=false"), "{snow}");
    }

    #[test]
    fn snow_v13_fresh_install_writes_quality_not_lite_file() {
        use crate::config::{LITE_MARKER, MANAGED_HEADER};
        use crate::manifest::load_fixture_manifest;
        use tempfile::tempdir;

        let tmp = tempdir().unwrap();
        let game = tmp.path();
        let manifest = load_fixture_manifest().unwrap();
        apply_quality(game, &manifest, QualityLevel::Lite, &["snow".into()], None).unwrap();
        let snow = fs::read_to_string(game.join("config/arnold-snow-client.properties")).unwrap();
        assert!(snow.starts_with(MANAGED_HEADER), "{snow}");
        assert!(!snow.starts_with(LITE_MARKER), "{snow}");
        assert!(snow.contains("quality=lite"), "{snow}");
        assert!(snow.contains("adaptiveFps=true"), "{snow}");
        assert!(snow.contains("fpsTarget=60"), "{snow}");
    }

    #[test]
    fn snow_v12_fallback_still_rewrites_owned_lite() {
        use crate::config::{LITE_MARKER, MANAGED_HEADER};
        use crate::manifest::load_fixture_manifest;
        use tempfile::tempdir;

        let tmp = tempdir().unwrap();
        let game = tmp.path();
        fs::create_dir_all(game.join("config")).unwrap();
        fs::write(
            game.join("config/arnold-snow-client.properties"),
            format!("{MANAGED_HEADER}\ndensity=24\ntreeCaps=true\n"),
        )
        .unwrap();
        let manifest = load_fixture_manifest().unwrap();
        apply_quality(game, &manifest, QualityLevel::Lite, &["snow".into()], None).unwrap();
        let snow = fs::read_to_string(game.join("config/arnold-snow-client.properties")).unwrap();
        assert!(snow.starts_with(LITE_MARKER), "{snow}");
        assert!(snow.contains("density=8"), "{snow}");
        assert!(snow.contains("quality=lite"), "{snow}");
        assert!(snow.contains("adaptiveFps=true"), "{snow}");
        assert!(snow.contains("fpsTarget=60"), "{snow}");
    }

    #[test]
    fn ships_never_write_water_mask_false() {
        use crate::config::MANAGED_HEADER;
        use crate::manifest::load_fixture_manifest;
        use tempfile::tempdir;

        let tmp = tempdir().unwrap();
        let game = tmp.path();
        fs::create_dir_all(game.join("config")).unwrap();
        fs::write(
            game.join("config/arnold-ships-client.properties"),
            format!("{MANAGED_HEADER}\nparticles=1.0\nwaterMask=false\nrigidRoot=false\ncalmWhenAboard=false\n"),
        )
        .unwrap();
        let manifest = load_fixture_manifest().unwrap();
        apply_quality(
            game,
            &manifest,
            QualityLevel::Potato,
            &["ships".into()],
            None,
        )
        .unwrap();
        let ships = fs::read_to_string(game.join("config/arnold-ships-client.properties")).unwrap();
        assert!(ships.contains("particles=0.3"), "{ships}");
        assert!(ships.contains("waterMask=true"), "{ships}");
        assert!(ships.contains("rigidRoot=true"), "{ships}");
        assert!(ships.contains("calmWhenAboard=true"), "{ships}");
        assert!(!ships.contains("waterMask=false"));
    }
}
