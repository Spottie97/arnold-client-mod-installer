use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::download::sha512_hex;
use crate::errors::AppError;
use crate::manifest::{component_by_id, Manifest};
use crate::plan::match_glob;
use crate::state::{load_state, record_file, save_state, InstalledComponent, InstalledState};

pub const SNOW_LEGACY_GLOBS: &[&str] = &["arnold-snow-client*.jar", "arnold-snowfall*.jar"];

/// Older shipping and review jars that install may replace by name.
pub const SNOW_LEGACY_JARS: &[&str] = &[
    "arnold-snowfall-1.2.0.jar",
    "arnold-snowfall-1.3.0.jar",
    "arnold-snowfall-1.3.1.jar",
    "arnold-snowfall-1.3.2.jar",
    "arnold-snow-client-1.2.0.jar",
    "arnold-snow-client-1.3.0.jar",
    "arnold-snow-client-1.3.1.jar",
    "arnold-snow-client-1.3.2.jar",
];

pub fn is_known_legacy_snow_jar(file_name: &str) -> bool {
    SNOW_LEGACY_JARS.contains(&file_name)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ImportJar {
    pub path: String,
    pub file_name: String,
    #[serde(default)]
    pub component_id: Option<String>,
    pub sha512: String,
    pub known_build: bool,
    pub tracked: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ImportBackup {
    pub path: String,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ImportOffer {
    pub found: bool,
    #[serde(default)]
    pub version_guess: Option<String>,
    pub jars: Vec<ImportJar>,
    pub backups: Vec<ImportBackup>,
    pub already_adopted: bool,
}

pub fn backup_roots(game_dir: &Path) -> Vec<(PathBuf, &'static str)> {
    vec![
        (game_dir.join("arnold-backups"), "folder"),
        (game_dir.join("mods").join(".arnold-backup"), "folder"),
        (game_dir.join("arnold-installer-2.3"), "folder"),
        (game_dir.join(".arnold-2.3"), "folder"),
        (game_dir.join("mods").join("backup"), "folder"),
    ]
}

pub fn detect_import(game_dir: &Path, manifest: &Manifest) -> Result<ImportOffer, AppError> {
    let already_adopted = load_state(game_dir)?.is_some();
    let mods = game_dir.join("mods");
    let mut jars = Vec::new();
    if mods.is_dir() {
        for entry in fs::read_dir(&mods)?.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let lower = name.to_ascii_lowercase();
            if !lower.starts_with("arnold-") || !lower.ends_with(".jar") {
                continue;
            }
            let bytes = fs::read(&path)?;
            let sha = sha512_hex(&bytes);
            let component_id = match_component(manifest, &name);
            let known_build = component_id
                .as_deref()
                .and_then(|id| component_by_id(manifest, id))
                .is_some_and(|c| {
                    c.known_sha512.iter().any(|h| h.eq_ignore_ascii_case(&sha))
                        || c.files.iter().any(|f| f.sha512.eq_ignore_ascii_case(&sha))
                });
            jars.push(ImportJar {
                path: format!("mods/{name}"),
                file_name: name,
                component_id,
                sha512: sha,
                known_build,
                tracked: false,
            });
        }
    }
    jars.sort_by(|a, b| a.file_name.cmp(&b.file_name));

    let mut backups = Vec::new();
    for (root, kind) in backup_roots(game_dir) {
        if root.is_dir() {
            backups.push(ImportBackup {
                path: rel_to_game(game_dir, &root),
                kind: kind.to_string(),
            });
        }
    }
    if mods.is_dir() {
        for entry in fs::read_dir(&mods)?.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.to_ascii_lowercase().ends_with(".jar.bak") {
                backups.push(ImportBackup {
                    path: format!("mods/{name}"),
                    kind: "bak".into(),
                });
            }
        }
    }

    let version_guess = if jars.iter().any(|j| j.file_name.contains("snowfall-1.2")) {
        Some("2.3.x / 1.2.0 snowfall".into())
    } else if jars.iter().any(|j| {
        j.file_name.contains("snowfall-1.3.0")
            || j.file_name.contains("snowfall-1.3.1")
            || j.file_name.contains("snowfall-1.3.2")
    }) {
        Some("2.3.x / 1.3.x snowfall".into())
    } else if !jars.is_empty() {
        Some("2.3.x".into())
    } else {
        None
    };

    Ok(ImportOffer {
        found: !jars.is_empty() || !backups.is_empty(),
        version_guess,
        jars,
        backups,
        already_adopted,
    })
}

fn match_component(manifest: &Manifest, file_name: &str) -> Option<String> {
    for component in &manifest.components {
        for file in &component.files {
            if match_glob(&file.match_glob, file_name) || file.file == file_name {
                return Some(component.id.clone());
            }
        }
        for pattern in &component.legacy_match {
            if match_glob(pattern, file_name) {
                return Some(component.id.clone());
            }
        }
    }
    if SNOW_LEGACY_GLOBS
        .iter()
        .any(|pattern| match_glob(pattern, file_name))
    {
        return Some("snow".into());
    }
    None
}

fn rel_to_game(game_dir: &Path, path: &Path) -> String {
    path.strip_prefix(game_dir)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path.to_string_lossy().replace('\\', "/"))
}

