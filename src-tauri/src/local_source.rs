use std::fs;
use std::path::{Path, PathBuf};

use url::Url;

use crate::errors::AppError;
use crate::fetch::{FetchKind, Fetcher};
use crate::manifest::{decode_signature, parse_and_verify, Manifest, ManifestLoad};

/// Shown in every local-source test build.
pub const LOCAL_SOURCE_BANNER: &str = "TEST BUILD - local source";
pub const LOCAL_SOURCE_ENV: &str = "ARNOLD_LOCAL_SOURCE";

pub const LOCAL_SETTINGS_FILES: &[(&str, &str)] = &[
    (
        "arnold-ships-client.properties",
        "config/arnold-ships-client.properties",
    ),
    (
        "arnold-snow-client.properties",
        "config/arnold-snow-client.properties",
    ),
];

pub fn is_enabled() -> bool {
    cfg!(feature = "local-source")
}

/// `--local-source <dir>`, `--local-source=<dir>`, then `ARNOLD_LOCAL_SOURCE`.
pub fn resolve_from_args_env(
    args: &[String],
    env_value: Option<&str>,
) -> Result<Option<PathBuf>, AppError> {
    if let Some(raw) = parse_cli_dir(args)? {
        return Ok(Some(existing_dir(&raw)?));
    }
    if let Some(raw) = env_value.map(str::trim).filter(|s| !s.is_empty()) {
        return Ok(Some(existing_dir(raw)?));
    }
    Ok(None)
}

fn parse_cli_dir(args: &[String]) -> Result<Option<String>, AppError> {
    let mut items = args.iter().peekable();
    if items.peek().is_some() {
        items.next();
    }
    while let Some(arg) = items.next() {
        if arg == "--local-source" {
            let value = items.next().ok_or_else(|| {
                AppError::State("TEST BUILD: --local-source needs a folder path".into())
            })?;
            return Ok(Some(value.clone()));
        }
        if let Some(value) = arg.strip_prefix("--local-source=") {
            if value.is_empty() {
                return Err(AppError::State(
                    "TEST BUILD: --local-source needs a folder path".into(),
                ));
            }
            return Ok(Some(value.to_string()));
        }
    }
    Ok(None)
}

/// `--local-source` and `ARNOLD_LOCAL_SOURCE` win. Otherwise search beside the exe.
pub fn resolve_local_source(
    args: &[String],
    env_value: Option<&str>,
    search_roots: &[PathBuf],
) -> Result<Option<PathBuf>, AppError> {
    if let Some(dir) = resolve_from_args_env(args, env_value)? {
        return Ok(Some(dir));
    }
    Ok(detect_local_source(search_roots))
}

pub fn exe_search_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            roots.push(dir.to_path_buf());
            if let Some(parent) = dir.parent() {
                roots.push(parent.to_path_buf());
            }
        }
    }
    roots
}

/// A mods folder is a direct child of the exe directory or of its parent,
/// and it holds both `manifest.json` and `manifest.json.sig`.
/// `mods-*` names win, and the newest name wins among those.
pub fn detect_local_source(search_roots: &[PathBuf]) -> Option<PathBuf> {
    let mut found = Vec::new();
    for root in search_roots {
        let Ok(entries) = fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if search_roots.iter().any(|root| same_dir(root, &path)) {
                continue;
            }
            if path.is_dir() && folder_has_signed_manifest(&path) {
                found.push(path);
            }
        }
    }
    let chosen = pick_local_source_dir(&found)?;
    Some(chosen.canonicalize().unwrap_or(chosen))
}

