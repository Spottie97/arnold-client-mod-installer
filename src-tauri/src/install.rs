use std::cell::RefCell;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;

use crate::backup::{create_snapshot, rollback};
use crate::config::{
    apply_properties, format_setting_value, ConfigSwitch, LITE_MARKER, MANAGED_HEADER,
};
use crate::detect::{game_running_with, live_minecraft_processes, vanilla_jar_exists};
use crate::download::verify_sha512;
use crate::errors::AppError;
use crate::fabric::install_loader;
use crate::fetch::Fetcher;
use crate::import::{is_known_legacy_snow_jar, SNOW_LEGACY_GLOBS};
use crate::local_source::copy_local_settings;
use crate::log::append_install_log;
use crate::manifest::{component_by_id, lite_preset, FileDest, Manifest};
use crate::plan::{match_glob, InstallPlan, PlanFile};
use crate::quality::{apply_quality, quality_snapshot_paths, record_quality_files, QualityLevel};
use crate::state::{
    hash_file, installer_dir, load_state, record_file, save_state, InstalledComponent,
    InstalledState,
};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProgressEvent {
    pub run_id: String,
    pub step: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    pub status: String,
    pub percent: u8,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplaceAction {
    Replace,
    Ask,
}

pub fn dest_rel(file: &PlanFile) -> String {
    let folder = match file.dest {
        FileDest::Mods => "mods",
        FileDest::Shaderpacks => "shaderpacks",
    };
    format!("{folder}/{}", file.file)
}

pub fn decide_legacy_replace(
    is_tracked: bool,
    file_sha512: Option<&str>,
    known: &[String],
    known_name: bool,
) -> ReplaceAction {
    if is_tracked || known_name {
        return ReplaceAction::Replace;
    }
    if let Some(hash) = file_sha512 {
        if known.iter().any(|k| k.eq_ignore_ascii_case(hash)) {
            return ReplaceAction::Replace;
        }
    }
    ReplaceAction::Ask
}

pub fn snow_replace_patterns(component_id: &str, extra: &[String]) -> Vec<String> {
    let mut patterns = extra.to_vec();
    if component_id == "snow" {
        for glob in SNOW_LEGACY_GLOBS {
            if !patterns.iter().any(|p| p == glob) {
                patterns.push((*glob).to_string());
            }
        }
    }
    patterns
}

fn cancelled(flag: &AtomicBool) -> Result<(), AppError> {
    if flag.load(Ordering::SeqCst) {
        Err(AppError::Cancelled)
    } else {
        Ok(())
    }
}

fn emit(progress: &dyn Fn(ProgressEvent), event: ProgressEvent) {
    progress(event);
}

fn list_dir_files(dir: &Path) -> Result<Vec<String>, AppError> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut names = Vec::new();
    for entry in fs::read_dir(dir)?.flatten() {
        if entry.path().is_file() {
            names.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    Ok(names)
}

struct PendingReplace {
    rel: String,
    action: ReplaceAction,
}

fn collect_replacements(
    game_dir: &Path,
    plan: &InstallPlan,
    state: Option<&InstalledState>,
) -> Result<Vec<PendingReplace>, AppError> {
    let mut out = Vec::new();
    let dests: Vec<String> = plan
        .add
        .iter()
        .chain(plan.update.iter())
        .map(dest_rel)
        .collect();
    for file in plan.add.iter().chain(plan.update.iter()) {
        let folder = match file.dest {
            FileDest::Mods => "mods",
            FileDest::Shaderpacks => "shaderpacks",
        };
        let patterns = snow_replace_patterns(&file.component_id, &file.legacy_match);
        let mut all_patterns = patterns;
        if !all_patterns.iter().any(|p| p == &file.match_glob) {
            all_patterns.push(file.match_glob.clone());
        }
        for name in list_dir_files(&game_dir.join(folder))? {
            if !all_patterns.iter().any(|p| match_glob(p, &name)) {
                continue;
            }
            let rel = format!("{folder}/{name}");
            if dests.iter().any(|d| d == &rel) {
                continue;
            }
            if out.iter().any(|p: &PendingReplace| p.rel == rel) {
                continue;
            }
            let path = game_dir.join(&rel);
            let hash = hash_file(&path).ok();
            let tracked = state.is_some_and(|s| s.file_by_path(&rel).is_some());
            let known_name = file.component_id == "snow" && is_known_legacy_snow_jar(&name);
            out.push(PendingReplace {
                rel,
                action: decide_legacy_replace(
                    tracked,
                    hash.as_deref(),
                    &file.known_sha512,
                    known_name,
                ),
            });
        }
    }
    Ok(out)
}

fn apply_plan_config(
    game_dir: &Path,
    manifest: &Manifest,
    plan: &InstallPlan,
) -> Result<(), AppError> {
    for item in &plan.configs {
        if item.component_id == "snow" {
            // Snowfall 1.3 writes `quality` in apply_quality. The lite-file
            // swap stays there, and only for migrating old installs.
            continue;
        }
        let Some(component) = component_by_id(manifest, &item.component_id) else {
            continue;
        };
        let Some(config) = &component.config else {
            continue;
        };
        let path = game_dir.join(&config.file);
        let existing = fs::read_to_string(&path).ok();
        let mut defaults = std::collections::BTreeMap::new();
        for (key, spec) in &config.settings {
            defaults.insert(key.clone(), format_setting_value(&spec.default));
        }
        let mut lite_over = std::collections::BTreeMap::new();
        if let Some(preset) = lite_preset(config) {
            for (key, value) in &preset.overrides {
                lite_over.insert(key.clone(), format_setting_value(value));
            }
        }
        let switch = if item.snow_lite {
            ConfigSwitch::ToLite
        } else {
            ConfigSwitch::ToNormal
        };
        let header = config.header.as_deref().unwrap_or(MANAGED_HEADER);
        let marker = lite_preset(config)
            .and_then(|p| p.marker.as_deref())
            .unwrap_or(LITE_MARKER);
        let outcome = apply_properties(
            existing.as_deref(),
            &defaults,
            if lite_over.is_empty() {
                None
            } else {
                Some(&lite_over)
            },
            header,
            marker,
            switch,
        );
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, outcome.content.as_bytes())?;
    }
    Ok(())
}

fn build_new_state(
    manifest: &Manifest,
    plan: &InstallPlan,
    files: Vec<(String, String, String)>,
) -> InstalledState {
    let mut state = InstalledState::new("installer", &manifest.manifest_version);
    state.kit = Some(plan.kit.clone());
    for id in &plan.components {
        if let Some(component) = component_by_id(manifest, id) {
            state.components.push(InstalledComponent {
                id: component.id.clone(),
                version: component.version.clone(),
                name: component.name.clone(),
            });
        }
    }
    for (path, sha, component_id) in files {
        record_file(&mut state, path, sha, component_id);
    }
    state
}

#[allow(clippy::too_many_arguments)]
pub fn run_install<F: Fetcher>(
    game_dir: &Path,
    plan: &InstallPlan,
    manifest: &Manifest,
    fetcher: &F,
    cancel: &AtomicBool,
    replace_collisions: bool,
    system_ram_gb: f64,
    progress: &dyn Fn(ProgressEvent),
) -> Result<String, AppError> {
    let run_id = format!("run-{}", crate::state::now_stamp());
    let emit_log = |step: &str, message: &str, percent: u8, status: &str, file: Option<String>| {
        let event = ProgressEvent {
            run_id: run_id.clone(),
            step: step.into(),
            file,
            status: status.into(),
            percent,
            message: message.into(),
        };
        let _ = append_install_log(
            game_dir,
            &serde_json::json!({
                "runId": event.run_id,
                "step": event.step,
                "status": event.status,
                "percent": event.percent,
                "message": event.message,
            }),
        );
        emit(progress, event);
    };

    let snapshot_id: RefCell<Option<String>> = RefCell::new(None);
    let created_new: RefCell<Vec<String>> = RefCell::new(Vec::new());
    let prior_state: RefCell<Option<InstalledState>> = RefCell::new(None);

    let result = (|| -> Result<String, AppError> {
        cancelled(cancel)?;
        if game_running_with(&live_minecraft_processes()) {
            return Err(AppError::GameRunning);
        }
        if !vanilla_jar_exists(game_dir, &manifest.minecraft) {
            return Err(AppError::McMissing {
                version: manifest.minecraft.clone(),
                path: game_dir.display().to_string(),
            });
        }

        emit_log("prepare", "Preparing install", 2, "running", None);
        let staging = installer_dir(game_dir).join("staging").join(&run_id);
        fs::create_dir_all(&staging)?;

        let downloads: Vec<&PlanFile> = plan.add.iter().chain(plan.update.iter()).collect();
        let total = downloads.len().max(1);
        for (index, file) in downloads.iter().enumerate() {
            cancelled(cancel)?;
            emit_log(
                "download",
                &format!("Downloading {} {}", file.name, file.version),
                (5 + (index * 60 / total)) as u8,
                "running",
                Some(file.file.clone()),
            );
            let bytes = fetcher.fetch(&file.url)?;
            verify_sha512(&bytes, &file.sha512)?;
            let staged = staging.join(dest_rel(file));
            if let Some(parent) = staged.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(staged, bytes)?;
        }

        if let Some(loader) = &plan.loader {
            cancelled(cancel)?;
            emit_log(
                "download",
                "Downloading Fabric loader profile",
                66,
                "running",
                Some(loader.id.clone()),
            );
            let bytes = fetcher.fetch_loader(&loader.url)?;
            verify_sha512(&bytes, &loader.sha512)?;
            fs::write(staging.join("loader.json"), bytes)?;
        }

        cancelled(cancel)?;
        let existing = load_state(game_dir)?;
        *prior_state.borrow_mut() = existing.clone();
        let replacements = collect_replacements(game_dir, plan, existing.as_ref())?;
        let asks: Vec<_> = replacements
            .iter()
            .filter(|r| r.action == ReplaceAction::Ask)
            .map(|r| r.rel.clone())
            .collect();
        if !asks.is_empty() && !replace_collisions {
            return Err(AppError::Collision {
                path: asks[0].clone(),
                detail: format!(
                    "Unknown Arnold-looking file(s) {}. Tracked files and known checksums are replaced automatically; this one was left for you to confirm.",
                    asks.join(", ")
                ),
            });
        }

        for file in plan.add.iter().chain(plan.update.iter()) {
            let rel = dest_rel(file);
            let dest = game_dir.join(&rel);
            if dest.is_file() {
                let actual = hash_file(&dest)?;
                if actual == file.sha512 {
                    continue;
                }
                let tracked = existing
                    .as_ref()
                    .is_some_and(|s| s.file_by_path(&rel).is_some());
                if !tracked && !replace_collisions {
                    return Err(AppError::Collision {
                        path: rel,
                        detail: "A different file is already in that slot".into(),
                    });
                }
            }
        }

        emit_log("snapshot", "Saving a restore point", 72, "running", None);
        let mut snap_state = existing
            .clone()
            .unwrap_or_else(|| InstalledState::new("installer", &manifest.manifest_version));
        for file in plan.add.iter().chain(plan.update.iter()) {
            let rel = dest_rel(file);
            let path = game_dir.join(&rel);
            if path.is_file() && snap_state.file_by_path(&rel).is_none() {
                record_file(&mut snap_state, &rel, hash_file(&path)?, "pre-existing");
            }
        }
        for replacement in &replacements {
            let path = game_dir.join(&replacement.rel);
            if path.is_file() && snap_state.file_by_path(&replacement.rel).is_none() {
                record_file(
                    &mut snap_state,
                    &replacement.rel,
                    hash_file(&path)?,
                    "pre-existing",
                );
            }
        }
        for rel in quality_snapshot_paths(manifest, &plan.components) {
            let path = game_dir.join(&rel);
            if path.is_file() && snap_state.file_by_path(&rel).is_none() {
                record_file(&mut snap_state, &rel, hash_file(&path)?, "pre-existing");
            }
        }
        let snapshot = create_snapshot(game_dir, &snap_state, "before-install")?;
        *snapshot_id.borrow_mut() = Some(snapshot.id.clone());
        emit_log(
            "snapshot",
            &format!("Backup saved ({})", snapshot.id),
            75,
            "done",
            None,
        );

        cancelled(cancel)?;
        emit_log("write", "Writing mods", 80, "running", None);
        for file in plan.add.iter().chain(plan.update.iter()) {
            cancelled(cancel)?;
            let rel = dest_rel(file);
            let staged = staging.join(&rel);
            let dest = game_dir.join(&rel);
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            if !dest.exists() {
                created_new.borrow_mut().push(rel.clone());
            }
            fs::copy(staged, dest)?;
        }

        for file in &plan.remove {
            let dest = game_dir.join(&file.file);
            if dest.is_file() {
                if let Some(state) = &existing {
                    if let Some(tracked) = state.file_by_path(&file.file) {
                        if hash_file(&dest).ok().as_deref() == Some(tracked.sha512.as_str()) {
                            fs::remove_file(dest)?;
                        }
                    }
                }
            }
        }
        for replacement in &replacements {
            if replacement.action == ReplaceAction::Replace || replace_collisions {
                let dest = game_dir.join(&replacement.rel);
                if dest.is_file() {
                    fs::remove_file(dest)?;
                }
            }
        }

        if let Some(root) = fetcher.local_root() {
            copy_local_settings(root, game_dir)?;
        }

        let quality_existed: Vec<String> = quality_snapshot_paths(manifest, &plan.components)
            .into_iter()
            .filter(|rel| game_dir.join(rel).is_file())
            .collect();
        apply_plan_config(game_dir, manifest, plan)?;

        emit_log("quality", "Applying quality preset", 88, "running", None);
        let quality_written = apply_quality(
            game_dir,
            manifest,
            plan.quality,
            &plan.components,
            existing.as_ref(),
        )?;
        for (rel, _, _) in &quality_written {
            if !quality_existed.iter().any(|p| p == rel) {
                created_new.borrow_mut().push(rel.clone());
            }
        }

        if let Some(loader) = &plan.loader {
            emit_log("fabric", "Installing Fabric 26.3", 90, "running", None);
            let staged = staging.join("loader.json");
            if staged.is_file() {
                let bytes = fs::read(&staged)?;
                crate::fabric::write_version_profile(game_dir, &loader.id, &bytes)?;
                crate::fabric::upsert_launcher_profile(game_dir, &loader.id, system_ram_gb)?;
            } else {
                install_loader(
                    game_dir,
                    &manifest.loader,
                    fetcher,
                    system_ram_gb,
                    &manifest.minecraft,
                )?;
            }
        } else if !crate::fabric::arnold_profile_present(game_dir) {
            if let Some(existing_id) =
                crate::fabric::existing_fabric_for(game_dir, &manifest.minecraft)
            {
                crate::fabric::upsert_launcher_profile(game_dir, &existing_id, system_ram_gb)?;
            }
        }

        emit_log("state", "Recording state.json", 96, "running", None);
        let mut recorded = Vec::new();
        for file in plan.add.iter().chain(plan.update.iter()) {
            let rel = dest_rel(file);
            recorded.push((rel, file.sha512.clone(), file.component_id.clone()));
        }
        if let Some(prev) = &existing {
            for tracked in &prev.files {
                if recorded.iter().any(|(p, _, _)| p == &tracked.path) {
                    continue;
                }
                if plan.remove.iter().any(|r| r.file == tracked.path) {
                    continue;
                }
                if replacements.iter().any(|r| r.rel == tracked.path) {
                    continue;
                }
                recorded.push((
                    tracked.path.clone(),
                    tracked.sha512.clone(),
                    tracked.component_id.clone(),
                ));
            }
        }
        let mut new_state = build_new_state(manifest, plan, recorded);
        record_quality_files(&mut new_state, quality_written, plan.quality);
        save_state(game_dir, &new_state)?;
        emit_log("done", "Install finished", 100, "done", None);
        let _ = fs::remove_dir_all(&staging);
        Ok(run_id.clone())
    })();

    if let Err(err) = &result {
        if matches!(err, AppError::Cancelled) {
            emit_log("rollback", "Cancelled — restoring", 100, "cancelled", None);
        } else {
            emit_log("error", &err.to_string(), 100, "error", None);
        }
        if let Some(id) = snapshot_id.borrow().clone() {
            let current = load_state(game_dir)
                .ok()
                .flatten()
                .unwrap_or_else(|| InstalledState::new("installer", &manifest.manifest_version));
            let _ = rollback(game_dir, &id, &current);
            match prior_state.borrow().as_ref() {
                Some(orig) => {
                    let _ = save_state(game_dir, orig);
                }
                None => {
                    let _ = fs::remove_file(crate::state::state_path(game_dir));
                }
            }
            for rel in created_new.borrow().iter() {
                let _ = fs::remove_file(game_dir.join(rel));
            }
        }
        let _ = fs::remove_dir_all(installer_dir(game_dir).join("staging").join(&run_id));
    }
    result
}

pub fn repair_tracked<F: Fetcher>(
    game_dir: &Path,
    manifest: &Manifest,
    fetcher: &F,
    cancel: &AtomicBool,
    system_ram_gb: f64,
    progress: &dyn Fn(ProgressEvent),
) -> Result<String, AppError> {
    let state = load_state(game_dir)?
        .ok_or_else(|| AppError::State("nothing is recorded to repair".into()))?;
    let mut add = Vec::new();
    for tracked in &state.files {
        let path = game_dir.join(&tracked.path);
        let needs = if !path.is_file() {
            true
        } else {
            hash_file(&path)? != tracked.sha512
        };
        if !needs {
            continue;
        }
        let Some(component) = component_by_id(manifest, &tracked.component_id) else {
            continue;
        };
        let file_name = Path::new(&tracked.path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&tracked.path);
        if let Some(file) = component
            .files
            .iter()
            .find(|f| f.file == file_name || match_glob(&f.match_glob, file_name))
        {
            add.push(PlanFile {
                component_id: component.id.clone(),
                name: component.name.clone(),
                version: component.version.clone(),
                file: file.file.clone(),
                dest: file.dest,
                url: file.url.clone(),
                sha512: file.sha512.clone(),
                size: file.size,
                match_glob: file.match_glob.clone(),
                legacy_match: component.legacy_match.clone(),
                known_sha512: component.known_sha512.clone(),
            });
        }
    }
    let plan = InstallPlan {
        add,
        update: Vec::new(),
        remove: Vec::new(),
        configs: Vec::new(),
        loader: None,
        snow_lite: state.quality.unwrap_or_default().snow_lite(),
        quality: state.quality.unwrap_or(QualityLevel::Normal),
        kit: state.kit.clone().unwrap_or_else(|| "repair".into()),
        components: state.components.iter().map(|c| c.id.clone()).collect(),
        warnings: Vec::new(),
        bytes: 0,
    };
    run_install(
        game_dir,
        &plan,
        manifest,
        fetcher,
        cancel,
        true,
        system_ram_gb,
        progress,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::download::sha512_hex;
    use crate::fetch::MapFetcher;
    use crate::manifest::load_bundled_manifest;
    use crate::plan::{build_plan, Selection};
    use std::sync::Mutex;
    use tempfile::tempdir;

    fn test_manifest_and_fetcher() -> (crate::manifest::Manifest, MapFetcher, Vec<u8>) {
        let mut manifest = load_bundled_manifest().unwrap();
        let mut fetcher = MapFetcher::default();
        for component in &mut manifest.components {
            for file in &mut component.files {
                let bytes = format!("blob-{}", file.file).into_bytes();
                file.sha512 = sha512_hex(&bytes);
                file.size = bytes.len() as u64;
                fetcher.insert(&file.url, bytes);
            }
        }
        let loader_bytes =
            b"{\"id\":\"fabric-loader-0.20.1-26.3\",\"inheritsFrom\":\"26.3\"}".to_vec();
        manifest.loader.sha512 = sha512_hex(&loader_bytes);
        fetcher.insert(&manifest.loader.url, loader_bytes.clone());
        (manifest, fetcher, loader_bytes)
    }

    fn prepare_game(game: &Path) {
        fs::create_dir_all(game.join("versions/26.3")).unwrap();
        fs::write(game.join("versions/26.3/26.3.jar"), b"jar").unwrap();
        fs::create_dir_all(game.join("mods")).unwrap();
    }

    fn basic_plan(manifest: &crate::manifest::Manifest) -> InstallPlan {
        build_plan(
            manifest,
            &Selection {
                kit: "basic".into(),
                custom_components: vec![],
                ocean: false,
                snow_lite: Some(true),
                quality: None,
            },
            None,
            true,
        )
        .unwrap()
    }

    #[test]
    fn decide_replace_only_when_tracked_or_known() {
        assert_eq!(
            decide_legacy_replace(true, Some("abc"), &[], false),
            ReplaceAction::Replace
        );
        assert_eq!(
            decide_legacy_replace(false, Some("known"), &["known".into()], false),
            ReplaceAction::Replace
        );
        assert_eq!(
            decide_legacy_replace(false, Some("mystery"), &["known".into()], false),
            ReplaceAction::Ask
        );
        assert_eq!(
            decide_legacy_replace(false, Some("mystery"), &[], true),
            ReplaceAction::Replace
        );
    }

    #[test]
    fn successful_install_writes_files_and_state() {
        let tmp = tempdir().unwrap();
        let game = tmp.path();
        prepare_game(game);
        let (manifest, fetcher, _) = test_manifest_and_fetcher();
        let plan = basic_plan(&manifest);
        let events = Mutex::new(Vec::new());
        let run = run_install(
            game,
            &plan,
            &manifest,
            &fetcher,
            &AtomicBool::new(false),
            false,
            16.0,
            &|e| events.lock().unwrap().push(e),
        )
        .unwrap();
        assert!(run.starts_with("run-"));
        let snow = game.join("mods/arnold-snowfall-1.3.3.jar");
        assert!(snow.is_file());
        let state = load_state(game).unwrap().unwrap();
        assert_eq!(state.source, "installer");
        assert!(state.files.iter().any(|f| f.path.contains("snowfall")));
        let config = fs::read_to_string(game.join("config/arnold-snow-client.properties")).unwrap();
        assert!(config.starts_with("# arnold-installer-managed"), "{config}");
        assert!(config.contains("quality=lite"), "{config}");
        assert!(!config.starts_with("# arnold-installer-lite"));
        let logged = events.lock().unwrap();
        assert!(logged.iter().any(|e| e.step == "download"));
        assert!(logged.last().unwrap().percent == 100);
    }

    #[test]
    fn hash_mismatch_leaves_dest_untouched() {
        let tmp = tempdir().unwrap();
        let game = tmp.path();
        prepare_game(game);
        let (manifest, mut fetcher, _) = test_manifest_and_fetcher();
        let plan = basic_plan(&manifest);
        let url = &plan.add[0].url;
        fetcher.insert(url, b"tampered".to_vec());
        let err = run_install(
            game,
            &plan,
            &manifest,
            &fetcher,
            &AtomicBool::new(false),
            false,
            16.0,
            &|_| {},
        )
        .unwrap_err();
        assert_eq!(err.code(), "E-HASH-MISMATCH");
        assert!(!game.join("mods").join(&plan.add[0].file).exists());
        assert!(load_state(game).unwrap().is_none());
    }

    #[test]
    fn cancel_before_write_leaves_folder_clean() {
        let tmp = tempdir().unwrap();
        let game = tmp.path();
        prepare_game(game);
        let (manifest, fetcher, _) = test_manifest_and_fetcher();
        let plan = basic_plan(&manifest);
        let cancel = AtomicBool::new(true);
        let err = run_install(
            game,
            &plan,
            &manifest,
            &fetcher,
            &cancel,
            false,
            16.0,
            &|_| {},
        )
        .unwrap_err();
        assert_eq!(err.code(), "E-CANCELLED");
        assert!(fs::read_dir(game.join("mods")).unwrap().next().is_none());
    }

    #[test]
    fn cancel_after_snapshot_rolls_back() {
        let tmp = tempdir().unwrap();
        let game = tmp.path();
        prepare_game(game);
        fs::write(game.join("mods/keep-me.jar"), b"user").unwrap();
        let (manifest, fetcher, _) = test_manifest_and_fetcher();
        let plan = basic_plan(&manifest);
        let cancel = AtomicBool::new(false);
        let err = run_install(
            game,
            &plan,
            &manifest,
            &fetcher,
            &cancel,
            false,
            16.0,
            &|event| {
                if event.step == "write" {
                    cancel.store(true, Ordering::SeqCst);
                }
            },
        )
        .unwrap_err();
        assert_eq!(err.code(), "E-CANCELLED");
        assert_eq!(fs::read(game.join("mods/keep-me.jar")).unwrap(), b"user");
        assert!(!game.join("mods/arnold-snowfall-1.3.3.jar").exists());
    }

    #[test]
    fn unknown_snow_jar_asks() {
        let tmp = tempdir().unwrap();
        let game = tmp.path();
        prepare_game(game);
        fs::write(game.join("mods/arnold-snowfall-mystery.jar"), b"not-ours").unwrap();
        let (manifest, fetcher, _) = test_manifest_and_fetcher();
        let plan = basic_plan(&manifest);
        let err = run_install(
            game,
            &plan,
            &manifest,
            &fetcher,
            &AtomicBool::new(false),
            false,
            16.0,
            &|_| {},
        )
        .unwrap_err();
        assert_eq!(err.code(), "P-COLLISION");
        assert!(game.join("mods/arnold-snowfall-mystery.jar").exists());
        assert!(!game.join("mods/arnold-snowfall-1.3.3.jar").exists());
    }

    #[test]
    fn known_legacy_snow_is_replaced() {
        let tmp = tempdir().unwrap();
        let game = tmp.path();
        prepare_game(game);
        let (mut manifest, mut fetcher, _) = test_manifest_and_fetcher();
        let old = b"legacy-snow-client";
        let old_hash = sha512_hex(old);
        if let Some(snow) = manifest.components.iter_mut().find(|c| c.id == "snow") {
            snow.known_sha512.push(old_hash.clone());
            snow.legacy_match = vec![
                "arnold-snow-client*.jar".into(),
                "arnold-snowfall*.jar".into(),
            ];
        }
        for component in &manifest.components {
            for file in &component.files {
                fetcher.insert(&file.url, format!("blob-{}", file.file).into_bytes());
            }
        }
        fs::write(game.join("mods/arnold-snow-client-0.8.0.jar"), old).unwrap();
        let mut plan = basic_plan(&manifest);
        for file in plan.add.iter_mut().chain(plan.update.iter_mut()) {
            if file.component_id == "snow" {
                file.known_sha512 = vec![old_hash.clone()];
                file.legacy_match = vec![
                    "arnold-snow-client*.jar".into(),
                    "arnold-snowfall*.jar".into(),
                ];
            }
        }
        run_install(
            game,
            &plan,
            &manifest,
            &fetcher,
            &AtomicBool::new(false),
            false,
            16.0,
            &|_| {},
        )
        .unwrap();
        assert!(!game.join("mods/arnold-snow-client-0.8.0.jar").exists());
        assert!(game.join("mods/arnold-snowfall-1.3.3.jar").exists());
    }

    #[test]
    fn named_legacy_snowfall_jars_are_replaced() {
        let tmp = tempdir().unwrap();
        let game = tmp.path();
        prepare_game(game);
        let (manifest, fetcher, _) = test_manifest_and_fetcher();
        for name in [
            "arnold-snowfall-1.2.0.jar",
            "arnold-snowfall-1.3.0.jar",
            "arnold-snowfall-1.3.1.jar",
            "arnold-snowfall-1.3.2.jar",
        ] {
            fs::write(game.join("mods").join(name), format!("old-{name}")).unwrap();
        }
        let plan = basic_plan(&manifest);
        run_install(
            game,
            &plan,
            &manifest,
            &fetcher,
            &AtomicBool::new(false),
            false,
            16.0,
            &|_| {},
        )
        .unwrap();
        for name in [
            "arnold-snowfall-1.2.0.jar",
            "arnold-snowfall-1.3.0.jar",
            "arnold-snowfall-1.3.1.jar",
            "arnold-snowfall-1.3.2.jar",
        ] {
            assert!(!game.join("mods").join(name).exists(), "{name} should go");
        }
        assert!(game.join("mods/arnold-snowfall-1.3.3.jar").exists());
    }

    #[test]
    fn repair_redownloads_bad_tracked_file() {
        let tmp = tempdir().unwrap();
        let game = tmp.path();
        prepare_game(game);
        let (manifest, fetcher, _) = test_manifest_and_fetcher();
        let plan = basic_plan(&manifest);
        run_install(
            game,
            &plan,
            &manifest,
            &fetcher,
            &AtomicBool::new(false),
            false,
            16.0,
            &|_| {},
        )
        .unwrap();
        fs::write(game.join("mods/arnold-snowfall-1.3.3.jar"), b"broken").unwrap();
        repair_tracked(
            game,
            &manifest,
            &fetcher,
            &AtomicBool::new(false),
            16.0,
            &|_| {},
        )
        .unwrap();
        let expected = "blob-arnold-snowfall-1.3.3.jar".to_string();
        assert_eq!(
            fs::read(game.join("mods/arnold-snowfall-1.3.3.jar")).unwrap(),
            expected.as_bytes()
        );
    }

    #[test]
    fn install_writes_sodium_and_records_quality() {
        let tmp = tempdir().unwrap();
        let game = tmp.path();
        prepare_game(game);
        let (manifest, fetcher, _) = test_manifest_and_fetcher();
        let mut plan = basic_plan(&manifest);
        plan.quality = crate::quality::QualityLevel::Potato;
        run_install(
            game,
            &plan,
            &manifest,
            &fetcher,
            &AtomicBool::new(false),
            false,
            16.0,
            &|_| {},
        )
        .unwrap();
        let sodium: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(game.join("config/sodium-options.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(sodium["quality"]["weather_quality"], "FAST");
        assert_eq!(sodium["quality"]["leaves_quality"], "FAST");
        let options = fs::read_to_string(game.join("options.txt")).unwrap();
        assert!(options.contains("renderDistance:6"));
        assert!(options.contains("cloudStatus:fast"));
        let state = load_state(game).unwrap().unwrap();
        assert_eq!(state.quality, Some(crate::quality::QualityLevel::Potato));
        assert!(state
            .files
            .iter()
            .any(|f| f.path == "config/sodium-options.json"));
        assert!(!state.files.iter().any(|f| f.path == "options.txt"));
    }
}
