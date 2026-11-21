use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::download::{is_allowed_source_url, sha512_hex};
use crate::errors::AppError;
use crate::fetch::Fetcher;
use crate::local_source::path_to_file_url;
use crate::manifest::{
    self, load_bundled_manifest_with_key, load_cached_manifest_with_key, write_manifest_cache,
    Loader, Manifest, ManifestLoad, MANIFEST_SCHEMA,
};
use crate::updater::APP_VERSION;

pub const PACK_INDEX_KIND: &str = "arnold-pack-index";
pub const PACK_INDEX_SCHEMA: u32 = 1;
pub const BA_PLUS_PACK_ID: &str = "better-adventures-plus";
pub const REMOTE_PACK_INDEX_URL: &str =
    "https://github.com/Spottie97/arnold-client-mod-installer/releases/latest/download/packs.json";
pub const REMOTE_PACK_INDEX_SIG_URL: &str = "https://github.com/Spottie97/arnold-client-mod-installer/releases/latest/download/packs.json.sig";

/// Official Matcha Flavoured project page. Opened only by `open_credit_link`.
pub const MATCHA_SOURCE_URL: &str = "https://modrinth.com/resourcepack/matcha-flavoured";
/// CC BY-NC-SA 4.0 deed. Opened only by `open_credit_link`.
pub const CC_BY_NC_SA_4_URL: &str = "https://creativecommons.org/licenses/by-nc-sa/4.0/";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PackIndex {
    pub kind: String,
    pub schema: u32,
    pub index_version: String,
    pub min_app_version: String,
    pub packs: Vec<PackEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PackEntry {
    pub id: String,
    pub name: String,
    pub minecraft: String,
    #[serde(default)]
    pub display_version: Option<String>,
    pub manifest_url: String,
    pub manifest_sig_url: String,
    #[serde(default)]
    pub manifest_sha512: Option<String>,
    #[serde(default)]
    pub art: Option<String>,
    #[serde(default)]
    pub min_app_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PackIndexLoad {
    pub index: PackIndex,
    pub source: String,
    #[serde(default)]
    pub message: Option<String>,
}

pub fn credit_url(which: &str) -> Result<&'static str, AppError> {
    match which {
        "matcha-source" => Ok(MATCHA_SOURCE_URL),
        "cc-by-nc-sa-4" => Ok(CC_BY_NC_SA_4_URL),
        _ => Err(AppError::Other {
            code: "E-CREDIT-LINK".into(),
            title: "That credit link is not available".into(),
            hint: "The installer only opens the Matcha Flavoured source page and the CC BY-NC-SA 4.0 licence.".into(),
            detail: which.to_string(),
        }),
    }
}

pub fn pack_manifest_error(detail: impl Into<String>) -> AppError {
    AppError::Other {
        code: "E-PACK-MANIFEST".into(),
        title: "The pack list could not be checked".into(),
        hint: "Nothing was changed. Try again when you are online.".into(),
        detail: detail.into(),
    }
}

pub fn folder_choice_error(detail: impl Into<String>) -> AppError {
    AppError::Other {
        code: "E-PACK-FOLDER-CHOICE".into(),
        title: "Choose where this pack should live".into(),
        hint: "Keep the current Minecraft folder, or set up a fresh pack folder. Nothing is moved until you choose.".into(),
        detail: detail.into(),
    }
}

pub fn folder_owner_error(root: &Path, reason: impl Into<String>) -> AppError {
    AppError::Other {
        code: "E-FOLDER-OWNER".into(),
        title: "We can't tell which pack this Minecraft folder belongs to".into(),
        hint: "Nothing was moved or changed. Give this pack its own folder in Settings, or pick a different Minecraft folder. If the folder came from an older installer, export the log and send it to us.".into(),
        detail: format!("{} ({})", reason.into(), root.display()),
    }
}

pub fn needs_newer_app(entry: &PackEntry) -> bool {
    entry
        .min_app_version
        .as_deref()
        .is_some_and(|required| version_greater(required, APP_VERSION))
}

pub fn version_greater(left: &str, right: &str) -> bool {
    let left = version_parts(left);
    let right = version_parts(right);
    let len = left.len().max(right.len());
    for index in 0..len {
        let a = left.get(index).copied().unwrap_or(0);
        let b = right.get(index).copied().unwrap_or(0);
        if a != b {
            return a > b;
        }
    }
    false
}

fn version_parts(raw: &str) -> Vec<u32> {
    raw.split(|c: char| !c.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .map(|part| part.parse().unwrap_or(0))
        .collect()
}

pub fn validate_pack_index(index: &PackIndex) -> Result<(), AppError> {
    validate_pack_index_inner(index, false)
}

pub fn validate_local_pack_index(index: &PackIndex) -> Result<(), AppError> {
    validate_pack_index_inner(index, true)
}

fn validate_pack_index_inner(index: &PackIndex, allow_relative: bool) -> Result<(), AppError> {
    if index.kind != PACK_INDEX_KIND {
        return Err(AppError::ManifestSchema(format!(
            "pack index kind must be {PACK_INDEX_KIND}"
        )));
    }
    if index.schema != PACK_INDEX_SCHEMA {
        return Err(AppError::ManifestSchema(format!(
            "pack index schema must be {PACK_INDEX_SCHEMA}"
        )));
    }
    let mut seen = BTreeSet::new();
    for pack in &index.packs {
        if !valid_pack_id(&pack.id) {
            return Err(AppError::ManifestSchema(format!(
                "pack id is not a safe folder name: {}",
                pack.id
            )));
        }
        if !seen.insert(pack.id.clone()) {
            return Err(AppError::ManifestSchema(format!(
                "duplicate pack id {}",
                pack.id
            )));
        }
        if pack.name.is_empty()
            || pack.name.chars().count() > 64
            || pack.name.chars().any(|c| c.is_control())
        {
            return Err(AppError::ManifestSchema(format!(
                "pack name is empty, too long, or has control characters: {}",
                pack.id
            )));
        }
        if pack.minecraft.is_empty() || pack.minecraft.chars().any(|c| c.is_control()) {
            return Err(AppError::ManifestSchema(format!(
                "pack minecraft version is missing: {}",
                pack.id
            )));
        }
        if !manifest_ref_ok(&pack.manifest_url, allow_relative)
            || !manifest_ref_ok(&pack.manifest_sig_url, allow_relative)
        {
            return Err(AppError::UrlNotAllowed {
                url: pack.manifest_url.clone(),
            });
        }
        if let Some(hash) = &pack.manifest_sha512 {
            if !valid_sha512_hex(hash) {
                return Err(AppError::ManifestSchema(format!(
                    "manifestSha512 must be 128 lowercase hex characters ({})",
                    pack.id
                )));
            }
        }
        if let Some(art) = &pack.art {
            if !safe_art_key(art) {
                return Err(AppError::ManifestSchema(format!(
                    "pack art key is not a safe name: {art}"
                )));
            }
        }
    }
    Ok(())
}

fn manifest_ref_ok(raw: &str, allow_relative: bool) -> bool {
    if raw.starts_with("file:") || is_allowed_source_url(raw) {
        return true;
    }
    allow_relative && safe_relative_ref(raw)
}

fn safe_relative_ref(raw: &str) -> bool {
    if raw.is_empty() || raw.starts_with('/') || raw.contains('\\') || raw.contains(':') {
        return false;
    }
    Path::new(raw)
        .components()
        .all(|part| matches!(part, Component::Normal(_) | Component::CurDir))
}

fn safe_art_key(art: &str) -> bool {
    !art.is_empty()
        && art.chars().count() <= 64
        && art
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn valid_pack_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    if bytes.is_empty() || bytes.len() > 48 {
        return false;
    }
    let first = bytes[0];
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        return false;
    }
    bytes
        .iter()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
}

fn valid_sha512_hex(value: &str) -> bool {
    value.len() == 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub fn parse_and_verify_index(
    bytes: &[u8],
    sig: &[u8],
    key_hex: &str,
) -> Result<PackIndex, AppError> {
    manifest::verify_signature(bytes, sig, key_hex)?;
    let index: PackIndex = serde_json::from_slice(bytes)?;
    validate_pack_index(&index)?;
    Ok(index)
}

fn parse_local_index(bytes: &[u8], sig: &[u8], key_hex: &str) -> Result<PackIndex, AppError> {
    manifest::verify_signature(bytes, sig, key_hex)?;
    let index: PackIndex = serde_json::from_slice(bytes)?;
    validate_local_pack_index(&index)?;
    Ok(index)
}

pub fn builtin_index() -> PackIndex {
    PackIndex {
        kind: PACK_INDEX_KIND.into(),
        schema: PACK_INDEX_SCHEMA,
        index_version: "2026.11.21".into(),
        min_app_version: APP_VERSION.into(),
        packs: vec![PackEntry {
            id: BA_PLUS_PACK_ID.into(),
            name: "Better Adventures+".into(),
            minecraft: "26.3".into(),
            display_version: Some("2026.11.7".into()),
            manifest_url: manifest::REMOTE_MANIFEST_URL.into(),
            manifest_sig_url: manifest::REMOTE_MANIFEST_SIG_URL.into(),
            manifest_sha512: None,
            art: Some("matcha-flavoured".into()),
            min_app_version: Some(APP_VERSION.into()),
        }],
    }
}

pub fn pack_by_id<'a>(index: &'a PackIndex, id: &str) -> Option<&'a PackEntry> {
    index.packs.iter().find(|pack| pack.id == id)
}

