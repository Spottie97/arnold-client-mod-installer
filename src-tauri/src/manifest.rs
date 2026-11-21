use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use std::fs;
use std::path::Path;

use crate::download::{
    is_allowed_loader_url, is_allowed_source_url, ARNOLD_RELEASE_TAG, RELEASE_OWNER, RELEASE_REPO,
};
use crate::errors::AppError;
use crate::fetch::Fetcher;
use crate::quality::QualityTable;

/// TEST-ONLY Ed25519 public key embedded in the app. Not a production key.
pub const TEST_MANIFEST_PUBLIC_KEY_HEX: &str =
    "b44547ef0ceb7e4bac652ab9b73b46b2ff3d86d27196a479b674be29e87fdbef";

/// All-zero placeholder. A public release build must refuse this key.
pub const MANIFEST_PUBLIC_KEY_PLACEHOLDER: &str =
    "0000000000000000000000000000000000000000000000000000000000000000";

/// Production Ed25519 public key for the remote mod list.
/// The matching private seed lives only as secret `MANIFEST_SIGNING_KEY` in the
/// GitHub environment `manifest-release`. Safe to commit.
/// `ARNOLD_MANIFEST_PUBKEY` overrides this at compile time when it is non-empty.
pub const PRODUCTION_MANIFEST_PUBLIC_KEY_HEX: &str =
    "f7c8766e77505afd2b39d2f72501d61b020b73d852da18a56873f97f0d809ceb";

/// Test-signed fixture. Public builds verify it with the active key and skip it
/// when that check fails. `local-source` builds verify it with the test key.
pub const BUNDLED_MANIFEST_JSON: &str = include_str!("../../fixtures/manifest.v3.json");
pub const BUNDLED_MANIFEST_SIG_HEX: &str = include_str!("../../fixtures/manifest.v3.json.sig");

/// Which manifest public key a build is allowed to trust.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ManifestKeyPolicy {
    pub local_source: bool,
    /// `true` for a release profile (`debug_assertions` off). Release public
    /// builds never accept the test key.
    pub release_build: bool,
    /// `ARNOLD_ALLOW_TEST_MANIFEST_KEY` on a debug/dev build. Ignored for release.
    pub dev_flag: bool,
}

/// Compile-time public key. Empty `ARNOLD_MANIFEST_PUBKEY` keeps the committed key.
pub fn configured_production_public_key() -> &'static str {
    option_env!("ARNOLD_MANIFEST_PUBKEY")
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .unwrap_or(PRODUCTION_MANIFEST_PUBLIC_KEY_HEX)
}

pub fn current_manifest_key_policy() -> ManifestKeyPolicy {
    ManifestKeyPolicy {
        local_source: cfg!(feature = "local-source"),
        release_build: !cfg!(debug_assertions),
        dev_flag: dev_test_manifest_key_flag(),
    }
}

fn dev_flag_enabled(value: &str) -> bool {
    let value = value.trim();
    value == "1" || value.eq_ignore_ascii_case("true") || value.eq_ignore_ascii_case("yes")
}

fn dev_test_manifest_key_flag() -> bool {
    std::env::var("ARNOLD_ALLOW_TEST_MANIFEST_KEY")
        .ok()
        .is_some_and(|value| dev_flag_enabled(&value))
        || option_env!("ARNOLD_ALLOW_TEST_MANIFEST_KEY").is_some_and(dev_flag_enabled)
}

/// `true` when a public build must not treat `key` as a production verifying key.
pub fn production_manifest_key_rejected(key: &str) -> bool {
    let key = key.trim();
    if key.len() != 64 || !key.chars().all(|c| c.is_ascii_hexdigit()) {
        return true;
    }
    let lower = key.to_ascii_lowercase();
    if lower == TEST_MANIFEST_PUBLIC_KEY_HEX || lower == MANIFEST_PUBLIC_KEY_PLACEHOLDER {
        return true;
    }
    let Ok(bytes) = hex::decode(&lower) else {
        return true;
    };
    let Ok(arr) = <[u8; 32]>::try_from(bytes.as_slice()) else {
        return true;
    };
    VerifyingKey::from_bytes(&arr).is_err()
}

pub fn unconfigured_manifest_key() -> AppError {
    AppError::Other {
        code: "E-MANIFEST-KEY".into(),
        title: "This build isn't configured".into(),
        hint: "This installer will not load a mod list until it is built with the production signing key. Download the published installer.".into(),
        detail: "The manifest public key is missing, still the placeholder, or equal to the test key. A release build refuses that key. A debug build can set ARNOLD_ALLOW_TEST_MANIFEST_KEY=1 to keep using the test key.".into(),
    }
}

/// Key used for the remote manifest, the cached manifest, and the bundled fixture.
pub fn select_manifest_public_key(
    policy: ManifestKeyPolicy,
    production_key: &str,
) -> Result<&str, AppError> {
    if policy.local_source {
        return Ok(TEST_MANIFEST_PUBLIC_KEY_HEX);
    }
    let key = production_key.trim();
    if production_manifest_key_rejected(key) {
        if !policy.release_build && policy.dev_flag {
            return Ok(TEST_MANIFEST_PUBLIC_KEY_HEX);
        }
        return Err(unconfigured_manifest_key());
    }
    Ok(key)
}

pub fn active_manifest_public_key() -> Result<&'static str, AppError> {
    select_manifest_public_key(
        current_manifest_key_policy(),
        configured_production_public_key(),
    )
}

pub const MANIFEST_SCHEMA: u32 = 3;
pub const SNOW_LITE_PRESET: &str = "snow.lite";
pub const REMOTE_MANIFEST_URL: &str =
    "https://github.com/Spottie97/arnold-client-mod-installer/releases/latest/download/manifest.json";
