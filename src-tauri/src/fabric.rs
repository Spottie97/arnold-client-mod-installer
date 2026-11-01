use serde_json::{json, Value};
use std::fs;
use std::path::Path;

use crate::download::verify_sha512;
use crate::errors::AppError;
use crate::fetch::Fetcher;
use crate::manifest::Loader;

pub const ARNOLD_PROFILE_KEY: &str = "arnold-fabric-26.3";
pub const ARNOLD_PROFILE_NAME: &str = "Arnold – Fabric 26.3";

pub fn fabric_meta_profile_url(minecraft: &str, loader_version: &str) -> String {
    format!(
        "https://meta.fabricmc.net/v2/versions/loader/{minecraft}/{loader_version}/profile/json"
    )
}

pub fn loader_version_from_id(loader_id: &str) -> Option<String> {
    // fabric-loader-0.20.1-26.3
    let rest = loader_id.strip_prefix("fabric-loader-")?;
    let (version, _) = rest.rsplit_once('-')?;
    Some(version.to_string())
}

pub fn jvm_heap_gb(system_ram_gb: f64) -> u32 {
    if system_ram_gb + f64::EPSILON < 8.0 {
        2
    } else if system_ram_gb + f64::EPSILON < 16.0 {
        4
    } else if system_ram_gb + f64::EPSILON < 32.0 {
        6
    } else {
        8
    }
}

pub fn java_args_for_ram(system_ram_gb: f64) -> String {
    let heap = jvm_heap_gb(system_ram_gb);
    format!(
        "-Xmx{heap}G -Xms{}G -XX:+UnlockExperimentalVMOptions -XX:+UseG1GC",
        heap / 2
    )
}

pub fn existing_fabric_for(game_dir: &Path, minecraft: &str) -> Option<String> {
    let versions = game_dir.join("versions");
    let entries = fs::read_dir(versions).ok()?;
    let suffix = format!("-{minecraft}");
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("fabric-loader-") && name.ends_with(&suffix) {
            let json = entry.path().join(format!("{name}.json"));
            if json.is_file() {
                found.push(name);
            }
        }
    }
    found.sort();
    found.pop()
}

pub fn arnold_profile_present(game_dir: &Path) -> bool {
    let path = game_dir.join("launcher_profiles.json");
    let Ok(text) = fs::read_to_string(path) else {
        return false;
    };
    text.contains(ARNOLD_PROFILE_NAME) || text.contains(ARNOLD_PROFILE_KEY)
}

pub fn install_loader<F: Fetcher>(
    game_dir: &Path,
    loader: &Loader,
    fetcher: &F,
    system_ram_gb: f64,
    minecraft: &str,
) -> Result<LoaderInstall, AppError> {
    if let Some(existing) = existing_fabric_for(game_dir, minecraft) {
        upsert_launcher_profile(game_dir, &existing, system_ram_gb)?;
        return Ok(LoaderInstall {
            id: existing,
            fetched: false,
            profile_name: ARNOLD_PROFILE_NAME.to_string(),
        });
    }

    let bytes = fetcher.fetch_loader(&loader.url)?;
    verify_sha512(&bytes, &loader.sha512)?;
    write_version_profile(game_dir, &loader.id, &bytes)?;
    upsert_launcher_profile(game_dir, &loader.id, system_ram_gb)?;
    Ok(LoaderInstall {
        id: loader.id.clone(),
        fetched: true,
        profile_name: ARNOLD_PROFILE_NAME.to_string(),
    })
}

pub fn write_version_profile(
    game_dir: &Path,
    loader_id: &str,
    bytes: &[u8],
) -> Result<(), AppError> {
    let dir = game_dir.join("versions").join(loader_id);
    fs::create_dir_all(&dir)?;
    let dest = dir.join(format!("{loader_id}.json"));
    let tmp = dest.with_extension("json.tmp");
    fs::write(&tmp, bytes)?;
    fs::rename(tmp, dest)?;
    Ok(())
}

