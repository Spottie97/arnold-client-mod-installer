use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::errors::AppError;
use crate::state::{hash_file, installer_dir, InstalledState};

pub const SNAPSHOT_KEEP: usize = 5;
pub const SNAPSHOTS_DIR: &str = "snapshots";
pub const JOURNAL_FILE: &str = "journal.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotMeta {
    pub id: String,
    pub created_at: String,
    pub label: String,
    #[serde(default)]
    pub kit: Option<String>,
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotJournal {
    pub snapshots: Vec<SnapshotMeta>,
}

pub fn snapshots_dir(game_dir: &Path) -> PathBuf {
    installer_dir(game_dir).join(SNAPSHOTS_DIR)
}

pub fn journal_path(game_dir: &Path) -> PathBuf {
    installer_dir(game_dir).join(JOURNAL_FILE)
}

pub fn load_journal(game_dir: &Path) -> Result<SnapshotJournal, AppError> {
    let path = journal_path(game_dir);
    if !path.exists() {
        return Ok(SnapshotJournal::default());
    }
    let text = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&text)?)
}

fn save_journal(game_dir: &Path, journal: &SnapshotJournal) -> Result<(), AppError> {
    fs::create_dir_all(installer_dir(game_dir))?;
    fs::write(journal_path(game_dir), serde_json::to_vec_pretty(journal)?)?;
    Ok(())
}

fn copy_tracked_file(game_dir: &Path, rel: &str, dest_root: &Path) -> Result<(), AppError> {
    let src = game_dir.join(rel);
    if !src.is_file() {
        return Ok(());
    }
    let dest = dest_root.join(rel);
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(src, dest)?;
    Ok(())
}

pub fn create_snapshot(
    game_dir: &Path,
    state: &InstalledState,
    label: &str,
) -> Result<SnapshotMeta, AppError> {
    let id = format!("snap-{}", crate::state::now_stamp());
    let dest = snapshots_dir(game_dir).join(&id);
    fs::create_dir_all(&dest)?;
    let mut files = Vec::new();
    for tracked in &state.files {
        copy_tracked_file(game_dir, &tracked.path, &dest)?;
        files.push(tracked.path.clone());
    }
    fs::write(dest.join("state.json"), serde_json::to_vec_pretty(state)?)?;
    let meta = SnapshotMeta {
        id: id.clone(),
        created_at: crate::state::now_stamp(),
        label: label.to_string(),
        kit: state.kit.clone(),
        files,
    };
    fs::write(dest.join("meta.json"), serde_json::to_vec_pretty(&meta)?)?;

    let mut journal = load_journal(game_dir)?;
    journal.snapshots.push(meta.clone());
    prune_snapshots(game_dir, &mut journal, SNAPSHOT_KEEP)?;
    save_journal(game_dir, &journal)?;
    Ok(meta)
}

pub fn list_snapshots(game_dir: &Path) -> Result<Vec<SnapshotMeta>, AppError> {
    Ok(load_journal(game_dir)?.snapshots)
}

pub fn rollback(
    game_dir: &Path,
    snapshot_id: &str,
    state: &InstalledState,
) -> Result<InstalledState, AppError> {
    let dest = snapshots_dir(game_dir).join(snapshot_id);
    if !dest.is_dir() {
        return Err(AppError::Snapshot(format!(
            "restore point {snapshot_id} was not found"
        )));
    }
    let tracked: Vec<String> = state.files.iter().map(|f| f.path.clone()).collect();
    let snap_state_path = dest.join("state.json");
    let snap_state: InstalledState = if snap_state_path.exists() {
        serde_json::from_str(&fs::read_to_string(snap_state_path)?)?
    } else {
        state.clone()
    };

    // Only restore paths recorded on the snapshot / current tracked set.
    let restore_paths: Vec<String> = if !snap_state.files.is_empty() {
        snap_state.files.iter().map(|f| f.path.clone()).collect()
    } else {
        tracked
    };

    for rel in &restore_paths {
        let src = dest.join(rel);
        let target = game_dir.join(rel);
        if src.is_file() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(src, target)?;
        } else if target.is_file() {
            // Tracked now but missing from the snapshot: remove only that tracked file.
            fs::remove_file(target)?;
        }
    }
    crate::state::save_state(game_dir, &snap_state)?;
    Ok(snap_state)
}

pub fn prune_snapshots(
    game_dir: &Path,
    journal: &mut SnapshotJournal,
    keep: usize,
) -> Result<(), AppError> {
    while journal.snapshots.len() > keep {
        let old = journal.snapshots.remove(0);
        let dir = snapshots_dir(game_dir).join(&old.id);
        if dir.exists() {
            fs::remove_dir_all(dir)?;
        }
    }
    Ok(())
}