pub const REMOTE_MANIFEST_SIG_URL: &str =
    "https://github.com/Spottie97/arnold-client-mod-installer/releases/latest/download/manifest.json.sig";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub schema: u32,
    pub manifest_version: String,
    pub min_app_version: String,
    pub minecraft: String,
    #[serde(default)]
    pub current_server_shipwright: Option<String>,
    pub loader: Loader,
    pub components: Vec<Component>,
    pub kits: BTreeMap<String, Kit>,
    #[serde(default)]
    pub addons: BTreeMap<String, Addon>,
    #[serde(default)]
    pub recommend: Option<RecommendConfig>,
    /// Per-level Sodium / Iris / Complementary / snow / ships knobs.
    /// Arnold can ship new particle values later without an installer rebuild.
    #[serde(default)]
    pub quality_presets: Option<QualityTable>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Loader {
    pub id: String,
    pub url: String,
    pub sha512: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Component {
    pub id: String,
    pub group: ComponentGroup,
    pub kind: ComponentKind,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub requires: Vec<String>,
    pub name: String,
    pub version: String,
    pub description: String,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub uninstall_note: Option<String>,
    #[serde(default)]
    pub required_in_all_kits: bool,
    #[serde(default)]
    pub min_server_shipwright: Option<String>,
    #[serde(default)]
    pub config: Option<ModConfig>,
    #[serde(default)]
    pub jvm_flags: Vec<JvmFlag>,
    #[serde(default)]
    pub pending: Option<PendingInfo>,
    #[serde(default)]
    pub legacy_match: Vec<String>,
    #[serde(default)]
    pub known_sha512: Vec<String>,
    pub files: Vec<ComponentFile>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ComponentGroup {
    Base,
    Shaders,
    Arnold,
    Ocean,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ComponentKind {
    Base,
    Download,
    Pending,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ComponentFile {
    pub file: String,
    pub dest: FileDest,
    #[serde(rename = "match")]
    pub match_glob: String,
    pub url: String,
    pub sha512: String,
    pub size: u64,
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default)]
    pub keep_existing: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FileDest {
    Mods,
    Shaderpacks,
    Resourcepacks,
}

impl FileDest {
    pub const fn folder(self) -> &'static str {
        match self {
            FileDest::Mods => "mods",
            FileDest::Shaderpacks => "shaderpacks",
            FileDest::Resourcepacks => "resourcepacks",
        }
    }

    /// Dest of a tracked relative path such as `resourcepacks/x.zip`; `None` if unknown.
    pub fn from_rel_path(path: &str) -> Option<FileDest> {
        let folder = path.split('/').next()?;
        [
            FileDest::Mods,
            FileDest::Shaderpacks,
            FileDest::Resourcepacks,
        ]
        .into_iter()
        .find(|dest| dest.folder() == folder)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModConfig {
    pub file: String,
    pub format: String,
    #[serde(default)]
    pub header: Option<String>,
    #[serde(default)]
    pub merge: Option<String>,
    #[serde(default)]
    pub settings: BTreeMap<String, SettingSpec>,
    #[serde(default)]
    pub presets: BTreeMap<String, ConfigPreset>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SettingSpec {
    #[serde(rename = "type")]
    pub value_type: String,
    pub default: Value,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub advanced: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConfigPreset {
    #[serde(default)]
    pub marker: Option<String>,
    #[serde(default)]
    pub overrides: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JvmFlag {
    pub flag: String,
    pub purpose: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingInfo {
    pub source: String,
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub page: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Kit {
    pub name: String,
    pub components: Vec<String>,
    #[serde(default)]
    pub snow_lite: bool,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Addon {
    pub name: String,
    pub components: Vec<String>,
    #[serde(default)]
    pub default_on: BTreeMap<String, bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RecommendConfig {
    #[serde(default)]
    pub full: Option<HardwareThresholds>,
    #[serde(default)]
    pub mid_snow_full: Option<HardwareThresholds>,
    #[serde(default, rename = "warnRamBelowGB")]
    pub warn_ram_below_gb: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HardwareThresholds {
    #[serde(default)]
    pub dedicated_gpu: Option<bool>,
    #[serde(default, rename = "vramGB")]
    pub vram_gb: Option<f64>,
    #[serde(default, rename = "ramGB")]
    pub ram_gb: Option<f64>,
    #[serde(default)]
    pub cores: Option<u32>,
}

pub fn verify_signature(
    bytes: &[u8],
    signature: &[u8],
    public_key_hex: &str,
) -> Result<(), AppError> {
    let pk_bytes = hex::decode(public_key_hex.trim()).map_err(|e| AppError::SignatureInvalid {
        detail: format!("public key hex: {e}"),
    })?;
    let pk_arr: [u8; 32] =
        pk_bytes
            .as_slice()
            .try_into()
            .map_err(|_| AppError::SignatureInvalid {
                detail: "public key must be 32 bytes".into(),
            })?;
    let key = VerifyingKey::from_bytes(&pk_arr).map_err(|e| AppError::SignatureInvalid {
        detail: e.to_string(),
    })?;
    let sig_arr: [u8; 64] = signature
        .try_into()
        .map_err(|_| AppError::SignatureInvalid {
            detail: "signature must be 64 bytes".into(),
        })?;
    let sig = Signature::from_bytes(&sig_arr);
    key.verify(bytes, &sig)
        .map_err(|e| AppError::SignatureInvalid {
            detail: e.to_string(),
        })
}

fn valid_sha512(value: &str) -> bool {
    value.len() == 128 && value.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f'))
}

fn valid_id(id: &str) -> bool {
    let mut chars = id.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {
            chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        }
        _ => false,
    }
}

fn valid_match(pattern: &str) -> bool {
    if pattern.contains('/') || pattern.contains('\\') {
        return false;
    }
    let Some((stem, ext)) = pattern.rsplit_once('.') else {
        return false;
    };
    if ext != "jar" && ext != "zip" {
        return false;
    }
    let mut cs = stem.chars();
    match cs.next() {
        Some(c) if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | '-') => {
            cs.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | '-' | '*'))
        }
        _ => false,
    }
}

pub fn parse_manifest(bytes: &[u8]) -> Result<Manifest, AppError> {
    let parsed: Manifest = serde_json::from_slice(bytes)?;
    validate_manifest(&parsed)?;
    Ok(parsed)
}

pub fn parse_and_verify(
    bytes: &[u8],
    signature: &[u8],
    public_key_hex: &str,
) -> Result<Manifest, AppError> {
    verify_signature(bytes, signature, public_key_hex)?;
    parse_manifest(bytes)
}

pub fn load_bundled_manifest() -> Result<Manifest, AppError> {
    load_bundled_manifest_with_key(active_manifest_public_key()?)
}

pub fn load_bundled_manifest_with_key(public_key_hex: &str) -> Result<Manifest, AppError> {
    let sig = hex::decode(BUNDLED_MANIFEST_SIG_HEX.trim())?;
    parse_and_verify(BUNDLED_MANIFEST_JSON.as_bytes(), &sig, public_key_hex)
}

/// Parsed bundled JSON for unit tests that need the mod list, not its signature.
#[cfg(test)]
pub fn load_fixture_manifest() -> Result<Manifest, AppError> {
    parse_manifest(BUNDLED_MANIFEST_JSON.as_bytes())
}

pub fn validate_manifest(manifest: &Manifest) -> Result<(), AppError> {
    if manifest.schema != MANIFEST_SCHEMA {
        return Err(AppError::ManifestSchema(format!(
            "schema must be {MANIFEST_SCHEMA}, got {}",
            manifest.schema
        )));
    }
    if manifest.manifest_version.trim().is_empty()
        || manifest.min_app_version.trim().is_empty()
        || manifest.minecraft.trim().is_empty()
    {
        return Err(AppError::ManifestSchema(
            "required string field is empty".into(),
        ));
    }
    if !valid_sha512(&manifest.loader.sha512) {
        return Err(AppError::ManifestSchema(
            "loader.sha512 is not 128 lowercase hex".into(),
        ));
    }
    if !is_allowed_loader_url(&manifest.loader.url) {
        return Err(AppError::UrlNotAllowed {
            url: manifest.loader.url.clone(),
        });
    }

    if manifest.components.is_empty() {
        return Err(AppError::ManifestSchema(
            "components must not be empty".into(),
        ));
    }

    let mut ids = HashSet::new();
    for component in &manifest.components {
        if !valid_id(&component.id) {
            return Err(AppError::ManifestSchema(format!(
                "invalid component id {}",
                component.id
            )));
        }
        if !ids.insert(component.id.clone()) {
            return Err(AppError::ManifestSchema(format!(
                "duplicate component id {}",
                component.id
            )));
        }
        if component.kind == ComponentKind::Pending {
            continue;
        }
        if component.files.is_empty() {
            return Err(AppError::ManifestSchema(format!(
                "component {} has no files",
                component.id
            )));
        }
        for file in &component.files {
            if !valid_sha512(&file.sha512) {
                return Err(AppError::ManifestSchema(format!(
                    "bad sha512 on {}",
                    file.file
                )));
            }
            if !valid_match(&file.match_glob) {
                return Err(AppError::ManifestSchema(format!(
                    "bad match glob {}",
                    file.match_glob
                )));
            }
            for extra in &component.legacy_match {
                if !valid_match(extra) {
                    return Err(AppError::ManifestSchema(format!(
                        "bad legacy match glob {extra}"
                    )));
                }
            }
            for known in &component.known_sha512 {
                if !valid_sha512(known) {
                    return Err(AppError::ManifestSchema(format!(
                        "bad knownSha512 on {}",
                        component.id
                    )));
                }
            }
            if !is_allowed_source_url(&file.url) {
                return Err(AppError::UrlNotAllowed {
                    url: file.url.clone(),
                });
            }
        }
    }

    for (kit_id, kit) in &manifest.kits {
        for id in &kit.components {
            if !ids.contains(id) {
                return Err(AppError::ManifestRequires(format!(
                    "kit {kit_id} references unknown component {id}"
                )));
            }
        }
    }

    for (addon_id, addon) in &manifest.addons {
        if !valid_id(addon_id) {
            return Err(AppError::ManifestSchema(format!(
                "invalid addon id {addon_id}"
            )));
        }
        if addon.name.trim().is_empty() {
            return Err(AppError::ManifestSchema(format!(
                "addon {addon_id} has an empty name"
            )));
        }
        for id in &addon.components {
            if !ids.contains(id) {
                return Err(AppError::ManifestRequires(format!(
                    "addon {addon_id} references unknown component {id}"
                )));
            }
        }
        for kit_id in addon.default_on.keys() {
            if !manifest.kits.contains_key(kit_id) {
                return Err(AppError::ManifestSchema(format!(
                    "addon {addon_id} defaultOn references unknown kit {kit_id}"
                )));
            }
        }
    }

    for component in &manifest.components {
        for req in &component.requires {
            if !ids.contains(req) {
                return Err(AppError::ManifestRequires(format!(
                    "component {} requires unknown {req}",
                    component.id
                )));
            }
        }
    }

    detect_cycles(manifest)?;

    for component in &manifest.components {
        if !component.required_in_all_kits {
            continue;
        }
        for (kit_id, kit) in &manifest.kits {
            if !kit.components.iter().any(|id| id == &component.id) {
                return Err(AppError::ManifestRequires(format!(
                    "kit {kit_id} omits requiredInAllKits component {}",
                    component.id
                )));
            }
        }
    }

    let _ = (RELEASE_OWNER, RELEASE_REPO);
    Ok(())
}

fn detect_cycles(manifest: &Manifest) -> Result<(), AppError> {
    let mut edges: HashMap<&str, Vec<&str>> = HashMap::new();
    for component in &manifest.components {
        edges.insert(
            component.id.as_str(),
            component.requires.iter().map(String::as_str).collect(),
        );
    }
    fn visit<'a>(
        id: &'a str,
        edges: &HashMap<&'a str, Vec<&'a str>>,
        stack: &mut Vec<&'a str>,
        seen: &mut HashSet<&'a str>,
    ) -> Result<(), AppError> {
        if stack.contains(&id) {
            return Err(AppError::ManifestRequires(format!(
                "requires cycle involving {id}"
            )));
        }
        if !seen.insert(id) {
            return Ok(());
        }
        stack.push(id);
        if let Some(reqs) = edges.get(id) {
            for req in reqs {
                visit(req, edges, stack, seen)?;
            }
        }
        stack.pop();
        Ok(())
    }
    let mut seen = HashSet::new();
    let mut stack = Vec::new();
    for component in &manifest.components {
        visit(component.id.as_str(), &edges, &mut stack, &mut seen)?;
    }
    Ok(())
}

pub fn resolve_requires(manifest: &Manifest, selected: &[String]) -> Result<Vec<String>, AppError> {
    let by_id: HashMap<&str, &Component> = manifest
        .components
        .iter()
        .map(|c| (c.id.as_str(), c))
        .collect();
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    fn walk<'a>(
        id: &'a str,
        by_id: &HashMap<&'a str, &'a Component>,
        seen: &mut BTreeSet<String>,
        out: &mut Vec<String>,
    ) -> Result<(), AppError> {
        if !seen.insert(id.to_string()) {
            return Ok(());
        }
        let component = by_id
            .get(id)
            .ok_or_else(|| AppError::ManifestRequires(format!("unknown component {id}")))?;
        for req in &component.requires {
            walk(req, by_id, seen, out)?;
        }
        out.push(id.to_string());
        Ok(())
    }
    for id in selected {
        walk(id, &by_id, &mut seen, &mut out)?;
    }
    Ok(out)
}

pub fn component_by_id<'a>(manifest: &'a Manifest, id: &str) -> Option<&'a Component> {
    manifest.components.iter().find(|c| c.id == id)
}

pub fn lite_preset(config: &ModConfig) -> Option<&ConfigPreset> {
    config
        .presets
        .get(SNOW_LITE_PRESET)
        .or_else(|| config.presets.get("lite"))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ManifestLoad {
    pub manifest: Manifest,
    pub source: String,
    pub offline: bool,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub local_source: bool,
    #[serde(default)]
    pub local_source_dir: Option<String>,
}

pub fn cache_paths(cache_dir: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
    (
        cache_dir.join("manifest.json"),
        cache_dir.join("manifest.json.sig"),
    )
}

pub fn write_manifest_cache(
    cache_dir: &Path,
    bytes: &[u8],
    signature: &[u8],
) -> Result<(), AppError> {
    fs::create_dir_all(cache_dir)?;
    let (json_path, sig_path) = cache_paths(cache_dir);
    fs::write(json_path, bytes)?;
    fs::write(sig_path, hex::encode(signature))?;
    Ok(())
}

pub fn load_cached_manifest(cache_dir: &Path) -> Result<Manifest, AppError> {
    load_cached_manifest_with_key(cache_dir, active_manifest_public_key()?)
}

pub fn load_cached_manifest_with_key(
    cache_dir: &Path,
    public_key_hex: &str,
) -> Result<Manifest, AppError> {
    let (json_path, sig_path) = cache_paths(cache_dir);
    let bytes = fs::read(json_path)?;
    let sig_text = fs::read_to_string(sig_path)?;
    let signature = hex::decode(sig_text.trim())?;
    parse_and_verify(&bytes, &signature, public_key_hex)
}

pub fn fetch_remote_manifest<F: Fetcher>(
    fetcher: &F,
) -> Result<(Manifest, Vec<u8>, Vec<u8>), AppError> {
    fetch_remote_manifest_with_key(fetcher, active_manifest_public_key()?)
}

pub fn fetch_signed_manifest_with_key<F: Fetcher>(
    fetcher: &F,
    manifest_url: &str,
    sig_url: &str,
    public_key_hex: &str,
) -> Result<(Manifest, Vec<u8>, Vec<u8>), AppError> {
    let bytes = fetcher.fetch(manifest_url)?;
    let sig_raw = fetcher.fetch(sig_url)?;
    let signature = decode_signature(&sig_raw)?;
    let manifest = parse_and_verify(&bytes, &signature, public_key_hex)?;
    Ok((manifest, bytes, signature))
}

pub fn fetch_remote_manifest_with_key<F: Fetcher>(
    fetcher: &F,
    public_key_hex: &str,
) -> Result<(Manifest, Vec<u8>, Vec<u8>), AppError> {
    fetch_signed_manifest_with_key(
        fetcher,
        REMOTE_MANIFEST_URL,
        REMOTE_MANIFEST_SIG_URL,
        public_key_hex,
    )
}

pub fn load_manifest_from_urls<F: Fetcher>(
    fetcher: &F,
    manifest_url: &str,
    sig_url: &str,
    cache_dir: &Path,
    public_key_hex: &str,
    bundled_fallback: bool,
) -> ManifestLoad {
    match fetch_signed_manifest_with_key(fetcher, manifest_url, sig_url, public_key_hex) {
        Ok((manifest, bytes, signature)) => {
            let _ = write_manifest_cache(cache_dir, &bytes, &signature);
            ManifestLoad {
                manifest,
                source: "remote".into(),
                offline: false,
                message: None,
                local_source: false,
                local_source_dir: None,
            }
        }
        Err(remote_err) => match load_cached_manifest_with_key(cache_dir, public_key_hex) {
            Ok(manifest) => ManifestLoad {
                manifest,
                source: "cache".into(),
                offline: true,
                message: Some(format!(
                    "Using the last good mod list (offline). {remote_err}"
                )),
                local_source: false,
                local_source_dir: None,
            },
            Err(_) if bundled_fallback => match load_bundled_manifest_with_key(public_key_hex) {
                Ok(manifest) => ManifestLoad {
                    manifest,
                    source: "bundled".into(),
                    offline: true,
                    message: Some(format!(
                        "Using the bundled mod list. Remote fetch failed: {remote_err}"
                    )),
                    local_source: false,
                    local_source_dir: None,
                },
                Err(bundled_err) => {
                    unavailable_manifest(remote_err.to_string(), bundled_err.to_string())
                }
            },
            Err(cache_err) => unavailable_manifest(remote_err.to_string(), cache_err.to_string()),
        },
    }
}

fn unavailable_manifest(remote_err: String, bundled_err: String) -> ManifestLoad {
    ManifestLoad {
        manifest: Manifest {
            schema: MANIFEST_SCHEMA,
            manifest_version: "unavailable".into(),
            min_app_version: "3.0.0".into(),
            minecraft: "26.3".into(),
            current_server_shipwright: None,
            loader: Loader {
                id: "missing".into(),
                url: "https://cdn.modrinth.com/missing".into(),
                sha512: "0".repeat(128),
            },
            components: Vec::new(),
            kits: BTreeMap::new(),
            addons: BTreeMap::new(),
            recommend: None,
            quality_presets: None,
        },
        source: "error".into(),
        offline: true,
        message: Some(format!(
            "No signed mod list is available. {remote_err}; bundled: {bundled_err}"
        )),
        local_source: false,
        local_source_dir: None,
    }
}

pub(crate) fn decode_signature(raw: &[u8]) -> Result<Vec<u8>, AppError> {
    if raw.len() == 64 {
        return Ok(raw.to_vec());
    }
    let text = std::str::from_utf8(raw)
        .map_err(|e| AppError::SignatureInvalid {
            detail: format!("signature is not utf-8 hex: {e}"),
        })?
        .trim();
    hex::decode(text).map_err(|e| AppError::SignatureInvalid {
        detail: format!("signature hex: {e}"),
    })
}

/// Remote first, then last-good cache, then the bundled fixture if it verifies
/// with the active key. Public builds skip the test-signed fixture.
/// `offline` is set when the network copy could not be used.
pub fn load_best_manifest<F: Fetcher>(
    fetcher: &F,
    cache_dir: &Path,
) -> Result<ManifestLoad, AppError> {
    let key = active_manifest_public_key()?;
    Ok(load_best_manifest_with_key(fetcher, cache_dir, key))
}

pub fn load_best_manifest_with_key<F: Fetcher>(
    fetcher: &F,
    cache_dir: &Path,
    public_key_hex: &str,
) -> ManifestLoad {
    load_manifest_from_urls(
        fetcher,
        REMOTE_MANIFEST_URL,
        REMOTE_MANIFEST_SIG_URL,
        cache_dir,
        public_key_hex,
        true,
    )
}

pub fn default_cache_dir() -> std::path::PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("arnold-client-mods")
        .join("cache")
}

pub fn release_asset_url(file_name: &str) -> String {
    let _ = ARNOLD_RELEASE_TAG;
    crate::download::arnold_release_asset_url(file_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    const TEST_PRIVATE_KEY_HEX: &str =
        "48ab0651056657627942de64e86d5435d7391e1ecd922679081c2fbadc376c98";

    fn sign(bytes: &[u8]) -> Vec<u8> {
        let raw = hex::decode(TEST_PRIVATE_KEY_HEX).unwrap();
        let arr: [u8; 32] = raw.try_into().unwrap();
        let key = SigningKey::from_bytes(&arr);
        key.sign(bytes).to_bytes().to_vec()
    }

    fn valid_json() -> serde_json::Value {
        serde_json::from_str(BUNDLED_MANIFEST_JSON).unwrap()
    }

    #[test]
    fn bundled_fixture_verifies_with_embedded_test_key() {
        let manifest = load_bundled_manifest_with_key(TEST_MANIFEST_PUBLIC_KEY_HEX).unwrap();
        assert_eq!(manifest.schema, 3);
        assert_eq!(manifest.manifest_version, "2026.11.7");
        assert_eq!(manifest.minecraft, "26.3");
        assert_eq!(
            manifest.current_server_shipwright.as_deref(),
            Some("0.11.2")
        );
        assert!(manifest
            .components
            .iter()
            .any(|c| c.id == "deck" && c.required_in_all_kits));
    }

    fn file_of<'a>(manifest: &'a Manifest, id: &str, name: &str) -> &'a ComponentFile {
        manifest
            .components
            .iter()
            .find(|c| c.id == id)
            .unwrap()
            .files
            .iter()
            .find(|f| f.file == name)
            .unwrap()
    }

    #[test]
    fn bundled_download_entries_match_verified_modrinth_and_arnold_assets() {
        let manifest = load_fixture_manifest().unwrap();

        let fabric = file_of(&manifest, "base", "fabric-api-0.166.2+26.3.jar");
        assert_eq!(
            fabric.url,
            "https://cdn.modrinth.com/data/P7dR8mSH/versions/Zns5SSRA/fabric-api-0.166.2%2B26.3.jar"
        );
        assert_eq!(
            fabric.sha512,
            "fb686a9fa923248328107a12498229d6951e316257f18b7d0e4cbd6885ba447ed50a44076a8a5ba47cfe2043b329db3587ab000aeab1face26b2587a5d4ba6b5"
        );
        assert_eq!(fabric.size, 1_797_523);
        assert_eq!(
            fabric.sha1.as_deref(),
            Some("65636253571429971f25f5619692e6c1a6362028")
        );

        let sodium = file_of(&manifest, "sodium", "sodium-fabric-0.9.2+mc26.3.jar");
        assert_eq!(
            sodium.url,
            "https://cdn.modrinth.com/data/AANobbMI/versions/bAZQdGpg/sodium-fabric-0.9.2%2Bmc26.3.jar"
        );
        assert_eq!(
            sodium.sha512,
            "f3f260f204b8ce5e8777c2c73f537956755ec61f3817b0b78b5e5568e8aa40f14891f146475075109747665c7c88a45ca0ca9a8df3c68d79c58ecdaa8d8c4b93"
        );
        assert_eq!(sodium.size, 1_908_068);

        let iris = file_of(&manifest, "shaders", "iris-fabric-1.11.7+mc26.3.jar");
        assert_eq!(
            iris.url,
            "https://cdn.modrinth.com/data/YL57xq9U/versions/vTN4NRGW/iris-fabric-1.11.7%2Bmc26.3.jar"
        );
        assert_eq!(
            iris.sha512,
            "d8d3312f77fc52a48e4a6cd836fda33669bdebcb4de0b8c0d30adc808b287330a533e0baef355cc1ce6336a26851d9775369a802a4666b7b7a6266af20088f42"
        );
        assert_eq!(iris.size, 2_850_938);

        let complementary = file_of(&manifest, "shaders", "ComplementaryReimagined_r5.9.3.zip");
        assert_eq!(
            complementary.url,
            "https://cdn.modrinth.com/data/HVnmMxH1/versions/Bqen1mJX/ComplementaryReimagined_r5.9.3.zip"
        );
        assert_eq!(
            complementary.sha512,
            "45304b1d7862afdb2177b7e6226fc9c6737911d17a4ad46cc9c519ca4deb935f05072cd337cd8b19a15bb8062b57b54eceec7bdee4cd572d94858e2946d37d85"
        );
        assert_eq!(complementary.size, 553_397);

        let deck = file_of(&manifest, "deck", "arnold-deck-client-0.11.1.jar");
        assert_eq!(
            deck.sha512,
            "a5619fffe5e4008054340f8f2bb6b40a30861b2763404b7f398534831907bdfce3fbdb15f307c1d52ecaf4e51522e47f70c7c25267b0206769108b43660b0b58"
        );
        assert_eq!(deck.size, 11_243);

        let ships = file_of(&manifest, "ships", "arnold-ships-client-0.10.3.jar");
        assert!(ships.url.ends_with(&format!(
            "/{}/arnold-ships-client-0.10.3.jar",
            crate::download::ARNOLD_RELEASE_TAG
        )));
        assert_eq!(
            ships.sha512,
            "bf6a3a5a3395b46e69a16144af682b9610b0868bd40b0ae531698705c1c16f552b41eb914176564cbf2f4406dcb8e4c2cb0cb71b47663f5534a22c5b252f7249"
        );
        assert_eq!(ships.size, 7_460_714);

        let rain = file_of(
            &manifest,
            "particlerain",
            "particlerain-4.0.3+26.3-fabric.jar",
        );
        assert_eq!(rain.match_glob, "particlerain-*.jar");
        assert_eq!(
            rain.url,
            "https://cdn.modrinth.com/data/nrikgvxm/versions/ddleyxF8/particlerain-4.0.3%2B26.3-fabric.jar"
        );
        assert_eq!(
            rain.sha512,
            "c0e2ecea7d1ffc76e9b95103ff2c6443374907cdf050bdfb5838073c38c042299015ceb9421b8530635cd7c0e4011002d280d158719e4a0c6ac7bc46c4c1f2df"
        );
        assert_eq!(rain.size, 628_245);

        let wakes = file_of(&manifest, "wakes", "wakes-0.7.3+26.3.jar");
        assert_eq!(
            wakes.url,
            "https://cdn.modrinth.com/data/dlNu0RQY/versions/8kcYA6A0/wakes-0.7.3%2B26.3.jar"
        );
        assert_eq!(
            wakes.sha512,
            "d74b17ddcadd72b95fa23e9f2d8397cb1e753f70792b4d1ca363ab010daa69272c341ed538175284b5926addc1e27b843b3a434da01b47afda80b164b01ae8ee"
        );
        assert_eq!(wakes.size, 292_134);

        let malilib = file_of(&manifest, "malilib", "malilib-fabric-26.3-0.30.2.jar");
        assert_eq!(
            malilib.url,
            "https://cdn.modrinth.com/data/GcWjdA9I/versions/JX9bESec/malilib-fabric-26.3-0.30.2.jar"
        );
        assert_eq!(
            malilib.sha512,
            "8f17a5d70cb2f461aeb540f4a99de01c3351f8d3678d902d3d00cfe5d10c039fdc111c1079fe23aacb7936b793ad1cd045a8dc3a9e7926865820c254d92345b8"
        );
        assert_eq!(malilib.size, 2_081_094);

        let litematica = file_of(&manifest, "litematica", "litematica-fabric-26.3-0.29.1.jar");
        assert_eq!(
            litematica.url,
            "https://cdn.modrinth.com/data/bEpr0Arc/versions/ZqV316KY/litematica-fabric-26.3-0.29.1.jar"
        );
        assert_eq!(
            litematica.sha512,
            "45a8d468463afb97b3fc72efa168024ad69e670e77f5455ecf588583434a1787b32795edf5236579c32c828b02873efb39a4808caa08bbbaf1d8b6b090b160c3"
        );
        assert_eq!(litematica.size, 1_972_556);

        let architect = file_of(&manifest, "architect", "architect-0.3.2+26.3.jar");
        assert_eq!(
            architect.url,
            "https://app.thearchitectmod.com/dl/architect-0.3.2+26.3.jar"
        );
        assert_eq!(
            architect.sha512,
            "86db71d8953a8187a81583786aa5ba078d1f46c7ef4753a047824e5fee9e26cf51bf456a9a16e9d737f4b93312dbae8f7d2fe5c95e7e533156c44d0ed28050fe"
        );
        assert_eq!(architect.size, 2_570_616);
        assert_eq!(architect.match_glob, "architect-*.jar");
    }

    #[test]
    fn github_release_files_use_the_tag_publish_checks() {
        let manifest = load_fixture_manifest().unwrap();
        let tag = crate::download::ARNOLD_RELEASE_TAG;
        assert_eq!(tag, "mods-2026.11.7");
        let prefix = format!(
            "https://github.com/{}/{}/releases/download/{tag}/",
            crate::download::RELEASE_OWNER,
            crate::download::RELEASE_REPO
        );
        let mut github_files = 0;
        let mut github_ids = std::collections::BTreeSet::new();
        for component in &manifest.components {
            for file in &component.files {
                if !file.url.starts_with("https://github.com/") {
                    continue;
                }
                github_files += 1;
                github_ids.insert(component.id.clone());
                assert_eq!(
                    file.url,
                    format!("{prefix}{}", file.file),
                    "{} must sit on {tag} or Publish mods release rejects it",
                    file.file
                );
            }
        }
        // deck, ships, snow, Diet+, plus the rehosted BA+ cosmetics:
        // unboundthirdperson, planeadvancements, audiohotkeys, weatherglass.
        assert_eq!(github_files, 8);
        assert_eq!(
            github_ids,
            [
                "deck",
                "ships",
                "snow",
                "dietplus",
                "unboundthirdperson",
                "planeadvancements",
                "audiohotkeys",
                "weatherglass",
            ]
            .into_iter()
            .map(str::to_string)
            .collect()
        );
        assert_eq!(
            manifest.loader.url,
            format!("{prefix}fabric-loader-0.20.1-26.3.json")
        );
        assert_eq!(
            manifest.loader.sha512,
            "babd24cc307a2654b95026e3a82d83e60028d9dfc2974531b45f51498352372637091ee062d7d7b29580a7c2ed8f7bccbaf69acd20d061422ed4d9c7c06436a1"
        );
    }

    #[test]
    fn diet_plus_is_in_every_kit_and_appleskin_in_full_and_recommended() {
        let manifest = load_fixture_manifest().unwrap();
        let by_id = |id: &str| {
            manifest
                .components
                .iter()
                .find(|c| c.id == id)
                .unwrap_or_else(|| panic!("missing {id}"))
        };

        // Diet+ registers a synced attribute, so once the server runs it every
        // client needs it. Keep it in every kit and locked in Custom.
        let diet = by_id("dietplus");
        assert_eq!(diet.version, "2.2.0");
        assert_eq!(diet.group, ComponentGroup::Arnold);
        assert!(diet.required_in_all_kits);
        assert_eq!(diet.requires, vec!["base".to_string()]);
        assert_eq!(diet.license.as_deref(), Some("LGPL-3.0-or-later"));
        let diet_file = file_of(&manifest, "dietplus", "dietplus-fabric-2.2.0+26.3.jar");
        assert_eq!(
            diet_file.url,
            crate::download::arnold_release_asset_url("dietplus-fabric-2.2.0+26.3.jar")
        );
        assert_eq!(
            diet_file.sha512,
            "605ceff04a93b8a243c351d0ff8f27e58e2088b24dfc95f6e07564a4e7dda941c6e9799fc453576d1d855679bb39c38c8ab198e5c04d18661f1ec4f08a8126c9"
        );
        assert_eq!(diet_file.size, 873_679);
        assert_eq!(diet_file.match_glob, "dietplus-fabric-*.jar");

        let apple = by_id("appleskin");
        assert_eq!(apple.version, "3.0.10");
        assert_eq!(apple.group, ComponentGroup::Arnold);
        assert!(!apple.required_in_all_kits);
        assert_eq!(apple.license.as_deref(), Some("Unlicense"));
        let apple_file = file_of(&manifest, "appleskin", "appleskin-fabric-mc26.3-3.0.10.jar");
        assert_eq!(
            apple_file.url,
            "https://cdn.modrinth.com/data/EsAfCjCV/versions/PHjDtQay/appleskin-fabric-mc26.3-3.0.10.jar"
        );
        assert_eq!(
            apple_file.sha512,
            "17d257af419b7530aa8617c2e8c0e4bbfabcf888f26655790e0188e7957e0bcd931f85f9a4c69fd27e889dee5082cf7b2b8741a14a6ed4425e9ef4bfe13a8b8c"
        );
        assert_eq!(apple_file.size, 180_194);
        assert_eq!(apple_file.match_glob, "appleskin-*.jar");

        for kit in ["basic", "full", "recommended"] {
            assert!(
                manifest.kits[kit]
                    .components
                    .iter()
                    .any(|id| id == "dietplus"),
                "{kit} missing dietplus"
            );
        }
        for kit in ["full", "recommended"] {
            assert!(
                manifest.kits[kit]
                    .components
                    .iter()
                    .any(|id| id == "appleskin"),
                "{kit} missing appleskin"
            );
        }
        assert!(!manifest.kits["basic"]
            .components
            .iter()
            .any(|id| id == "appleskin"));
    }

    #[test]
    fn serene_seasons_and_glitchcore_come_from_modrinth_in_every_kit() {
        let manifest = load_fixture_manifest().unwrap();
        let by_id = |id: &str| {
            manifest
                .components
                .iter()
                .find(|c| c.id == id)
                .unwrap_or_else(|| panic!("missing {id}"))
        };

        // Both are All Rights Reserved: download from Modrinth, never rehost.
        let pins = [
            (
                "glitchcore",
                "26.3.0.0.5",
                "GlitchCore-fabric-26.3-26.3.0.0.5.jar",
                "GlitchCore-fabric-*.jar",
                "https://cdn.modrinth.com/data/s3dmwKy5/versions/kfFeDnrp/GlitchCore-fabric-26.3-26.3.0.0.5.jar",
                "f975535e561285898163e7512008fe61ffaac66b6fc60a4e4763d0f2279617f75900d7a2ab2708d8dc348337119df61d2783aa101a9f1a24020f99888aa0d457",
                "33efbdb5c192e8af21f6e1ee3c2799682bd4adf8",
                339_788,
            ),
            (
                "sereneseasons",
                "26.1.2.0.7",
                "SereneSeasons-fabric-26.3-26.1.2.0.7.jar",
                "SereneSeasons-fabric-*.jar",
                "https://cdn.modrinth.com/data/e0bNACJD/versions/V9PxJPuw/SereneSeasons-fabric-26.3-26.1.2.0.7.jar",
                "f0ef802ab4a7d95df76855100656f3c0f65c7954b52330d7e3c224a0900359d8e8db8953dde3aca52231a5a90428e9925cac99ca01b22485d4af37640620069c",
                "6b3513d4f7c35cea1222143a2d2b66928d9de939",
                410_505,
            ),
        ];
        for (id, version, name, glob, url, sha512, sha1, size) in pins {
            let component = by_id(id);
            assert_eq!(component.version, version, "{id}");
            assert_eq!(component.group, ComponentGroup::Arnold, "{id}");
            assert!(component.required_in_all_kits, "{id} must be locked");
            assert_eq!(
                component.license.as_deref(),
                Some("LicenseRef-All-Rights-Reserved"),
                "{id}"
            );
            assert_eq!(component.files.len(), 1, "{id}");
            let file = file_of(&manifest, id, name);
            assert_eq!(file.match_glob, glob, "{id}");
            assert_eq!(file.url, url, "{id}");
            assert!(
                file.url.starts_with("https://cdn.modrinth.com/"),
                "{id} {}",
                file.url
            );
            assert!(
                !file.url.starts_with("https://github.com/"),
                "{id} must not be rehosted"
            );
            assert_eq!(file.sha512, sha512, "{id}");
            assert_eq!(file.sha1.as_deref(), Some(sha1), "{id}");
            assert_eq!(file.size, size, "{id}");
            for kit in ["basic", "full", "recommended"] {
                assert!(
                    manifest.kits[kit].components.iter().any(|item| item == id),
                    "{kit} missing {id}"
                );
            }
        }
        assert_eq!(by_id("glitchcore").requires, vec!["base".to_string()]);
        assert_eq!(
            by_id("sereneseasons").requires,
            vec!["base".to_string(), "glitchcore".to_string()]
        );
    }

    #[test]
    fn drink_beer_refill_comes_from_modrinth_in_every_kit() {
        let manifest = load_fixture_manifest().unwrap();
        let beer = manifest
            .components
            .iter()
            .find(|component| component.id == "drinkbeer")
            .expect("missing drinkbeer");
        assert_eq!(beer.version, "1.4.2");
        assert_eq!(beer.name, "Drink Beer Refill");
        assert_eq!(beer.group, ComponentGroup::Arnold);
        assert!(beer.required_in_all_kits, "drinkbeer must be locked");
        assert_eq!(beer.requires, vec!["base".to_string()]);
        assert_eq!(
            beer.license.as_deref(),
            Some("LicenseRef-All-Rights-Reserved")
        );
        let description = beer.description.to_ascii_lowercase();
        assert!(
            description.contains("brewing") && description.contains("bartending"),
            "{}",
            beer.description
        );
        assert!(
            beer.description
                .contains("The server runs it too, so every kit installs it."),
            "{}",
            beer.description
        );
        assert_eq!(beer.files.len(), 1);
        let file = file_of(
            &manifest,
            "drinkbeer",
            "drinkbeer-refill-26.3-fabric-1.4.2+fabric.jar",
        );
        assert_eq!(file.match_glob, "drinkbeer-refill-*.jar");
        assert_eq!(
            file.url,
            "https://cdn.modrinth.com/data/RZwVw5iA/versions/tRYmjF1L/drinkbeer-refill-26.3-fabric-1.4.2%2Bfabric.jar"
        );
        assert!(file.url.starts_with("https://cdn.modrinth.com/"));
        assert!(
            !file.url.starts_with("https://github.com/"),
            "drinkbeer must not be rehosted"
        );
        assert_eq!(
            file.sha512,
            "79ecd4aca42bd1a7f3fd109b21dd178f2c18f5690590152c0f51cc014218db796f254aaa5e84f6fbaee6ad4f7bdc9693b0dc9d14142eca867a2aed240b4b0c8e"
        );
        assert_eq!(
            file.sha1.as_deref(),
            Some("4cbc42aa098adb7bf9d0828368a65f5829044d90")
        );
        assert_eq!(file.size, 1_693_588);
        for kit in ["basic", "full", "recommended"] {
            assert!(
                manifest.kits[kit]
                    .components
                    .iter()
                    .any(|id| id == "drinkbeer"),
                "{kit} missing drinkbeer"
            );
        }
    }

    #[test]
    fn farming_and_revival_mods_come_from_modrinth_in_every_kit() {
        let manifest = load_fixture_manifest().unwrap();
        let by_id = |id: &str| {
            manifest
                .components
                .iter()
                .find(|component| component.id == id)
                .unwrap_or_else(|| panic!("missing {id}"))
        };
        let pins = [
            (
                "farmersdelight",
                "Farmer's Delight Refabricated",
                "26.3-3.6.30",
                "MIT",
                "FarmersDelight-26.3-3.6.30+refabricated.jar",
                "FarmersDelight-*.jar",
                "https://cdn.modrinth.com/data/7vxePowz/versions/dRScyUeE/FarmersDelight-26.3-3.6.30%2Brefabricated.jar",
                "099f724599022ac9232f265d62907fdde6ad48a73b246d45930d938a7f850be6a8b520244ffa218a59fdef96b0170fc0c04f00ff91f108b5a02bbc4cb2a013bc",
                "691967a54ec63e012c7447ff3960eb595ca31d13",
                3_482_244_u64,
            ),
            (
                "balm",
                "Balm",
                "26.3.0.3",
                "LicenseRef-All-Rights-Reserved",
                "balm-fabric-26.3-26.3.0.3.jar",
                "balm-fabric-*.jar",
                "https://cdn.modrinth.com/data/MBAkmtvl/versions/MYj31XNG/balm-fabric-26.3-26.3.0.3.jar",
                "aaf07f5b94f7015c87df7b242f3f45f0729fb3f72d887745ad5c5ceb31eb071e425254009158774101676d6480f31c832dd60860d79da5b661cc10b84241c319",
                "1b44d97048c1c2e9cc7e0a03c00f5cb117463a60",
                1_213_047,
            ),
            (
                "hardcorerevival",
                "Hardcore Revival",
                "26.3.0.1",
                "LicenseRef-All-Rights-Reserved",
                "hardcorerevival-fabric-26.3-26.3.0.1.jar",
                "hardcorerevival-fabric-*.jar",
                "https://cdn.modrinth.com/data/HqKoXaXz/versions/9d4cslv4/hardcorerevival-fabric-26.3-26.3.0.1.jar",
                "89df9da2716ef91234568123ab62bcc62f320956eb8355ea88e1eb5cbe317447b44e47c8a943a1735556b7e3a9a8a7b9989827d72f2b394c1cb9b776c1204638",
                "b3b4bb2937f88502c0211a14eeea3216944b0728",
                370_684,
            ),
        ];
        for (id, name, version, license, filename, glob, url, sha512, sha1, size) in pins {
            let component = by_id(id);
            assert_eq!(component.name, name, "{id}");
            assert_eq!(component.version, version, "{id}");
            assert_eq!(component.group, ComponentGroup::Arnold, "{id}");
            assert_eq!(component.kind, ComponentKind::Download, "{id}");
            assert!(component.required_in_all_kits, "{id} must be locked");
            assert_eq!(component.license.as_deref(), Some(license), "{id}");
            assert!(
                component
                    .description
                    .contains("The server runs it too, so every kit installs it."),
                "{id}"
            );
            assert_eq!(component.files.len(), 1, "{id}");
            let file = file_of(&manifest, id, filename);
            assert_eq!(file.dest, FileDest::Mods, "{id}");
            assert_eq!(file.match_glob, glob, "{id}");
            assert_eq!(file.url, url, "{id}");
            assert!(file.url.starts_with("https://cdn.modrinth.com/"), "{id}");
            assert!(
                !file.url.starts_with("https://github.com/"),
                "{id} must not be rehosted"
            );
            assert_eq!(file.sha512, sha512, "{id}");
            assert_eq!(file.sha1.as_deref(), Some(sha1), "{id}");
            assert_eq!(file.size, size, "{id}");
            for kit in ["basic", "full", "recommended"] {
                assert!(
                    manifest.kits[kit].components.iter().any(|item| item == id),
                    "{kit} missing {id}"
                );
            }
        }
        assert_eq!(by_id("farmersdelight").requires, vec!["base".to_string()]);
        assert_eq!(by_id("balm").requires, vec!["base".to_string()]);
        assert_eq!(
            by_id("hardcorerevival").requires,
            vec!["base".to_string(), "balm".to_string()]
        );
        assert_eq!(manifest.components.len(), 127);
        assert_eq!(manifest.kits["basic"].components.len(), 70);
        assert_eq!(manifest.kits["full"].components.len(), 121);
        assert_eq!(manifest.kits["recommended"].components.len(), 121);
    }

    #[test]
    fn waystones_crops_and_xaero_maps_come_from_modrinth_in_every_kit() {
        let manifest = load_fixture_manifest().unwrap();
        let by_id = |id: &str| {
            manifest
                .components
                .iter()
                .find(|component| component.id == id)
                .unwrap_or_else(|| panic!("missing {id}"))
        };
        let pins = [
            (
                "shogi",
                "Shogi",
                "26.3.0.3",
                "Library needed by Waystones. The server runs it too, so every kit installs it.",
                "shogi-fabric-26.3-26.3.0.3.jar",
                "shogi-*.jar",
                "https://cdn.modrinth.com/data/bi4iCmsw/versions/9gkJoFhj/shogi-fabric-26.3-26.3.0.3.jar",
                "f050b6360b188a82ca7e3fbb0d00eff4c02b9b39bc991fbddcf4a3588f594887d15f6421e50d5a7452a53ec35d44ec9b325d7826f77393af9543fe3383cb142d",
                "5a24c09a6b175c9fc2a0f4a9cab114c14e00aaa9",
                466_899_u64,
            ),
            (
                "waystones",
                "Waystones",
                "26.3.0.3",
                "Teleport stones for the Arnold server. The server runs it too, so every kit installs it.",
                "waystones-fabric-26.3-26.3.0.3.jar",
                "waystones-*.jar",
                "https://cdn.modrinth.com/data/LOpKHB2A/versions/prbIOsDC/waystones-fabric-26.3-26.3.0.3.jar",
                "1b4d1208382f4568f83341c7cc412354beb5c360e4f0bf0718615908e05a4a6eddc1abca8e3a7b233d273d5d95f93dff7f954072e4939f97edb8180021e83619",
                "606b37a18939e59bf0a3fbcb48db5d27ebd7f7ae",
                1_262_275,
            ),
            (
                "culturaldelights",
                "Cultural Delights Refabricated",
                "0.19.8.4-26.3-refabricated",
                "More crops and foods for Farmer's Delight. The server runs it too, so every kit installs it.",
                "culturaldelights-26.3-0.19.8.4-refabricated.jar",
                "culturaldelights-*.jar",
                "https://cdn.modrinth.com/data/YttyNOFA/versions/Tm0VloHM/culturaldelights-26.3-0.19.8.4-refabricated.jar",
                "b67f6b5183d57a3a198d2ef90df8466d2e32f426d6874e22a72f25433b84b92f445482c06c470f982fea754813dc9f4d6d8465a257a90ec50706e57bee5cd077",
                "c47a843c7e7ccef3223533c8532949acc4af9bd6",
                1_209_907,
            ),
            (
                "xaerominimap",
                "Xaero's Minimap",
                "26.6.2",
                "Minimap. Fair-play settings come from the server, so every kit gets the same map.",
                "xaerominimap-fabric-26.3-26.6.2.jar",
                "xaerominimap-*.jar",
                "https://cdn.modrinth.com/data/1bokaNcj/versions/MfxfgXaO/xaerominimap-fabric-26.3-26.6.2.jar",
                "edf57b80cfe0e0fe229728a6babc3b25c364c07a0406bcdb3a21f73fc225b98954c68ff456c7b1af3182c48512f29d47238ddc501dbfd5eaca0a4609fddcc280",
                "9b44e5718f9258ed32283a391e19a1d46464188a",
                2_233_189,
            ),
            (
                "xaeroworldmap",
                "Xaero's World Map",
                "1.47.2",
                "Full-screen world map of places you have seen. Every kit gets it so nobody is left without a map.",
                "xaeroworldmap-fabric-26.3-1.47.2.jar",
                "xaeroworldmap-*.jar",
                "https://cdn.modrinth.com/data/NcUtCpym/versions/VM8lNSSM/xaeroworldmap-fabric-26.3-1.47.2.jar",
                "cac3c0fb3121ddebc7f073571dcbce5f1811ca001ea94ef03c1da667e8a3413144f570c4b593f632358eff98fa138631ed7907fd1bd5da7d43c037ea782125fb",
                "1282302b6c92050cfab7c1db45cf3d92d9a331c5",
                1_488_669,
            ),
        ];
        for (id, name, version, description, filename, glob, url, sha512, sha1, size) in pins {
            let component = by_id(id);
            assert_eq!(component.name, name, "{id}");
            assert_eq!(component.version, version, "{id}");
            assert_eq!(component.description, description, "{id}");
            assert_eq!(component.group, ComponentGroup::Arnold, "{id}");
            assert_eq!(component.kind, ComponentKind::Download, "{id}");
            assert!(component.required_in_all_kits, "{id} must be locked");
            assert_eq!(
                component.license.as_deref(),
                Some("LicenseRef-All-Rights-Reserved"),
                "{id}"
            );
            assert_eq!(component.files.len(), 1, "{id}");
            let file = file_of(&manifest, id, filename);
            assert_eq!(file.dest, FileDest::Mods, "{id}");
            assert_eq!(file.match_glob, glob, "{id}");
            assert_eq!(file.url, url, "{id}");
            assert!(file.url.starts_with("https://cdn.modrinth.com/"), "{id}");
            assert!(
                !file.url.starts_with("https://github.com/"),
                "{id} must not be rehosted"
            );
            assert_eq!(file.sha512, sha512, "{id}");
            assert_eq!(file.sha1.as_deref(), Some(sha1), "{id}");
            assert_eq!(file.size, size, "{id}");
            for kit in ["basic", "full", "recommended"] {
                assert!(
                    manifest.kits[kit].components.iter().any(|item| item == id),
                    "{kit} missing {id}"
                );
            }
        }
        assert_eq!(by_id("shogi").requires, vec!["base".to_string()]);
        assert_eq!(
            by_id("waystones").requires,
            vec!["base".to_string(), "balm".to_string(), "shogi".to_string()]
        );
        assert_eq!(
            by_id("culturaldelights").requires,
            vec!["base".to_string(), "farmersdelight".to_string()]
        );
        assert_eq!(by_id("xaerominimap").requires, vec!["base".to_string()]);
        assert_eq!(by_id("xaeroworldmap").requires, vec!["base".to_string()]);
    }

    #[test]
    fn shippy_ships_and_ingenium_come_from_modrinth_in_every_kit() {
        let manifest = load_fixture_manifest().unwrap();
        let by_id = |id: &str| {
            manifest
                .components
                .iter()
                .find(|component| component.id == id)
                .unwrap_or_else(|| panic!("missing {id}"))
        };
        let api = by_id("ingeniumapi");
        assert_eq!(api.name, "Ingenium API");
        assert_eq!(api.version, "1.1");
        assert_eq!(api.group, ComponentGroup::Arnold);
        assert_eq!(api.kind, ComponentKind::Download);
        assert!(api.required_in_all_kits);
        assert_eq!(api.requires, vec!["base".to_string()]);
        assert_eq!(api.license.as_deref(), Some("MIT"));
        assert_eq!(
            api.description,
            "Library needed by Shippy Ships. The server runs it too, so every kit installs it."
        );
        let api_file = file_of(
            &manifest,
            "ingeniumapi",
            "ingeniumapi-1.1-FABRIC-MC-26.X.jar",
        );
        assert_eq!(api_file.match_glob, "ingeniumapi-*.jar");
        assert_eq!(
            api_file.url,
            "https://cdn.modrinth.com/data/bhnA6sV8/versions/bRhQB9Tl/ingeniumapi-1.1-FABRIC-MC-26.X.jar"
        );
        assert!(api_file.url.starts_with("https://cdn.modrinth.com/"));
        assert!(!api_file.url.starts_with("https://github.com/"));
        assert_eq!(
            api_file.sha512,
            "680ce9a30090466b8e475706d99a5bd733fc423abde58d6be668ee8afca0309ba990e497f5dbc1b63f3b30cc3f24a935f91c06fd4e9a2f93c9a5f89aa5b91445"
        );
        assert_eq!(
            api_file.sha1.as_deref(),
            Some("c932919958eaa7908a86b0074946f5b72d4ee846")
        );
        assert_eq!(api_file.size, 13_267);

        let ships = by_id("shippyships");
        assert_eq!(ships.name, "Shippy Ships");
        // Modrinth labels this release 1.0.28.3. The jar filename still says 1.0.28.2.
        assert_eq!(ships.version, "1.0.28.3");
        assert_eq!(ships.group, ComponentGroup::Arnold);
        assert!(ships.required_in_all_kits);
        assert_eq!(
            ships.requires,
            vec!["base".to_string(), "ingeniumapi".to_string()]
        );
        assert_eq!(
            ships.license.as_deref(),
            Some("LicenseRef-Custom-License---Limited-Rights-Granted")
        );
        assert!(ships.description.contains("Caesius_Leo"));
        assert!(ships.description.contains("never rehosted"));
        let file = file_of(
            &manifest,
            "shippyships",
            "shippy-ships-1.0.28.2-FABRIC-MC-26.3.jar",
        );
        assert!(file.file.contains("1.0.28.2"));
        assert!(!file.file.contains("1.0.28.3"));
        assert_eq!(file.match_glob, "shippy-ships-*.jar");
        assert_eq!(
            file.url,
            "https://cdn.modrinth.com/data/oxBNHOmi/versions/dZcofTOJ/shippy-ships-1.0.28.2-FABRIC-MC-26.3.jar"
        );
        assert!(file.url.starts_with("https://cdn.modrinth.com/"));
        assert!(!file.url.starts_with("https://github.com/"));
        assert_eq!(
            file.sha512,
            "29941b69f1c5eb5f9c3d452afcb39c60b16ebcd1dc70b050bb6b0ef12b9110562059d00dadc9c2402f9d09567d39f6f2f70e7d20340f4bf21e699e3ca8730a23"
        );
        assert_eq!(
            file.sha1.as_deref(),
            Some("57e9afc6553eb00933dbe00ebb51b733ddb827df")
        );
        assert_eq!(file.size, 2_608_709);
        for id in ["ingeniumapi", "shippyships"] {
            for kit in ["basic", "full", "recommended"] {
                assert!(
                    manifest.kits[kit].components.iter().any(|item| item == id),
                    "{kit} missing {id}"
                );
            }
        }
        for skipped in ["distanthorizons", "fastipping"] {
            assert!(
                manifest
                    .components
                    .iter()
                    .all(|component| component.id != skipped),
                "{skipped} should stay out"
            );
        }
    }

    #[test]
    fn keo_performance_mods_are_client_addons_and_keep_the_pinned_renderer() {
        let manifest = load_fixture_manifest().unwrap();
        let by_id = |id: &str| {
            manifest
                .components
                .iter()
                .find(|c| c.id == id)
                .unwrap_or_else(|| panic!("missing {id}"))
        };

        let fabric = by_id("base");
        assert_eq!(fabric.version, "0.166.2");
        assert!(fabric.required);
        let sodium = by_id("sodium");
        assert_eq!(sodium.version, "0.9.2");
        assert_eq!(by_id("shaders").version, "1.11.7");

        let added = [
            ("badoptimizations", "2.4.1", "MIT"),
            ("modernfix", "5.27.20-build.2", "LGPL-3.0-only"),
            ("sodiumextra", "0.9.5", "LGPL-3.0-only"),
            ("immediatelyfast", "1.17.1", "LGPL-3.0-or-later"),
            (
                "entityculling",
                "1.11.3",
                "LicenseRef-tr7zw-Protective-License",
            ),
            ("asynclogger", "2.2.3", "LGPL-3.0-only"),
            ("jasione", "1.0.9", "LGPL-3.0-only"),
            ("asyncparticles", "26.3.2.1", "LGPL-3.0-only"),
            ("obe", "1.2.1", "LGPL-3.0-or-later"),
        ];
        for (id, version, license) in added {
            let component = by_id(id);
            assert_eq!(component.version, version, "{id}");
            assert_eq!(component.license.as_deref(), Some(license), "{id}");
            assert_eq!(component.group, ComponentGroup::Shaders, "{id}");
            assert!(!component.required, "{id} must not be required");
            assert!(
                !component.required_in_all_kits,
                "{id} must not be forced into every kit"
            );
            assert!(
                component
                    .description
                    .to_ascii_lowercase()
                    .contains("client"),
                "{id} description should say it is client-side"
            );
            assert_eq!(component.files.len(), 1, "{id}");
            let file = &component.files[0];
            assert!(
                file.url.starts_with("https://cdn.modrinth.com/"),
                "{id} {}",
                file.url
            );
            assert!(valid_sha512(&file.sha512), "{id}");
            assert!(file.size > 0, "{id}");
            for kit in ["basic", "full", "recommended"] {
                assert!(
                    manifest.kits[kit].components.iter().any(|item| item == id),
                    "{kit} missing {id}"
                );
            }
        }

        // Lithium and Clumps are in the Better Adventures+ server set: locked in every kit.
        // Provisional: Lithium stays on our 0.26.3 (the pack ships 0.26.2). Flip here and in the manifest if Reinhardt picks 0.26.2.
        for (id, version, license) in [
            ("lithium", "0.26.3", "LGPL-3.0-only"),
            ("clumps", "26.3.2", "MIT"),
        ] {
            let component = by_id(id);
            assert_eq!(component.version, version, "{id}");
            assert_eq!(component.license.as_deref(), Some(license), "{id}");
            assert_eq!(component.group, ComponentGroup::Arnold, "{id}");
            assert!(component.required_in_all_kits, "{id} must be locked");
        }

        for skipped in [
            "fism",
            "spark",
            "zfastnoise",
            "zconfig",
            "cupboard",
            "gpumemleakfix",
            "embeddium",
        ] {
            assert!(
                manifest.components.iter().all(|c| c.id != skipped),
                "{skipped} should stay out"
            );
        }

        let extra = by_id("sodiumextra");
        assert_eq!(extra.requires, vec!["sodium".to_string()]);
        assert_eq!(extra.files[0].match_glob, "sodium-extra-*.jar");
        assert_ne!(sodium.files[0].match_glob, extra.files[0].match_glob);
    }

    #[test]
    fn tampered_manifest_is_rejected() {
        let mut bytes = BUNDLED_MANIFEST_JSON.as_bytes().to_vec();
        bytes.push(b' ');
        let sig = hex::decode(BUNDLED_MANIFEST_SIG_HEX.trim()).unwrap();
        let err = parse_and_verify(&bytes, &sig, TEST_MANIFEST_PUBLIC_KEY_HEX).unwrap_err();
        assert_eq!(err.code(), "E-SIG-INVALID");
    }

    #[test]
    fn wrong_key_is_rejected() {
        let other = SigningKey::from_bytes(&[7u8; 32]);
        let bytes = BUNDLED_MANIFEST_JSON.as_bytes();
        let sig = other.sign(bytes).to_bytes();
        let err = verify_signature(bytes, &sig, TEST_MANIFEST_PUBLIC_KEY_HEX).unwrap_err();
        assert_eq!(err.code(), "E-SIG-INVALID");
    }

    #[test]
    fn rejects_wrong_schema_and_bad_hash() {
        let mut v = valid_json();
        v["schema"] = serde_json::json!(2);
        let bytes = serde_json::to_vec(&v).unwrap();
        assert!(parse_manifest(&bytes).is_err());

        let mut v = valid_json();
        v["components"][1]["files"][0]["sha512"] = serde_json::json!("abcd");
        let bytes = serde_json::to_vec(&v).unwrap();
        let err = parse_manifest(&bytes).unwrap_err();
        assert_eq!(err.code(), "E-MANIFEST-SCHEMA");
    }

    #[test]
    fn rejects_bad_url_host() {
        let mut v = valid_json();
        v["components"][1]["files"][0]["url"] =
            serde_json::json!("https://evil.example/sodium.jar");
        let bytes = serde_json::to_vec(&v).unwrap();
        let err = parse_manifest(&bytes).unwrap_err();
        assert_eq!(err.code(), "E-URL-NOT-ALLOWED");
    }

    #[test]
    fn rejects_unknown_requires_and_cycles() {
        let mut v = valid_json();
        v["components"][1]["requires"] = serde_json::json!(["doesnotexist"]);
        let err = parse_manifest(&serde_json::to_vec(&v).unwrap()).unwrap_err();
        assert_eq!(err.code(), "E-MANIFEST-REQUIRES");

        let mut v = valid_json();
        v["components"][0]["requires"] = serde_json::json!(["sodium"]);
        v["components"][1]["requires"] = serde_json::json!(["base"]);
        let err = parse_manifest(&serde_json::to_vec(&v).unwrap()).unwrap_err();
        assert_eq!(err.code(), "E-MANIFEST-REQUIRES");
        assert!(err.to_string().contains("cycle"));
    }

    #[test]
    fn kit_missing_deck_fails() {
        let mut v = valid_json();
        v["kits"]["basic"]["components"] = serde_json::json!(["base", "sodium", "ships", "snow"]);
        let err = parse_manifest(&serde_json::to_vec(&v).unwrap()).unwrap_err();
        assert!(err.to_string().contains("deck"));
    }

    #[test]
    fn resolve_requires_adds_sodium_and_base_for_shaders() {
        let manifest = load_fixture_manifest().unwrap();
        let resolved = resolve_requires(&manifest, &["shaders".into()]).unwrap();
        assert!(resolved.contains(&"base".into()));
        assert!(resolved.contains(&"sodium".into()));
        assert!(resolved.contains(&"shaders".into()));
    }

    #[test]
    fn freshly_signed_bytes_verify() {
        let body = br#"{"ok":true}"#;
        let sig = sign(body);
        verify_signature(body, &sig, TEST_MANIFEST_PUBLIC_KEY_HEX).unwrap();
    }

    #[test]
    fn remote_manifest_is_cached_and_used_offline() {
        use crate::fetch::MapFetcher;
        use tempfile::tempdir;

        let bytes = BUNDLED_MANIFEST_JSON.as_bytes();
        let sig = hex::decode(BUNDLED_MANIFEST_SIG_HEX.trim()).unwrap();
        let mut fetcher = MapFetcher::default();
        fetcher.insert(REMOTE_MANIFEST_URL, bytes.to_vec());
        fetcher.insert(REMOTE_MANIFEST_SIG_URL, hex::encode(&sig).into_bytes());
        let tmp = tempdir().unwrap();
        let loaded =
            load_best_manifest_with_key(&fetcher, tmp.path(), TEST_MANIFEST_PUBLIC_KEY_HEX);
        assert_eq!(loaded.source, "remote");
        assert!(!loaded.offline);
        assert_eq!(loaded.manifest.minecraft, "26.3");

        let mut offline = MapFetcher::default();
        offline.fail.insert(REMOTE_MANIFEST_URL.to_string());
        let cached =
            load_best_manifest_with_key(&offline, tmp.path(), TEST_MANIFEST_PUBLIC_KEY_HEX);
        assert_eq!(cached.source, "cache");
        assert!(cached.offline);
        assert!(cached
            .message
            .as_deref()
            .unwrap_or("")
            .contains("last good"));
    }

    #[test]
    fn bad_remote_signature_falls_back_to_bundle() {
        use crate::fetch::MapFetcher;
        use tempfile::tempdir;

        let mut fetcher = MapFetcher::default();
        fetcher.insert(REMOTE_MANIFEST_URL, b"{\"tampered\":true}".to_vec());
        fetcher.insert(REMOTE_MANIFEST_SIG_URL, b"00".repeat(64));
        let tmp = tempdir().unwrap();
        let loaded =
            load_best_manifest_with_key(&fetcher, tmp.path(), TEST_MANIFEST_PUBLIC_KEY_HEX);
        assert_eq!(loaded.source, "bundled");
        assert!(loaded.offline);
    }

    #[test]
    fn quality_presets_table_matches_arnold_ships() {
        let manifest = load_fixture_manifest().unwrap();
        let table = manifest.quality_presets.expect("qualityPresets in fixture");
        assert_eq!(table.potato.ships.particles, 0.3);
        assert!(!table.potato.ships.spray);
        assert!(!table.potato.ships.hd_models);
        assert_eq!(table.lite.ships.particles, 0.5);
        assert!(!table.lite.ships.hd_models);
        assert_eq!(table.normal.ships.particles, 1.0);
        assert_eq!(table.ultra.ships.particles, 1.6);
        assert_eq!(table.normal.snow.fps_target, 60);
        assert!(table.normal.snow.adaptive_fps);
    }

    #[test]
    fn snow_lite_preset_id_is_snow_dot_lite() {
        let manifest = load_fixture_manifest().unwrap();
        let snow = component_by_id(&manifest, "snow").unwrap();
        let preset = lite_preset(snow.config.as_ref().unwrap()).unwrap();
        assert!(snow
            .config
            .as_ref()
            .unwrap()
            .presets
            .contains_key(SNOW_LITE_PRESET));
        assert_eq!(preset.marker.as_deref(), Some("# arnold-installer-lite"));
        assert_eq!(
            snow.config
                .as_ref()
                .unwrap()
                .settings
                .get("quality")
                .and_then(|s| s.default.as_str()),
            Some("normal")
        );
        assert_eq!(
            preset.overrides.get("quality").and_then(|v| v.as_str()),
            Some("lite")
        );
    }

    #[test]
    fn loader_may_use_fabric_meta() {
        let mut v = valid_json();
        v["loader"]["url"] = serde_json::json!(
            "https://meta.fabricmc.net/v2/versions/loader/26.3/0.20.1/profile/json"
        );
        parse_manifest(&serde_json::to_vec(&v).unwrap()).unwrap();
    }

    fn release_policy() -> ManifestKeyPolicy {
        ManifestKeyPolicy {
            local_source: false,
            release_build: true,
            dev_flag: true,
        }
    }

    #[test]
    fn committed_production_key_is_not_the_test_key() {
        assert_ne!(
            PRODUCTION_MANIFEST_PUBLIC_KEY_HEX,
            TEST_MANIFEST_PUBLIC_KEY_HEX
        );
        assert!(!production_manifest_key_rejected(
            PRODUCTION_MANIFEST_PUBLIC_KEY_HEX
        ));
        assert!(production_manifest_key_rejected(
            TEST_MANIFEST_PUBLIC_KEY_HEX
        ));
        assert!(production_manifest_key_rejected(
            MANIFEST_PUBLIC_KEY_PLACEHOLDER
        ));
    }

    #[test]
    fn release_public_policy_rejects_test_key_and_placeholder() {
        for bad in [
            TEST_MANIFEST_PUBLIC_KEY_HEX,
            MANIFEST_PUBLIC_KEY_PLACEHOLDER,
            "",
            "abcd",
        ] {
            let err = select_manifest_public_key(release_policy(), bad).unwrap_err();
            assert_eq!(err.code(), "E-MANIFEST-KEY");
            let payload = err.to_payload();
            assert!(
                payload.title.contains("isn't configured"),
                "{}",
                payload.title
            );
        }
    }

    #[test]
    fn release_public_policy_uses_only_the_production_key() {
        let key = select_manifest_public_key(release_policy(), PRODUCTION_MANIFEST_PUBLIC_KEY_HEX)
            .unwrap();
        assert_eq!(key, PRODUCTION_MANIFEST_PUBLIC_KEY_HEX);
        let bytes = BUNDLED_MANIFEST_JSON.as_bytes();
        let sig = hex::decode(BUNDLED_MANIFEST_SIG_HEX.trim()).unwrap();
        let err = parse_and_verify(bytes, &sig, key).unwrap_err();
        assert_eq!(err.code(), "E-SIG-INVALID");
    }

    #[test]
    fn debug_dev_flag_uses_test_key_only_when_production_key_is_unusable() {
        let debug = ManifestKeyPolicy {
            local_source: false,
            release_build: false,
            dev_flag: true,
        };
        assert_eq!(
            select_manifest_public_key(debug, MANIFEST_PUBLIC_KEY_PLACEHOLDER).unwrap(),
            TEST_MANIFEST_PUBLIC_KEY_HEX
        );
        let no_flag = ManifestKeyPolicy {
            dev_flag: false,
            ..debug
        };
        assert_eq!(
            select_manifest_public_key(no_flag, TEST_MANIFEST_PUBLIC_KEY_HEX)
                .unwrap_err()
                .code(),
            "E-MANIFEST-KEY"
        );
        assert_eq!(
            select_manifest_public_key(debug, PRODUCTION_MANIFEST_PUBLIC_KEY_HEX).unwrap(),
            PRODUCTION_MANIFEST_PUBLIC_KEY_HEX
        );
    }

    #[test]
    fn local_source_policy_keeps_the_test_key() {
        let policy = ManifestKeyPolicy {
            local_source: true,
            release_build: true,
            dev_flag: false,
        };
        assert_eq!(
            select_manifest_public_key(policy, PRODUCTION_MANIFEST_PUBLIC_KEY_HEX).unwrap(),
            TEST_MANIFEST_PUBLIC_KEY_HEX
        );
    }

    #[test]
    fn configured_generated_key_accepts_its_manifest_and_rejects_the_test_signature() {
        let mut seed = [0u8; 32];
        for (i, byte) in seed.iter_mut().enumerate() {
            *byte = u8::try_from((i * 17 + 3) % 251).unwrap_or(1);
        }
        let signing = SigningKey::from_bytes(&seed);
        let public_hex = hex::encode(signing.verifying_key().to_bytes());
        let key = select_manifest_public_key(release_policy(), &public_hex).unwrap();
        assert_eq!(key, public_hex);

        let bytes = BUNDLED_MANIFEST_JSON.as_bytes();
        let signature = signing.sign(bytes).to_bytes();
        parse_and_verify(bytes, &signature, key).unwrap();

        let test_sig = hex::decode(BUNDLED_MANIFEST_SIG_HEX.trim()).unwrap();
        let err = parse_and_verify(bytes, &test_sig, key).unwrap_err();
        assert_eq!(err.code(), "E-SIG-INVALID");

        use crate::fetch::MapFetcher;
        use tempfile::tempdir;
        let mut fetcher = MapFetcher::default();
        fetcher.insert(REMOTE_MANIFEST_URL, bytes.to_vec());
        fetcher.insert(REMOTE_MANIFEST_SIG_URL, hex::encode(signature).into_bytes());
        let tmp = tempdir().unwrap();
        let loaded = load_best_manifest_with_key(&fetcher, tmp.path(), key);
        assert_eq!(loaded.source, "remote");
        assert_eq!(loaded.manifest.minecraft, "26.3");
    }

    #[cfg(feature = "local-source")]
    #[test]
    fn active_key_follows_local_source_test_key() {
        assert_eq!(
            active_manifest_public_key().unwrap(),
            TEST_MANIFEST_PUBLIC_KEY_HEX
        );
        assert!(load_bundled_manifest().is_ok());
    }

    #[cfg(not(feature = "local-source"))]
    #[test]
    fn public_build_rejects_test_signed_remote_cache_and_bundle() {
        assert_eq!(
            active_manifest_public_key().unwrap(),
            PRODUCTION_MANIFEST_PUBLIC_KEY_HEX
        );
        assert_eq!(load_bundled_manifest().unwrap_err().code(), "E-SIG-INVALID");

        use crate::fetch::MapFetcher;
        use tempfile::tempdir;
        let bytes = BUNDLED_MANIFEST_JSON.as_bytes();
        let mut fetcher = MapFetcher::default();
        fetcher.insert(REMOTE_MANIFEST_URL, bytes.to_vec());
        fetcher.insert(
            REMOTE_MANIFEST_SIG_URL,
            BUNDLED_MANIFEST_SIG_HEX.trim().as_bytes().to_vec(),
        );
        let tmp = tempdir().unwrap();
        let loaded = load_best_manifest(&fetcher, tmp.path()).unwrap();
        assert_eq!(loaded.source, "error");
        assert!(loaded.manifest.components.is_empty());

        std::fs::write(tmp.path().join("manifest.json"), bytes).unwrap();
        std::fs::write(
            tmp.path().join("manifest.json.sig"),
            BUNDLED_MANIFEST_SIG_HEX.trim(),
        )
        .unwrap();
        let mut offline = MapFetcher::default();
        offline.fail.insert(REMOTE_MANIFEST_URL.to_string());
        let cached = load_best_manifest(&offline, tmp.path()).unwrap();
        assert_eq!(cached.source, "error");
        assert!(cached.manifest.components.is_empty());
    }

    const BA_PLUS_SERVER_SET: [&str; 42] = [
        "base",
        "fallingtree",
        "c2me",
        "soundphysics",
        "inventorysorter",
        "universalbonemeal",
        "geckolib",
        "voicechat",
        "lootr",
        "fzzyconfig",
        "clumps",
        "vanillaconfig",
        "puzzleslib",
        "hidearmor",
        "scholar",
        "forgeconfigapiport",
        "clothconfig",
        "rightclickharvest",
        "zoomify",
        "diagonalfences",
        "amplifiednether",
        "matcha",
        "cristellib",
        "architectury",
        "mru",
        "connectiblechains",
        "jakesbuildtools",
        "easyshulkerboxes",
        "tradecycling",
        "modonomicon",
        "yacl",
        "diagonalwalls",
        "streamsreflowing",
        "kotlin",
        "shulkerboxtooltip",
        "lithium",
        "axiom",
        "terrainslabs",
        "bettertrees",
        "jamlib",
        "slabbed",
        "nullscape",
    ];
    const BA_PLUS_CLIENT: [&str; 37] = [
        "freecam",
        "modmenu",
        "sounds",
        "bettersnowcoverage",
        "flyspeed",
        "betterclouds",
        "smoothsteps",
        "denseflowers",
        "bridgingmod",
        "atmosfera",
        "windy",
        "betterclimbing",
        "continuity",
        "overflowingbars",
        "visuality",
        "musiccontrol",
        "invmove",
        "bettersmoke",
        "emfcompat",
        "emf",
        "etf",
        "advancementsfullscreen",
        "punchy",
        "coolrain",
        "stacktonearbychests",
        "presencefootsteps",
        "irissearch",
        "mousetweaks",
        "smoothswapping",
        "polytone",
        "flashback",
        "disablefrontcamera",
        "mossylib",
        "bobby",
        "lambdynlights",
        "clickthrough",
        "betterf3",
    ];
    const CLIENT_EXTRAS: [&str; 5] = [
        "unboundthirdperson",
        "planeadvancements",
        "audiohotkeys",
        "miniature",
        "miniaturexl",
    ];

    fn is_lower_sha1(value: &str) -> bool {
        value.len() == 40
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    }

    #[test]
    fn ba_plus_server_set_is_locked_in_every_kit() {
        let manifest = load_fixture_manifest().unwrap();
        let by_id = |id: &str| {
            manifest
                .components
                .iter()
                .find(|c| c.id == id)
                .unwrap_or_else(|| panic!("missing {id}"))
        };
        let mut ids: Vec<&str> = BA_PLUS_SERVER_SET.to_vec();
        ids.push("midnightlib");
        for id in ids {
            let component = by_id(id);
            assert!(component.required_in_all_kits, "{id}");
            for kit in ["basic", "full", "recommended"] {
                assert!(
                    manifest.kits[kit].components.iter().any(|item| item == id),
                    "{kit} missing {id}"
                );
            }
            let group = if id == "base" {
                ComponentGroup::Base
            } else {
                ComponentGroup::Arnold
            };
            assert_eq!(component.group, group, "{id}");
            for file in &component.files {
                assert!(
                    file.url.starts_with("https://cdn.modrinth.com/"),
                    "{id} {}",
                    file.url
                );
                if id != "lithium" && id != "clumps" {
                    let sha1 = file
                        .sha1
                        .as_deref()
                        .unwrap_or_else(|| panic!("{id} missing sha1"));
                    assert!(is_lower_sha1(sha1), "{id} sha1 {sha1}");
                }
            }
        }

        let locked: std::collections::BTreeSet<String> = manifest
            .components
            .iter()
            .filter(|component| component.required_in_all_kits)
            .map(|component| component.id.clone())
            .collect();
        let mut expected: std::collections::BTreeSet<String> = BA_PLUS_SERVER_SET
            .iter()
            .map(|id| (*id).to_string())
            .collect();
        expected.insert("midnightlib".to_string());
        expected.insert("dietplus".to_string());
        expected.insert("glitchcore".to_string());
        expected.insert("sereneseasons".to_string());
        expected.insert("drinkbeer".to_string());
        expected.insert("farmersdelight".to_string());
        expected.insert("balm".to_string());
        expected.insert("hardcorerevival".to_string());
        expected.insert("shogi".to_string());
        expected.insert("waystones".to_string());
        expected.insert("culturaldelights".to_string());
        expected.insert("xaerominimap".to_string());
        expected.insert("xaeroworldmap".to_string());
        expected.insert("ingeniumapi".to_string());
        expected.insert("shippyships".to_string());
        expected.insert("deck".to_string());
        assert_eq!(locked.len(), 58);
        assert_eq!(locked, expected);
    }

    #[test]
    fn midnightlib_is_pinned_for_terrain_slabs() {
        let manifest = load_fixture_manifest().unwrap();
        let midnight = file_of(
            &manifest,
            "midnightlib",
            "midnightlib-fabric-1.9.3+26.3-rc-2.jar",
        );
        assert_eq!(
            midnight.url,
            "https://cdn.modrinth.com/data/codAaoxh/versions/wJeXgIoa/midnightlib-fabric-1.9.3%2B26.3-rc-2.jar"
        );
        assert_eq!(
            midnight.sha512,
            "2b5c5ed195d44d97dd6fafdb8583d1d883f10303c0cde331da72c6004f88cf27207167aaf77f0c692113399db42f616d1174144671898a25dfa69e9525135b8b"
        );
        assert_eq!(midnight.size, 60_602);
        assert_eq!(
            midnight.sha1.as_deref(),
            Some("2c0e2ba8a9a2b66729b6074325b40a901740238b")
        );

        let terrain = manifest
            .components
            .iter()
            .find(|component| component.id == "terrainslabs")
            .unwrap();
        assert!(terrain.requires.iter().any(|id| id == "midnightlib"));
        assert!(terrain.requires.iter().any(|id| id == "architectury"));
        let slabs = file_of(&manifest, "terrainslabs", "TerrainSlabs-v3.4.2-26.3.jar");
        assert_eq!(slabs.file, "TerrainSlabs-v3.4.2-26.3.jar");
        assert_eq!(slabs.match_glob, "Terrain*.jar");
        assert!(slabs.url.ends_with("Terrain%20Slabs%20v3.4.2-26.3.jar"));
        assert!(crate::plan::match_glob(
            "Terrain*.jar",
            "Terrain Slabs v3.4.2-26.3.jar"
        ));
    }

    #[test]
    fn ba_plus_client_mods_are_full_and_recommended_only() {
        let manifest = load_fixture_manifest().unwrap();
        let by_id = |id: &str| {
            manifest
                .components
                .iter()
                .find(|component| component.id == id)
                .unwrap_or_else(|| panic!("missing {id}"))
        };
        for id in BA_PLUS_CLIENT.into_iter().chain(CLIENT_EXTRAS) {
            let component = by_id(id);
            assert!(!component.required_in_all_kits, "{id}");
            assert_eq!(component.group, ComponentGroup::Shaders, "{id}");
            for kit in ["full", "recommended"] {
                assert!(
                    manifest.kits[kit].components.iter().any(|item| item == id),
                    "{kit} missing {id}"
                );
            }
            assert!(
                !manifest.kits["basic"]
                    .components
                    .iter()
                    .any(|item| item == id),
                "basic has {id}"
            );
            for file in &component.files {
                // Those three cosmetics are rehosted on the GitHub release.
                if matches!(
                    id,
                    "unboundthirdperson" | "planeadvancements" | "audiohotkeys"
                ) {
                    assert!(
                        file.url.starts_with("https://github.com/Spottie97/"),
                        "{id} {}",
                        file.url
                    );
                    continue;
                }
                assert!(
                    file.url.starts_with("https://cdn.modrinth.com/"),
                    "{id} {}",
                    file.url
                );
            }
        }

        for id in ["sodium", "entityculling"] {
            let component = by_id(id);
            assert_eq!(component.group, ComponentGroup::Shaders, "{id}");
            assert!(!component.required_in_all_kits, "{id}");
            for kit in ["basic", "full", "recommended"] {
                assert!(
                    manifest.kits[kit].components.iter().any(|item| item == id),
                    "{kit} missing {id}"
                );
            }
        }

        let rain = by_id("particlerain");
        assert_eq!(rain.version, "4.0.3");
        assert_eq!(rain.group, ComponentGroup::Ocean);
        for (kit_id, kit) in &manifest.kits {
            assert!(
                !kit.components.iter().any(|id| id == "particlerain"),
                "{kit_id} includes particlerain"
            );
        }
        assert!(manifest.addons["ocean"]
            .components
            .iter()
            .any(|id| id == "particlerain"));
        assert_eq!(by_id("irissearch").requires, vec!["shaders".to_string()]);
        assert_eq!(by_id("miniature").requires, vec!["shaders".to_string()]);
        assert_eq!(by_id("miniaturexl").requires, vec!["shaders".to_string()]);
    }

    #[test]
    fn client_cosmetics_are_rehosted_pack_builds() {
        type CosmeticPin<'a> = (
            &'a str,
            &'a str,
            u64,
            &'a str,
            &'a str,
            &'a str,
            Option<&'a [&'a str]>,
            Option<&'a str>,
        );
        let manifest = load_fixture_manifest().unwrap();
        let by_id = |id: &str| {
            manifest
                .components
                .iter()
                .find(|component| component.id == id)
                .unwrap_or_else(|| panic!("missing {id}"))
        };
        let prefix = "https://github.com/Spottie97/arnold-client-mod-installer/releases/download/mods-2026.11.7/";
        let pins: [CosmeticPin<'_>; 4] = [
            (
                "unboundthirdperson",
                "UnboundThirdPerson-mc26.3-v1.0.6-fabric-no-zoom-animation.jar",
                422_446,
                "d804757fbf4b934109c19556f0d7959a5005f19d4e62993d2213caa54e9cb3cc6a870e0430c44a3481ff7ae58be67bd9a95cc620c58598ae3c9a603013410d15",
                "8d3123a17ac6f2358640abd1a65f6a1e4ba9046d",
                "MIT",
                None,
                Some("43d4c15445b26decb24a350881545c6c578efaf402828c745bc5e0c179d775f14ae68498a1cda083e8b630a456f8922205a116bc5131bb3291e23509d2987a03"),
            ),
            (
                "planeadvancements",
                "plane_advancements-1.6-mc26.3-no-toolbar.jar",
                404_997,
                "91363f134aa0a0ea4f2780a4bf5f1cf82ecca9c06fb8f51699b253c87d7b63fd3d012918f78f1622b365537c2b9181292f3f54ef66a32e5b104fe159b2f4c178",
                "a9d206ff3174597a4b6600bc49e742859219cd88",
                "LGPL-3.0-only",
                Some(&["base"][..]),
                Some("d6ab8cc78d1b3ada3c4fd647823026ce5b435964c5bcbcc67ac857d85b57e7cf124255d82edefee62161f854b3ca3761076bd55ccacc34c537c255e2cbdad966"),
            ),
            (
                "audiohotkeys",
                "audio_hotkeys-1.0.14+26.3-no-overlay.jar",
                51_677,
                "3a8e391f62e0a0ee91b88f373729462482d82ed17f75f354240de4ec7bade924ccd90606c2b64afa8b0044c35bb1ba8a7ff6339aebf40c2f41600ce7fa10defb",
                "8e81b2d01ca9a66913f8da1a9c7e45eed8e7d7ec",
                "GPL-3.0-only",
                Some(&["vanillaconfig"][..]),
                Some("d68aa7ce486fa50db761336fed5882a0ebbb7a79017ed131cfb62275eecc5f5f054a8ff3080a9218c07f7ad299005588805f699287b1fe6c8d14af1049a858a6"),
            ),
            (
                "weatherglass",
                "weatherglass-0.6.0-26.3-fabric.jar",
                622_512,
                "98df091fc6082772f7025c0117faa629abef9c3bc0989a5f8a9eaf83972bd5cf0b4d5dc058bfdba2bf372e60f2af79711eb55ace1e3afb8ff031b367f4cff4c7",
                "5ebc52e155265980d507d587b4475dfed49c2713",
                "PolyForm-Shield-1.0.0",
                Some(&["base"][..]),
                None,
            ),
        ];
        for (id, filename, size, sha512, sha1, license, requires, known_stock) in pins {
            let component = by_id(id);
            assert_eq!(component.group, ComponentGroup::Shaders, "{id}");
            assert_eq!(component.kind, ComponentKind::Download, "{id}");
            assert!(!component.required_in_all_kits, "{id}");
            assert_eq!(component.license.as_deref(), Some(license), "{id}");
            assert_eq!(component.files.len(), 1, "{id}");
            let file = &component.files[0];
            assert_eq!(file.dest, FileDest::Mods, "{id}");
            assert_eq!(file.file, filename, "{id}");
            assert_eq!(file.url, format!("{prefix}{filename}"), "{id}");
            assert_eq!(file.sha512, sha512, "{id}");
            assert_eq!(file.size, size, "{id}");
            assert_eq!(file.sha1.as_deref(), Some(sha1), "{id}");
            assert!(!file.url.starts_with("https://cdn.modrinth.com"), "{id}");
            match requires {
                Some(deps) => assert_eq!(
                    component.requires,
                    deps.iter()
                        .map(|dep| (*dep).to_string())
                        .collect::<Vec<_>>(),
                    "{id}"
                ),
                None => assert!(component.requires.is_empty(), "{id}"),
            }
            match known_stock {
                Some(stock) => assert_eq!(component.known_sha512, vec![stock.to_string()], "{id}"),
                None => assert!(component.known_sha512.is_empty(), "{id}"),
            }
            for kit in ["full", "recommended"] {
                assert!(
                    manifest.kits[kit].components.iter().any(|item| item == id),
                    "{kit} missing {id}"
                );
            }
            assert!(
                !manifest.kits["basic"]
                    .components
                    .iter()
                    .any(|item| item == id),
                "basic has {id}"
            );
        }

        let stock_names = [
            "UnboundThirdPerson-mc26.3-v1.0.6-fabric.jar",
            "plane_advancements-1.6-mc26.3.jar",
            "audio_hotkeys-1.0.14+26.3.jar",
        ];
        for component in &manifest.components {
            assert_ne!(component.id, "touchnmotion");
            for file in &component.files {
                assert!(!file.file.starts_with("touch_n_motion"), "{}", file.file);
                assert!(
                    !file.file.starts_with("iris-fabric-1.11.6"),
                    "{}",
                    file.file
                );
                assert!(!stock_names.contains(&file.file.as_str()), "{}", file.file);
            }
        }
    }

    #[test]
    fn ba_plus_resource_packs_install_from_modrinth() {
        let manifest = load_fixture_manifest().unwrap();
        let by_id = |id: &str| {
            manifest
                .components
                .iter()
                .find(|component| component.id == id)
                .unwrap_or_else(|| panic!("missing {id}"))
        };
        let pins = [
            (
                "freshanimations",
                "Fresh Animations",
                "1.10.5",
                "LicenseRef-See-Terms-of-Use-in-Description",
                &["emf", "etf"][..],
                "FreshAnimations_v1.10.5.zip",
                "FreshAnimations_v*.zip",
                "https://cdn.modrinth.com/data/50dA9Sha/versions/RGIzA5em/FreshAnimations_v1.10.5.zip",
                "f4f2a1d294c2e0afe245b6987fea4a84ff120f131a7f2cfb16779e9cc5d0a00b3d49bf1585ac827d6690c36421e8dda203b0c6e642bb0c6aa067c5921dd8dc30",
                "0d069c5583d1dd4591e55ec5ff6dd0905f3f8615",
                645_816_u64,
            ),
            (
                "mfrp",
                "Matcha Flavoured resource pack",
                "1.12.7-alpha",
                "CC-BY-NC-SA-4.0",
                &[] as &[&str],
                "MFRP_1_12_7_26_3.zip",
                "MFRP_*.zip",
                "https://cdn.modrinth.com/data/QI0EmgZ1/versions/leNTHiGm/MFRP_1_12_7_26_3.zip",
                "d2d5a0fffa845298285fcc4f5142496bcf2924ed29919be9a9010043a0d03814fa9af21dcf14f641590c0d81b93b4cd91572a9de9343a5e07dd830091645c18d",
                "6982ab0a3659e27759d55fc389f7817ee1080f99",
                19_729_113,
            ),
            (
                "matchablockoverlays",
                "Matcha Block Overlays",
                "2+continuity",
                "CC-BY-NC-SA-4.0",
                &["continuity"][..],
                "Matcha-Block-Overlays-2+continuity.zip",
                "Matcha-Block-Overlays-*.zip",
                "https://cdn.modrinth.com/data/NsganuaN/versions/IcpWfApp/Matcha%20Block%20Overlays%202%2Bcontinuity.zip",
                "f3a227ab60e8b1e5dc329abb52a82104441c3f899796b7e10773a23b9d88583cec2bb0a172960e949b54bd09c28d814f2ae359c4d43d944eaf77f66bf7a69ad5",
                "11b062960ca7b5c81173b14a6b11ad87066dfb6f",
                1_227_532,
            ),
            (
                "bushypetals",
                "Bushy pink petals, wildflowers & leaf litter",
                "1.0.1",
                "Apache-2.0",
                &[] as &[&str],
                "Bushy-pink-petals-wildflowers-leaf-litter.zip",
                "Bushy-pink-petals-*.zip",
                "https://cdn.modrinth.com/data/IVU3SHSl/versions/y3AuodI0/Bushy%20pink%20petals%2C%20wildflowers%20%20leaf%20litter.zip",
                "aee85080703b79119b79b4ddcf79d7e5eccf8bbab47c0fbe03fba918bff1fbcd89368f72de6a86714b148eae86a3b59a780a0039b5bf228e5975435d4113f7ce",
                "a287f4205901ba93ed3b847e7332431df0872367",
                230_208,
            ),
            (
                "rainbowsfoliage",
                "Rainbow's Foliage: Polytone",
                "2.0",
                "LicenseRef-All-Rights-Reserved",
                &["polytone"][..],
                "rainbows-foliage-polytone-edition-v2.0.zip",
                "rainbows-foliage-*.zip",
                "https://cdn.modrinth.com/data/bY2CaVIh/versions/JcLjpSJg/rainbows-foliage-polytone-edition-v2.0.zip",
                "823a0afb2cc074046565de404bde4bbe63a9329fae9a5f95fb80ff519e0bbea831bf228d2f2e3f6985b6feba9b5d4d3141b2b91f84baeb79d97a891a0bddea76",
                "290ea3c92c46ef78e977ec74806ad77c93e97af1",
                4_434_050,
            ),
            (
                "grassrise",
                "Grassrise",
                "1.0.3",
                "LicenseRef-All-Rights-Reserved",
                &[] as &[&str],
                "Grassrise-1.0.3-mc26.3.zip",
                "Grassrise-*.zip",
                "https://cdn.modrinth.com/data/S5FvlY0N/versions/WFG9Lpck/Grassrise-1.0.3-mc26.3.zip",
                "8e03a816e982adb2059a2472c81efaf8616396486d17b7c6c3df87b5eccdad7377666a85a55492cd6a8a5c3d3890ef00a2821fbec55cce4668ef798747567d82",
                "47afa7eadaf3e62bee32d5c6887acb767f42e919",
                35_425,
            ),
        ];
        for (id, name, version, license, requires, filename, glob, url, sha512, sha1, size) in pins
        {
            let component = by_id(id);
            assert_eq!(component.name, name, "{id}");
            assert_eq!(component.version, version, "{id}");
            assert_eq!(component.license.as_deref(), Some(license), "{id}");
            assert_eq!(component.group, ComponentGroup::Shaders, "{id}");
            assert_eq!(component.kind, ComponentKind::Download, "{id}");
            assert!(!component.required_in_all_kits, "{id}");
            assert_eq!(
                component.requires,
                requires
                    .iter()
                    .map(|dep| (*dep).to_string())
                    .collect::<Vec<_>>(),
                "{id}"
            );
            assert_eq!(component.files.len(), 1, "{id}");
            let file = &component.files[0];
            assert_eq!(file.dest, FileDest::Resourcepacks, "{id}");
            assert_eq!(file.file, filename, "{id}");
            assert_eq!(file.match_glob, glob, "{id}");
            assert_eq!(file.url, url, "{id}");
            assert_eq!(file.sha512, sha512, "{id}");
            assert_eq!(file.sha1.as_deref(), Some(sha1), "{id}");
            assert_eq!(file.size, size, "{id}");
            assert!(file.url.starts_with("https://cdn.modrinth.com/"), "{id}");
            assert!(component.description.contains("Client only"), "{id}");
            assert!(component.description.contains("Not needed to join"), "{id}");
            for kit in ["full", "recommended"] {
                assert!(
                    manifest.kits[kit].components.iter().any(|item| item == id),
                    "{kit} missing {id}"
                );
            }
            assert!(
                !manifest.kits["basic"]
                    .components
                    .iter()
                    .any(|item| item == id),
                "basic has {id}"
            );
            for addon in manifest.addons.values() {
                assert!(
                    !addon.components.iter().any(|item| item == id),
                    "{id} is in an addon"
                );
            }
        }
        let mut pack_files = 0;
        for component in &manifest.components {
            for file in &component.files {
                if file.dest == FileDest::Resourcepacks {
                    pack_files += 1;
                    assert!(
                        file.url.starts_with("https://cdn.modrinth.com/"),
                        "{}",
                        file.file
                    );
                }
            }
        }
        assert_eq!(pack_files, 6);
    }

    #[test]
    fn file_dest_resourcepacks_round_trips() {
        assert_eq!(
            serde_json::from_str::<FileDest>("\"resourcepacks\"").unwrap(),
            FileDest::Resourcepacks
        );
        assert_eq!(
            serde_json::to_string(&FileDest::Resourcepacks).unwrap(),
            "\"resourcepacks\""
        );
        assert_eq!(FileDest::Mods.folder(), "mods");
        assert_eq!(FileDest::Shaderpacks.folder(), "shaderpacks");
        assert_eq!(FileDest::Resourcepacks.folder(), "resourcepacks");
        assert_eq!(
            FileDest::from_rel_path("resourcepacks/a.zip"),
            Some(FileDest::Resourcepacks)
        );
        assert_eq!(FileDest::from_rel_path("config/x"), None);
    }

    #[test]
    fn ba_plus_arr_mods_download_only_from_modrinth() {
        let manifest = load_fixture_manifest().unwrap();
        let by_id = |id: &str| {
            manifest
                .components
                .iter()
                .find(|component| component.id == id)
                .unwrap_or_else(|| panic!("missing {id}"))
        };
        for id in [
            "coolrain",
            "tradecycling",
            "flashback",
            "streamsreflowing",
            "sounds",
            "mru",
            "axiom",
            "punchy",
            "voicechat",
            "rainbowsfoliage",
            "grassrise",
            "glitchcore",
            "sereneseasons",
            "drinkbeer",
            "balm",
            "hardcorerevival",
            "shogi",
            "waystones",
            "culturaldelights",
            "xaerominimap",
            "xaeroworldmap",
        ] {
            let component = by_id(id);
            assert_eq!(
                component.license.as_deref(),
                Some("LicenseRef-All-Rights-Reserved"),
                "{id}"
            );
            for file in &component.files {
                assert!(
                    file.url.starts_with("https://cdn.modrinth.com/"),
                    "{id} {}",
                    file.url
                );
                assert!(!file.url.starts_with("https://github.com/"), "{id}");
            }
        }
        for component in &manifest.components {
            if component.license.as_deref() != Some("LicenseRef-All-Rights-Reserved") {
                continue;
            }
            for file in &component.files {
                assert!(
                    !file.url.starts_with("https://github.com/"),
                    "{}",
                    component.id
                );
            }
        }
        let fresh = by_id("freshanimations");
        assert_eq!(
            fresh.license.as_deref(),
            Some("LicenseRef-See-Terms-of-Use-in-Description")
        );
        for file in &fresh.files {
            assert!(!file.url.contains("github.com"), "{}", file.url);
        }
    }

    #[test]
    fn locked_components_only_require_locked_components() {
        let manifest = load_fixture_manifest().unwrap();
        let locked: std::collections::BTreeSet<String> = manifest
            .components
            .iter()
            .filter(|component| component.required_in_all_kits)
            .map(|component| component.id.clone())
            .collect();
        for component in &manifest.components {
            if !component.required_in_all_kits {
                continue;
            }
            for req in &component.requires {
                assert!(
                    locked.contains(req),
                    "{} requires unlocked {req}",
                    component.id
                );
            }
        }
    }

    #[test]
    fn match_globs_never_claim_another_components_file() {
        let manifest = load_fixture_manifest().unwrap();
        let mut rows: Vec<(&str, FileDest, &str, &str)> = Vec::new();
        for component in &manifest.components {
            for file in &component.files {
                assert!(
                    crate::plan::match_glob(&file.match_glob, &file.file),
                    "{} {} does not match {}",
                    component.id,
                    file.file,
                    file.match_glob
                );
                assert!(!file.file.contains(' '), "{}", file.file);
                rows.push((
                    component.id.as_str(),
                    file.dest,
                    file.file.as_str(),
                    file.match_glob.as_str(),
                ));
            }
        }
        for left in &rows {
            for right in &rows {
                if left.0 != right.0 && left.1 == right.1 {
                    assert!(
                        !crate::plan::match_glob(left.3, right.2),
                        "glob overlap {} {} ~ {} {}",
                        left.0,
                        left.3,
                        right.0,
                        right.2
                    );
                }
            }
        }
    }

    #[test]
    fn miniature_shader_ships_once() {
        let manifest = load_fixture_manifest().unwrap();
        let sha512 = "bc2d9f3d135e8608f4735e5372b893e17cdc630b7536ec1e0b9a4d2734167d1a2102e60de60731d21d8a9741cec1a82530d03bf634b7e7cf9a0f107077c0e7fa";
        let mut count = 0;
        for component in &manifest.components {
            for file in &component.files {
                if file.sha512 == sha512 && file.dest == FileDest::Shaderpacks {
                    count += 1;
                }
            }
        }
        assert_eq!(count, 1);
        let by_id = |id: &str| {
            manifest
                .components
                .iter()
                .find(|component| component.id == id)
                .unwrap()
        };
        assert_eq!(
            by_id("miniature").files[0].match_glob,
            "miniature-shader-*.zip"
        );
        assert_eq!(by_id("miniaturexl").files[0].match_glob, "MiniatureXL*.zip");
        assert!(!crate::plan::match_glob(
            "miniature-shader-*.zip",
            "MiniatureXL-1.0.zip"
        ));
        assert!(!crate::plan::match_glob(
            "MiniatureXL*.zip",
            "miniature-shader-2.19.zip"
        ));
    }

    #[test]
    fn quality_presets_never_skip_a_locked_component() {
        let manifest = load_fixture_manifest().unwrap();
        let locked: Vec<String> = manifest
            .components
            .iter()
            .filter(|component| component.required_in_all_kits)
            .map(|component| component.id.clone())
            .collect();
        let table = manifest.quality_presets.expect("qualityPresets");
        for (level, row) in [
            ("potato", &table.potato),
            ("lite", &table.lite),
            ("normal", &table.normal),
            ("ultra", &table.ultra),
        ] {
            for skip in &row.skip_components {
                assert!(
                    !locked.iter().any(|id| id == skip),
                    "{level} skips locked {skip}"
                );
            }
        }
    }
}