pub fn load_best_index<F: Fetcher>(fetcher: &F, cache_dir: &Path, key: &str) -> PackIndexLoad {
    match fetch_index(
        fetcher,
        REMOTE_PACK_INDEX_URL,
        REMOTE_PACK_INDEX_SIG_URL,
        key,
    ) {
        Ok((index, bytes, signature)) => {
            let _ = write_index_cache(cache_dir, &bytes, &signature);
            PackIndexLoad {
                index,
                source: "remote".into(),
                message: None,
            }
        }
        Err(remote_err) => match load_cached_index(cache_dir, key) {
            Ok(index) => PackIndexLoad {
                index,
                source: "cache".into(),
                message: Some(remote_err.to_string()),
            },
            Err(_) => PackIndexLoad {
                index: builtin_index(),
                source: "builtin".into(),
                message: Some(format!("pack index unavailable: {remote_err}")),
            },
        },
    }
}

fn fetch_index<F: Fetcher>(
    fetcher: &F,
    url: &str,
    sig_url: &str,
    key: &str,
) -> Result<(PackIndex, Vec<u8>, Vec<u8>), AppError> {
    let bytes = fetcher.fetch(url)?;
    let sig_raw = fetcher.fetch(sig_url)?;
    let signature = manifest::decode_signature(&sig_raw)?;
    let index = parse_and_verify_index(&bytes, &signature, key)?;
    Ok((index, bytes, signature))
}

