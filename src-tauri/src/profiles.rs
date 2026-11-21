use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::errors::AppError;
use crate::fabric::{ARNOLD_PROFILE_KEY, ARNOLD_PROFILE_NAME};

/// VERIFY schema against a real TlauncherProfiles.json before shipping.
/// The writer uses the official launcher field names when the file is missing
/// or already has a `profiles` object. Any other shape is left byte-for-byte.
pub const TLAUNCHER_PROFILES_FILE: &str = "TlauncherProfiles.json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileSpec {
    pub key: String,
    pub name: String,
    pub version_id: String,
    pub game_dir: PathBuf,
    pub java_args: String,
    /// When the legacy `arnold-fabric-26.3` entry exists, update that entry
    /// instead of adding a second profile.
    pub update_legacy_in_place: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileTargets {
    pub official: Option<PathBuf>,
    pub tlauncher: Option<PathBuf>,
    pub also_pack_dir: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TlWrite {
    Written,
    SkippedUnknownShape,
}

pub fn upsert_official(
    launcher_root: &Path,
    spec: &ProfileSpec,
    backup_dir: &Path,
) -> Result<(), AppError> {
    let path = launcher_root.join("launcher_profiles.json");
    upsert_profile_file(&path, spec, backup_dir)
}

pub fn upsert_tlauncher(
    tl_game_dir: &Path,
    spec: &ProfileSpec,
    backup_dir: &Path,
) -> Result<TlWrite, AppError> {
    let path = tl_game_dir.join(TLAUNCHER_PROFILES_FILE);
    if path.is_file() {
        let original = fs::read(&path)?;
        let parsed: Value = match serde_json::from_slice(&original) {
            Ok(value) => value,
            Err(_) => return Ok(TlWrite::SkippedUnknownShape),
        };
        if !parsed
            .get("profiles")
            .map(Value::is_object)
            .unwrap_or(false)
        {
            return Ok(TlWrite::SkippedUnknownShape);
        }
        upsert_profile_file(&path, spec, backup_dir)?;
        return Ok(TlWrite::Written);
    }
    upsert_profile_file(&path, spec, backup_dir)?;
    Ok(TlWrite::Written)
}

fn upsert_profile_file(path: &Path, spec: &ProfileSpec, backup_dir: &Path) -> Result<(), AppError> {
    let existing = if path.is_file() {
        Some(fs::read(path)?)
    } else {
        None
    };
    if existing.is_some() {
        backup_profile_file(path, backup_dir)?;
    }
    let mut root = if let Some(bytes) = &existing {
        serde_json::from_slice(bytes)?
    } else {
        json!({ "profiles": {} })
    };
    if !root.get("profiles").map(Value::is_object).unwrap_or(false) {
        root["profiles"] = json!({});
    }
    let key = if spec.update_legacy_in_place
        && root["profiles"]
            .get(ARNOLD_PROFILE_KEY)
            .map(Value::is_object)
            .unwrap_or(false)
    {
        ARNOLD_PROFILE_KEY
    } else {
        spec.key.as_str()
    };
    let mut entry = json!({
        "name": spec.name,
        "type": "custom",
        "lastVersionId": spec.version_id,
        "gameDir": spec.game_dir,
        "javaArgs": spec.java_args,
        "icon": "Furnace",
        "lastUsed": chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
    });
    if key == ARNOLD_PROFILE_KEY {
        if let Some(previous) = root["profiles"].get(ARNOLD_PROFILE_KEY) {
            if let (Some(map), Some(prev)) = (entry.as_object_mut(), previous.as_object()) {
                for (field, value) in prev {
                    if !map.contains_key(field) {
                        map.insert(field.clone(), value.clone());
                    }
                }
            }
        }
    }
    root["profiles"][key] = entry;
    write_json_atomic(path, &root)
}

pub fn remove_profile(file: &Path, key: &str) -> Result<Option<Value>, AppError> {
    if !file.is_file() {
        return Ok(None);
    }
    let original = fs::read(file)?;
    let mut root: Value = serde_json::from_slice(&original)?;
    let Some(profiles) = root.get_mut("profiles").and_then(Value::as_object_mut) else {
        return Ok(None);
    };
    let Some(entry) = profiles.remove(key) else {
        return Ok(None);
    };
    write_json_atomic(file, &root)?;
    Ok(Some(entry))
}

pub fn restore_profile(file: &Path, key: &str, entry: Value) -> Result<(), AppError> {
    let mut root = if file.is_file() {
        serde_json::from_slice(&fs::read(file)?)?
    } else {
        json!({ "profiles": {} })
    };
    if !root.get("profiles").map(Value::is_object).unwrap_or(false) {
        root["profiles"] = json!({});
    }
    root["profiles"][key] = entry;
    write_json_atomic(file, &root)
}

pub fn backup_profile_file(file: &Path, backup_dir: &Path) -> Result<(), AppError> {
    if !file.is_file() {
        return Ok(());
    }
    let stamp = crate::state::now_stamp();
    let dest_dir = backup_dir.join(stamp);
    fs::create_dir_all(&dest_dir)?;
    let name = file.file_name().ok_or_else(|| {
        AppError::Io(format!("profile path has no file name: {}", file.display()))
    })?;
    fs::copy(file, dest_dir.join(name))?;
    prune_backup_dirs(backup_dir, 5)?;
    Ok(())
}

fn prune_backup_dirs(backup_dir: &Path, keep: usize) -> Result<(), AppError> {
    if !backup_dir.is_dir() {
        return Ok(());
    }
    let mut dirs: Vec<PathBuf> = fs::read_dir(backup_dir)?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    dirs.sort();
    while dirs.len() > keep {
        let old = dirs.remove(0);
        fs::remove_dir_all(old)?;
    }
    Ok(())
}

fn write_json_atomic(path: &Path, value: &Value) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(value)?)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

