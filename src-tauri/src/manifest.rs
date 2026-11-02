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

pub const BUNDLED_MANIFEST_JSON: &str = include_str!("../../fixtures/manifest.v3.json");
pub const BUNDLED_MANIFEST_SIG_HEX: &str = include_str!("../../fixtures/manifest.v3.json.sig");

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
            chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
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
    let sig = hex::decode(BUNDLED_MANIFEST_SIG_HEX.trim())?;
    parse_and_verify(
        BUNDLED_MANIFEST_JSON.as_bytes(),
        &sig,
        TEST_MANIFEST_PUBLIC_KEY_HEX,
    )
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

pub fn load_cached_manifest(cache_dir: &Path, public_key_hex: &str) -> Result<Manifest, AppError> {
    let (json_path, sig_path) = cache_paths(cache_dir);
    let bytes = fs::read(json_path)?;
    let sig_text = fs::read_to_string(sig_path)?;
    let signature = hex::decode(sig_text.trim())?;
    parse_and_verify(&bytes, &signature, public_key_hex)
}

pub fn fetch_remote_manifest<F: Fetcher>(
    fetcher: &F,
    public_key_hex: &str,
) -> Result<(Manifest, Vec<u8>, Vec<u8>), AppError> {
    let bytes = fetcher.fetch(REMOTE_MANIFEST_URL)?;
    let sig_raw = fetcher.fetch(REMOTE_MANIFEST_SIG_URL)?;
    let signature = decode_signature(&sig_raw)?;
    let manifest = parse_and_verify(&bytes, &signature, public_key_hex)?;
    Ok((manifest, bytes, signature))
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

/// Remote first, then last-good cache, then the signed bundle. `offline` is set
/// when the network copy could not be used.
pub fn load_best_manifest<F: Fetcher>(
    fetcher: &F,
    cache_dir: &Path,
    public_key_hex: &str,
) -> ManifestLoad {
    match fetch_remote_manifest(fetcher, public_key_hex) {
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
        Err(remote_err) => match load_cached_manifest(cache_dir, public_key_hex) {
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
            Err(_) => match load_bundled_manifest() {
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
                Err(bundled_err) => ManifestLoad {
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
                },
            },
        },
    }
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
        let manifest = load_bundled_manifest().unwrap();
        assert_eq!(manifest.schema, 3);
        assert_eq!(manifest.minecraft, "26.3");
        assert_eq!(
            manifest.current_server_shipwright.as_deref(),
            Some("0.11.1")
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
        let manifest = load_bundled_manifest().unwrap();

        let fabric = file_of(&manifest, "base", "fabric-api-0.165.0+26.3.jar");
        assert_eq!(
            fabric.url,
            "https://cdn.modrinth.com/data/P7dR8mSH/versions/EQKgpeKu/fabric-api-0.165.0%2B26.3.jar"
        );
        assert_eq!(
            fabric.sha512,
            "0a4b8a14e847b71567b557b43d906c2167fa263eeeef37454a821ba286996e3da1616dc2d9647736d28a250d5ecf400541cae808359b29a62324a8a6f2ad5e30"
        );
        assert_eq!(fabric.size, 1_778_588);

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

        let ships = file_of(&manifest, "ships", "arnold-ships-client-0.10.2.jar");
        assert_eq!(
            ships.sha512,
            "6d8f1ead05738a551c98b61fd50c6cedcee9e4e8ec7f15426a534af1498589b30a1a1aae416c4a0d44ce232e6ba9387f75845ca6084f6d4b9bed69eae8b7497f"
        );
        assert_eq!(ships.size, 7_460_810);

        let rain = file_of(
            &manifest,
            "particlerain",
            "particlerain-4.0.2+26.3-fabric.jar",
        );
        assert_eq!(rain.match_glob, "particlerain-*.jar");
        assert_eq!(
            rain.url,
            "https://cdn.modrinth.com/data/nrikgvxm/versions/ecVAaY1R/particlerain-4.0.2%2B26.3-fabric.jar"
        );
        assert_eq!(
            rain.sha512,
            "b87684651246c05d0985c88cc0e05ba0e5bdf04d6e5c3ec90962d4342d9f70bc13a4ca86a4276022d71afdcfd69709596e070f3cfe1ae0ee3484dc2924121f19"
        );
        assert_eq!(rain.size, 627_188);

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
        let manifest = load_bundled_manifest().unwrap();
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
        let loaded = load_best_manifest(&fetcher, tmp.path(), TEST_MANIFEST_PUBLIC_KEY_HEX);
        assert_eq!(loaded.source, "remote");
        assert!(!loaded.offline);
        assert_eq!(loaded.manifest.minecraft, "26.3");

        let mut offline = MapFetcher::default();
        offline.fail.insert(REMOTE_MANIFEST_URL.to_string());
        let cached = load_best_manifest(&offline, tmp.path(), TEST_MANIFEST_PUBLIC_KEY_HEX);
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
        let loaded = load_best_manifest(&fetcher, tmp.path(), TEST_MANIFEST_PUBLIC_KEY_HEX);
        assert_eq!(loaded.source, "bundled");
        assert!(loaded.offline);
    }

    #[test]
    fn quality_presets_table_matches_arnold_ships() {
        let manifest = load_bundled_manifest().unwrap();
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
        let manifest = load_bundled_manifest().unwrap();
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
}