fn index_cache_paths(cache_dir: &Path) -> (PathBuf, PathBuf) {
    (
        cache_dir.join("packs.json"),
        cache_dir.join("packs.json.sig"),
    )
}

fn write_index_cache(cache_dir: &Path, bytes: &[u8], signature: &[u8]) -> Result<(), AppError> {
    fs::create_dir_all(cache_dir)?;
    let (json_path, sig_path) = index_cache_paths(cache_dir);
    fs::write(json_path, bytes)?;
    fs::write(sig_path, hex::encode(signature))?;
    Ok(())
}

fn load_cached_index(cache_dir: &Path, key: &str) -> Result<PackIndex, AppError> {
    let (json_path, sig_path) = index_cache_paths(cache_dir);
    let bytes = fs::read(json_path)?;
    let sig_text = fs::read_to_string(sig_path)?;
    let signature = hex::decode(sig_text.trim())?;
    parse_and_verify_index(&bytes, &signature, key)
}

pub fn load_local_index(dir: &Path, key: &str) -> Result<PackIndexLoad, AppError> {
    let json_path = dir.join("packs.json");
    let sig_path = dir.join("packs.json.sig");
    if json_path.is_file() && sig_path.is_file() {
        let bytes = fs::read(&json_path)?;
        let signature = manifest::decode_signature(&fs::read(&sig_path)?)?;
        let mut index = parse_local_index(&bytes, &signature, key)?;
        for pack in &mut index.packs {
            pack.manifest_url = resolve_index_manifest_ref(&pack.manifest_url, dir)?;
            pack.manifest_sig_url = resolve_index_manifest_ref(&pack.manifest_sig_url, dir)?;
        }
        return Ok(PackIndexLoad {
            index,
            source: "local".into(),
            message: None,
        });
    }
    let manifest_path = dir.join("manifest.json");
    if !manifest_path.is_file() {
        return Err(AppError::ManifestSchema(format!(
            "manifest.json is missing in {}",
            dir.display()
        )));
    }
    let sig = dir.join("manifest.json.sig");
    Ok(PackIndexLoad {
        index: PackIndex {
            kind: PACK_INDEX_KIND.into(),
            schema: PACK_INDEX_SCHEMA,
            index_version: "local".into(),
            min_app_version: APP_VERSION.into(),
            packs: vec![PackEntry {
                id: BA_PLUS_PACK_ID.into(),
                name: "Better Adventures+".into(),
                minecraft: "26.3".into(),
                display_version: None,
                manifest_url: path_to_file_url(&manifest_path)?,
                manifest_sig_url: path_to_file_url(&sig)?,
                manifest_sha512: None,
                art: Some("matcha-flavoured".into()),
                min_app_version: Some(APP_VERSION.into()),
            }],
        },
        source: "local-synth".into(),
        message: None,
    })
}