pub fn legacy_profile_key_present(root: &Path) -> bool {
    let path = root.join("launcher_profiles.json");
    let Ok(text) = fs::read_to_string(path) else {
        return false;
    };
    text.contains(ARNOLD_PROFILE_KEY) || text.contains(ARNOLD_PROFILE_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn spec(dir: &Path) -> ProfileSpec {
        ProfileSpec {
            key: "arnold-pack-better-adventures-plus".into(),
            name: "Arnold — Better Adventures+".into(),
            version_id: "fabric-loader-0.20.1-26.3".into(),
            game_dir: dir.join("pack"),
            java_args: "-Xmx6G".into(),
            update_legacy_in_place: false,
        }
    }

    #[test]
    fn upsert_keeps_other_profiles_unknown_keys_and_order() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        fs::write(
            root.join("launcher_profiles.json"),
            r#"{"clientToken":"abc","profiles":{"vanilla":{"name":"Latest"},"other":{"name":"Other"}}}"#,
        )
        .unwrap();
        let backups = root.join("backups");
        upsert_official(root, &spec(root), &backups).unwrap();
        let text = fs::read_to_string(root.join("launcher_profiles.json")).unwrap();
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["clientToken"], "abc");
        assert_eq!(value["profiles"]["vanilla"]["name"], "Latest");
        assert_eq!(value["profiles"]["other"]["name"], "Other");
        let entry = &value["profiles"]["arnold-pack-better-adventures-plus"];
        assert_eq!(entry["name"], "Arnold — Better Adventures+");
        assert_eq!(entry["lastVersionId"], "fabric-loader-0.20.1-26.3");
        assert_eq!(entry["javaArgs"], "-Xmx6G");
        assert!(entry["gameDir"].as_str().unwrap().ends_with("pack"));
        assert!(text.find("clientToken").unwrap() < text.find("profiles").unwrap());
        assert!(backups.read_dir().unwrap().next().is_some());
    }

    #[test]
    fn missing_file_and_folder_are_created() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("missing").join("game");
        upsert_official(&root, &spec(tmp.path()), &tmp.path().join("backups")).unwrap();
        assert!(root.join("launcher_profiles.json").is_file());
        let value: Value =
            serde_json::from_str(&fs::read_to_string(root.join("launcher_profiles.json")).unwrap())
                .unwrap();
        assert!(value["profiles"]["arnold-pack-better-adventures-plus"].is_object());
    }

    #[test]
    fn tlauncher_creates_upserts_and_skips_unknown_shapes() {
        let tmp = tempdir().unwrap();
        let game = tmp.path().join("tl");
        let backups = tmp.path().join("backups");
        assert_eq!(
            upsert_tlauncher(&game, &spec(tmp.path()), &backups).unwrap(),
            TlWrite::Written
        );
        assert!(game.join(TLAUNCHER_PROFILES_FILE).is_file());

        let existing = tmp.path().join("shaped");
        fs::create_dir_all(&existing).unwrap();
        fs::write(
            existing.join(TLAUNCHER_PROFILES_FILE),
            r#"{"profiles":{"vanilla":{"name":"Latest"}}}"#,
        )
        .unwrap();
        assert_eq!(
            upsert_tlauncher(&existing, &spec(tmp.path()), &backups).unwrap(),
            TlWrite::Written
        );
        let text = fs::read_to_string(existing.join(TLAUNCHER_PROFILES_FILE)).unwrap();
        assert!(text.contains("Latest"));
        assert!(text.contains("Arnold — Better Adventures+"));

        let weird = tmp.path().join("weird");
        fs::create_dir_all(&weird).unwrap();
        let original = br#"["not","an","object"]"#;
        fs::write(weird.join(TLAUNCHER_PROFILES_FILE), original).unwrap();
        assert_eq!(
            upsert_tlauncher(&weird, &spec(tmp.path()), &backups).unwrap(),
            TlWrite::SkippedUnknownShape
        );
        assert_eq!(
            fs::read(weird.join(TLAUNCHER_PROFILES_FILE)).unwrap(),
            original
        );

        let other = tmp.path().join("other");
        fs::create_dir_all(&other).unwrap();
        let original = br#"{"profiles":["nope"]}"#;
        fs::write(other.join(TLAUNCHER_PROFILES_FILE), original).unwrap();
        assert_eq!(
            upsert_tlauncher(&other, &spec(tmp.path()), &backups).unwrap(),
            TlWrite::SkippedUnknownShape
        );
        assert_eq!(
            fs::read(other.join(TLAUNCHER_PROFILES_FILE)).unwrap(),
            original
        );
    }

    #[test]
    fn keep_path_updates_the_legacy_entry_and_deletes_nothing() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        fs::write(
            root.join("launcher_profiles.json"),
            r#"{"profiles":{"vanilla":{"name":"Latest"},"arnold-fabric-26.3":{"name":"Arnold – Fabric 26.3","type":"custom","lastVersionId":"old","icon":"Furnace"}}}"#,
        )
        .unwrap();
        let mut kept = spec(root);
        kept.update_legacy_in_place = true;
        kept.name = "Arnold — Better Adventures+".into();
        upsert_official(root, &kept, &root.join("backups")).unwrap();
        let value: Value =
            serde_json::from_str(&fs::read_to_string(root.join("launcher_profiles.json")).unwrap())
                .unwrap();
        assert!(value["profiles"]
            .get("arnold-pack-better-adventures-plus")
            .is_none());
        assert_eq!(value["profiles"]["vanilla"]["name"], "Latest");
        let legacy = &value["profiles"][ARNOLD_PROFILE_KEY];
        assert_eq!(legacy["name"], "Arnold — Better Adventures+");
        assert!(legacy["gameDir"].is_string());
        assert_eq!(legacy["icon"], "Furnace");
    }
}
