use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::detect::{looks_like_legacy_install, HostOs, ProcessInfo};
use crate::errors::AppError;
use crate::manifest::Manifest;
use crate::packs::{self, BA_PLUS_PACK_ID};
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
    /// Outgoing owner was inferred because the folder had no active-pack.json.
    pub legacy: bool,
    /// Where the outgoing pack's files were saved.
    pub stash_path: String,
    pub partial_restore: bool,
    /// Saved-copy paths the incoming pack could not put back because a different file is there.
    /// Untracked in the live state.
    pub conflicts: Vec<String>,
    /// Older saved copies that were renamed aside (never deleted).
    pub saved_copies: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OwnerSource {
    Marker,
    State,
    LegacyBaPlus,
    Empty,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum FolderOwner {
    Empty,
    Marked {
        pack_id: String,
    },
    Inferred {
        pack_id: String,
        source: OwnerSource,
    },
    Ambiguous {
        reason: String,
    },
    /// Arnold jars are present, but there is no active-pack.json and no state.json.
    Occupied {
        files: Vec<String>,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DestinationEntry {
    pub pack_id: String,
    pub path: PathBuf,
    pub mode: String,
    pub owner_before: Option<FolderOwner>,
    pub switch: Option<SwitchReport>,
    pub adopted: bool,
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
    processes: &[ProcessInfo],
    legacy: bool,
) -> Result<SwitchReport, AppError> {
    if crate::detect::game_running_with(processes) {
        return Err(AppError::GameRunning);
    }
    let state =
        state::load_state(root)?.unwrap_or_else(|| InstalledState::new("installer", "unknown"));
    let _snapshot = crate::backup::create_snapshot(root, &state, "before-pack-switch")?;
    let (move_rels, mut flagged) = managed_set(root, &state, to_manifest, from_id);
    let stash = stash_dir(root, from_id);
    let mut saved_copies = Vec::new();
    if stash.exists() {
        let stamp = state::now_stamp();
        let replaced_name = format!("{from_id}.replaced-{stamp}");
        let replaced = stash
            .parent()
            .map(|parent| parent.join(&replaced_name))
            .ok_or_else(|| {
                AppError::Io(format!("saved copy of {from_id} has no parent directory"))
            })?;
        fs::rename(&stash, &replaced)?;
        saved_copies.push(format!("arnold-installer/packs/{replaced_name}"));
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
        "profile-official.json",
    )?;
    stash_profile(
        root,
        &stash,
        TLAUNCHER_PROFILES_FILE,
        &profile_key(from_id),
        "profile-tlauncher.json",
    )?;
    if legacy {
        stash_profile(
            root,
            &stash,
            "launcher_profiles.json",
            profiles::ARNOLD_PROFILE_KEY,
            "profile-official-legacy.json",
        )?;
    }

    let restore = restore_incoming(root, to_id)?;
    let restored = restore.is_some();
    let (conflicts, notes) = restore
        .map(|item| (item.conflicts, item.notes))
        .unwrap_or_default();
    flagged.extend(notes);
    write_active_pack(root, to_id, Some(from_id), OwnerSource::Marker)?;
    Ok(SwitchReport {
        from: from_id.to_string(),
        to: to_id.to_string(),
        restored,
        flagged,
        legacy,
        stash_path: stash.display().to_string(),
        partial_restore: !conflicts.is_empty(),
        conflicts,
        saved_copies,
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
        return Err(AppError::Io(format!(
            "saved copy already has {rel}; that copy was left as it is"
        )));
    }
    if fs::rename(&src, &dest).is_err() {
        fs::copy(&src, &dest)?;
        fs::remove_file(&src)?;
    }
    Ok(())
}

fn stash_profile(
    root: &Path,
    stash: &Path,
    file_name: &str,
    key: &str,
    dest_name: &str,
) -> Result<(), AppError> {
    let live = root.join(file_name);
    if let Some(entry) = profiles::remove_profile(&live, key)? {
        fs::write(stash.join(dest_name), serde_json::to_vec_pretty(&entry)?)?;
    }
    Ok(())
}

const STASH_STATE_REL: &str = "arnold-installer/state.json";
const STASH_PROFILE_FILES: [&str; 3] = [
    "profile-official.json",
    "profile-tlauncher.json",
    "profile-official-legacy.json",
];
const KEPT_ASIDE_REL: &str = "arnold-installer/kept-aside.json";

/// A file the installer left in place because a saved copy could not be put back.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct KeptAside {
    pub path: String,
    pub pack_id: String,
    pub sha512: String,
    pub kept_at: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct KeptAsideFile {
    schema: u32,
    #[serde(default)]
    files: Vec<KeptAside>,
}

fn kept_aside_unreadable(path: &Path) -> AppError {
    AppError::State(format!(
        "{} can't be read, so nothing was changed. It lists your own files that the installer must not replace. Fix or remove that file and try again.",
        path.display()
    ))
}

fn load_kept_aside(root: &Path) -> Result<KeptAsideFile, AppError> {
    let path = root.join(KEPT_ASIDE_REL);
    if !path.exists() {
        return Ok(KeptAsideFile {
            schema: 1,
            files: Vec::new(),
        });
    }
    let text = fs::read_to_string(&path).map_err(|_| kept_aside_unreadable(&path))?;
    let record: KeptAsideFile =
        serde_json::from_str(&text).map_err(|_| kept_aside_unreadable(&path))?;
    if record.schema != 1 {
        return Err(kept_aside_unreadable(&path));
    }
    Ok(record)
}

fn save_kept_aside(root: &Path, record: &KeptAsideFile) -> Result<(), AppError> {
    let path = root.join(KEPT_ASIDE_REL);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(record)?)?;
    fs::rename(tmp, path)?;
    Ok(())
}

/// Paths of the user's files that no install may remove or replace without consent.
pub fn kept_aside_paths(root: &Path) -> Result<Vec<String>, AppError> {
    Ok(load_kept_aside(root)?
        .files
        .into_iter()
        .map(|file| file.path)
        .collect())
}

fn record_kept_aside(root: &Path, pack_id: &str, rels: &[String]) -> Result<(), AppError> {
    if rels.is_empty() {
        return Ok(());
    }
    let mut record = load_kept_aside(root)?;
    for rel in rels {
        let sha = hash_file(&root.join(rel)).unwrap_or_default();
        let entry = KeptAside {
            path: rel.clone(),
            pack_id: pack_id.to_string(),
            sha512: sha,
            kept_at: state::now_stamp(),
        };
        if let Some(existing) = record.files.iter_mut().find(|item| item.path == *rel) {
            *existing = entry;
        } else {
            record.files.push(entry);
        }
    }
    record.schema = 1;
    save_kept_aside(root, &record)
}

/// After a successful install, drop entries whose file is gone or is now tracked.
/// Anything else stays protected. An empty list is written; the file is never deleted.
pub fn prune_kept_aside(root: &Path, state: &InstalledState) -> Result<(), AppError> {
    if !root.join(KEPT_ASIDE_REL).exists() {
        return Ok(());
    }
    let mut record = load_kept_aside(root)?;
    let before = record.files.len();
    record
        .files
        .retain(|file| root.join(&file.path).exists() && state.file_by_path(&file.path).is_none());
    if record.files.len() != before {
        save_kept_aside(root, &record)?;
    }
    Ok(())
}

struct RestoreResult {
    conflicts: Vec<String>,
    notes: Vec<String>,
}

/// Copies `to_id`'s saved copy into `root`. The saved copy is read only.
fn restore_incoming(root: &Path, to_id: &str) -> Result<Option<RestoreResult>, AppError> {
    let stash = stash_dir(root, to_id);
    if !stash.is_dir() {
        return Ok(None);
    }
    let mut rels = Vec::new();
    collect_stash_files(&stash, &stash, &mut rels)?;
    rels.sort();
    let mut conflicts = Vec::new();
    let mut copies = Vec::new();
    for rel in rels {
        let dest = root.join(&rel);
        let src = stash.join(&rel);
        if !dest.exists() {
            copies.push(rel);
            continue;
        }
        let same = dest.is_file()
            && matches!(
                (hash_file(&dest), hash_file(&src)),
                (Ok(left), Ok(right)) if left == right
            );
        if same {
            continue;
        }
        conflicts.push(rel);
    }
    record_kept_aside(root, to_id, &conflicts)?;
    for rel in &copies {
        let dest = root.join(rel);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(stash.join(rel), dest)?;
    }
    let mut notes = Vec::new();
    if state::state_path(&stash).is_file() {
        match state::load_state(&stash) {
            Ok(Some(mut saved)) => {
                saved.files.retain(|file| !conflicts.contains(&file.path));
                if !state::state_path(root).is_file() {
                    state::save_state(root, &saved)?;
                } else {
                    notes.push(format!(
                        "{to_id}'s saved install record was left in its saved copy because this folder already has one"
                    ));
                }
            }
            _ => notes.push(format!(
                "{to_id}'s saved install record couldn't be read; it was left in its saved copy"
            )),
        }
    }
    put_back_profile(
        &stash,
        "profile-official.json",
        &root.join("launcher_profiles.json"),
        &profile_key(to_id),
    )?;
    put_back_profile(
        &stash,
        "profile-tlauncher.json",
        &root.join(TLAUNCHER_PROFILES_FILE),
        &profile_key(to_id),
    )?;
    put_back_profile(
        &stash,
        "profile-official-legacy.json",
        &root.join("launcher_profiles.json"),
        profiles::ARNOLD_PROFILE_KEY,
    )?;
    Ok(Some(RestoreResult { conflicts, notes }))
}

fn collect_stash_files(
    stash_root: &Path,
    dir: &Path,
    out: &mut Vec<String>,
) -> Result<(), AppError> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_stash_files(stash_root, &path, out)?;
            continue;
        }
        let rel = path.strip_prefix(stash_root).unwrap_or(&path);
        let rel_str = rel_to_string(rel);
        if rel_str == STASH_STATE_REL || rel_str == "arnold-installer/state.json.tmp" {
            continue;
        }
        if STASH_PROFILE_FILES.contains(&rel_str.as_str()) {
            continue;
        }
        out.push(rel_str);
    }
    Ok(())
}