pub fn resolve_index_manifest_ref(raw: &str, root: &Path) -> Result<String, AppError> {
    let trimmed = raw.trim();
    if trimmed.starts_with("https://") || trimmed.starts_with("http://") {
        if is_allowed_source_url(trimmed) {
            return Ok(trimmed.to_string());
        }
        return Err(AppError::UrlNotAllowed {
            url: trimmed.to_string(),
        });
    }
    if trimmed.starts_with("file:") {
        return Err(AppError::UrlNotAllowed {
            url: trimmed.to_string(),
        });
    }
    if !safe_relative_ref(trimmed) {
        return Err(AppError::UrlNotAllowed {
            url: trimmed.to_string(),
        });
    }
    let mut acc = root.to_path_buf();
    for part in Path::new(trimmed).components() {
        if let Component::Normal(name) = part {
            acc.push(name);
        }
    }
    if acc.exists() {
        let root_canon = root.canonicalize().map_err(|_| AppError::UrlNotAllowed {
            url: trimmed.to_string(),
        })?;
        let acc_canon = acc.canonicalize().map_err(|_| AppError::UrlNotAllowed {
            url: trimmed.to_string(),
        })?;
        if !acc_canon.starts_with(&root_canon) {
            return Err(AppError::UrlNotAllowed {
                url: trimmed.to_string(),
            });
        }
        return path_to_file_url(&acc_canon);
    }
    path_to_file_url(&acc)
}

