use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::detect::{looks_like_legacy_install, HostOs};
use crate::errors::AppError;
use crate::manifest::Manifest;
use crate::prefs::{PackLocation, Prefs};
use crate::profiles::{self, TLAUNCHER_PROFILES_FILE};
use crate::quality::quality_managed_paths;
use crate::state::{self, hash_file, InstalledState};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LegacyInstall {
    pub root: PathBuf,
    pub manifest_version: Option<String>,
    pub kit: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedDir {
    pub path: PathBuf,
    pub mode: String,
    pub needs_choice: Option<LegacyInstall>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyItem {
    Options,
    Servers,
    Saves,
}

impl CopyItem {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "options" => Some(Self::Options),
            "servers" => Some(Self::Servers),
            "saves" => Some(Self::Saves),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Options => "options",
            Self::Servers => "servers",
            Self::Saves => "saves",
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CopyReport {
    pub item: String,
    pub copied: Vec<String>,
    pub skipped: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SwitchReport {
    pub from: String,
    pub to: String,
    pub restored: bool,
    pub flagged: Vec<String>,
}

pub fn packs_root_for(home: &Path, os: HostOs) -> PathBuf {
    match os {
        HostOs::Windows => home.join("AppData").join("Roaming").join("arnold-packs"),
        HostOs::Macos => home
            .join("Library")
            .join("Application Support")
            .join("arnold-packs"),
        HostOs::Linux => home.join(".local").join("share").join("arnold-packs"),
    }
}

pub fn packs_root() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("arnold-packs")
}

pub fn default_pack_dir(pack_id: &str) -> PathBuf {
    packs_root().join(pack_id)
}

pub fn profile_name(pack_name: &str) -> String {
    format!("Arnold — {pack_name}")
}

pub fn profile_key(pack_id: &str) -> String {
    format!("arnold-pack-{pack_id}")
}

pub fn detect_legacy_install(root: &Path) -> Option<LegacyInstall> {
    let has_state = state::state_path(root).is_file();
    if !has_state && !looks_like_legacy_install(root) {
        return None;
    }
    let loaded = state::load_state(root).ok().flatten();
    Some(LegacyInstall {
        root: root.to_path_buf(),
        manifest_version: loaded.as_ref().map(|item| item.manifest_version.clone()),
        kit: loaded.and_then(|item| item.kit),
    })
}

pub fn find_legacy_install(primary: &Path, extra: &[PathBuf]) -> Option<LegacyInstall> {
    let mut seen: Vec<PathBuf> = Vec::new();
    for root in std::iter::once(primary.to_path_buf()).chain(extra.iter().cloned()) {
        if seen.iter().any(|path| path == &root) {
            continue;
        }
        seen.push(root.clone());
        if let Some(found) = detect_legacy_install(&root) {
            return Some(found);
        }
    }
    None
}

pub fn resolve_pack_dir(
    prefs: &Prefs,
    pack_id: &str,
    launcher_root: &Path,
    search_roots: &[PathBuf],
) -> ResolvedDir {
    if let Some(location) = prefs.pack_locations.get(pack_id) {
        return match location {
            PackLocation::OwnFolder => own_dir(pack_id),
            PackLocation::DefaultFolder => default_dir(launcher_root),
        };
    }
    if prefs.use_default_folder {
        return default_dir(launcher_root);
    }
    let mut extra = Vec::new();
    if let Some(last) = &prefs.last_game_dir {
        extra.push(last.clone());
    }
    extra.extend(search_roots.iter().cloned());
    if let Some(legacy) = find_legacy_install(launcher_root, &extra) {
        return ResolvedDir {
            path: default_pack_dir(pack_id),
            mode: "own".into(),
            needs_choice: Some(legacy),
        };
    }
    own_dir(pack_id)
}

fn own_dir(pack_id: &str) -> ResolvedDir {
    ResolvedDir {
        path: default_pack_dir(pack_id),
        mode: "own".into(),
        needs_choice: None,
    }
}

fn default_dir(launcher_root: &Path) -> ResolvedDir {
    ResolvedDir {
        path: launcher_root.to_path_buf(),
        mode: "default".into(),
        needs_choice: None,
    }
}

pub fn copy_from_old(
    old_root: &Path,
    pack_dir: &Path,
    item: CopyItem,
) -> Result<CopyReport, AppError> {
    let mut report = CopyReport {
        item: item.as_str().into(),
        copied: Vec::new(),
        skipped: Vec::new(),
    };
    match item {
        CopyItem::Options => copy_entry(
            &old_root.join("options.txt"),
            &pack_dir.join("options.txt"),
            "options.txt",
            &mut report,
        )?,
        CopyItem::Servers => copy_entry(
            &old_root.join("servers.dat"),
            &pack_dir.join("servers.dat"),
            "servers.dat",
            &mut report,
        )?,
        CopyItem::Saves => copy_entry(
            &old_root.join("saves"),
            &pack_dir.join("saves"),
            "saves",
            &mut report,
        )?,
    }
    Ok(report)
}

fn copy_entry(src: &Path, dest: &Path, rel: &str, report: &mut CopyReport) -> Result<(), AppError> {
    if !src.exists() {
        report.skipped.push(format!("{rel} (source missing)"));
        return Ok(());
    }
    if src.is_file() {
        if dest.exists() {
            report.skipped.push(rel.to_string());
            return Ok(());
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(src, dest)?;
        report.copied.push(rel.to_string());
        return Ok(());
    }
    let mut stack = vec![(src.to_path_buf(), dest.to_path_buf(), rel.to_string())];
    while let Some((from, to, prefix)) = stack.pop() {
        for entry in fs::read_dir(&from)? {
            let entry = entry?;
            let name = entry.file_name();
            let child_rel = if prefix.is_empty() {
                name.to_string_lossy().into_owned()
            } else {
                format!("{prefix}/{}", name.to_string_lossy())
            };
            let child_dest = to.join(&name);
            if entry.path().is_dir() {
                stack.push((entry.path(), child_dest, child_rel));
                continue;
            }
            if child_dest.exists() {
                report.skipped.push(child_rel);
                continue;
            }
            if let Some(parent) = child_dest.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(entry.path(), &child_dest)?;
            report.copied.push(child_rel);
        }
    }
    Ok(())
}

pub fn switch_shared_pack(
    root: &Path,
    from_id: &str,
    to_id: &str,
    to_manifest: &Manifest,
    processes: &[crate::detect::ProcessInfo],
) -> Result<SwitchReport, AppError> {
    if crate::detect::game_running_with(processes) {
        return Err(AppError::GameRunning);
    }
    let state =
        state::load_state(root)?.unwrap_or_else(|| InstalledState::new("installer", "unknown"));
    let _snapshot = crate::backup::create_snapshot(root, &state, "before-pack-switch")?;
    let (move_rels, flagged) = managed_set(root, &state, to_manifest, from_id);
    let stash = stash_dir(root, from_id);
    if stash.exists() {
        fs::remove_dir_all(&stash)?;
    }
    fs::create_dir_all(&stash)?;
    for rel in &move_rels {
        move_rel(root, &stash, rel)?;
    }
    let state_rel = "arnold-installer/state.json";
    if state::state_path(root).is_file() {
        move_rel(root, &stash, state_rel)?;
    }
    stash_profile(
        root,
        &stash,
        "launcher_profiles.json",
        &profile_key(from_id),
    )?;
    stash_profile(root, &stash, TLAUNCHER_PROFILES_FILE, &profile_key(from_id))?;

    let to_stash = stash_dir(root, to_id);
    let restored = if to_stash.is_dir() {
        restore_stash(root, &to_stash, &profile_key(to_id))?;
        fs::remove_dir_all(&to_stash)?;
        true
    } else {
        false
    };
    write_active_pack(root, to_id)?;
    Ok(SwitchReport {
        from: from_id.to_string(),
        to: to_id.to_string(),
        restored,
        flagged,
    })
}

fn stash_dir(root: &Path, pack_id: &str) -> PathBuf {
    state::installer_dir(root).join("packs").join(pack_id)
}

fn managed_set(
    root: &Path,
    state: &InstalledState,
    manifest: &Manifest,
    from_id: &str,
) -> (Vec<String>, Vec<String>) {
    let mut move_rels = Vec::new();
    let mut flagged = Vec::new();
    let ids: Vec<String> = state
        .components
        .iter()
        .map(|item| item.id.clone())
        .collect();
    let mut candidates: Vec<String> = state.files.iter().map(|file| file.path.clone()).collect();
    for rel in quality_managed_paths(manifest, &ids) {
        if !candidates.iter().any(|have| have == &rel) {
            candidates.push(rel);
        }
    }
    for component in &manifest.components {
        if let Some(config) = &component.config {
            if !candidates.iter().any(|have| have == &config.file) {
                candidates.push(config.file.clone());
            }
        }
    }
    for rel in candidates {
        if protected_rel(&rel) || rel.split(['/', '\\']).any(|part| part == "..") {
            continue;
        }
        let path = root.join(&rel);
        if !path.is_file() {
            continue;
        }
        match state.file_by_path(&rel) {
            Some(tracked) => match hash_file(&path) {
                Ok(actual) if actual == tracked.sha512 => move_rels.push(rel),
                _ => flagged.push(format!("Not part of {from_id}: {rel} left in place")),
            },
            None => {}
        }
    }
    for folder in ["mods", "resourcepacks", "shaderpacks"] {
        let dir = root.join(folder);
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let lower = name.to_ascii_lowercase();
            if !lower.ends_with(".jar") && !lower.ends_with(".zip") {
                continue;
            }
            let rel = format!("{folder}/{name}");
            if state.file_by_path(&rel).is_none() {
                flagged.push(format!("Not part of {from_id}: {rel} left in place"));
            }
        }
    }
    (move_rels, flagged)
}

fn protected_rel(rel: &str) -> bool {
    rel == "options.txt"
        || rel == "servers.dat"
        || rel.starts_with("saves/")
        || rel.starts_with("screenshots/")
        || rel.starts_with("logs/")
        || rel.starts_with("crash-reports/")
}

fn move_rel(root: &Path, stash: &Path, rel: &str) -> Result<(), AppError> {
    let src = root.join(rel);
    if !src.exists() {
        return Ok(());
    }
    let dest = stash.join(rel);
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    if dest.exists() {
        if dest.is_dir() {
            fs::remove_dir_all(&dest)?;
        } else {
            fs::remove_file(&dest)?;
        }
    }
    if fs::rename(&src, &dest).is_err() {
        fs::copy(&src, &dest)?;
        fs::remove_file(&src)?;
    }
    Ok(())
}

fn stash_profile(root: &Path, stash: &Path, file_name: &str, key: &str) -> Result<(), AppError> {
    let live = root.join(file_name);
    if let Some(entry) = profiles::remove_profile(&live, key)? {
        let name = if file_name == TLAUNCHER_PROFILES_FILE {
            "profile-tlauncher.json"
        } else {
            "profile-official.json"
        };
        fs::write(stash.join(name), serde_json::to_vec_pretty(&entry)?)?;
    }
    Ok(())
}

fn restore_stash(root: &Path, stash: &Path, profile_key: &str) -> Result<(), AppError> {
    restore_tree(stash, stash, root)?;
    if let Some(text) = fs::read(stash.join("profile-official.json")).ok() {
        if let Ok(entry) = serde_json::from_slice(&text) {
            profiles::restore_profile(&root.join("launcher_profiles.json"), profile_key, entry)?;
        }
    }
    if let Some(text) = fs::read(stash.join("profile-tlauncher.json")).ok() {
        if let Ok(entry) = serde_json::from_slice(&text) {
            profiles::restore_profile(&root.join(TLAUNCHER_PROFILES_FILE), profile_key, entry)?;
        }
    }
    Ok(())
}

fn restore_tree(stash_root: &Path, dir: &Path, game_root: &Path) -> Result<(), AppError> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            restore_tree(stash_root, &path, game_root)?;
            continue;
        }
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name == "profile-official.json" || name == "profile-tlauncher.json" {
            continue;
        }
        let rel = path.strip_prefix(stash_root).unwrap_or(&path);
        let dest = game_root.join(rel);
        if dest.exists() {
            continue;
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(&path, &dest)?;
    }
    Ok(())
}

fn write_active_pack(root: &Path, pack_id: &str) -> Result<(), AppError> {
    let path = state::installer_dir(root).join("active-pack.json");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let body = json!({
        "packId": pack_id,
        "profileKey": profile_key(pack_id),
    });
    fs::write(path, serde_json::to_vec_pretty(&body)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detect::{HostOs, ProcessInfo};
    use crate::download::sha512_hex;
    use crate::manifest::TEST_MANIFEST_PUBLIC_KEY_HEX;
    use crate::state::{record_file, save_state};
    use tempfile::tempdir;

    #[test]
    fn pack_roots_and_profile_name() {
        assert_eq!(
            packs_root_for(Path::new("C:/Users/Sam"), HostOs::Windows),
            PathBuf::from("C:/Users/Sam/AppData/Roaming/arnold-packs")
        );
        assert_eq!(
            packs_root_for(Path::new("/Users/sam"), HostOs::Macos),
            PathBuf::from("/Users/sam/Library/Application Support/arnold-packs")
        );
        assert_eq!(
            packs_root_for(Path::new("/home/sam"), HostOs::Linux),
            PathBuf::from("/home/sam/.local/share/arnold-packs")
        );
        assert_eq!(
            profile_name("Better Adventures+"),
            "Arnold — Better Adventures+"
        );
        assert_eq!(
            profile_key("better-adventures-plus"),
            "arnold-pack-better-adventures-plus"
        );
        assert!(profile_name("Better Adventures+").contains('\u{2014}'));
    }

    #[test]
    fn resolve_asks_before_touching_a_legacy_folder() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("minecraft");
        fs::create_dir_all(root.join("arnold-installer")).unwrap();
        fs::write(root.join("arnold-installer/state.json"), b"{\"schema\":1}").unwrap();
        fs::write(root.join("mods-note.txt"), b"keep").unwrap();
        let before = fs::read(root.join("mods-note.txt")).unwrap();
        let prefs = Prefs::default();
        let resolved = resolve_pack_dir(&prefs, "better-adventures-plus", &root, &[]);
        assert!(resolved.needs_choice.is_some());
        assert_eq!(fs::read(root.join("mods-note.txt")).unwrap(), before);
        assert!(!root.join("arnold-packs").exists());

        let mut keep = prefs.clone();
        keep.pack_locations
            .insert("better-adventures-plus".into(), PackLocation::DefaultFolder);
        let kept = resolve_pack_dir(&keep, "better-adventures-plus", &root, &[]);
        assert!(kept.needs_choice.is_none());
        assert_eq!(kept.path, root);
        assert_eq!(kept.mode, "default");

        let mut fresh = prefs.clone();
        fresh
            .pack_locations
            .insert("better-adventures-plus".into(), PackLocation::OwnFolder);
        let owned = resolve_pack_dir(&fresh, "better-adventures-plus", &root, &[]);
        assert!(owned.needs_choice.is_none());
        assert_eq!(owned.mode, "own");
        assert_eq!(
            owned.path.file_name().and_then(|n| n.to_str()),
            Some("better-adventures-plus")
        );
        assert_eq!(
            owned
                .path
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str()),
            Some("arnold-packs")
        );

        let mut shared = Prefs::default();
        shared.use_default_folder = true;
        let shared_dir = resolve_pack_dir(&shared, "better-adventures-plus", &root, &[]);
        assert_eq!(shared_dir.path, root);
        assert!(shared_dir.needs_choice.is_none());
    }

    #[test]
    fn empty_folder_uses_the_pack_directory() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("minecraft");
        fs::create_dir_all(&root).unwrap();
        let resolved = resolve_pack_dir(&Prefs::default(), "better-adventures-plus", &root, &[]);
        assert!(resolved.needs_choice.is_none());
        assert_eq!(resolved.mode, "own");
        assert!(!resolved.path.exists() || resolved.path.starts_with(packs_root()));
    }

    #[test]
    fn copy_from_old_copies_only_the_asked_item_and_skips_existing() {
        let tmp = tempdir().unwrap();
        let old = tmp.path().join("old");
        let pack = tmp.path().join("pack");
        fs::create_dir_all(old.join("saves/world")).unwrap();
        fs::create_dir_all(&pack).unwrap();
        fs::write(old.join("options.txt"), b"options").unwrap();
        fs::write(old.join("servers.dat"), b"servers").unwrap();
        fs::write(old.join("saves/world/level.dat"), b"world").unwrap();
        fs::write(pack.join("options.txt"), b"already").unwrap();

        let options = copy_from_old(&old, &pack, CopyItem::Options).unwrap();
        assert!(options.copied.is_empty());
        assert!(options
            .skipped
            .iter()
            .any(|row| row.contains("options.txt")));
        assert_eq!(fs::read(pack.join("options.txt")).unwrap(), b"already");
        assert_eq!(fs::read(old.join("options.txt")).unwrap(), b"options");

        let servers = copy_from_old(&old, &pack, CopyItem::Servers).unwrap();
        assert_eq!(servers.copied, vec!["servers.dat".to_string()]);
        assert_eq!(fs::read(old.join("servers.dat")).unwrap(), b"servers");
        assert_eq!(fs::read(pack.join("servers.dat")).unwrap(), b"servers");
        assert!(!pack.join("saves").exists());

        let saves = copy_from_old(&old, &pack, CopyItem::Saves).unwrap();
        assert!(saves.copied.iter().any(|row| row.ends_with("level.dat")));
        assert_eq!(
            fs::read(old.join("saves/world/level.dat")).unwrap(),
            b"world"
        );
        assert_eq!(
            fs::read(pack.join("saves/world/level.dat")).unwrap(),
            b"world"
        );
    }

    #[test]
    fn switch_round_trip_restores_managed_files_and_leaves_the_rest() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        let managed = b"managed-jar";
        let edited = b"edited-by-player";
        let user = b"user-jar";
        fs::create_dir_all(root.join("mods")).unwrap();
        fs::create_dir_all(root.join("saves/world")).unwrap();
        fs::create_dir_all(root.join("screenshots")).unwrap();
        fs::write(root.join("mods/managed.jar"), managed).unwrap();
        fs::write(root.join("mods/edited.jar"), edited).unwrap();
        fs::write(root.join("mods/user.jar"), user).unwrap();
        fs::write(root.join("saves/world/level.dat"), b"world").unwrap();
        fs::write(root.join("screenshots/shot.png"), b"shot").unwrap();
        fs::write(root.join("options.txt"), b"opt").unwrap();
        fs::write(root.join("servers.dat"), b"srv").unwrap();
        let mut state = InstalledState::new("installer", "2026.11.6");
        record_file(
            &mut state,
            "mods/managed.jar",
            sha512_hex(managed),
            "managed",
        );
        record_file(
            &mut state,
            "mods/edited.jar",
            sha512_hex(b"original"),
            "edited",
        );
        save_state(root, &state).unwrap();
        fs::write(
            root.join("launcher_profiles.json"),
            r#"{"profiles":{"vanilla":{"name":"Latest"},"arnold-pack-pack-a":{"name":"Arnold — A","type":"custom"}}}"#,
        )
        .unwrap();
        let manifest =
            crate::manifest::load_bundled_manifest_with_key(TEST_MANIFEST_PUBLIC_KEY_HEX).unwrap();

        let running = switch_shared_pack(
            root,
            "pack-a",
            "pack-b",
            &manifest,
            &[ProcessInfo {
                name: "java".into(),
                cmd: "net.minecraft.client.main.Main".into(),
            }],
        );
        assert_eq!(running.unwrap_err().code(), "E-GAME-RUNNING");
        assert_eq!(fs::read(root.join("mods/managed.jar")).unwrap(), managed);
        assert!(!stash_dir(root, "pack-a").exists());

        let away = switch_shared_pack(root, "pack-a", "pack-b", &manifest, &[]).unwrap();
        assert!(!away.restored);
        assert!(away.flagged.iter().any(|row| row.contains("user.jar")));
        assert!(away.flagged.iter().any(|row| row.contains("edited.jar")));
        assert!(!root.join("mods/managed.jar").exists());
        assert_eq!(fs::read(root.join("mods/user.jar")).unwrap(), user);
        assert_eq!(fs::read(root.join("mods/edited.jar")).unwrap(), edited);
        assert_eq!(
            fs::read(root.join("saves/world/level.dat")).unwrap(),
            b"world"
        );
        assert_eq!(
            fs::read(root.join("screenshots/shot.png")).unwrap(),
            b"shot"
        );
        assert_eq!(fs::read(root.join("options.txt")).unwrap(), b"opt");
        assert_eq!(fs::read(root.join("servers.dat")).unwrap(), b"srv");
        let profiles: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(root.join("launcher_profiles.json")).unwrap())
                .unwrap();
        assert!(profiles["profiles"].get("arnold-pack-pack-a").is_none());
        assert_eq!(profiles["profiles"]["vanilla"]["name"], "Latest");
        assert!(stash_dir(root, "pack-a").join("mods/managed.jar").is_file());

        let back = switch_shared_pack(root, "pack-b", "pack-a", &manifest, &[]).unwrap();
        assert!(back.restored);
        assert_eq!(fs::read(root.join("mods/managed.jar")).unwrap(), managed);
        assert_eq!(fs::read(root.join("mods/user.jar")).unwrap(), user);
        assert_eq!(fs::read(root.join("mods/edited.jar")).unwrap(), edited);
        assert_eq!(
            fs::read(root.join("saves/world/level.dat")).unwrap(),
            b"world"
        );
        assert_eq!(fs::read(root.join("options.txt")).unwrap(), b"opt");
        assert_eq!(fs::read(root.join("servers.dat")).unwrap(), b"srv");
        let restored = state::load_state(root).unwrap().unwrap();
        assert_eq!(restored.manifest_version, "2026.11.6");
        let profiles: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(root.join("launcher_profiles.json")).unwrap())
                .unwrap();
        assert_eq!(profiles["profiles"]["vanilla"]["name"], "Latest");
        assert!(profiles["profiles"].get("arnold-pack-pack-a").is_some());
        let active: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(state::installer_dir(root).join("active-pack.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(active["packId"], "pack-a");
    }
}
