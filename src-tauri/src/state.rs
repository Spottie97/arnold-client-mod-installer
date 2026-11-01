use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::download::sha512_hex;
use crate::errors::AppError;

pub const STATE_SCHEMA: u32 = 1;
pub const INSTALLER_DIR: &str = "arnold-installer";
pub const STATE_FILE: &str = "state.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TrackedFile {
    pub path: String,
    pub sha512: String,
    pub component_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InstalledComponent {
    pub id: String,
    pub version: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InstalledState {
    pub schema: u32,
    pub source: String,
    pub manifest_version: String,
    #[serde(default)]
    pub kit: Option<String>,
    #[serde(default)]
    pub components: Vec<InstalledComponent>,
    #[serde(default)]
    pub files: Vec<TrackedFile>,
    pub updated_at: String,
}

impl InstalledState {
    pub fn new(source: impl Into<String>, manifest_version: impl Into<String>) -> Self {
        Self {
            schema: STATE_SCHEMA,
            source: source.into(),
            manifest_version: manifest_version.into(),
            kit: None,
            components: Vec::new(),
            files: Vec::new(),
            updated_at: now_stamp(),
        }
    }

    pub fn tracked_paths(&self) -> Vec<String> {
        self.files.iter().map(|f| f.path.clone()).collect()
    }

    pub fn file_by_path(&self, path: &str) -> Option<&TrackedFile> {
        self.files.iter().find(|f| f.path == path)
    }
}

pub fn installer_dir(game_dir: &Path) -> PathBuf {
    game_dir.join(INSTALLER_DIR)
}

pub fn state_path(game_dir: &Path) -> PathBuf {
    installer_dir(game_dir).join(STATE_FILE)
}

pub fn now_stamp() -> String {
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("unix-{}-{:09}", dur.as_secs(), dur.subsec_nanos())
}

pub fn load_state(game_dir: &Path) -> Result<Option<InstalledState>, AppError> {
    let path = state_path(game_dir);
    if !path.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(&path).map_err(|e| AppError::State(e.to_string()))?;
    let state: InstalledState =
        serde_json::from_str(&text).map_err(|e| AppError::State(e.to_string()))?;
    if state.schema != STATE_SCHEMA {
        return Err(AppError::State(format!(
            "unsupported state schema {}",
            state.schema
        )));
    }
    Ok(Some(state))
}

pub fn save_state(game_dir: &Path, state: &InstalledState) -> Result<(), AppError> {
    fs::create_dir_all(installer_dir(game_dir))?;
    let tmp = state_path(game_dir).with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(state)?)?;
    fs::rename(tmp, state_path(game_dir))?;
    Ok(())
}

pub fn record_file(
    state: &mut InstalledState,
    path: impl Into<String>,
    sha512: impl Into<String>,
    component_id: impl Into<String>,
) {
    let path = path.into();
    state.files.retain(|f| f.path != path);
    state.files.push(TrackedFile {
        path,
        sha512: sha512.into(),
        component_id: component_id.into(),
    });
    state.updated_at = now_stamp();
}

pub fn remove_component(state: &mut InstalledState, id: &str) -> Vec<TrackedFile> {
    state.components.retain(|c| c.id != id);
    let removed: Vec<_> = state
        .files
        .iter()
        .filter(|f| f.component_id == id)
        .cloned()
        .collect();
    state.files.retain(|f| f.component_id != id);
    state.updated_at = now_stamp();
    removed
}

pub fn hash_file(path: &Path) -> Result<String, AppError> {
    let bytes = fs::read(path)?;
    Ok(sha512_hex(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn write_read_round_trip() {
        let tmp = tempdir().unwrap();
        let game = tmp.path();
        let mut state = InstalledState::new("installer", "2026.11.01-1");
        state.kit = Some("full".into());
        record_file(&mut state, "mods/sodium.jar", "abc", "sodium");
        save_state(game, &state).unwrap();
        let loaded = load_state(game).unwrap().unwrap();
        assert_eq!(loaded.kit.as_deref(), Some("full"));
        assert_eq!(loaded.files.len(), 1);
        assert_eq!(loaded.files[0].component_id, "sodium");
        assert_eq!(loaded.tracked_paths(), vec!["mods/sodium.jar".to_string()]);
    }

    #[test]
    fn missing_state_is_none() {
        let tmp = tempdir().unwrap();
        assert!(load_state(tmp.path()).unwrap().is_none());
    }

    #[test]
    fn remove_component_drops_only_its_files() {
        let mut state = InstalledState::new("installer", "1");
        record_file(&mut state, "mods/a.jar", "1", "ships");
        record_file(&mut state, "mods/b.jar", "2", "deck");
        let removed = remove_component(&mut state, "ships");
        assert_eq!(removed.len(), 1);
        assert_eq!(state.files.len(), 1);
        assert_eq!(state.files[0].component_id, "deck");
    }
}