pub fn load_pack_manifest<F: Fetcher>(
    fetcher: &F,
    entry: &PackEntry,
    cache_dir: &Path,
    key: &str,
) -> ManifestLoad {
    let pack_cache = cache_dir.join("packs").join(&entry.id);
    match fetch_pack_bytes(fetcher, entry) {
        Ok(bytes) => match accept_pack_bytes(&bytes, fetcher, entry, key) {
            Ok((manifest, signature)) => {
                let _ = write_manifest_cache(&pack_cache, &bytes, &signature);
                return ManifestLoad {
                    manifest,
                    source: "remote".into(),
                    offline: false,
                    message: None,
                    local_source: fetcher.local_root().is_some(),
                    local_source_dir: fetcher.local_root().map(|path| path.display().to_string()),
                };
            }
            Err(err) => {
                return error_load(err);
            }
        },
        Err(remote_err) => {
            if let Some(load) = cached_pack_manifest(&pack_cache, entry, key, &remote_err) {
                return load;
            }
            if entry.id == BA_PLUS_PACK_ID {
                if let Ok(manifest) = load_cached_manifest_with_key(cache_dir, key) {
                    let (json_path, _) = manifest::cache_paths(cache_dir);
                    if let Ok(bytes) = fs::read(json_path) {
                        if manifest_matches(entry, &manifest, &bytes) {
                            return ManifestLoad {
                                manifest,
                                source: "cache".into(),
                                offline: true,
                                message: Some(format!(
                                    "Using the last good mod list (offline). {remote_err}"
                                )),
                                local_source: false,
                                local_source_dir: None,
                            };
                        }
                    }
                }
                if let Ok(manifest) = load_bundled_manifest_with_key(key) {
                    let bytes = manifest::BUNDLED_MANIFEST_JSON.as_bytes();
                    if manifest_matches(entry, &manifest, bytes) {
                        return ManifestLoad {
                            manifest,
                            source: "bundled".into(),
                            offline: true,
                            message: Some(format!(
                                "Using the bundled mod list. Remote fetch failed: {remote_err}"
                            )),
                            local_source: false,
                            local_source_dir: None,
                        };
                    }
                }
            }
            error_load(pack_manifest_error(remote_err.to_string()))
        }
    }
}

fn fetch_pack_bytes<F: Fetcher>(fetcher: &F, entry: &PackEntry) -> Result<Vec<u8>, AppError> {
    fetcher.fetch(&entry.manifest_url)
}

fn accept_pack_bytes<F: Fetcher>(
    bytes: &[u8],
    fetcher: &F,
    entry: &PackEntry,
    key: &str,
) -> Result<(Manifest, Vec<u8>), AppError> {
    if let Some(expected) = &entry.manifest_sha512 {
        let actual = sha512_hex(bytes);
        if actual != *expected {
            return Err(pack_manifest_error(format!(
                "manifest sha512 does not match the pack index for {}",
                entry.id
            )));
        }
    }
    let sig_raw = fetcher.fetch(&entry.manifest_sig_url)?;
    let signature = manifest::decode_signature(&sig_raw)?;
    let manifest = manifest::parse_and_verify(bytes, &signature, key)?;
    if manifest.minecraft != entry.minecraft {
        return Err(pack_manifest_error(format!(
            "manifest minecraft {} does not match pack {}",
            manifest.minecraft, entry.minecraft
        )));
    }
    Ok((manifest, signature))
}

fn manifest_matches(entry: &PackEntry, manifest: &Manifest, bytes: &[u8]) -> bool {
    if manifest.minecraft != entry.minecraft {
        return false;
    }
    match &entry.manifest_sha512 {
        Some(expected) => sha512_hex(bytes) == *expected,
        None => true,
    }
}

fn cached_pack_manifest(
    pack_cache: &Path,
    entry: &PackEntry,
    key: &str,
    remote_err: &AppError,
) -> Option<ManifestLoad> {
    let (json_path, _) = manifest::cache_paths(pack_cache);
    let bytes = fs::read(json_path).ok()?;
    let manifest = load_cached_manifest_with_key(pack_cache, key).ok()?;
    if !manifest_matches(entry, &manifest, &bytes) {
        return None;
    }
    Some(ManifestLoad {
        manifest,
        source: "cache".into(),
        offline: true,
        message: Some(format!(
            "Using the last good mod list (offline). {remote_err}"
        )),
        local_source: false,
        local_source_dir: None,
    })
}