fn same_dir(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

pub fn folder_has_signed_manifest(path: &Path) -> bool {
    path.join("manifest.json").is_file() && path.join("manifest.json.sig").is_file()
}

fn is_mods_prefixed(name: &str) -> bool {
    name.to_ascii_lowercase().starts_with("mods-")
}

pub fn pick_local_source_dir(candidates: &[PathBuf]) -> Option<PathBuf> {
    let preferred: Vec<&PathBuf> = candidates
        .iter()
        .filter(|path| dir_name(path).is_some_and(|name| is_mods_prefixed(&name)))
        .collect();
    let pool: Vec<&PathBuf> = if preferred.is_empty() {
        candidates.iter().collect()
    } else {
        preferred
    };
    pool.into_iter()
        .max_by(|left, right| {
            let left_name = dir_name(left).unwrap_or_default();
            let right_name = dir_name(right).unwrap_or_default();
            left_name.cmp(&right_name).then_with(|| left.cmp(right))
        })
        .cloned()
}

fn dir_name(path: &Path) -> Option<String> {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
}

fn existing_dir(raw: &str) -> Result<PathBuf, AppError> {
    let path = PathBuf::from(raw);
    if !path.is_dir() {
        return Err(AppError::State(format!(
            "TEST BUILD: local source folder is missing: {raw}"
        )));
    }
    path.canonicalize().map_err(|e| {
        AppError::State(format!(
            "TEST BUILD: could not open local source folder {raw}: {e}"
        ))
    })
}

pub fn path_to_file_url(path: &Path) -> Result<String, AppError> {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    Url::from_file_path(&abs)
        .map(|url| url.to_string())
        .map_err(|_| AppError::UrlNotAllowed {
            url: abs.display().to_string(),
        })
}

pub fn is_file_url(raw: &str) -> bool {
    raw.starts_with("file:")
}

/// Read a `file://` URL only when the canonical path stays inside `root`.
pub fn read_local_file(url: &str, root: &Path) -> Result<Vec<u8>, AppError> {
    let parsed = Url::parse(url).map_err(|_| AppError::UrlNotAllowed {
        url: url.to_string(),
    })?;
    if parsed.scheme() != "file" {
        return Err(AppError::UrlNotAllowed {
            url: url.to_string(),
        });
    }
    let path = parsed.to_file_path().map_err(|_| AppError::UrlNotAllowed {
        url: url.to_string(),
    })?;
    let root = root.canonicalize().map_err(|_| AppError::UrlNotAllowed {
        url: url.to_string(),
    })?;
    let canon = path.canonicalize().map_err(|_| AppError::UrlNotAllowed {
        url: url.to_string(),
    })?;
    if !canon.starts_with(&root) {
        return Err(AppError::UrlNotAllowed {
            url: url.to_string(),
        });
    }
    Ok(fs::read(canon)?)
}

pub fn bind_manifest_to_folder(manifest: &mut Manifest, folder: &Path) {
    for component in &mut manifest.components {
        for file in &mut component.files {
            let path = folder.join(&file.file);
            if path.is_file() {
                if let Ok(url) = path_to_file_url(&path) {
                    file.url = url;
                }
            }
        }
    }
}

pub fn load_local_manifest(dir: &Path, public_key_hex: &str) -> Result<ManifestLoad, AppError> {
    let json_path = dir.join("manifest.json");
    let sig_path = dir.join("manifest.json.sig");
    if !json_path.is_file() {
        return Err(AppError::ManifestSchema(format!(
            "manifest.json is missing in {}",
            dir.display()
        )));
    }
    if !sig_path.is_file() {
        return Err(AppError::SignatureInvalid {
            detail: format!("manifest.json.sig is missing in {}", dir.display()),
        });
    }
    let bytes = fs::read(&json_path)?;
    let sig_raw = fs::read(&sig_path)?;
    let signature = decode_signature(&sig_raw)?;
    let mut manifest = parse_and_verify(&bytes, &signature, public_key_hex)?;
    bind_manifest_to_folder(&mut manifest, dir);
    Ok(ManifestLoad {
        manifest,
        source: "local".into(),
        offline: false,
        message: Some(LOCAL_SOURCE_BANNER.into()),
        local_source: true,
        local_source_dir: Some(dir.display().to_string()),
    })
}

pub fn copy_local_settings(root: &Path, game_dir: &Path) -> Result<Vec<String>, AppError> {
    let mut copied = Vec::new();
    for (name, dest) in LOCAL_SETTINGS_FILES {
        let src = root.join(name);
        if !src.is_file() {
            continue;
        }
        let dest_path = game_dir.join(dest);
        if dest_path.exists() {
            continue;
        }
        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(&src, &dest_path)?;
        copied.push((*dest).to_string());
    }
    Ok(copied)
}

/// HTTPS still goes through the inner fetcher (and its allowlist).
/// `file://` is allowed only for canonical paths inside `root`.
pub struct LocalAwareFetcher<F> {
    inner: F,
    root: Option<PathBuf>,
}

impl<F> LocalAwareFetcher<F> {
    pub fn new(inner: F, root: Option<PathBuf>) -> Self {
        Self { inner, root }
    }
}

impl<F: Fetcher> Fetcher for LocalAwareFetcher<F> {
    fn fetch_kind(&self, url: &str, kind: FetchKind) -> Result<Vec<u8>, AppError> {
        if is_file_url(url) {
            let root = self.root.as_ref().ok_or_else(|| AppError::UrlNotAllowed {
                url: url.to_string(),
            })?;
            return read_local_file(url, root);
        }
        self.inner.fetch_kind(url, kind)
    }

    fn local_root(&self) -> Option<&Path> {
        self.root.as_deref()
    }
}

#[cfg(feature = "local-source")]
pub fn pick_folder() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title("TEST BUILD — pick the folder with manifest.json")
        .pick_folder()
        .and_then(|path| path.canonicalize().ok())
}

