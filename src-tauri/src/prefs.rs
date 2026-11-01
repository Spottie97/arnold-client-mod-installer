use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::errors::AppError;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Prefs {
    #[serde(default)]
    pub last_game_dir: Option<PathBuf>,
    #[serde(default)]
    pub tour_done: bool,
}

pub fn prefs_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("arnold-client-mods")
        .join("prefs.json")
}

pub fn load_prefs_from(path: &Path) -> Result<Prefs, AppError> {
    if !path.is_file() {
        return Ok(Prefs::default());
    }
    let text = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&text)?)
}

pub fn save_prefs_to(path: &Path, prefs: &Prefs) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(prefs)?)?;
    fs::rename(tmp, path)?;
    Ok(())
}

pub fn load_prefs() -> Result<Prefs, AppError> {
    load_prefs_from(&prefs_path())
}

pub fn save_prefs(prefs: &Prefs) -> Result<(), AppError> {
    save_prefs_to(&prefs_path(), prefs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn remembers_last_folder_and_tour() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("prefs.json");
        let prefs = Prefs {
            last_game_dir: Some(PathBuf::from("/home/sam/.minecraft")),
            tour_done: true,
        };
        save_prefs_to(&path, &prefs).unwrap();
        let loaded = load_prefs_from(&path).unwrap();
        assert_eq!(
            loaded.last_game_dir.unwrap(),
            PathBuf::from("/home/sam/.minecraft")
        );
        assert!(loaded.tour_done);
    }

    #[test]
    fn missing_file_is_default() {
        let tmp = tempdir().unwrap();
        let prefs = load_prefs_from(&tmp.path().join("missing.json")).unwrap();
        assert!(prefs.last_game_dir.is_none());
        assert!(!prefs.tour_done);
    }
}