pub fn uninstall_tracked(
    game_dir: &Path,
    state: &mut InstalledState,
    component_ids: &[String],
) -> Result<UninstallResult, AppError> {
    let mut result = UninstallResult::default();
    let ids: Vec<String> = if component_ids.is_empty() {
        state.components.iter().map(|c| c.id.clone()).collect()
    } else {
        component_ids.to_vec()
    };
    for id in ids {
        let files = crate::state::remove_component(state, &id);
        for tracked in files {
            let path = game_dir.join(&tracked.path);
            if !path.is_file() {
                result.missing.push(tracked.path);
                continue;
            }
            let actual = hash_file(&path)?;
            if actual != tracked.sha512 {
                result.skipped_hash_mismatch.push(tracked.path);
                continue;
            }
            fs::remove_file(&path)?;
            result.removed.push(tracked.path);
        }
    }
    crate::state::save_state(game_dir, state)?;
    Ok(result)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UninstallResult {
    pub removed: Vec<String>,
    pub skipped_hash_mismatch: Vec<String>,
    pub missing: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::download::sha512_hex;
    use crate::state::{record_file, save_state, InstalledState};
    use tempfile::tempdir;

    fn write_tracked(game: &Path, rel: &str, bytes: &[u8], state: &mut InstalledState, id: &str) {
        let path = game.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
        record_file(state, rel, sha512_hex(bytes), id);
    }

    #[test]
    fn snapshot_and_rollback_touch_only_tracked_files() {
        let tmp = tempdir().unwrap();
        let game = tmp.path();
        let mut state = InstalledState::new("installer", "2026.11.01-1");
        state.kit = Some("full".into());
        write_tracked(game, "mods/sodium.jar", b"sodium-v1", &mut state, "sodium");
        fs::create_dir_all(game.join("mods")).unwrap();
        fs::write(game.join("mods/user-extra.jar"), b"keep-me").unwrap();
        save_state(game, &state).unwrap();

        let snap = create_snapshot(game, &state, "before-update").unwrap();
        assert_eq!(snap.files, vec!["mods/sodium.jar".to_string()]);
        assert!(!snapshots_dir(game)
            .join(&snap.id)
            .join("mods/user-extra.jar")
            .exists());

        fs::write(game.join("mods/sodium.jar"), b"sodium-v2").unwrap();
        fs::write(game.join("mods/user-extra.jar"), b"keep-me-still").unwrap();
        fs::write(game.join("mods/another-user.jar"), b"also-keep").unwrap();

        rollback(game, &snap.id, &state).unwrap();
        assert_eq!(
            fs::read(game.join("mods/sodium.jar")).unwrap(),
            b"sodium-v1"
        );
        assert_eq!(
            fs::read(game.join("mods/user-extra.jar")).unwrap(),
            b"keep-me-still"
        );
        assert_eq!(
            fs::read(game.join("mods/another-user.jar")).unwrap(),
            b"also-keep"
        );
    }

    #[test]
    fn uninstall_leaves_unrecorded_user_jar() {
        let tmp = tempdir().unwrap();
        let game = tmp.path();
        let mut state = InstalledState::new("installer", "1");
        write_tracked(game, "mods/arnold-ships.jar", b"ships", &mut state, "ships");
        fs::create_dir_all(game.join("mods")).unwrap();
        fs::write(game.join("mods/user-mod.jar"), b"mine").unwrap();
        save_state(game, &state).unwrap();

        let result = uninstall_tracked(game, &mut state, &["ships".into()]).unwrap();
        assert_eq!(result.removed, vec!["mods/arnold-ships.jar".to_string()]);
        assert!(game.join("mods/user-mod.jar").is_file());
        assert!(!game.join("mods/arnold-ships.jar").exists());
    }

    #[test]
    fn uninstall_skips_replaced_file() {
        let tmp = tempdir().unwrap();
        let game = tmp.path();
        let mut state = InstalledState::new("installer", "1");
        write_tracked(game, "mods/sodium.jar", b"official", &mut state, "sodium");
        save_state(game, &state).unwrap();
        fs::write(game.join("mods/sodium.jar"), b"replaced-by-user").unwrap();
        let result = uninstall_tracked(game, &mut state, &["sodium".into()]).unwrap();
        assert_eq!(
            result.skipped_hash_mismatch,
            vec!["mods/sodium.jar".to_string()]
        );
        assert!(game.join("mods/sodium.jar").is_file());
    }

    #[test]
    fn keeps_only_last_five_snapshots() {
        let tmp = tempdir().unwrap();
        let game = tmp.path();
        let mut state = InstalledState::new("installer", "1");
        write_tracked(game, "mods/a.jar", b"a", &mut state, "a");
        save_state(game, &state).unwrap();
        for i in 0..7 {
            std::thread::sleep(std::time::Duration::from_millis(8));
            create_snapshot(game, &state, &format!("n{i}")).unwrap();
        }
        let listed = list_snapshots(game).unwrap();
        assert_eq!(listed.len(), 5);
        let dirs: Vec<_> = fs::read_dir(snapshots_dir(game))
            .unwrap()
            .flatten()
            .collect();
        assert_eq!(dirs.len(), 5);
    }
}