pub fn upsert_launcher_profile(
    game_dir: &Path,
    version_id: &str,
    system_ram_gb: f64,
) -> Result<(), AppError> {
    let path = game_dir.join("launcher_profiles.json");
    let mut root = if path.is_file() {
        serde_json::from_str(&fs::read_to_string(&path)?)?
    } else {
        json!({ "profiles": {} })
    };
    if !root.get("profiles").map(Value::is_object).unwrap_or(false) {
        root["profiles"] = json!({});
    }
    root["profiles"][ARNOLD_PROFILE_KEY] = json!({
        "name": ARNOLD_PROFILE_NAME,
        "type": "custom",
        "lastVersionId": version_id,
        "javaArgs": java_args_for_ram(system_ram_gb),
        "icon": "Furnace",
    });
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(&root)?)?;
    fs::rename(tmp, path)?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoaderInstall {
    pub id: String,
    pub fetched: bool,
    pub profile_name: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::download::sha512_hex;
    use crate::fetch::MapFetcher;
    use crate::manifest::Loader;
    use tempfile::tempdir;

    fn profile_bytes() -> Vec<u8> {
        serde_json::to_vec(&json!({
            "id": "fabric-loader-0.20.1-26.3",
            "inheritsFrom": "26.3",
            "type": "release",
            "mainClass": "net.fabricmc.loader.impl.launch.knot.KnotClient",
            "libraries": []
        }))
        .unwrap()
    }

    #[test]
    fn heap_scales_with_system_ram() {
        assert_eq!(jvm_heap_gb(6.0), 2);
        assert_eq!(jvm_heap_gb(8.0), 4);
        assert_eq!(jvm_heap_gb(16.0), 6);
        assert_eq!(jvm_heap_gb(32.0), 8);
        assert!(java_args_for_ram(16.0).contains("-Xmx6G"));
    }

    #[test]
    fn meta_url_and_id_parse() {
        assert_eq!(
            fabric_meta_profile_url("26.3", "0.20.1"),
            "https://meta.fabricmc.net/v2/versions/loader/26.3/0.20.1/profile/json"
        );
        assert_eq!(
            loader_version_from_id("fabric-loader-0.20.1-26.3").as_deref(),
            Some("0.20.1")
        );
    }

    #[test]
    fn writes_profile_and_launcher_entry() {
        let tmp = tempdir().unwrap();
        let game = tmp.path();
        let bytes = profile_bytes();
        let url = "https://meta.fabricmc.net/v2/versions/loader/26.3/0.20.1/profile/json";
        let mut fetcher = MapFetcher::default();
        fetcher.insert(url, bytes.clone());
        let loader = Loader {
            id: "fabric-loader-0.20.1-26.3".into(),
            url: url.into(),
            sha512: sha512_hex(&bytes),
        };
        let result = install_loader(game, &loader, &fetcher, 16.0, "26.3").unwrap();
        assert!(result.fetched);
        assert_eq!(result.profile_name, ARNOLD_PROFILE_NAME);
        let written = fs::read(
            game.join("versions/fabric-loader-0.20.1-26.3/fabric-loader-0.20.1-26.3.json"),
        )
        .unwrap();
        assert_eq!(written, bytes);
        let profiles = fs::read_to_string(game.join("launcher_profiles.json")).unwrap();
        assert!(profiles.contains(ARNOLD_PROFILE_NAME));
        assert!(profiles.contains("-Xmx6G"));
    }

    #[test]
    fn detects_existing_fabric_and_only_adds_profile() {
        let tmp = tempdir().unwrap();
        let game = tmp.path();
        fs::create_dir_all(game.join("versions/fabric-loader-0.19.0-26.3")).unwrap();
        fs::write(
            game.join("versions/fabric-loader-0.19.0-26.3/fabric-loader-0.19.0-26.3.json"),
            b"{}",
        )
        .unwrap();
        fs::write(
            game.join("launcher_profiles.json"),
            r#"{"profiles":{"vanilla":{"name":"Latest"}}}"#,
        )
        .unwrap();
        let loader = Loader {
            id: "fabric-loader-0.20.1-26.3".into(),
            url: "https://meta.fabricmc.net/v2/versions/loader/26.3/0.20.1/profile/json".into(),
            sha512: "0".repeat(128),
        };
        let result = install_loader(game, &loader, &MapFetcher::default(), 8.0, "26.3").unwrap();
        assert!(!result.fetched);
        assert_eq!(result.id, "fabric-loader-0.19.0-26.3");
        let profiles = fs::read_to_string(game.join("launcher_profiles.json")).unwrap();
        assert!(profiles.contains("Latest"));
        assert!(profiles.contains(ARNOLD_PROFILE_NAME));
        assert!(!game.join("versions/fabric-loader-0.20.1-26.3").exists());
    }

    #[test]
    fn hash_mismatch_does_not_write_version() {
        let tmp = tempdir().unwrap();
        let game = tmp.path();
        let url = "https://meta.fabricmc.net/v2/versions/loader/26.3/0.20.1/profile/json";
        let mut fetcher = MapFetcher::default();
        fetcher.insert(url, b"bad".to_vec());
        let loader = Loader {
            id: "fabric-loader-0.20.1-26.3".into(),
            url: url.into(),
            sha512: sha512_hex(b"good"),
        };
        let err = install_loader(game, &loader, &fetcher, 16.0, "26.3").unwrap_err();
        assert_eq!(err.code(), "E-HASH-MISMATCH");
        assert!(!game.join("versions/fabric-loader-0.20.1-26.3").exists());
    }
}