fn error_load(err: AppError) -> ManifestLoad {
    ManifestLoad {
        manifest: Manifest {
            schema: MANIFEST_SCHEMA,
            manifest_version: "unavailable".into(),
            min_app_version: APP_VERSION.into(),
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
        message: Some(err.to_string()),
        local_source: false,
        local_source_dir: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fetch::MapFetcher;
    use crate::manifest::TEST_MANIFEST_PUBLIC_KEY_HEX;
    use ed25519_dalek::{Signer, SigningKey};
    use tempfile::tempdir;

    const TEST_PRIVATE_KEY_HEX: &str =
        "48ab0651056657627942de64e86d5435d7391e1ecd922679081c2fbadc376c98";

    fn sign(bytes: &[u8]) -> Vec<u8> {
        let raw = hex::decode(TEST_PRIVATE_KEY_HEX).unwrap();
        let arr: [u8; 32] = raw.try_into().unwrap();
        SigningKey::from_bytes(&arr).sign(bytes).to_bytes().to_vec()
    }

    fn sample_index() -> serde_json::Value {
        serde_json::json!({
            "kind": PACK_INDEX_KIND,
            "schema": 1,
            "indexVersion": "2026.11.21",
            "minAppVersion": "3.2.0",
            "packs": [{
                "id": BA_PLUS_PACK_ID,
                "name": "Better Adventures+",
                "minecraft": "26.3",
                "displayVersion": "2026.11.7",
                "manifestUrl": manifest::REMOTE_MANIFEST_URL,
                "manifestSigUrl": manifest::REMOTE_MANIFEST_SIG_URL,
                "art": "matcha-flavoured",
                "minAppVersion": "3.2.0"
            }]
        })
    }

    fn signed_sample() -> (Vec<u8>, Vec<u8>) {
        let bytes = serde_json::to_vec(&sample_index()).unwrap();
        let signature = sign(&bytes);
        (bytes, signature)
    }

    #[test]
    fn test_signed_index_verifies_and_a_different_key_is_rejected() {
        let (bytes, signature) = signed_sample();
        let index =
            parse_and_verify_index(&bytes, &signature, TEST_MANIFEST_PUBLIC_KEY_HEX).unwrap();
        assert_eq!(index.packs[0].id, BA_PLUS_PACK_ID);
        let other = "11".repeat(32);
        assert!(parse_and_verify_index(&bytes, &signature, &other).is_err());
    }

    #[test]
    fn a_signed_manifest_is_rejected_as_an_index() {
        let bytes = crate::manifest::BUNDLED_MANIFEST_JSON.as_bytes();
        let signature = sign(bytes);
        let err =
            parse_and_verify_index(bytes, &signature, TEST_MANIFEST_PUBLIC_KEY_HEX).unwrap_err();
        assert_eq!(err.code(), "E-MANIFEST-SCHEMA");
    }

    #[test]
    fn rejects_bad_ids_duplicates_urls_and_hashes() {
        let mut bad = sample_index();
        for id in ["../x", "A", "", "a/b"] {
            bad["packs"][0]["id"] = serde_json::json!(id);
            let index: PackIndex = serde_json::from_value(bad.clone()).unwrap();
            assert!(validate_pack_index(&index).is_err(), "{id}");
        }
        bad = sample_index();
        let duplicate = bad["packs"][0].clone();
        bad["packs"].as_array_mut().unwrap().push(duplicate);
        let index: PackIndex = serde_json::from_value(bad).unwrap();
        assert!(validate_pack_index(&index).is_err());

        let mut evil = sample_index();
        evil["packs"][0]["manifestUrl"] = serde_json::json!("https://evil.example/m.json");
        let index: PackIndex = serde_json::from_value(evil).unwrap();
        assert_eq!(
            validate_pack_index(&index).unwrap_err().code(),
            "E-URL-NOT-ALLOWED"
        );

        let mut hash = sample_index();
        hash["packs"][0]["manifestSha512"] = serde_json::json!("abcd");
        let index: PackIndex = serde_json::from_value(hash).unwrap();
        assert!(validate_pack_index(&index).is_err());
    }

    #[test]
    fn newer_min_app_version_stays_listed_and_is_marked() {
        let mut value = sample_index();
        value["packs"][0]["minAppVersion"] = serde_json::json!("9.0.0");
        let index: PackIndex = serde_json::from_value(value).unwrap();
        validate_pack_index(&index).unwrap();
        assert!(needs_newer_app(&index.packs[0]));
        assert!(!needs_newer_app(&builtin_index().packs[0]));
    }

    #[test]
    fn builtin_index_is_ba_plus_and_uses_the_manifest_urls() {
        let index = builtin_index();
        assert_eq!(index.packs.len(), 1);
        assert_eq!(index.packs[0].id, BA_PLUS_PACK_ID);
        assert_eq!(index.packs[0].manifest_url, manifest::REMOTE_MANIFEST_URL);
        assert_eq!(
            index.packs[0].manifest_sig_url,
            manifest::REMOTE_MANIFEST_SIG_URL
        );
        assert_eq!(index.packs[0].art.as_deref(), Some("matcha-flavoured"));
    }

    #[test]
    fn load_best_index_walks_remote_then_cache_then_builtin() {
        let (bytes, signature) = signed_sample();
        let mut remote = MapFetcher::default();
        remote.insert(REMOTE_PACK_INDEX_URL, bytes.clone());
        remote.insert(
            REMOTE_PACK_INDEX_SIG_URL,
            hex::encode(&signature).into_bytes(),
        );
        let tmp = tempdir().unwrap();
        let loaded = load_best_index(&remote, tmp.path(), TEST_MANIFEST_PUBLIC_KEY_HEX);
        assert_eq!(loaded.source, "remote");

        let cache_only = tempdir().unwrap();
        write_index_cache(cache_only.path(), &bytes, &signature).unwrap();
        let loaded = load_best_index(
            &MapFetcher::default(),
            cache_only.path(),
            TEST_MANIFEST_PUBLIC_KEY_HEX,
        );
        assert_eq!(loaded.source, "cache");

        let empty = tempdir().unwrap();
        let loaded = load_best_index(
            &MapFetcher::default(),
            empty.path(),
            TEST_MANIFEST_PUBLIC_KEY_HEX,
        );
        assert_eq!(loaded.source, "builtin");
        assert_eq!(loaded.index.packs[0].id, BA_PLUS_PACK_ID);
    }

    #[test]
    fn pack_manifest_sha_or_minecraft_mismatch_is_rejected() {
        let manifest_bytes = crate::manifest::BUNDLED_MANIFEST_JSON.as_bytes().to_vec();
        let signature = sign(&manifest_bytes);
        let sig_url = "https://github.com/Spottie97/arnold-client-mod-installer/releases/latest/download/manifest.json.sig";
        let url = manifest::REMOTE_MANIFEST_URL;
        let mut fetcher = MapFetcher::default();
        fetcher.insert(url, manifest_bytes.clone());
        fetcher.insert(sig_url, hex::encode(&signature).into_bytes());
        let mut entry = builtin_index().packs[0].clone();
        entry.manifest_sha512 = Some("ab".repeat(64));
        let tmp = tempdir().unwrap();
        let loaded = load_pack_manifest(&fetcher, &entry, tmp.path(), TEST_MANIFEST_PUBLIC_KEY_HEX);
        assert_eq!(loaded.source, "error");
        assert!(loaded.message.unwrap_or_default().contains("sha512"));

        entry.manifest_sha512 = None;
        entry.minecraft = "1.0".into();
        let loaded = load_pack_manifest(&fetcher, &entry, tmp.path(), TEST_MANIFEST_PUBLIC_KEY_HEX);
        assert_eq!(loaded.source, "error");
        assert!(loaded.message.unwrap_or_default().contains("minecraft"));
    }

    #[test]
    fn builtin_ba_plus_manifest_keeps_flashback_in_recommended_and_full() {
        let mut index = builtin_index();
        let entry = index.packs.remove(0);
        let tmp = tempdir().unwrap();
        let loaded = load_pack_manifest(
            &MapFetcher::default(),
            &entry,
            tmp.path(),
            TEST_MANIFEST_PUBLIC_KEY_HEX,
        );
        assert_eq!(loaded.source, "bundled");
        let flash = loaded
            .manifest
            .components
            .iter()
            .find(|component| component.id == "flashback")
            .unwrap();
        assert_eq!(flash.version, "0.43.6");
        let kits: Vec<_> = loaded.manifest.kits.keys().cloned().collect();
        assert_eq!(
            kits,
            vec![
                "basic".to_string(),
                "full".to_string(),
                "recommended".to_string()
            ]
        );
        assert!(!loaded.manifest.kits["basic"]
            .components
            .iter()
            .any(|id| id == "flashback"));
        assert!(loaded.manifest.kits["recommended"]
            .components
            .iter()
            .any(|id| id == "flashback"));
        assert!(loaded.manifest.kits["full"]
            .components
            .iter()
            .any(|id| id == "flashback"));
    }

    #[test]
    fn pinned_offline_fallback_checks_the_cached_and_bundled_bytes() {
        let bytes = crate::manifest::BUNDLED_MANIFEST_JSON.as_bytes();
        let signature = hex::decode(crate::manifest::BUNDLED_MANIFEST_SIG_HEX.trim()).unwrap();
        let mut entry = builtin_index().packs.remove(0);
        entry.manifest_sha512 = Some("ab".repeat(64));

        let wrong_cache = tempdir().unwrap();
        let pack_cache = wrong_cache.path().join("packs").join(&entry.id);
        crate::manifest::write_manifest_cache(&pack_cache, bytes, &signature).unwrap();
        let loaded = load_pack_manifest(
            &MapFetcher::default(),
            &entry,
            wrong_cache.path(),
            TEST_MANIFEST_PUBLIC_KEY_HEX,
        );
        assert_eq!(loaded.source, "error");
        assert_eq!(loaded.manifest.manifest_version, "unavailable");

        let legacy = tempdir().unwrap();
        crate::manifest::write_manifest_cache(legacy.path(), bytes, &signature).unwrap();
        let loaded = load_pack_manifest(
            &MapFetcher::default(),
            &entry,
            legacy.path(),
            TEST_MANIFEST_PUBLIC_KEY_HEX,
        );
        assert_eq!(loaded.source, "error");

        entry.manifest_sha512 = Some(crate::download::sha512_hex(bytes));
        let matched = tempdir().unwrap();
        let pack_cache = matched.path().join("packs").join(&entry.id);
        crate::manifest::write_manifest_cache(&pack_cache, bytes, &signature).unwrap();
        let loaded = load_pack_manifest(
            &MapFetcher::default(),
            &entry,
            matched.path(),
            TEST_MANIFEST_PUBLIC_KEY_HEX,
        );
        assert_eq!(loaded.source, "cache");
        assert!(loaded.offline);

        entry.manifest_sha512 = None;
        let bundled = tempdir().unwrap();
        let loaded = load_pack_manifest(
            &MapFetcher::default(),
            &entry,
            bundled.path(),
            TEST_MANIFEST_PUBLIC_KEY_HEX,
        );
        assert_eq!(loaded.source, "bundled");
    }
}