pub fn adopt_import(
    game_dir: &Path,
    manifest: &Manifest,
    confirmed: bool,
) -> Result<InstalledState, AppError> {
    if !confirmed {
        return Err(AppError::State("import was not confirmed".into()));
    }
    let offer = detect_import(game_dir, manifest)?;
    if !offer.found {
        return Err(AppError::State("no 2.3.x install was found".into()));
    }
    let mut state = InstalledState::new("Imported from 2.3.x", "2.3.x");
    state.kit = Some("imported".into());
    for jar in &offer.jars {
        let id = jar.component_id.clone().unwrap_or_else(|| "unknown".into());
        if let Some(component) = component_by_id(manifest, &id) {
            if !state.components.iter().any(|c| c.id == id) {
                state.components.push(InstalledComponent {
                    id: id.clone(),
                    version: component.version.clone(),
                    name: component.name.clone(),
                });
            }
        }
        record_file(&mut state, &jar.path, &jar.sha512, id);
    }
    save_state(game_dir, &state)?;
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::download::sha512_hex;
    use crate::manifest::load_bundled_manifest;
    use tempfile::tempdir;

    #[test]
    fn snow_legacy_globs_cover_both_names() {
        assert!(match_glob(
            "arnold-snow-client*.jar",
            "arnold-snow-client-0.8.0.jar"
        ));
        assert!(match_glob(
            "arnold-snowfall*.jar",
            "arnold-snowfall-1.2.0.jar"
        ));
        assert!(match_glob(
            "arnold-snowfall*.jar",
            "arnold-snowfall-1.3.2.jar"
        ));
        assert!(match_glob(
            "arnold-snowfall*.jar",
            "arnold-snowfall-1.3.3.jar"
        ));
        assert!(match_glob(
            "arnold-snowfall*.jar",
            "arnold-snowfall-1.1.0.jar"
        ));
        assert!(!match_glob(
            "arnold-snow-client*.jar",
            "arnold-snow-0.8.0.jar"
        ));
        assert!(is_known_legacy_snow_jar("arnold-snowfall-1.2.0.jar"));
        assert!(is_known_legacy_snow_jar("arnold-snowfall-1.3.0.jar"));
        assert!(is_known_legacy_snow_jar("arnold-snowfall-1.3.1.jar"));
        assert!(is_known_legacy_snow_jar("arnold-snowfall-1.3.2.jar"));
        assert!(!is_known_legacy_snow_jar("arnold-snowfall-1.3.3.jar"));
        assert!(!is_known_legacy_snow_jar("arnold-snowfall-mystery.jar"));
    }

    #[test]
    fn detects_jars_and_backups() {
        let tmp = tempdir().unwrap();
        let game = tmp.path();
        fs::create_dir_all(game.join("mods")).unwrap();
        fs::create_dir_all(game.join("arnold-backups")).unwrap();
        fs::write(game.join("mods/arnold-snowfall-1.2.0.jar"), b"snow").unwrap();
        fs::write(game.join("mods/arnold-ships-client-0.10.2.jar"), b"ships").unwrap();
        fs::write(game.join("mods/arnold-deck-client-0.11.1.jar"), b"deck").unwrap();
        fs::write(game.join("mods/user-extra.jar"), b"keep").unwrap();
        fs::write(game.join("mods/arnold-snowfall-1.1.0.jar.bak"), b"old").unwrap();
        let manifest = load_bundled_manifest().unwrap();
        let offer = detect_import(game, &manifest).unwrap();
        assert!(offer.found);
        assert!(!offer.already_adopted);
        assert_eq!(offer.jars.len(), 3);
        assert!(offer
            .jars
            .iter()
            .any(|j| j.component_id.as_deref() == Some("snow")));
        assert!(offer.backups.iter().any(|b| b.path == "arnold-backups"));
        assert!(offer.backups.iter().any(|b| b.kind == "bak"));
    }

    #[test]
    fn adopt_requires_confirm_and_writes_state() {
        let tmp = tempdir().unwrap();
        let game = tmp.path();
        fs::create_dir_all(game.join("mods")).unwrap();
        fs::write(game.join("mods/arnold-snowfall-1.2.0.jar"), b"snow").unwrap();
        let manifest = load_bundled_manifest().unwrap();
        let err = adopt_import(game, &manifest, false).unwrap_err();
        assert_eq!(err.code(), "E-STATE");
        let state = adopt_import(game, &manifest, true).unwrap();
        assert_eq!(state.source, "Imported from 2.3.x");
        assert_eq!(state.files[0].sha512, sha512_hex(b"snow"));
        assert!(load_state(game).unwrap().is_some());
    }
}