#[cfg(feature = "local-source")]
pub fn resolve_startup() -> Result<Option<PathBuf>, AppError> {
    let env = std::env::var(LOCAL_SOURCE_ENV).ok();
    let args: Vec<String> = std::env::args().collect();
    if let Some(dir) = resolve_local_source(&args, env.as_deref(), &exe_search_roots())? {
        return Ok(Some(dir));
    }
    Ok(pick_folder())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::download::sha512_hex;
    use crate::fetch::MapFetcher;
    use crate::manifest::{
        BUNDLED_MANIFEST_JSON, BUNDLED_MANIFEST_SIG_HEX, TEST_MANIFEST_PUBLIC_KEY_HEX,
    };
    use ed25519_dalek::{Signer, SigningKey};
    use tempfile::tempdir;

    const TEST_PRIVATE_KEY_HEX: &str =
        "48ab0651056657627942de64e86d5435d7391e1ecd922679081c2fbadc376c98";

    fn sign_bytes(bytes: &[u8]) -> String {
        let raw = hex::decode(TEST_PRIVATE_KEY_HEX).unwrap();
        let arr: [u8; 32] = raw.try_into().unwrap();
        let key = SigningKey::from_bytes(&arr);
        hex::encode(key.sign(bytes).to_bytes())
    }

    fn write_signed_folder(dir: &Path, snow_bytes: &[u8]) -> String {
        let mut manifest: serde_json::Value = serde_json::from_str(BUNDLED_MANIFEST_JSON).unwrap();
        let hash = sha512_hex(snow_bytes);
        for component in manifest["components"].as_array_mut().unwrap() {
            if component["id"] == "snow" {
                for file in component["files"].as_array_mut().unwrap() {
                    if file["file"] == "arnold-snowfall-1.3.3.jar" {
                        file["sha512"] = serde_json::Value::String(hash.clone());
                        file["size"] = serde_json::Value::from(snow_bytes.len() as u64);
                    }
                }
            }
        }
        let json = serde_json::to_vec_pretty(&manifest).unwrap();
        fs::write(dir.join("manifest.json"), &json).unwrap();
        fs::write(dir.join("manifest.json.sig"), sign_bytes(&json)).unwrap();
        fs::write(dir.join("arnold-snowfall-1.3.3.jar"), snow_bytes).unwrap();
        fs::write(
            dir.join("arnold-snow-client.properties"),
            "quality=normal\nfpsTarget=60\nadaptiveFps=true\n",
        )
        .unwrap();
        hash
    }

    #[test]
    fn feature_flag_matches_cfg() {
        assert_eq!(is_enabled(), cfg!(feature = "local-source"));
    }

    #[test]
    fn cli_and_env_resolve_existing_folder() {
        let tmp = tempdir().unwrap();
        let dir = tmp.path().canonicalize().unwrap();
        let args = vec![
            "arnold-client-mods".into(),
            "--local-source".into(),
            dir.display().to_string(),
        ];
        let got = resolve_from_args_env(&args, None).unwrap().unwrap();
        assert_eq!(got, dir);

        let eq = vec![
            "arnold-client-mods".into(),
            format!("--local-source={}", dir.display()),
        ];
        assert_eq!(resolve_from_args_env(&eq, None).unwrap().unwrap(), dir);

        let env_only =
            resolve_from_args_env(&["arnold-client-mods".into()], Some(dir.to_str().unwrap()))
                .unwrap()
                .unwrap();
        assert_eq!(env_only, dir);

        assert!(
            resolve_from_args_env(&["arnold-client-mods".into()], Some(""))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn missing_folder_is_an_error() {
        let args = vec![
            "app".into(),
            "--local-source".into(),
            "/no/such/arnold-local-source-dir".into(),
        ];
        let err = resolve_from_args_env(&args, None).unwrap_err();
        assert_eq!(err.code(), "E-STATE");
    }

    #[test]
    fn load_local_verifies_sig_and_rewrites_present_jars() {
        let tmp = tempdir().unwrap();
        let snow = b"local-snow-jar";
        write_signed_folder(tmp.path(), snow);
        let loaded = load_local_manifest(tmp.path(), TEST_MANIFEST_PUBLIC_KEY_HEX).unwrap();
        assert_eq!(loaded.source, "local");
        assert!(loaded.local_source);
        assert_eq!(loaded.message.as_deref(), Some(LOCAL_SOURCE_BANNER));
        let snow_file = loaded
            .manifest
            .components
            .iter()
            .find(|c| c.id == "snow")
            .unwrap()
            .files
            .first()
            .unwrap();
        assert!(snow_file.url.starts_with("file:"), "{}", snow_file.url);
        let sodium = loaded
            .manifest
            .components
            .iter()
            .find(|c| c.id == "sodium")
            .unwrap()
            .files
            .first()
            .unwrap();
        assert!(sodium.url.starts_with("https://"));
    }

    #[test]
    fn bad_signature_is_rejected() {
        let tmp = tempdir().unwrap();
        write_signed_folder(tmp.path(), b"jar");
        fs::write(tmp.path().join("manifest.json.sig"), "00".repeat(64)).unwrap();
        let err = load_local_manifest(tmp.path(), TEST_MANIFEST_PUBLIC_KEY_HEX).unwrap_err();
        assert_eq!(err.code(), "E-SIG-INVALID");
    }

    #[test]
    fn bundled_sig_still_loads_when_copied() {
        let tmp = tempdir().unwrap();
        fs::write(tmp.path().join("manifest.json"), BUNDLED_MANIFEST_JSON).unwrap();
        fs::write(
            tmp.path().join("manifest.json.sig"),
            BUNDLED_MANIFEST_SIG_HEX.trim(),
        )
        .unwrap();
        let loaded = load_local_manifest(tmp.path(), TEST_MANIFEST_PUBLIC_KEY_HEX).unwrap();
        assert_eq!(loaded.manifest.schema, 3);
        assert_eq!(loaded.source, "local");
    }

    #[test]
    fn file_url_stays_inside_the_folder() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("root");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("ok.jar"), b"inside").unwrap();
        let outside = tmp.path().join("outside.jar");
        fs::write(&outside, b"nope").unwrap();
        let ok = path_to_file_url(&root.join("ok.jar")).unwrap();
        assert_eq!(read_local_file(&ok, &root).unwrap(), b"inside");
        let escaped = path_to_file_url(&outside).unwrap();
        let err = read_local_file(&escaped, &root).unwrap_err();
        assert_eq!(err.code(), "E-URL-NOT-ALLOWED");
    }

    #[test]
    fn fetcher_uses_folder_for_file_and_allowlist_for_https() {
        let tmp = tempdir().unwrap();
        fs::write(tmp.path().join("mod.jar"), b"from-disk").unwrap();
        let file_url = path_to_file_url(&tmp.path().join("mod.jar")).unwrap();
        let https = "https://cdn.modrinth.com/data/AANobbMI/versions/sodium.jar";
        let mut map = MapFetcher::default();
        map.insert(https, b"from-net".to_vec());
        let fetcher = LocalAwareFetcher::new(map, Some(tmp.path().to_path_buf()));
        assert_eq!(fetcher.fetch(&file_url).unwrap(), b"from-disk");
        assert_eq!(fetcher.fetch(https).unwrap(), b"from-net");
        let err = fetcher.fetch("https://evil.example/a.jar").unwrap_err();
        assert_eq!(err.code(), "E-URL-NOT-ALLOWED");
    }

    #[test]
    fn file_url_without_root_is_rejected() {
        let tmp = tempdir().unwrap();
        fs::write(tmp.path().join("mod.jar"), b"x").unwrap();
        let url = path_to_file_url(&tmp.path().join("mod.jar")).unwrap();
        let fetcher = LocalAwareFetcher::new(MapFetcher::default(), None);
        let err = fetcher.fetch(&url).unwrap_err();
        assert_eq!(err.code(), "E-URL-NOT-ALLOWED");
    }

    fn touch_manifest(dir: &Path) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join("manifest.json"), b"{}").unwrap();
        fs::write(dir.join("manifest.json.sig"), b"00").unwrap();
    }

    #[test]
    fn detects_mods_folder_beside_the_exe_and_prefers_newest_mods_name() {
        let tmp = tempdir().unwrap();
        let exe_dir = tmp.path().join("app");
        fs::create_dir_all(&exe_dir).unwrap();
        touch_manifest(&exe_dir.join("mods-2026.11.1"));
        touch_manifest(&exe_dir.join("mods-2026.11.2"));
        touch_manifest(&exe_dir.join("release-mods-2026.12.9"));
        fs::write(exe_dir.join("manifest.json"), b"{}").unwrap();
        fs::write(exe_dir.join("manifest.json.sig"), b"00").unwrap();
        let got = detect_local_source(&[exe_dir.clone(), tmp.path().to_path_buf()]).unwrap();
        assert!(got.ends_with("mods-2026.11.2"), "{}", got.display());
    }

    #[test]
    fn detects_a_folder_in_the_parent_and_ignores_unsigned_dirs() {
        let tmp = tempdir().unwrap();
        let exe_dir = tmp.path().join("Program Files").join("Arnold Client Mods");
        fs::create_dir_all(&exe_dir).unwrap();
        let unsigned = tmp.path().join("mods-unsigned");
        fs::create_dir_all(&unsigned).unwrap();
        fs::write(unsigned.join("manifest.json"), b"{}").unwrap();
        touch_manifest(&tmp.path().join("mods-2026.11.1"));
        let got = detect_local_source(&[exe_dir, tmp.path().to_path_buf()]).unwrap();
        assert!(got.ends_with("mods-2026.11.1"), "{}", got.display());
    }

    #[test]
    fn cli_and_env_win_over_a_folder_beside_the_exe() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("bundle");
        fs::create_dir_all(&root).unwrap();
        touch_manifest(&root.join("mods-auto"));
        let explicit = tmp.path().join("explicit");
        fs::create_dir_all(&explicit).unwrap();
        let explicit = explicit.canonicalize().unwrap();
        let args = vec![
            "arnold-client-mods".into(),
            "--local-source".into(),
            explicit.display().to_string(),
        ];
        let roots = [root];
        let got = resolve_local_source(&args, None, &roots).unwrap().unwrap();
        assert_eq!(got, explicit);

        let from_env = resolve_local_source(
            &["arnold-client-mods".into()],
            Some(explicit.to_str().unwrap()),
            &roots,
        )
        .unwrap()
        .unwrap();
        assert_eq!(from_env, explicit);

        let auto = resolve_local_source(&["arnold-client-mods".into()], Some(""), &roots)
            .unwrap()
            .unwrap();
        assert!(auto.ends_with("mods-auto"), "{}", auto.display());
    }

    #[test]
    fn missing_cli_folder_does_not_fall_through_to_auto_detect() {
        let tmp = tempdir().unwrap();
        touch_manifest(&tmp.path().join("mods-auto"));
        let args = vec![
            "app".into(),
            "--local-source".into(),
            "/no/such/arnold-local-source-dir".into(),
        ];
        let err = resolve_local_source(&args, None, &[tmp.path().to_path_buf()]).unwrap_err();
        assert_eq!(err.code(), "E-STATE");
    }

    #[test]
    fn copies_settings_when_missing() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("src");
        let game = tmp.path().join("game");
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("arnold-snow-client.properties"),
            "quality=normal\n",
        )
        .unwrap();
        let copied = copy_local_settings(&root, &game).unwrap();
        assert_eq!(
            copied,
            vec!["config/arnold-snow-client.properties".to_string()]
        );
        assert_eq!(
            fs::read_to_string(game.join("config/arnold-snow-client.properties")).unwrap(),
            "quality=normal\n"
        );
        fs::write(
            game.join("config/arnold-snow-client.properties"),
            "quality=ultra\n",
        )
        .unwrap();
        let again = copy_local_settings(&root, &game).unwrap();
        assert!(again.is_empty());
        assert_eq!(
            fs::read_to_string(game.join("config/arnold-snow-client.properties")).unwrap(),
            "quality=ultra\n"
        );
    }
}