fn put_back_profile(stash: &Path, name: &str, live_file: &Path, key: &str) -> Result<(), AppError> {
    let path = stash.join(name);
    if !path.is_file() {
        return Ok(());
    }
    let text = fs::read(&path)?;
    let Ok(entry) = serde_json::from_slice(&text) else {
        return Ok(());
    };
    profiles::restore_profile(live_file, key, entry)?;
    Ok(())
}

fn rel_to_string(rel: &Path) -> String {
    rel.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

pub fn read_active_pack(root: &Path) -> Option<String> {
    let path = state::installer_dir(root).join("active-pack.json");
    let text = fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value
        .get("packId")
        .and_then(|item| item.as_str())
        .map(str::to_string)
}

pub fn write_active_pack(
    root: &Path,
    pack_id: &str,
    previous: Option<&str>,
    source: OwnerSource,
) -> Result<(), AppError> {
    let path = state::installer_dir(root).join("active-pack.json");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let body = json!({
        "schema": 1,
        "packId": pack_id,
        "profileKey": profile_key(pack_id),
        "previous": previous,
        "ownerSource": source,
        "claimedAt": state::now_stamp(),
    });
    fs::write(path, serde_json::to_vec_pretty(&body)?)?;
    Ok(())
}

pub fn resolve_folder_owner(root: &Path) -> FolderOwner {
    let marker_path = state::installer_dir(root).join("active-pack.json");
    if marker_path.exists() {
        let reason = format!("active-pack.json in {} can't be read", root.display());
        let text = match fs::read_to_string(&marker_path) {
            Ok(text) => text,
            Err(_) => return FolderOwner::Ambiguous { reason },
        };
        let value: serde_json::Value = match serde_json::from_str(&text) {
            Ok(value) => value,
            Err(_) => return FolderOwner::Ambiguous { reason },
        };
        let Some(pack_id) = value
            .get("packId")
            .and_then(|item| item.as_str())
            .filter(|id| !id.is_empty())
        else {
            return FolderOwner::Ambiguous { reason };
        };
        let pack_id = pack_id.to_string();
        let state_id = state::state_path(root)
            .is_file()
            .then(|| state::load_state(root).ok().flatten())
            .flatten()
            .and_then(|installed| installed.pack_id)
            .filter(|id| !id.is_empty() && id != &pack_id);
        if let Some(state_id) = state_id {
            return FolderOwner::Ambiguous {
                reason: format!("active-pack.json says {pack_id} but state.json says {state_id}"),
            };
        }
        return FolderOwner::Marked { pack_id };
    }
    if !state::state_path(root).is_file() {
        let files: Vec<String> = crate::detect::arnold_jar_names(root)
            .into_iter()
            .map(|name| format!("mods/{name}"))
            .collect();
        return if files.is_empty() {
            FolderOwner::Empty
        } else {
            FolderOwner::Occupied { files }
        };
    }
    let reason = format!("state.json in {} can't be read", root.display());
    match state::load_state(root) {
        Ok(Some(installed)) => {
            if let Some(pack_id) = installed.pack_id.filter(|id| !id.is_empty()) {
                FolderOwner::Inferred {
                    pack_id,
                    source: OwnerSource::State,
                }
            } else {
                FolderOwner::Inferred {
                    pack_id: BA_PLUS_PACK_ID.to_string(),
                    source: OwnerSource::LegacyBaPlus,
                }
            }
        }
        _ => FolderOwner::Ambiguous { reason },
    }
}

/// State that belongs to `pack_id` in a shared folder. Never another pack's live state.
pub fn shared_state_for(root: &Path, pack_id: &str) -> Result<Option<InstalledState>, AppError> {
    match resolve_folder_owner(root) {
        FolderOwner::Ambiguous { reason } => Err(packs::folder_owner_error(root, reason)),
        FolderOwner::Marked { pack_id: owner } | FolderOwner::Inferred { pack_id: owner, .. }
            if owner == pack_id =>
        {
            state::load_state(root)
        }
        _ => {
            let stash = stash_dir(root, pack_id);
            if state::state_path(&stash).is_file() {
                state::load_state(&stash)
            } else {
                Ok(None)
            }
        }
    }
}

pub fn require_owner(root: &Path, pack_id: &str) -> Result<(), AppError> {
    match resolve_folder_owner(root) {
        FolderOwner::Ambiguous { reason } => Err(packs::folder_owner_error(root, reason)),
        FolderOwner::Empty => Ok(()),
        FolderOwner::Occupied { files } => Err(packs::folder_occupied_error(root, pack_id, &files)),
        FolderOwner::Marked { pack_id: owner } | FolderOwner::Inferred { pack_id: owner, .. }
            if owner == pack_id =>
        {
            Ok(())
        }
        FolderOwner::Marked { pack_id: other } | FolderOwner::Inferred { pack_id: other, .. } => {
            Err(AppError::State(format!(
                "this Minecraft folder is still set up for {other}; open {pack_id} again to switch"
            )))
        }
    }
}

/// The only function that may make a shared folder belong to `pack_id`.
/// Preferences are not saved here.
pub fn enter_destination(
    prefs: &Prefs,
    pack_id: &str,
    launcher_root: &Path,
    search_roots: &[PathBuf],
    load_incoming_manifest: impl FnOnce() -> Result<Manifest, AppError>,
    processes: &[ProcessInfo],
) -> Result<DestinationEntry, AppError> {
    let resolved = resolve_pack_dir(prefs, pack_id, launcher_root, search_roots);
    if resolved.needs_choice.is_some() || resolved.mode != "default" {
        return Ok(DestinationEntry {
            pack_id: pack_id.to_string(),
            path: resolved.path,
            mode: resolved.mode,
            owner_before: None,
            switch: None,
            adopted: false,
        });
    }
    let path = resolved.path;
    let mode = resolved.mode;
    kept_aside_paths(&path)?;
    match resolve_folder_owner(&path) {
        FolderOwner::Ambiguous { reason } => Err(packs::folder_owner_error(&path, reason)),
        FolderOwner::Marked { pack_id: owner_id } if owner_id == pack_id => Ok(DestinationEntry {
            pack_id: pack_id.to_string(),
            path,
            mode,
            owner_before: Some(FolderOwner::Marked { pack_id: owner_id }),
            switch: None,
            adopted: false,
        }),
        FolderOwner::Inferred {
            pack_id: owner_id,
            source,
        } if owner_id == pack_id => {
            write_active_pack(&path, pack_id, None, source)?;
            Ok(DestinationEntry {
                pack_id: pack_id.to_string(),
                path,
                mode,
                owner_before: Some(FolderOwner::Inferred {
                    pack_id: owner_id,
                    source,
                }),
                switch: None,
                adopted: true,
            })
        }
        FolderOwner::Marked { pack_id: other } => {
            let owner_before = FolderOwner::Marked {
                pack_id: other.clone(),
            };
            finish_switch(
                &path,
                mode,
                pack_id,
                &other,
                false,
                owner_before,
                load_incoming_manifest,
                processes,
            )
        }
        FolderOwner::Inferred {
            pack_id: other,
            source,
        } => {
            let owner_before = FolderOwner::Inferred {
                pack_id: other.clone(),
                source,
            };
            finish_switch(
                &path,
                mode,
                pack_id,
                &other,
                true,
                owner_before,
                load_incoming_manifest,
                processes,
            )
        }
        FolderOwner::Occupied { files } => {
            Err(packs::folder_occupied_error(&path, pack_id, &files))
        }
        FolderOwner::Empty => {
            let to_stash = stash_dir(&path, pack_id);
            let mut switch = None;
            if to_stash.is_dir() {
                if crate::detect::game_running_with(processes) {
                    return Err(AppError::GameRunning);
                }
                if let Some(result) = restore_incoming(&path, pack_id)? {
                    switch = Some(SwitchReport {
                        from: String::new(),
                        to: pack_id.to_string(),
                        restored: true,
                        flagged: result.notes,
                        legacy: false,
                        stash_path: to_stash.display().to_string(),
                        partial_restore: !result.conflicts.is_empty(),
                        conflicts: result.conflicts,
                        saved_copies: Vec::new(),
                    });
                }
            }
            write_active_pack(&path, pack_id, None, OwnerSource::Empty)?;
            Ok(DestinationEntry {
                pack_id: pack_id.to_string(),
                path,
                mode,
                owner_before: Some(FolderOwner::Empty),
                switch,
                adopted: false,
            })
        }
    }
}

fn finish_switch(
    path: &Path,
    mode: String,
    pack_id: &str,
    other: &str,
    legacy: bool,
    owner_before: FolderOwner,
    load_incoming_manifest: impl FnOnce() -> Result<Manifest, AppError>,
    processes: &[ProcessInfo],
) -> Result<DestinationEntry, AppError> {
    if crate::detect::game_running_with(processes) {
        return Err(AppError::GameRunning);
    }
    let manifest = load_incoming_manifest()?;
    let report = switch_shared_pack(path, other, pack_id, &manifest, processes, legacy)?;
    Ok(DestinationEntry {
        pack_id: pack_id.to_string(),
        path: path.to_path_buf(),
        mode,
        owner_before: Some(owner_before),
        switch: Some(report),
        adopted: false,
    })
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
            false,
        );
        assert_eq!(running.unwrap_err().code(), "E-GAME-RUNNING");
        assert_eq!(fs::read(root.join("mods/managed.jar")).unwrap(), managed);
        assert!(!stash_dir(root, "pack-a").exists());

        let away = switch_shared_pack(root, "pack-a", "pack-b", &manifest, &[], false).unwrap();
        assert!(!away.restored);
        assert!(!away.legacy);
        assert!(!away.partial_restore);
        assert!(away.stash_path.contains("pack-a"));
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

        let back = switch_shared_pack(root, "pack-b", "pack-a", &manifest, &[], false).unwrap();
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
        assert!(back.conflicts.is_empty());
        assert_eq!(
            fs::read(stash_dir(root, "pack-a").join("mods/managed.jar")).unwrap(),
            managed
        );
        let active: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(state::installer_dir(root).join("active-pack.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(active["packId"], "pack-a");
        assert_eq!(active["ownerSource"], "marker");
        assert_eq!(active["previous"], "pack-b");
    }

    #[test]
    fn shared_state_never_reports_another_packs_state() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        let (ba, ba_state) = ba_plus_legacy_folder(root);
        assert_eq!(
            shared_state_for(root, BA_PLUS_PACK_ID)
                .unwrap()
                .unwrap()
                .manifest_version,
            ba_state.manifest_version
        );
        assert!(shared_state_for(root, "pack-x").unwrap().is_none());
        require_owner(root, BA_PLUS_PACK_ID).unwrap();
        assert!(require_owner(root, "pack-x").is_err());
        assert_eq!(fs::read(root.join("mods/ba.jar")).unwrap(), ba);

        fs::create_dir_all(root.join("mods")).unwrap();
        let body = b"managed-jar";
        fs::write(root.join("mods/managed.jar"), body).unwrap();
        let mut state = InstalledState::new("installer", "2026.11.6");
        state.pack_id = Some("pack-a".into());
        record_file(&mut state, "mods/managed.jar", sha512_hex(body), "managed");
        save_state(root, &state).unwrap();
        let manifest = bundled_manifest();
        let prefs = default_prefs("pack-b");
        let entry = enter_at(root, &prefs, "pack-b", manifest, &[]).unwrap();
        assert!(entry.switch.is_some());
        assert!(!root.join("mods/managed.jar").exists());
        assert!(stash_dir(root, "pack-a").join("mods/managed.jar").is_file());
        let stashed = shared_state_for(root, "pack-a").unwrap().unwrap();
        assert_eq!(stashed.manifest_version, "2026.11.6");
        assert!(shared_state_for(root, "pack-b").unwrap().is_none());
        assert!(shared_state_for(root, "pack-c").unwrap().is_none());
        require_owner(root, "pack-b").unwrap();
        let blocked = require_owner(root, "pack-a").unwrap_err();
        assert!(blocked.to_string().contains("pack-a"));
        assert_eq!(read_active_pack(root).as_deref(), Some("pack-b"));
    }

    fn bundled_manifest() -> crate::manifest::Manifest {
        crate::manifest::load_bundled_manifest_with_key(TEST_MANIFEST_PUBLIC_KEY_HEX).unwrap()
    }

    fn default_prefs(pack_id: &str) -> Prefs {
        Prefs {
            selected_pack: Some(pack_id.to_string()),
            use_default_folder: true,
            ..Prefs::default()
        }
    }

    fn keep_prefs(pack_id: &str) -> Prefs {
        let mut prefs = default_prefs(pack_id);
        prefs.use_default_folder = false;
        prefs
            .pack_locations
            .insert(pack_id.to_string(), PackLocation::DefaultFolder);
        prefs
    }

    fn own_prefs(pack_id: &str) -> Prefs {
        Prefs {
            selected_pack: Some(pack_id.to_string()),
            use_default_folder: false,
            ..Prefs::default()
        }
    }

    fn enter_at(
        root: &Path,
        prefs: &Prefs,
        pack_id: &str,
        manifest: crate::manifest::Manifest,
        processes: &[ProcessInfo],
    ) -> Result<DestinationEntry, AppError> {
        enter_destination(prefs, pack_id, root, &[], || Ok(manifest), processes)
    }

    fn enter_without_manifest(
        root: &Path,
        prefs: &Prefs,
        pack_id: &str,
        processes: &[ProcessInfo],
    ) -> Result<DestinationEntry, AppError> {
        enter_destination(
            prefs,
            pack_id,
            root,
            &[],
            || panic!("manifest loader must not run"),
            processes,
        )
    }

    fn ba_plus_legacy_folder(root: &Path) -> (Vec<u8>, InstalledState) {
        let ba = b"ba-jar-bytes".to_vec();
        fs::create_dir_all(root.join("mods")).unwrap();
        fs::create_dir_all(root.join("saves/world")).unwrap();
        fs::create_dir_all(root.join("screenshots")).unwrap();
        fs::write(root.join("mods/ba.jar"), &ba).unwrap();
        fs::write(root.join("mods/user.jar"), b"user-jar").unwrap();
        fs::write(root.join("saves/world/level.dat"), b"world").unwrap();
        fs::write(root.join("screenshots/shot.png"), b"shot").unwrap();
        fs::write(root.join("options.txt"), b"options").unwrap();
        fs::write(root.join("servers.dat"), b"servers").unwrap();
        let mut state = InstalledState::new("installer", "2026.11.6");
        record_file(&mut state, "mods/ba.jar", sha512_hex(&ba), "ba");
        save_state(root, &state).unwrap();
        fs::write(
            root.join("launcher_profiles.json"),
            serde_json::json!({
                "profiles": {
                    "vanilla": {"name": "Latest"},
                    profiles::ARNOLD_PROFILE_KEY: {"name": "Arnold", "type": "custom"}
                }
            })
            .to_string(),
        )
        .unwrap();
        (ba, state)
    }

    fn marker_json(root: &Path) -> serde_json::Value {
        serde_json::from_str(
            &fs::read_to_string(state::installer_dir(root).join("active-pack.json")).unwrap(),
        )
        .unwrap()
    }

    fn tree_fingerprint(root: &Path) -> Vec<(String, String)> {
        let mut out = Vec::new();
        fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, String)>) {
            let mut entries: Vec<_> = fs::read_dir(dir).unwrap().flatten().collect();
            entries.sort_by_key(|entry| entry.file_name());
            for entry in entries {
                let path = entry.path();
                let rel = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                if path.is_dir() {
                    out.push((format!("{rel}/"), String::new()));
                    walk(root, &path, out);
                } else {
                    out.push((rel, sha512_hex(&fs::read(&path).unwrap())));
                }
            }
        }
        if root.exists() {
            walk(root, root, &mut out);
        }
        out
    }

    fn assert_ba_plus_stashed(root: &Path, ba: &[u8], version: &str) {
        let stash = stash_dir(root, BA_PLUS_PACK_ID);
        assert_eq!(fs::read(stash.join("mods/ba.jar")).unwrap(), ba);
        let stashed = state::load_state(&stash).unwrap().unwrap();
        assert_eq!(stashed.manifest_version, version);
        assert!(!root.join("mods/ba.jar").exists());
        assert!(!state::state_path(root).is_file());
        assert_eq!(fs::read(root.join("mods/user.jar")).unwrap(), b"user-jar");
        assert_eq!(
            fs::read(root.join("saves/world/level.dat")).unwrap(),
            b"world"
        );
        assert_eq!(
            fs::read(root.join("screenshots/shot.png")).unwrap(),
            b"shot"
        );
        assert_eq!(fs::read(root.join("options.txt")).unwrap(), b"options");
        assert_eq!(fs::read(root.join("servers.dat")).unwrap(), b"servers");
        assert_eq!(read_active_pack(root).as_deref(), Some("pack-x"));
        let marker = marker_json(root);
        assert_eq!(marker["previous"], BA_PLUS_PACK_ID);
        let profiles: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(root.join("launcher_profiles.json")).unwrap())
                .unwrap();
        assert!(profiles["profiles"]
            .get(profiles::ARNOLD_PROFILE_KEY)
            .is_none());
        assert_eq!(profiles["profiles"]["vanilla"]["name"], "Latest");
        let legacy: serde_json::Value =
            serde_json::from_slice(&fs::read(stash.join("profile-official-legacy.json")).unwrap())
                .unwrap();
        assert_eq!(legacy["name"], "Arnold");
        assert!(shared_state_for(root, "pack-x").unwrap().is_none());
        assert_eq!(
            shared_state_for(root, BA_PLUS_PACK_ID)
                .unwrap()
                .unwrap()
                .manifest_version,
            version
        );
    }

    #[test]
    fn resolve_folder_owner_rules() {
        let tmp = tempdir().unwrap();
        let empty = tmp.path().join("empty");
        fs::create_dir_all(&empty).unwrap();
        assert_eq!(resolve_folder_owner(&empty), FolderOwner::Empty);

        let legacy = tmp.path().join("legacy");
        fs::create_dir_all(&legacy).unwrap();
        ba_plus_legacy_folder(&legacy);
        assert_eq!(
            resolve_folder_owner(&legacy),
            FolderOwner::Inferred {
                pack_id: BA_PLUS_PACK_ID.into(),
                source: OwnerSource::LegacyBaPlus,
            }
        );

        let stated = tmp.path().join("stated");
        fs::create_dir_all(&stated).unwrap();
        let mut state = InstalledState::new("installer", "1");
        state.pack_id = Some("pack-x".into());
        save_state(&stated, &state).unwrap();
        assert_eq!(
            resolve_folder_owner(&stated),
            FolderOwner::Inferred {
                pack_id: "pack-x".into(),
                source: OwnerSource::State,
            }
        );

        let marked = tmp.path().join("marked");
        fs::create_dir_all(state::installer_dir(&marked)).unwrap();
        fs::write(
            state::installer_dir(&marked).join("active-pack.json"),
            br#"{"packId":"pack-a"}"#,
        )
        .unwrap();
        assert_eq!(
            resolve_folder_owner(&marked),
            FolderOwner::Marked {
                pack_id: "pack-a".into()
            }
        );

        let garbage_marker = tmp.path().join("garbage-marker");
        fs::create_dir_all(state::installer_dir(&garbage_marker)).unwrap();
        fs::write(
            state::installer_dir(&garbage_marker).join("active-pack.json"),
            b"nope",
        )
        .unwrap();
        assert!(matches!(
            resolve_folder_owner(&garbage_marker),
            FolderOwner::Ambiguous { .. }
        ));

        let garbage_state = tmp.path().join("garbage-state");
        fs::create_dir_all(state::installer_dir(&garbage_state)).unwrap();
        fs::write(state::state_path(&garbage_state), b"nope").unwrap();
        assert!(matches!(
            resolve_folder_owner(&garbage_state),
            FolderOwner::Ambiguous { .. }
        ));

        let clash = tmp.path().join("clash");
        fs::create_dir_all(&clash).unwrap();
        let mut clash_state = InstalledState::new("installer", "1");
        clash_state.pack_id = Some("b".into());
        save_state(&clash, &clash_state).unwrap();
        fs::write(
            state::installer_dir(&clash).join("active-pack.json"),
            br#"{"packId":"a"}"#,
        )
        .unwrap();
        match resolve_folder_owner(&clash) {
            FolderOwner::Ambiguous { reason } => {
                assert!(reason.contains("says a"), "{reason}");
                assert!(reason.contains("says b"), "{reason}");
            }
            other => panic!("expected ambiguous, got {other:?}"),
        }

        let jars_only = tmp.path().join("jars-only");
        fs::create_dir_all(jars_only.join("mods")).unwrap();
        fs::write(
            jars_only.join("mods/arnold-snowfall-1.3.3.jar"),
            b"leftover",
        )
        .unwrap();
        assert_eq!(
            resolve_folder_owner(&jars_only),
            FolderOwner::Occupied {
                files: vec!["mods/arnold-snowfall-1.3.3.jar".into()],
            }
        );

        let sodium = tmp.path().join("sodium-only");
        fs::create_dir_all(sodium.join("mods")).unwrap();
        fs::write(sodium.join("mods/sodium.jar"), b"na").unwrap();
        assert_eq!(resolve_folder_owner(&sodium), FolderOwner::Empty);

        let stashed = tmp.path().join("stash-only");
        fs::create_dir_all(stashed.join("arnold-installer/packs/pack-x/mods")).unwrap();
        fs::write(
            stashed.join("arnold-installer/packs/pack-x/mods/x.jar"),
            b"x",
        )
        .unwrap();
        assert_eq!(resolve_folder_owner(&stashed), FolderOwner::Empty);
    }

    #[test]
    fn legacy_folder_default_toggle_stashes_ba_plus_before_plan() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        let (ba, state) = ba_plus_legacy_folder(root);
        let before = tree_fingerprint(root);
        let mut prefs = own_prefs("pack-x");
        let entry = enter_without_manifest(root, &prefs, "pack-x", &[]).unwrap();
        assert_eq!(entry.mode, "own");
        assert!(entry.switch.is_none());
        assert!(resolve_pack_dir(&prefs, "pack-x", root, &[])
            .needs_choice
            .is_some());
        assert_eq!(tree_fingerprint(root), before);

        prefs.use_default_folder = true;
        let entry = enter_at(root, &prefs, "pack-x", bundled_manifest(), &[]).unwrap();
        let report = entry.switch.expect("legacy folder switches");
        assert!(report.legacy);
        assert_eq!(report.from, BA_PLUS_PACK_ID);
        assert_ba_plus_stashed(root, &ba, &state.manifest_version);
    }

    #[test]
    fn legacy_folder_keep_choice_stashes_ba_plus() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        let (ba, state) = ba_plus_legacy_folder(root);
        let manifest = bundled_manifest();
        let entry = enter_at(root, &keep_prefs("pack-x"), "pack-x", manifest.clone(), &[]).unwrap();
        assert!(entry.switch.unwrap().legacy);
        assert_ba_plus_stashed(root, &ba, &state.manifest_version);

        let stash = stash_dir(root, BA_PLUS_PACK_ID);
        let before_stash = tree_fingerprint(&stash);
        let entry = enter_at(
            root,
            &keep_prefs(BA_PLUS_PACK_ID),
            BA_PLUS_PACK_ID,
            manifest,
            &[],
        )
        .unwrap();
        assert!(entry.switch.unwrap().restored);
        assert_eq!(fs::read(root.join("mods/ba.jar")).unwrap(), ba);
        let live = state::load_state(root).unwrap().unwrap();
        assert_eq!(live.manifest_version, state.manifest_version);
        let profiles: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(root.join("launcher_profiles.json")).unwrap())
                .unwrap();
        assert_eq!(
            profiles["profiles"][profiles::ARNOLD_PROFILE_KEY]["name"],
            "Arnold"
        );
        assert_eq!(profiles["profiles"]["vanilla"]["name"], "Latest");
        assert_eq!(read_active_pack(root).as_deref(), Some(BA_PLUS_PACK_ID));
        assert_eq!(tree_fingerprint(&stash), before_stash);
        assert_eq!(fs::read(stash.join("mods/ba.jar")).unwrap(), ba);
        assert_eq!(
            state::load_state(&stash).unwrap().unwrap().manifest_version,
            state.manifest_version
        );
        let legacy: serde_json::Value =
            serde_json::from_slice(&fs::read(stash.join("profile-official-legacy.json")).unwrap())
                .unwrap();
        assert_eq!(legacy["name"], "Arnold");
    }

    #[test]
    fn marked_other_pack_completes_transition_instead_of_blocking() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        let a_bytes = b"pack-a-jar";
        fs::create_dir_all(root.join("mods")).unwrap();
        fs::write(root.join("mods/a.jar"), a_bytes).unwrap();
        let mut a_state = InstalledState::new("installer", "a-1");
        a_state.pack_id = Some("pack-a".into());
        record_file(&mut a_state, "mods/a.jar", sha512_hex(a_bytes), "a");
        save_state(root, &a_state).unwrap();
        write_active_pack(root, "pack-a", None, OwnerSource::Marker).unwrap();

        let stash = stash_dir(root, "pack-x");
        let mut x_state = InstalledState::new("installer", "x-1");
        x_state.pack_id = Some("pack-x".into());
        save_state(&stash, &x_state).unwrap();
        fs::create_dir_all(stash.join("mods")).unwrap();
        fs::write(stash.join("mods/x.jar"), b"x-jar").unwrap();

        let entry = enter_at(
            root,
            &default_prefs("pack-x"),
            "pack-x",
            bundled_manifest(),
            &[],
        )
        .unwrap();
        let report = entry.switch.unwrap();
        assert!(report.restored);
        assert!(!report.legacy);
        assert_eq!(
            fs::read(stash_dir(root, "pack-a").join("mods/a.jar")).unwrap(),
            a_bytes
        );
        assert!(!root.join("mods/a.jar").exists());
        assert_eq!(fs::read(root.join("mods/x.jar")).unwrap(), b"x-jar");
        assert_eq!(read_active_pack(root).as_deref(), Some("pack-x"));
        assert_eq!(
            shared_state_for(root, "pack-x")
                .unwrap()
                .unwrap()
                .manifest_version,
            "x-1"
        );
        require_owner(root, "pack-x").unwrap();
        assert!(require_owner(root, "pack-a").is_err());
    }

    #[test]
    fn same_pack_legacy_folder_is_adopted_without_moving() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        ba_plus_legacy_folder(root);
        let before = tree_fingerprint(root);
        let entry =
            enter_without_manifest(root, &default_prefs(BA_PLUS_PACK_ID), BA_PLUS_PACK_ID, &[])
                .unwrap();
        assert!(entry.adopted);
        assert!(entry.switch.is_none());
        let after: Vec<_> = tree_fingerprint(root)
            .into_iter()
            .filter(|(rel, _)| !rel.contains("active-pack.json"))
            .collect();
        assert_eq!(after, before);
        let marker = marker_json(root);
        assert_eq!(marker["packId"], BA_PLUS_PACK_ID);
        assert_eq!(marker["ownerSource"], "legacy-ba-plus");
    }

    #[test]
    fn ambiguous_folder_refuses_and_changes_nothing() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir_all(state::installer_dir(root)).unwrap();
        fs::write(state::state_path(root), b"not-json").unwrap();
        fs::write(root.join("mods-note.txt"), b"keep").unwrap();
        let before = tree_fingerprint(root);
        let err =
            enter_without_manifest(root, &default_prefs("pack-x"), "pack-x", &[]).unwrap_err();
        assert_eq!(err.to_payload().code, "E-FOLDER-OWNER");
        assert_eq!(tree_fingerprint(root), before);
        assert!(shared_state_for(root, "pack-x").is_err());
    }

    #[test]
    fn existing_outgoing_stash_is_kept_not_deleted() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        let body = b"pack-a-jar";
        fs::create_dir_all(root.join("mods")).unwrap();
        fs::write(root.join("mods/a.jar"), body).unwrap();
        let mut state = InstalledState::new("installer", "a-1");
        state.pack_id = Some("pack-a".into());
        record_file(&mut state, "mods/a.jar", sha512_hex(body), "a");
        save_state(root, &state).unwrap();
        write_active_pack(root, "pack-a", None, OwnerSource::Marker).unwrap();
        let old = stash_dir(root, "pack-a");
        fs::create_dir_all(&old).unwrap();
        fs::write(old.join("old.jar"), b"old").unwrap();

        let entry = enter_at(
            root,
            &default_prefs("pack-x"),
            "pack-x",
            bundled_manifest(),
            &[],
        )
        .unwrap();
        let report = entry.switch.unwrap();
        assert!(report
            .saved_copies
            .iter()
            .any(|row| row.contains("pack-a.replaced-")));
        assert!(report
            .flagged
            .iter()
            .all(|row| !row.contains("Older saved copy")));
        let packs = state::installer_dir(root).join("packs");
        let replaced = fs::read_dir(&packs)
            .unwrap()
            .flatten()
            .find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("pack-a.replaced-")
            })
            .unwrap();
        assert_eq!(fs::read(replaced.path().join("old.jar")).unwrap(), b"old");
    }

    fn marked_pack_a(root: &Path, bytes: &[u8]) {
        fs::create_dir_all(root.join("mods")).unwrap();
        fs::write(root.join("mods/a.jar"), bytes).unwrap();
        let mut state = InstalledState::new("installer", "a-1");
        state.pack_id = Some("pack-a".into());
        record_file(&mut state, "mods/a.jar", sha512_hex(bytes), "a");
        save_state(root, &state).unwrap();
        write_active_pack(root, "pack-a", None, OwnerSource::Marker).unwrap();
    }

    fn stash_pack_x_files(root: &Path, files: &[(&str, &[u8])]) {
        let stash = stash_dir(root, "pack-x");
        let mut state = InstalledState::new("installer", "x-1");
        state.pack_id = Some("pack-x".into());
        for (rel, bytes) in files {
            let dest = stash.join(rel);
            fs::create_dir_all(dest.parent().unwrap()).unwrap();
            fs::write(&dest, bytes).unwrap();
            record_file(&mut state, *rel, sha512_hex(bytes), "x");
        }
        save_state(&stash, &state).unwrap();
    }

    #[test]
    fn restore_conflict_untracks_the_users_file() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        marked_pack_a(root, b"pack-a-jar");
        fs::write(root.join("mods/x.jar"), b"live").unwrap();
        stash_pack_x_files(root, &[("mods/x.jar", b"stash"), ("mods/y.jar", b"y")]);
        let stash = stash_dir(root, "pack-x");
        let before_stash = tree_fingerprint(&stash);

        let entry = enter_at(
            root,
            &default_prefs("pack-x"),
            "pack-x",
            bundled_manifest(),
            &[],
        )
        .unwrap();
        let report = entry.switch.unwrap();
        assert!(report.partial_restore);
        assert_eq!(report.conflicts, vec!["mods/x.jar".to_string()]);
        assert!(report
            .flagged
            .iter()
            .all(|row| { !(row.contains("mods/x.jar") && !row.contains("Not part of")) }));
        assert_eq!(fs::read(root.join("mods/x.jar")).unwrap(), b"live");
        assert_eq!(fs::read(root.join("mods/y.jar")).unwrap(), b"y");
        let live = state::load_state(root).unwrap().unwrap();
        assert!(live.file_by_path("mods/y.jar").is_some());
        assert!(live.file_by_path("mods/x.jar").is_none());
        let stash = stash_dir(root, "pack-x");
        assert_eq!(fs::read(stash.join("mods/x.jar")).unwrap(), b"stash");
        let saved = state::load_state(&stash).unwrap().unwrap();
        assert!(saved.file_by_path("mods/x.jar").is_some());
        assert_eq!(
            kept_aside_paths(root).unwrap(),
            vec!["mods/x.jar".to_string()]
        );
        let sidecar: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(root.join("arnold-installer/kept-aside.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(sidecar["files"][0]["packId"], "pack-x");
        assert_eq!(sidecar["files"][0]["sha512"], sha512_hex(b"live"));
        assert_eq!(tree_fingerprint(&stash), before_stash);
    }

    #[test]
    fn restore_identical_file_stays_tracked() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        marked_pack_a(root, b"pack-a-jar");
        fs::write(root.join("mods/x.jar"), b"stash").unwrap();
        stash_pack_x_files(root, &[("mods/x.jar", b"stash"), ("mods/y.jar", b"y")]);

        let entry = enter_at(
            root,
            &default_prefs("pack-x"),
            "pack-x",
            bundled_manifest(),
            &[],
        )
        .unwrap();
        let report = entry.switch.unwrap();
        assert!(report.conflicts.is_empty());
        assert!(!report.partial_restore);
        let live = state::load_state(root).unwrap().unwrap();
        assert!(live.file_by_path("mods/x.jar").is_some());
        assert!(!root.join("arnold-installer/kept-aside.json").exists());
    }

    #[test]
    fn restore_keeps_the_saved_copy() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        marked_pack_a(root, b"pack-a-jar");
        stash_pack_x_files(root, &[("mods/x.jar", b"x-bytes")]);
        let stash = stash_dir(root, "pack-x");
        fs::write(stash.join("profile-official.json"), br#"{"name":"X"}"#).unwrap();
        fs::write(stash.join("profile-tlauncher.json"), br#"{"name":"XT"}"#).unwrap();
        let before = tree_fingerprint(&stash);

        let entry = enter_at(
            root,
            &default_prefs("pack-x"),
            "pack-x",
            bundled_manifest(),
            &[],
        )
        .unwrap();
        assert!(entry.switch.unwrap().restored);
        assert_eq!(tree_fingerprint(&stash), before);
        let profiles: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(root.join("launcher_profiles.json")).unwrap())
                .unwrap();
        assert!(profiles["profiles"].get("arnold-pack-pack-x").is_some());
    }

    #[test]
    fn restash_after_restore_renames_never_deletes() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        let original = b"pack-a-jar";
        marked_pack_a(root, original);
        let manifest = bundled_manifest();
        enter_at(
            root,
            &default_prefs("pack-x"),
            "pack-x",
            manifest.clone(),
            &[],
        )
        .unwrap();
        assert!(stash_dir(root, "pack-a").join("mods/a.jar").is_file());
        enter_at(
            root,
            &default_prefs("pack-a"),
            "pack-a",
            manifest.clone(),
            &[],
        )
        .unwrap();
        assert!(stash_dir(root, "pack-a").is_dir());
        let third = enter_at(root, &default_prefs("pack-x"), "pack-x", manifest, &[]).unwrap();
        let report = third.switch.unwrap();
        assert_eq!(report.saved_copies.len(), 1);
        assert!(report.saved_copies[0].contains("pack-a.replaced-"));
        assert!(report
            .flagged
            .iter()
            .all(|row| !row.contains("Older saved copy")));
        let packs = state::installer_dir(root).join("packs");
        let replaced: Vec<_> = fs::read_dir(&packs)
            .unwrap()
            .flatten()
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("pack-a.replaced-")
            })
            .collect();
        assert_eq!(replaced.len(), 1);
        assert_eq!(
            fs::read(replaced[0].path().join("mods/a.jar")).unwrap(),
            original
        );
        assert!(replaced[0]
            .path()
            .join("arnold-installer/state.json")
            .is_file());
        assert_eq!(
            fs::read(stash_dir(root, "pack-a").join("mods/a.jar")).unwrap(),
            original
        );
    }

    #[test]
    fn occupied_folder_without_records_is_refused_and_unchanged() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("mods")).unwrap();
        fs::create_dir_all(root.join("saves/w")).unwrap();
        fs::write(root.join("mods/arnold-snowfall-1.3.3.jar"), b"leftover").unwrap();
        fs::write(root.join("options.txt"), b"options").unwrap();
        fs::write(root.join("saves/w/level.dat"), b"world").unwrap();
        let before = tree_fingerprint(root);
        for prefs in [keep_prefs("pack-x"), default_prefs("pack-x")] {
            let err = enter_without_manifest(root, &prefs, "pack-x", &[]).unwrap_err();
            let payload = err.to_payload();
            assert_eq!(payload.code, "E-FOLDER-OCCUPIED");
            assert!(payload.detail.contains("mods/arnold-snowfall-1.3.3.jar"));
            assert!(payload.hint.contains("Nothing was moved or changed"));
            assert!(!payload.hint.contains("Adopt into this installer"));
            assert!(payload
                .hint
                .contains("open Better Adventures+ and adopt them first"));
        }
        assert_eq!(tree_fingerprint(root), before);
        assert!(!state::installer_dir(root).exists());
        assert!(read_active_pack(root).is_none());
        let err = require_owner(root, "pack-x").unwrap_err();
        assert_eq!(err.to_payload().code, "E-FOLDER-OCCUPIED");
        assert!(shared_state_for(root, "pack-x").unwrap().is_none());
    }

    #[test]
    fn occupied_folder_is_refused_for_ba_plus_too() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("mods")).unwrap();
        fs::write(
            root.join("mods/arnold-snowfall-1.3.2.jar"),
            b"arbitrary-user-bytes",
        )
        .unwrap();
        fs::write(root.join("mods/arnold-snowfall-1.3.3.jar"), b"leftover").unwrap();
        fs::write(root.join("options.txt"), b"options").unwrap();
        let before = tree_fingerprint(root);
        for prefs in [default_prefs(BA_PLUS_PACK_ID), keep_prefs(BA_PLUS_PACK_ID)] {
            let err = enter_without_manifest(root, &prefs, BA_PLUS_PACK_ID, &[]).unwrap_err();
            let payload = err.to_payload();
            assert_eq!(payload.code, "E-FOLDER-OCCUPIED");
            assert!(payload.hint.contains("Adopt into this installer"));
            assert!(payload.hint.contains("Nothing was moved or changed"));
            assert!(payload.detail.contains("mods/arnold-snowfall-1.3.2.jar"));
            let owner = require_owner(root, BA_PLUS_PACK_ID).unwrap_err();
            assert_eq!(owner.to_payload().code, "E-FOLDER-OCCUPIED");
        }
        assert_eq!(tree_fingerprint(root), before);
        assert!(!state::installer_dir(root).exists());
        assert!(read_active_pack(root).is_none());
        assert!(shared_state_for(root, BA_PLUS_PACK_ID).unwrap().is_none());
    }

    #[test]
    fn legacy_state_folder_still_enters_for_ba_plus() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        let bytes = b"legacy-ba-snow";
        fs::create_dir_all(root.join("mods")).unwrap();
        fs::write(root.join("mods/arnold-snowfall-1.3.2.jar"), bytes).unwrap();
        let mut state = InstalledState::new("installer", "2.3.x");
        record_file(
            &mut state,
            "mods/arnold-snowfall-1.3.2.jar",
            sha512_hex(bytes),
            "snow",
        );
        assert!(state.pack_id.is_none());
        save_state(root, &state).unwrap();
        let entry =
            enter_without_manifest(root, &default_prefs(BA_PLUS_PACK_ID), BA_PLUS_PACK_ID, &[])
                .unwrap();
        assert!(entry.adopted);
        assert_eq!(
            entry.owner_before,
            Some(FolderOwner::Inferred {
                pack_id: BA_PLUS_PACK_ID.into(),
                source: OwnerSource::LegacyBaPlus,
            })
        );
        assert_eq!(read_active_pack(root).as_deref(), Some(BA_PLUS_PACK_ID));
        assert_eq!(
            fs::read(root.join("mods/arnold-snowfall-1.3.2.jar")).unwrap(),
            bytes
        );
    }

    #[test]
    fn unreadable_kept_aside_blocks_entry_and_changes_nothing() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        marked_pack_a(root, b"pack-a-jar");
        fs::write(root.join("arnold-installer/kept-aside.json"), b"{not json").unwrap();
        stash_pack_x_files(root, &[("mods/x.jar", b"x")]);
        let before = tree_fingerprint(root);
        let err =
            enter_without_manifest(root, &default_prefs("pack-x"), "pack-x", &[]).unwrap_err();
        assert_eq!(err.to_payload().code, "E-STATE");
        assert!(err.to_string().contains("kept-aside.json"), "{err}");
        assert_eq!(tree_fingerprint(root), before);
        assert_eq!(read_active_pack(root).as_deref(), Some("pack-a"));
    }

    #[test]
    fn prune_kept_aside_drops_gone_and_tracked_only() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("mods")).unwrap();
        fs::write(root.join("mods/now-tracked.jar"), b"tracked").unwrap();
        fs::write(root.join("mods/still-yours.jar"), b"yours").unwrap();
        record_kept_aside(
            root,
            "pack-x",
            &[
                "mods/gone.jar".into(),
                "mods/now-tracked.jar".into(),
                "mods/still-yours.jar".into(),
            ],
        )
        .unwrap();
        let mut state = InstalledState::new("installer", "1");
        record_file(
            &mut state,
            "mods/now-tracked.jar",
            sha512_hex(b"tracked"),
            "x",
        );
        prune_kept_aside(root, &state).unwrap();
        assert_eq!(
            kept_aside_paths(root).unwrap(),
            vec!["mods/still-yours.jar".to_string()]
        );
        assert!(root.join("arnold-installer/kept-aside.json").is_file());
    }

    #[test]
    fn game_running_blocks_transition() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        ba_plus_legacy_folder(root);
        let err = enter_without_manifest(
            root,
            &default_prefs("pack-x"),
            "pack-x",
            &[ProcessInfo {
                name: "java".into(),
                cmd: "net.minecraft.client.main.Main".into(),
            }],
        )
        .unwrap_err();
        assert_eq!(err.code(), "E-GAME-RUNNING");
        assert!(!stash_dir(root, BA_PLUS_PACK_ID).exists());
        assert!(root.join("mods/ba.jar").is_file());
        assert!(state::state_path(root).is_file());
    }
}
