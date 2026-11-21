use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
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
use crate::fetch::{FetchKind, Fetcher};
use crate::import::{is_known_legacy_snow_jar, SNOW_LEGACY_GLOBS};
use crate::local_source::copy_local_settings;
use crate::log::append_install_log;
use crate::manifest::{component_by_id, lite_preset, Manifest};
use crate::plan::{match_glob, InstallPlan, PlanFile};
use crate::profiles::{ProfileSpec, ProfileTargets, TlWrite};
use crate::quality::{apply_quality, quality_snapshot_paths, record_quality_files, QualityLevel};
use crate::state::{
    hash_file, installer_dir, load_state, record_file, save_state, InstalledComponent,
    InstalledState,
};

#[derive(Debug, Clone)]
pub struct InstallTarget {
    pub pack_dir: PathBuf,
    pub launcher_roots: Vec<PathBuf>,
    pub profiles: ProfileTargets,
    pub spec: ProfileSpec,
    pub require_vanilla: bool,
    /// Keeps the pre-pack install path: versions and the legacy profile stay in `pack_dir`.
    pub legacy_same_folder: bool,
}

impl InstallTarget {
    pub fn same_folder(game_dir: &Path) -> Self {
        Self {
            pack_dir: game_dir.to_path_buf(),
            launcher_roots: vec![game_dir.to_path_buf()],
            profiles: ProfileTargets {
                official: Some(game_dir.to_path_buf()),
                tlauncher: None,
                also_pack_dir: false,
            },
            spec: ProfileSpec {
                key: crate::fabric::ARNOLD_PROFILE_KEY.into(),
                name: crate::fabric::ARNOLD_PROFILE_NAME.into(),
                version_id: String::new(),
                game_dir: game_dir.to_path_buf(),
                java_args: String::new(),
                update_legacy_in_place: false,
            },
            require_vanilla: true,
            legacy_same_folder: true,
        }
    }
}

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
    format!("{}/{}", file.dest.folder(), file.file)
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
    kept_aside: bool,
}

fn kept_aside_detail(rel: &str) -> String {
    format!(
        "{rel} is your own file. It was kept aside when this pack's saved copy was put back, so the installer won't remove or replace it unless you confirm."
    )
}

fn refuse_unconfirmed_collisions(
    game_dir: &Path,
    plan: &InstallPlan,
    existing: Option<&InstalledState>,
    kept_aside: &[String],
    replacements: &[PendingReplace],
    replace_collisions: bool,
) -> Result<(), AppError> {
    if replace_collisions {
        return Ok(());
    }
    if let Some(kept) = replacements
        .iter()
        .find(|item| item.action == ReplaceAction::Ask && item.kept_aside)
    {
        return Err(AppError::Collision {
            path: kept.rel.clone(),
            detail: kept_aside_detail(&kept.rel),
        });
    }
    let asks: Vec<String> = replacements
        .iter()
        .filter(|item| item.action == ReplaceAction::Ask)
        .map(|item| item.rel.clone())
        .collect();
    if let Some(path) = asks.first() {
        return Err(AppError::Collision {
            path: path.clone(),
            detail: format!(
                "Unknown Arnold-looking file(s) {}. Tracked files and known checksums are replaced automatically; this one was left for you to confirm.",
                asks.join(", ")
            ),
        });
    }
    for file in plan.add.iter().chain(plan.update.iter()) {
        let rel = dest_rel(file);
        let dest = game_dir.join(&rel);
        if !dest.is_file() {
            continue;
        }
        let actual = hash_file(&dest)?;
        if actual == file.sha512 {
            continue;
        }
        let tracked = existing.is_some_and(|state| state.file_by_path(&rel).is_some());
        let kept = kept_aside.iter().any(|path| path == &rel);
        if !tracked || kept {
            return Err(AppError::Collision {
                path: rel,
                detail: if kept {
                    kept_aside_detail(&rel)
                } else {
                    "A different file is already in that slot".into()
                },
            });
        }
    }
    Ok(())
}

fn collect_replacements(
    game_dir: &Path,
    plan: &InstallPlan,
    state: Option<&InstalledState>,
    kept_aside: &[String],
) -> Result<Vec<PendingReplace>, AppError> {
    let mut out = Vec::new();
    let dests: Vec<String> = plan
        .add
        .iter()
        .chain(plan.update.iter())
        .map(dest_rel)
        .collect();
    for file in plan.add.iter().chain(plan.update.iter()) {
        let folder = file.dest.folder();
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
            let kept = kept_aside.iter().any(|item| item == &rel);
            let action = if kept {
                ReplaceAction::Ask
            } else {
                decide_legacy_replace(tracked, hash.as_deref(), &file.known_sha512, known_name)
            };
            out.push(PendingReplace {
                rel,
                action,
                kept_aside: kept,
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
    existing: Option<&InstalledState>,
    skipped: &BTreeSet<String>,
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
    if let Some(existing) = existing {
        for prev in &existing.components {
            if skipped.contains(&prev.id) && state.components.iter().all(|item| item.id != prev.id)
            {
                state.components.push(prev.clone());
            }
        }
    }
    for (path, sha, component_id) in files {
        record_file(&mut state, path, sha, component_id);
    }
    state.pack_id = if plan.pack_id.is_empty() {
        existing.and_then(|item| item.pack_id.clone())
    } else {
        Some(plan.pack_id.clone())
    };
    state
}

/// A component is optional only when the plan says so and a fresh read of the
/// manifest agrees. Kit members, locked components, and anything required by
/// a non-optional component stay required.
fn effective_optional(manifest: &Manifest, plan: &InstallPlan) -> BTreeSet<String> {
    let mut optional: BTreeSet<String> = plan.optional_components.iter().cloned().collect();
    optional.retain(|id| plan.components.iter().any(|picked| picked == id));
    optional.retain(|id| {
        component_by_id(manifest, id).is_some_and(|component| !component.required_in_all_kits)
    });
    if let Some(kit) = manifest.kits.get(&plan.kit) {
        optional.retain(|id| !kit.components.iter().any(|member| member == id));
    }
    let outside: Vec<String> = plan
        .components
        .iter()
        .filter(|id| !optional.contains(*id))
        .cloned()
        .collect();
    let mut required_by_outside = BTreeSet::new();
    for id in &outside {
        collect_requires(manifest, id, &mut required_by_outside);
    }
    optional.retain(|id| !required_by_outside.contains(id));
    optional
}

fn collect_requires(manifest: &Manifest, id: &str, out: &mut BTreeSet<String>) {
    let Some(component) = component_by_id(manifest, id) else {
        return;
    };
    for req in &component.requires {
        if out.insert(req.clone()) {
            collect_requires(manifest, req, out);
        }
    }
}

fn grow_skipped(manifest: &Manifest, optional: &BTreeSet<String>, dropped: &mut BTreeSet<String>) {
    loop {
        let mut grew = false;
        for id in optional {
            if dropped.contains(id) {
                continue;
            }
            if requires_dropped(manifest, id, dropped, &mut Vec::new()) {
                dropped.insert(id.clone());
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
}

fn requires_dropped(
    manifest: &Manifest,
    id: &str,
    dropped: &BTreeSet<String>,
    stack: &mut Vec<String>,
) -> bool {
    if stack.iter().any(|seen| seen == id) {
        return false;
    }
    let Some(component) = component_by_id(manifest, id) else {
        return false;
    };
    stack.push(id.to_string());
    let hit = component
        .requires
        .iter()
        .any(|req| dropped.contains(req) || requires_dropped(manifest, req, dropped, stack));
    stack.pop();
    hit
}

fn optional_can_skip(optional: &BTreeSet<String>, component_id: &str, err: &AppError) -> bool {
    !matches!(err, AppError::Cancelled) && optional.contains(component_id)
}

fn skip_reason(err: &AppError) -> &'static str {
    match err {
        AppError::HashMismatch { .. } => "check failed",
        _ => "download failed",
    }
}

fn skipped_done_message(
    manifest: &Manifest,
    components: &[String],
    dropped: &BTreeSet<String>,
    failed: &BTreeMap<String, AppError>,
) -> String {
    let mut parts = Vec::new();
    for id in components {
        if !dropped.contains(id) {
            continue;
        }
        let name = component_by_id(manifest, id)
            .map(|component| component.name.as_str())
            .unwrap_or(id.as_str());
        if let Some(err) = failed.get(id) {
            parts.push(format!("{name} ({})", skip_reason(err)));
        } else {
            parts.push(format!("{name} (needs a skipped add-on)"));
        }
    }
    if parts.is_empty() {
        return "Install finished".to_string();
    }
    format!(
        "Install finished — skipped optional add-on(s): {}. Everything else is installed; run Install again to retry.",
        parts.join(", ")
    )
}

struct SkipBook<'a> {
    manifest: &'a Manifest,
    optional: &'a BTreeSet<String>,
    skipped_errors: &'a mut BTreeMap<String, AppError>,
    dropped: &'a mut BTreeSet<String>,
}

fn record_optional_skip(
    book: &mut SkipBook<'_>,
    emit_log: &impl Fn(&str, &str, u8, &str, Option<String>),
    file: &PlanFile,
    index: usize,
    total_files: usize,
    err: AppError,
) {
    emit_log(
        "download",
        &format!("Skipped optional {} {}: {}", file.name, file.version, err),
        download_slot_percent(index, total_files, 0, None),
        "skipped",
        Some(file.file.clone()),
    );
    book.dropped.insert(file.component_id.clone());
    book.skipped_errors.insert(file.component_id.clone(), err);
    grow_skipped(book.manifest, book.optional, book.dropped);
}

fn expected_download_bytes(reported: Option<u64>, planned: u64) -> Option<u64> {
    if let Some(total) = reported.filter(|n| *n > 0) {
        return Some(total);
    }
    if planned > 0 {
        Some(planned)
    } else {
        None
    }
}

/// Percent inside this file's share of the download phase (5% through 65%).
fn download_slot_percent(index: usize, total_files: usize, done: u64, expected: Option<u64>) -> u8 {
    let total_files = total_files.max(1);
    let slot_lo = 5 + index * 60 / total_files;
    let slot_hi = 5 + (index + 1) * 60 / total_files;
    let Some(expected) = expected.filter(|n| *n > 0) else {
        return slot_lo as u8;
    };
    let span = (slot_hi - slot_lo) as u128;
    let frac = u128::from(done.min(expected)) * span / u128::from(expected);
    (slot_lo as u128 + frac).min(slot_hi as u128) as u8
}

fn download_progress_message(
    name: &str,
    version: &str,
    done: u64,
    expected: u64,
    note: &str,
) -> String {
    let done_mb = done as f64 / (1024.0 * 1024.0);
    let expected_mb = expected as f64 / (1024.0 * 1024.0);
    let what = if version.is_empty() {
        name.to_string()
    } else {
        format!("{name} {version}")
    };
    let base = format!("Downloading {what} — {done_mb:.1}/{expected_mb:.1} MB");
    if note.is_empty() {
        base
    } else {
        format!("{base} — {note}")
    }
}

fn running_download(
    run_id: &str,
    file: Option<String>,
    percent: u8,
    message: String,
) -> ProgressEvent {
    ProgressEvent {
        run_id: run_id.to_string(),
        step: "download".into(),
        file,
        status: "running".into(),
        percent,
        message,
    }
}

fn push_unique_root(roots: &mut Vec<PathBuf>, path: &Path) {
    if !roots.iter().any(|have| have == path) {
        roots.push(path.to_path_buf());
    }
}

fn profile_version_roots(target: &InstallTarget) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for root in &target.launcher_roots {
        push_unique_root(&mut roots, root);
    }
    if let Some(root) = &target.profiles.official {
        push_unique_root(&mut roots, root);
    }
    if let Some(root) = &target.profiles.tlauncher {
        push_unique_root(&mut roots, root);
    }
    if target.profiles.also_pack_dir {
        push_unique_root(&mut roots, &target.pack_dir);
    }
    roots
}

fn verified_loader_bytes<F: Fetcher>(
    url: &str,
    sha512: &str,
    fetcher: &F,
    staging: &Path,
) -> Result<Vec<u8>, AppError> {
    let staged = staging.join("loader.json");
    let bytes = if staged.is_file() {
        fs::read(&staged)?
    } else {
        fetcher.fetch_loader(url)?
    };
    verify_sha512(&bytes, sha512)?;
    Ok(bytes)
}

fn version_for<'a>(assigned: &'a [(PathBuf, String)], root: &Path) -> Result<&'a str, AppError> {
    assigned
        .iter()
        .find(|(path, _)| path == root)
        .map(|(_, id)| id.as_str())
        .ok_or_else(|| {
            AppError::State(format!(
                "no Fabric version is available in {}",
                root.display()
            ))
        })
}

fn install_loader_and_profiles<F, L>(
    target: &InstallTarget,
    plan: &InstallPlan,
    manifest: &Manifest,
    fetcher: &F,
    staging: &Path,
    system_ram_gb: f64,
    emit_log: &L,
) -> Result<(), AppError>
where
    F: Fetcher,
    L: Fn(&str, &str, u8, &str, Option<String>),
{
    let roots = profile_version_roots(target);
    let mut assigned: Vec<(PathBuf, String)> = Vec::new();
    if let Some(loader) = &plan.loader {
        let missing = roots
            .iter()
            .any(|root| !crate::fabric::version_json_present(root, &loader.id));
        if missing {
            let bytes = verified_loader_bytes(&loader.url, &loader.sha512, fetcher, staging)?;
            for root in &roots {
                if !crate::fabric::version_json_present(root, &loader.id) {
                    crate::fabric::write_version_profile(root, &loader.id, &bytes)?;
                }
            }
        }
        for root in &roots {
            assigned.push((root.clone(), loader.id.clone()));
        }
    } else {
        let mut missing = Vec::new();
        for root in &roots {
            if crate::fabric::version_json_present(root, &manifest.loader.id) {
                assigned.push((root.clone(), manifest.loader.id.clone()));
            } else if let Some(existing) =
                crate::fabric::existing_fabric_for(root, &manifest.minecraft)
            {
                assigned.push((root.clone(), existing));
            } else {
                missing.push(root.clone());
            }
        }
        if !missing.is_empty() {
            let bytes = verified_loader_bytes(
                &manifest.loader.url,
                &manifest.loader.sha512,
                fetcher,
                staging,
            )?;
            for root in missing {
                crate::fabric::write_version_profile(&root, &manifest.loader.id, &bytes)?;
                assigned.push((root, manifest.loader.id.clone()));
            }
        }
    }
    let mut spec = target.spec.clone();
    if spec.java_args.is_empty() {
        spec.java_args = crate::fabric::java_args_for_ram(system_ram_gb);
    }
    let backup = installer_dir(&target.pack_dir).join("launcher-backups");
    if let Some(root) = &target.profiles.official {
        emit_log(
            "profile",
            "Writing the launcher profile",
            92,
            "running",
            None,
        );
        let mut profile = spec.clone();
        profile.version_id = version_for(&assigned, root)?.to_string();
        crate::profiles::upsert_official(root, &profile, &backup)?;
    }
    if target.profiles.also_pack_dir {
        let mut profile = spec.clone();
        profile.version_id = version_for(&assigned, &target.pack_dir)?.to_string();
        crate::profiles::upsert_official(&target.pack_dir, &profile, &backup)?;
    }
    if let Some(root) = &target.profiles.tlauncher {
        let mut profile = spec.clone();
        profile.version_id = version_for(&assigned, root)?.to_string();
        if let TlWrite::SkippedUnknownShape =
            crate::profiles::upsert_tlauncher(root, &profile, &backup)?
        {
            emit_log(
                "profile",
                &format!(
                    "Choose version {} and folder {} in TLauncher yourself",
                    profile.version_id,
                    target.pack_dir.display()
                ),
                94,
                "skipped",
                Some(profile.version_id),
            );
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn run_install<F: Fetcher>(
    target: &InstallTarget,
    plan: &InstallPlan,
    manifest: &Manifest,
    fetcher: &F,
    cancel: &AtomicBool,
    replace_collisions: bool,
    system_ram_gb: f64,
    progress: &dyn Fn(ProgressEvent),
) -> Result<String, AppError> {
    let game_dir = target.pack_dir.as_path();
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
    // A kept-aside refusal or an unreadable sidecar returns before any write.
    let quiet_refusal = RefCell::new(false);

    let result = (|| -> Result<String, AppError> {
        cancelled(cancel)?;
        if game_running_with(&live_minecraft_processes()) {
            return Err(AppError::GameRunning);
        }
        if target.require_vanilla {
            let mut roots: Vec<&Path> =
                target.launcher_roots.iter().map(PathBuf::as_path).collect();
            if roots.is_empty() {
                roots.push(game_dir);
            }
            for root in roots {
                if !vanilla_jar_exists(root, &manifest.minecraft) {
                    return Err(AppError::McMissing {
                        version: manifest.minecraft.clone(),
                        path: root.display().to_string(),
                    });
                }
            }
        }

        let existing = load_state(game_dir)?;
        *prior_state.borrow_mut() = existing.clone();
        let kept_aside = match crate::pack_dirs::kept_aside_paths(game_dir) {
            Ok(paths) => paths,
            Err(err) => {
                *quiet_refusal.borrow_mut() = true;
                return Err(err);
            }
        };
        // Refuse a protected file before logs, staging, or downloads so the
        // folder is unchanged. Other collisions stay until after optional skips.
        let preflight = collect_replacements(game_dir, plan, existing.as_ref(), &kept_aside)?;
        if let Err(err) = refuse_unconfirmed_collisions(
            game_dir,
            plan,
            existing.as_ref(),
            &kept_aside,
            &preflight,
            replace_collisions,
        ) {
            let defer = matches!(
                &err,
                AppError::Collision { detail, .. } if !detail.contains("kept aside")
            );
            if !defer {
                *quiet_refusal.borrow_mut() = true;
                return Err(err);
            }
        }

        emit_log("prepare", "Preparing install", 2, "running", None);
        let staging = installer_dir(game_dir).join("staging").join(&run_id);
        fs::create_dir_all(&staging)?;

        let optional = effective_optional(manifest, plan);
        let mut skipped_errors: BTreeMap<String, AppError> = BTreeMap::new();
        let mut dropped: BTreeSet<String> = BTreeSet::new();
        let downloads: Vec<&PlanFile> = plan.add.iter().chain(plan.update.iter()).collect();
        let total = downloads.len().max(1);
        for (index, file) in downloads.iter().enumerate() {
            cancelled(cancel)?;
            if dropped.contains(&file.component_id) {
                continue;
            }
            emit_log(
                "download",
                &format!("Downloading {} {}", file.name, file.version),
                download_slot_percent(index, total, 0, None),
                "running",
                Some(file.file.clone()),
            );
            // Per-chunk updates stay out of the install log. The command sink throttles them.
            let fetched = fetcher.fetch_with_status(
                &file.url,
                FetchKind::Component,
                &mut |done, content_len, note| {
                    let expected = expected_download_bytes(content_len, file.size);
                    progress(running_download(
                        &run_id,
                        Some(file.file.clone()),
                        download_slot_percent(index, total, done, expected),
                        download_progress_message(
                            &file.name,
                            &file.version,
                            done,
                            expected.unwrap_or(0),
                            note,
                        ),
                    ));
                    !cancel.load(Ordering::SeqCst)
                },
            );
            let bytes = match fetched {
                Ok(bytes) => bytes,
                Err(err) if optional_can_skip(&optional, &file.component_id, &err) => {
                    record_optional_skip(
                        &mut SkipBook {
                            manifest,
                            optional: &optional,
                            skipped_errors: &mut skipped_errors,
                            dropped: &mut dropped,
                        },
                        &emit_log,
                        file,
                        index,
                        total,
                        err,
                    );
                    continue;
                }
                Err(err) => return Err(err),
            };
            if let Err(err) = verify_sha512(&bytes, &file.sha512) {
                if optional_can_skip(&optional, &file.component_id, &err) {
                    record_optional_skip(
                        &mut SkipBook {
                            manifest,
                            optional: &optional,
                            skipped_errors: &mut skipped_errors,
                            dropped: &mut dropped,
                        },
                        &emit_log,
                        file,
                        index,
                        total,
                        err,
                    );
                    continue;
                }
                return Err(err);
            }
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
            let bytes = fetcher.fetch_with_status(
                &loader.url,
                FetchKind::Loader,
                &mut |done, content_len, note| {
                    let expected = content_len.filter(|n| *n > 0).unwrap_or(0);
                    progress(running_download(
                        &run_id,
                        Some(loader.id.clone()),
                        66,
                        download_progress_message(
                            "Fabric loader profile",
                            "",
                            done,
                            expected,
                            note,
                        ),
                    ));
                    !cancel.load(Ordering::SeqCst)
                },
            )?;
            verify_sha512(&bytes, &loader.sha512)?;
            fs::write(staging.join("loader.json"), bytes)?;
        }

        let planned_components = plan.components.clone();
        let done_message = if dropped.is_empty() {
            "Install finished".to_string()
        } else {
            skipped_done_message(manifest, &planned_components, &dropped, &skipped_errors)
        };
        let filtered = if dropped.is_empty() {
            None
        } else {
            let mut effective = plan.clone();
            effective
                .add
                .retain(|file| !dropped.contains(&file.component_id));
            effective
                .update
                .retain(|file| !dropped.contains(&file.component_id));
            effective
                .configs
                .retain(|item| !dropped.contains(&item.component_id));
            effective.components.retain(|id| !dropped.contains(id));
            Some(effective)
        };
        let plan = filtered.as_ref().unwrap_or(plan);

        cancelled(cancel)?;
        let replacements = collect_replacements(game_dir, plan, existing.as_ref(), &kept_aside)?;
        refuse_unconfirmed_collisions(
            game_dir,
            plan,
            existing.as_ref(),
            &kept_aside,
            &replacements,
            replace_collisions,
        )?;

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

        if target.legacy_same_folder {
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
        } else {
            emit_log("fabric", "Installing Fabric 26.3", 90, "running", None);
            install_loader_and_profiles(
                target,
                plan,
                manifest,
                fetcher,
                &staging,
                system_ram_gb,
                &emit_log,
            )?;
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
        let mut new_state = build_new_state(manifest, plan, recorded, existing.as_ref(), &dropped);
        record_quality_files(&mut new_state, quality_written, plan.quality);
        save_state(game_dir, &new_state)?;
        let _ = crate::pack_dirs::prune_kept_aside(game_dir, &new_state);
        emit_log("done", &done_message, 100, "done", None);
        let _ = fs::remove_dir_all(&staging);
        Ok(run_id.clone())
    })();

    if !*quiet_refusal.borrow() {
        if let Err(err) = &result {
            if matches!(err, AppError::Cancelled) {
                emit_log("rollback", "Cancelled — restoring", 100, "cancelled", None);
            } else {
                emit_log("error", &err.to_string(), 100, "error", None);
            }
            if let Some(id) = snapshot_id.borrow().clone() {
                let current = load_state(game_dir).ok().flatten().unwrap_or_else(|| {
                    InstalledState::new("installer", &manifest.manifest_version)
                });
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
    }
    result
}

pub fn repair_tracked<F: Fetcher>(
    target: &InstallTarget,
    manifest: &Manifest,
    fetcher: &F,
    cancel: &AtomicBool,
    system_ram_gb: f64,
    progress: &dyn Fn(ProgressEvent),
) -> Result<String, AppError> {
    let game_dir = target.pack_dir.as_path();
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
        optional_components: Vec::new(),
        warnings: Vec::new(),
        bytes: 0,
        pack_id: String::new(),
        pack_dir: String::new(),
    };
    run_install(
        target,
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
pub(crate) mod test_support {
    use std::fs;
    use std::path::Path;

    use super::{record_file, save_state, InstalledState};
    use crate::download::sha512_hex;
    use crate::fetch::MapFetcher;
    use crate::manifest::load_fixture_manifest;
    use crate::plan::Selection;

    pub(crate) fn test_manifest_and_fetcher() -> (crate::manifest::Manifest, MapFetcher, Vec<u8>) {
        let mut manifest = load_fixture_manifest().unwrap();
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

    pub(crate) fn prepare_game(game: &Path) {
        fs::create_dir_all(game.join("versions/26.3")).unwrap();
        fs::write(game.join("versions/26.3/26.3.jar"), b"jar").unwrap();
        fs::create_dir_all(game.join("mods")).unwrap();
    }

    pub(crate) fn basic_selection() -> Selection {
        Selection {
            kit: "basic".into(),
            custom_components: vec![],
            ocean: false,
            addons: Default::default(),
            snow_lite: Some(true),
            quality: None,
        }
    }

    pub(crate) fn legacy_shared_game(game: &Path) -> (Vec<u8>, Vec<u8>) {
        let ba = b"ba-jar-bytes".to_vec();
        let snow = b"ba-plus-snowfall-original".to_vec();
        fs::write(game.join("mods/ba.jar"), &ba).unwrap();
        fs::write(game.join("mods/arnold-snowfall-1.3.3.jar"), &snow).unwrap();
        fs::write(game.join("mods/user.jar"), b"user-jar").unwrap();
        fs::create_dir_all(game.join("saves/world")).unwrap();
        fs::create_dir_all(game.join("screenshots")).unwrap();
        fs::write(game.join("saves/world/level.dat"), b"world").unwrap();
        fs::write(game.join("screenshots/shot.png"), b"shot").unwrap();
        fs::write(game.join("options.txt"), b"options").unwrap();
        fs::write(game.join("servers.dat"), b"servers").unwrap();
        let mut state = InstalledState::new("installer", "ba-plus-1");
        record_file(&mut state, "mods/ba.jar", sha512_hex(&ba), "ba-plus");
        record_file(
            &mut state,
            "mods/arnold-snowfall-1.3.3.jar",
            sha512_hex(&snow),
            "snow",
        );
        save_state(game, &state).unwrap();
        fs::write(
            game.join("launcher_profiles.json"),
            serde_json::json!({
                "profiles": {
                    "vanilla": {"name": "Latest"},
                    crate::profiles::ARNOLD_PROFILE_KEY: {"name": "Arnold", "type": "custom"}
                }
            })
            .to_string(),
        )
        .unwrap();
        (ba, snow)
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{basic_selection, prepare_game, test_manifest_and_fetcher};
    use super::*;
    use crate::download::sha512_hex;
    use crate::fetch::MapFetcher;
    use crate::plan::{build_plan, PlanFile, Selection};
    use std::sync::{Arc, Mutex};
    use tempfile::tempdir;

    fn basic_plan(manifest: &crate::manifest::Manifest) -> InstallPlan {
        build_plan(manifest, &basic_selection(), None, true).unwrap()
    }

    #[test]
    fn kept_aside_legacy_name_asks_instead_of_replacing() {
        let tmp = tempdir().unwrap();
        let game = tmp.path();
        prepare_game(game);
        fs::write(game.join("mods/arnold-snowfall-1.3.2.jar"), b"players-own").unwrap();
        let (manifest, _, _) = test_manifest_and_fetcher();
        let plan = basic_plan(&manifest);
        let rel = "mods/arnold-snowfall-1.3.2.jar";
        let plain = collect_replacements(game, &plan, None, &[]).unwrap();
        let found = plain.iter().find(|item| item.rel == rel).unwrap();
        assert_eq!(found.action, ReplaceAction::Replace);
        let protected = collect_replacements(game, &plan, None, &[rel.to_string()]).unwrap();
        let kept = protected.iter().find(|item| item.rel == rel).unwrap();
        assert_eq!(kept.action, ReplaceAction::Ask);
        assert!(kept.kept_aside);
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
            &InstallTarget::same_folder(game),
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
            &InstallTarget::same_folder(game),
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
            &InstallTarget::same_folder(game),
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
            &InstallTarget::same_folder(game),
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
            &InstallTarget::same_folder(game),
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
            &InstallTarget::same_folder(game),
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
            &InstallTarget::same_folder(game),
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
            &InstallTarget::same_folder(game),
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
            &InstallTarget::same_folder(game),
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
            &InstallTarget::same_folder(game),
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

    fn sample_plan_file(dest: crate::manifest::FileDest, file: &str) -> PlanFile {
        PlanFile {
            component_id: "sample".into(),
            name: "Sample".into(),
            version: "1".into(),
            file: file.into(),
            dest,
            url: "https://cdn.modrinth.com/sample".into(),
            sha512: "ab".into(),
            size: 1,
            match_glob: file.into(),
            legacy_match: Vec::new(),
            known_sha512: Vec::new(),
        }
    }

    #[test]
    fn dest_rel_maps_every_dest() {
        use crate::manifest::FileDest;
        assert_eq!(dest_rel(&sample_plan_file(FileDest::Mods, "x")), "mods/x");
        assert_eq!(
            dest_rel(&sample_plan_file(FileDest::Shaderpacks, "x.zip")),
            "shaderpacks/x.zip"
        );
        assert_eq!(
            dest_rel(&sample_plan_file(FileDest::Resourcepacks, "x.zip")),
            "resourcepacks/x.zip"
        );
    }

    fn custom_pack_plan(manifest: &crate::manifest::Manifest, ids: &[&str]) -> InstallPlan {
        build_plan(
            manifest,
            &Selection {
                kit: "custom".into(),
                custom_components: ids.iter().map(|id| (*id).to_string()).collect(),
                ocean: false,
                addons: Default::default(),
                snow_lite: None,
                quality: None,
            },
            None,
            true,
        )
        .unwrap()
    }

    #[test]
    fn resource_pack_installs_under_resourcepacks() {
        let tmp = tempdir().unwrap();
        let game = tmp.path();
        prepare_game(game);
        let (manifest, fetcher, _) = test_manifest_and_fetcher();
        let plan = custom_pack_plan(&manifest, &["freshanimations"]);
        run_install(
            &InstallTarget::same_folder(game),
            &plan,
            &manifest,
            &fetcher,
            &AtomicBool::new(false),
            false,
            16.0,
            &|_| {},
        )
        .unwrap();
        let pack = game.join("resourcepacks/FreshAnimations_v1.10.5.zip");
        assert!(pack.is_file());
        assert_eq!(
            fs::read(&pack).unwrap(),
            b"blob-FreshAnimations_v1.10.5.zip"
        );
        assert!(!game.join("mods/FreshAnimations_v1.10.5.zip").exists());
        assert!(!game
            .join("shaderpacks/FreshAnimations_v1.10.5.zip")
            .exists());
        let state = load_state(game).unwrap().unwrap();
        assert!(state.files.iter().any(|file| {
            file.path == "resourcepacks/FreshAnimations_v1.10.5.zip"
                && file.component_id == "freshanimations"
        }));
        for id in ["emf", "etf"] {
            let name = manifest
                .components
                .iter()
                .find(|component| component.id == id)
                .unwrap()
                .files[0]
                .file
                .clone();
            assert!(game.join("mods").join(name).is_file(), "{id}");
        }
    }

    #[test]
    fn old_resource_pack_copy_is_found_in_resourcepacks_only() {
        let tmp = tempdir().unwrap();
        let game = tmp.path();
        prepare_game(game);
        fs::create_dir_all(game.join("resourcepacks")).unwrap();
        fs::write(
            game.join("resourcepacks/FreshAnimations_v1.10.4.zip"),
            b"old-pack",
        )
        .unwrap();
        fs::write(game.join("mods/FreshAnimations_v1.10.4.zip"), b"decoy").unwrap();
        let (manifest, _, _) = test_manifest_and_fetcher();
        let plan = custom_pack_plan(&manifest, &["freshanimations"]);
        let found = collect_replacements(game, &plan, None, &[]).unwrap();
        let asks: Vec<&PendingReplace> = found
            .iter()
            .filter(|item| item.action == ReplaceAction::Ask)
            .collect();
        assert_eq!(
            asks.len(),
            1,
            "{:?}",
            found.iter().map(|item| &item.rel).collect::<Vec<_>>()
        );
        assert_eq!(asks[0].rel, "resourcepacks/FreshAnimations_v1.10.4.zip");
        assert!(found
            .iter()
            .all(|item| item.rel != "mods/FreshAnimations_v1.10.4.zip"));
    }

    #[test]
    fn download_slot_percent_interpolates_inside_the_file() {
        assert_eq!(download_slot_percent(0, 2, 0, None), 5);
        assert_eq!(download_slot_percent(0, 2, 0, Some(100)), 5);
        assert_eq!(download_slot_percent(0, 2, 50, Some(100)), 20);
        assert_eq!(download_slot_percent(0, 2, 100, Some(100)), 35);
        assert_eq!(download_slot_percent(1, 2, 0, Some(100)), 35);
        assert_eq!(download_slot_percent(1, 2, 100, Some(100)), 65);
        assert_eq!(download_slot_percent(0, 2, 1_000, Some(100)), 35);
        assert_eq!(download_slot_percent(0, 2, 50, Some(0)), 5);
    }

    struct CancelOnFetch {
        inner: MapFetcher,
        cancel: Arc<AtomicBool>,
    }

    impl Fetcher for CancelOnFetch {
        fn fetch_kind(&self, url: &str, kind: FetchKind) -> Result<Vec<u8>, AppError> {
            self.inner.fetch_kind(url, kind)
        }

        fn fetch_with_status(
            &self,
            url: &str,
            kind: FetchKind,
            on_bytes: &mut dyn FnMut(u64, Option<u64>, &str) -> bool,
        ) -> Result<Vec<u8>, AppError> {
            self.cancel.store(true, Ordering::SeqCst);
            self.inner.fetch_with_status(url, kind, on_bytes)
        }
    }

    struct ChunkedFetcher {
        inner: MapFetcher,
    }

    impl Fetcher for ChunkedFetcher {
        fn fetch_kind(&self, url: &str, kind: FetchKind) -> Result<Vec<u8>, AppError> {
            self.inner.fetch_kind(url, kind)
        }

        fn fetch_with_status(
            &self,
            url: &str,
            kind: FetchKind,
            on_bytes: &mut dyn FnMut(u64, Option<u64>, &str) -> bool,
        ) -> Result<Vec<u8>, AppError> {
            let bytes = self.fetch_kind(url, kind)?;
            let len = bytes.len() as u64;
            let steps = 4_u64;
            for step in 1..=steps {
                let done = (u128::from(len) * u128::from(step) / u128::from(steps)) as u64;
                if !on_bytes(done, Some(len), "") {
                    return Err(AppError::Cancelled);
                }
            }
            Ok(bytes)
        }
    }

    #[test]
    fn cancel_during_download_rolls_back() {
        let tmp = tempdir().unwrap();
        let game = tmp.path();
        prepare_game(game);
        let (manifest, fetcher, _) = test_manifest_and_fetcher();
        let plan = basic_plan(&manifest);
        let cancel = Arc::new(AtomicBool::new(false));
        let wrapped = CancelOnFetch {
            inner: fetcher,
            cancel: Arc::clone(&cancel),
        };
        let events = Mutex::new(Vec::new());
        let err = run_install(
            &InstallTarget::same_folder(game),
            &plan,
            &manifest,
            &wrapped,
            cancel.as_ref(),
            false,
            16.0,
            &|event| events.lock().unwrap().push(event),
        )
        .unwrap_err();
        assert_eq!(err.code(), "E-CANCELLED");
        let logged = events.lock().unwrap();
        assert_eq!(logged.last().unwrap().status, "cancelled");
        assert!(fs::read_dir(game.join("mods")).unwrap().next().is_none());
    }

    #[test]
    fn download_progress_events_stay_in_file_slot() {
        let tmp = tempdir().unwrap();
        let game = tmp.path();
        prepare_game(game);
        let (manifest, fetcher, _) = test_manifest_and_fetcher();
        let mut plan = basic_plan(&manifest);
        assert!(
            plan.add.len() >= 2,
            "basic kit should download at least two files"
        );
        plan.add.truncate(2);
        plan.update.clear();
        plan.loader = None;
        let wrapped = ChunkedFetcher { inner: fetcher };
        let events = Mutex::new(Vec::new());
        run_install(
            &InstallTarget::same_folder(game),
            &plan,
            &manifest,
            &wrapped,
            &AtomicBool::new(false),
            false,
            16.0,
            &|event| events.lock().unwrap().push(event),
        )
        .unwrap();
        let logged = events.lock().unwrap();
        let downloads: Vec<_> = logged
            .iter()
            .filter(|event| event.step == "download")
            .collect();
        assert!(!downloads.is_empty());
        let mut last_percent = 0_u8;
        for event in &downloads {
            assert!(
                (5..=65).contains(&event.percent),
                "download percent {} is outside 5..=65 ({})",
                event.percent,
                event.message
            );
            assert!(
                event.percent >= last_percent,
                "download percent went from {last_percent} to {}",
                event.percent
            );
            last_percent = event.percent;
        }
        assert_eq!(downloads.last().unwrap().percent, 65);
        let finished = logged.last().unwrap();
        assert_eq!(finished.status, "done");
        assert_eq!(finished.percent, 100);
    }

    fn kit_selection(kit: &str, addon: Option<&str>) -> Selection {
        let mut addons = BTreeMap::new();
        if let Some(addon) = addon {
            addons.insert(addon.to_string(), true);
        }
        Selection {
            kit: kit.into(),
            custom_components: vec![],
            ocean: false,
            addons,
            snow_lite: None,
            quality: None,
        }
    }

    fn kit_plan(
        manifest: &crate::manifest::Manifest,
        kit: &str,
        addon: Option<&str>,
    ) -> InstallPlan {
        build_plan(manifest, &kit_selection(kit, addon), None, true).unwrap()
    }

    fn planned_name<'a>(plan: &'a InstallPlan, id: &str) -> &'a str {
        plan.add
            .iter()
            .chain(plan.update.iter())
            .find(|file| file.component_id == id)
            .unwrap_or_else(|| panic!("plan is missing {id}"))
            .file
            .as_str()
    }

    fn install_events(
        game: &Path,
        plan: &InstallPlan,
        manifest: &crate::manifest::Manifest,
        fetcher: &MapFetcher,
    ) -> (Result<String, AppError>, Vec<ProgressEvent>) {
        let events = Mutex::new(Vec::new());
        let result = run_install(
            &InstallTarget::same_folder(game),
            plan,
            manifest,
            fetcher,
            &AtomicBool::new(false),
            false,
            16.0,
            &|event| events.lock().unwrap().push(event),
        );
        (result, events.into_inner().unwrap())
    }

    #[test]
    fn optional_addon_failure_installs_the_rest() {
        let tmp = tempdir().unwrap();
        let game = tmp.path();
        prepare_game(game);
        let (manifest, mut fetcher, _) = test_manifest_and_fetcher();
        let plan = kit_plan(&manifest, "recommended", Some("architect"));
        let architect = planned_name(&plan, "architect").to_string();
        let url = plan
            .add
            .iter()
            .find(|file| file.component_id == "architect")
            .unwrap()
            .url
            .clone();
        fetcher.fail.insert(url);
        let (result, events) = install_events(game, &plan, &manifest, &fetcher);
        assert!(result.is_ok(), "{result:?}");
        assert!(!game.join("mods").join(&architect).exists());
        assert!(game.join("mods/arnold-snowfall-1.3.3.jar").is_file());
        let state = load_state(game).unwrap().unwrap();
        assert!(state
            .components
            .iter()
            .all(|component| component.id != "architect"));
        assert!(state
            .components
            .iter()
            .any(|component| component.id == "base"));
        assert!(events.iter().any(|event| event.status == "skipped"));
        let done = events.last().unwrap();
        assert_eq!(done.status, "done");
        assert!(done.message.contains("Architect"), "{}", done.message);
        assert!(done.message.contains("download failed"), "{}", done.message);
        assert!(
            done.message.contains("Everything else is installed"),
            "{}",
            done.message
        );
    }

    #[test]
    fn optional_dependency_failure_drops_dependents() {
        let tmp = tempdir().unwrap();
        let game = tmp.path();
        prepare_game(game);
        let (manifest, mut fetcher, _) = test_manifest_and_fetcher();
        let plan = kit_plan(&manifest, "recommended", Some("architect"));
        let malilib = planned_name(&plan, "malilib").to_string();
        let litematica = planned_name(&plan, "litematica").to_string();
        let architect = planned_name(&plan, "architect").to_string();
        let url = plan
            .add
            .iter()
            .find(|file| file.component_id == "malilib")
            .unwrap()
            .url
            .clone();
        fetcher.fail.insert(url);
        let (result, _) = install_events(game, &plan, &manifest, &fetcher);
        assert!(result.is_ok(), "{result:?}");
        for name in [&malilib, &litematica, &architect] {
            assert!(!game.join("mods").join(name).exists(), "{name}");
        }
        assert!(game.join("mods/arnold-snowfall-1.3.3.jar").is_file());
        let state = load_state(game).unwrap().unwrap();
        for id in ["malilib", "litematica", "architect"] {
            assert!(state.components.iter().all(|component| component.id != id));
        }
    }

    #[test]
    fn required_flashback_failure_aborts_recommended() {
        let tmp = tempdir().unwrap();
        let game = tmp.path();
        prepare_game(game);
        let (manifest, mut fetcher, _) = test_manifest_and_fetcher();
        let plan = kit_plan(&manifest, "recommended", None);
        let url = plan
            .add
            .iter()
            .find(|file| file.component_id == "flashback")
            .unwrap()
            .url
            .clone();
        fetcher.fail.insert(url);
        let err = run_install(
            &InstallTarget::same_folder(game),
            &plan,
            &manifest,
            &fetcher,
            &AtomicBool::new(false),
            false,
            16.0,
            &|_| {},
        )
        .unwrap_err();
        assert_eq!(err.code(), "E-IO");
        assert!(fs::read_dir(game.join("mods")).unwrap().next().is_none());
        assert!(load_state(game).unwrap().is_none());
    }

    #[test]
    fn tampered_plan_cannot_make_flashback_optional() {
        let tmp = tempdir().unwrap();
        let game = tmp.path();
        prepare_game(game);
        let (manifest, mut fetcher, _) = test_manifest_and_fetcher();
        let mut plan = kit_plan(&manifest, "recommended", None);
        plan.optional_components = vec!["flashback".into()];
        let url = plan
            .add
            .iter()
            .find(|file| file.component_id == "flashback")
            .unwrap()
            .url
            .clone();
        fetcher.fail.insert(url);
        let err = run_install(
            &InstallTarget::same_folder(game),
            &plan,
            &manifest,
            &fetcher,
            &AtomicBool::new(false),
            false,
            16.0,
            &|_| {},
        )
        .unwrap_err();
        assert_eq!(err.code(), "E-IO");
        assert!(load_state(game).unwrap().is_none());
        assert!(!game
            .join("mods")
            .join(planned_name(&plan, "flashback"))
            .exists());
    }

    #[test]
    fn repair_is_strict() {
        let tmp = tempdir().unwrap();
        let game = tmp.path();
        prepare_game(game);
        let (manifest, mut fetcher, _) = test_manifest_and_fetcher();
        let plan = kit_plan(&manifest, "basic", Some("architect"));
        run_install(
            &InstallTarget::same_folder(game),
            &plan,
            &manifest,
            &fetcher,
            &AtomicBool::new(false),
            false,
            16.0,
            &|_| {},
        )
        .unwrap();
        let architect = planned_name(&plan, "architect");
        let architect_path = game.join("mods").join(architect);
        fs::write(&architect_path, b"broken").unwrap();
        let url = plan
            .add
            .iter()
            .find(|file| file.component_id == "architect")
            .unwrap()
            .url
            .clone();
        fetcher.fail.insert(url);
        let err = repair_tracked(
            &InstallTarget::same_folder(game),
            &manifest,
            &fetcher,
            &AtomicBool::new(false),
            16.0,
            &|_| {},
        )
        .unwrap_err();
        assert_eq!(err.code(), "E-IO");
        assert_eq!(fs::read(&architect_path).unwrap(), b"broken");
        assert!(game.join("mods/arnold-snowfall-1.3.3.jar").is_file());
        let state = load_state(game).unwrap().unwrap();
        assert!(state
            .components
            .iter()
            .any(|component| component.id == "architect"));
    }

    #[test]
    fn optional_update_failure_keeps_prior_component() {
        let tmp = tempdir().unwrap();
        let game = tmp.path();
        prepare_game(game);
        let (manifest, mut fetcher, _) = test_manifest_and_fetcher();
        let plan = kit_plan(&manifest, "basic", Some("architect"));
        run_install(
            &InstallTarget::same_folder(game),
            &plan,
            &manifest,
            &fetcher,
            &AtomicBool::new(false),
            false,
            16.0,
            &|_| {},
        )
        .unwrap();
        let installed = load_state(game).unwrap().unwrap();
        let prior = installed
            .components
            .iter()
            .find(|component| component.id == "architect")
            .unwrap()
            .clone();
        let mut again = build_plan(
            &manifest,
            &kit_selection("basic", Some("architect")),
            Some(&installed),
            true,
        )
        .unwrap();
        let mut update = again
            .add
            .iter()
            .chain(again.update.iter())
            .find(|file| file.component_id == "architect")
            .cloned()
            .unwrap_or_else(|| {
                plan.add
                    .iter()
                    .find(|file| file.component_id == "architect")
                    .unwrap()
                    .clone()
            });
        update.sha512 = sha512_hex(b"newer-architect");
        fetcher.fail.insert(update.url.clone());
        again.update.push(update);
        let (result, _) = install_events(game, &again, &manifest, &fetcher);
        assert!(result.is_ok(), "{result:?}");
        let architect = planned_name(&plan, "architect");
        assert_eq!(
            fs::read(game.join("mods").join(architect)).unwrap(),
            format!("blob-{architect}").into_bytes()
        );
        let state = load_state(game).unwrap().unwrap();
        let kept = state
            .components
            .iter()
            .find(|component| component.id == "architect")
            .expect("prior architect component");
        assert_eq!(kept.version, prior.version);
        assert_eq!(kept.name, prior.name);
    }

    fn split_target(pack: &Path, root: &Path, require_vanilla: bool) -> InstallTarget {
        InstallTarget {
            pack_dir: pack.to_path_buf(),
            launcher_roots: vec![root.to_path_buf()],
            profiles: ProfileTargets {
                official: Some(root.to_path_buf()),
                tlauncher: None,
                also_pack_dir: false,
            },
            spec: ProfileSpec {
                key: "arnold-pack-better-adventures-plus".into(),
                name: "Arnold — Better Adventures+".into(),
                version_id: String::new(),
                game_dir: pack.to_path_buf(),
                java_args: String::new(),
                update_legacy_in_place: false,
            },
            require_vanilla,
            legacy_same_folder: false,
        }
    }

    #[test]
    fn pack_files_land_in_the_pack_folder_and_the_jar_is_checked_on_the_launcher() {
        let tmp = tempdir().unwrap();
        let pack = tmp.path().join("pack");
        let root = tmp.path().join("minecraft");
        fs::create_dir_all(&pack).unwrap();
        prepare_game(&root);
        let (manifest, fetcher, _) = test_manifest_and_fetcher();
        let plan = basic_plan(&manifest);
        run_install(
            &split_target(&pack, &root, true),
            &plan,
            &manifest,
            &fetcher,
            &AtomicBool::new(false),
            false,
            16.0,
            &|_| {},
        )
        .unwrap();
        assert!(pack.join("mods").read_dir().unwrap().next().is_some());
        assert!(
            !root.join("mods").exists() || root.join("mods").read_dir().unwrap().next().is_none()
        );
        let profiles: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join("launcher_profiles.json")).unwrap())
                .unwrap();
        let entry = &profiles["profiles"]["arnold-pack-better-adventures-plus"];
        assert_eq!(entry["name"], "Arnold — Better Adventures+");
        assert_eq!(entry["gameDir"], pack.display().to_string());
    }

    #[test]
    fn missing_vanilla_jar_is_reported_on_the_launcher_root() {
        let tmp = tempdir().unwrap();
        let pack = tmp.path().join("pack");
        let root = tmp.path().join("minecraft");
        prepare_game(&pack);
        fs::create_dir_all(&root).unwrap();
        let (manifest, fetcher, _) = test_manifest_and_fetcher();
        let err = run_install(
            &split_target(&pack, &root, true),
            &basic_plan(&manifest),
            &manifest,
            &fetcher,
            &AtomicBool::new(false),
            false,
            16.0,
            &|_| {},
        )
        .unwrap_err();
        assert_eq!(err.code(), "E-MC-MISSING");
        match err {
            AppError::McMissing { path, .. } => {
                assert!(path.contains("minecraft"), "{path}");
            }
            other => panic!("expected a missing jar, got {other:?}"),
        }
    }

    #[test]
    fn no_launcher_skips_the_jar_check_and_writes_a_profile_beside_the_pack() {
        let tmp = tempdir().unwrap();
        let pack = tmp.path().join("pack");
        let root = tmp.path().join("official");
        fs::create_dir_all(&pack).unwrap();
        fs::create_dir_all(&root).unwrap();
        let (manifest, fetcher, _) = test_manifest_and_fetcher();
        let mut target = split_target(&pack, &root, false);
        target.profiles.also_pack_dir = true;
        target.launcher_roots = vec![root.clone(), pack.clone()];
        run_install(
            &target,
            &basic_plan(&manifest),
            &manifest,
            &fetcher,
            &AtomicBool::new(false),
            false,
            16.0,
            &|_| {},
        )
        .unwrap();
        assert!(root.join("launcher_profiles.json").is_file());
        assert!(pack.join("launcher_profiles.json").is_file());
    }

    #[test]
    fn unknown_tlauncher_profile_shape_does_not_fail_the_install() {
        let tmp = tempdir().unwrap();
        let pack = tmp.path().join("pack");
        let root = tmp.path().join("minecraft");
        fs::create_dir_all(&pack).unwrap();
        prepare_game(&root);
        let original = b"[1,2,3]".to_vec();
        fs::write(root.join("TlauncherProfiles.json"), &original).unwrap();
        let (manifest, fetcher, _) = test_manifest_and_fetcher();
        let mut target = split_target(&pack, &root, true);
        target.profiles.tlauncher = Some(root.clone());
        let events = Mutex::new(Vec::new());
        run_install(
            &target,
            &basic_plan(&manifest),
            &manifest,
            &fetcher,
            &AtomicBool::new(false),
            false,
            16.0,
            &|event| events.lock().unwrap().push(event),
        )
        .unwrap();
        assert_eq!(
            fs::read(root.join("TlauncherProfiles.json")).unwrap(),
            original
        );
        let skipped = events.lock().unwrap().iter().any(|event| {
            event.step == "profile"
                && event.status == "skipped"
                && event.message.contains("TLauncher")
        });
        assert!(skipped);
    }

    fn write_fabric(root: &Path, id: &str) {
        let dir = root.join("versions").join(id);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("{id}.json")), b"{}").unwrap();
    }

    fn profile_version(root: &Path, file: &str) -> String {
        let text = fs::read_to_string(root.join(file)).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        value["profiles"]["arnold-pack-better-adventures-plus"]["lastVersionId"]
            .as_str()
            .unwrap()
            .to_string()
    }

    fn loader_plan(manifest: &crate::manifest::Manifest) -> InstallPlan {
        build_plan(
            manifest,
            &Selection {
                kit: "basic".into(),
                custom_components: vec![],
                ocean: false,
                addons: Default::default(),
                snow_lite: Some(true),
                quality: None,
            },
            None,
            false,
        )
        .unwrap()
    }

    #[test]
    fn vanilla_check_uses_every_selected_root() {
        let tmp = tempdir().unwrap();
        let pack = tmp.path().join("pack");
        let official = tmp.path().join("official");
        let custom = tmp.path().join("custom");
        fs::create_dir_all(&pack).unwrap();
        fs::create_dir_all(&official).unwrap();
        prepare_game(&custom);
        let (manifest, fetcher, _) = test_manifest_and_fetcher();
        let mut target = split_target(&pack, &custom, true);
        target.launcher_roots = vec![custom.clone(), official.clone()];
        target.profiles.official = None;
        target.profiles.tlauncher = Some(custom.clone());
        fs::write(custom.join("TlauncherProfiles.json"), br#"{"profiles":{}}"#).unwrap();
        let err = run_install(
            &target,
            &basic_plan(&manifest),
            &manifest,
            &fetcher,
            &AtomicBool::new(false),
            false,
            16.0,
            &|_| {},
        )
        .unwrap_err();
        assert_eq!(err.code(), "E-MC-MISSING");
        match err {
            AppError::McMissing { path, .. } => assert!(path.contains("official"), "{path}"),
            other => panic!("expected a missing jar, got {other:?}"),
        }

        target.launcher_roots = vec![custom.clone()];
        run_install(
            &target,
            &basic_plan(&manifest),
            &manifest,
            &fetcher,
            &AtomicBool::new(false),
            false,
            16.0,
            &|_| {},
        )
        .unwrap();
        assert!(!official.join("launcher_profiles.json").exists());
        assert!(!official.join("versions").exists());
        assert!(custom.join("TlauncherProfiles.json").is_file());
    }

    #[test]
    fn requested_loader_is_installed_in_every_selected_root() {
        let tmp = tempdir().unwrap();
        let pack = tmp.path().join("pack");
        let official = tmp.path().join("official");
        let custom = tmp.path().join("custom");
        fs::create_dir_all(&pack).unwrap();
        prepare_game(&official);
        prepare_game(&custom);
        let older = "fabric-loader-0.16.0-26.3";
        write_fabric(&official, older);
        fs::write(custom.join("TlauncherProfiles.json"), br#"{"profiles":{}}"#).unwrap();
        let (manifest, fetcher, _) = test_manifest_and_fetcher();
        let mut target = split_target(&pack, &official, true);
        target.launcher_roots = vec![official.clone(), custom.clone()];
        target.profiles.official = Some(official.clone());
        target.profiles.tlauncher = Some(custom.clone());
        let plan = loader_plan(&manifest);
        let loader_id = plan.loader.as_ref().unwrap().id.clone();
        run_install(
            &target,
            &plan,
            &manifest,
            &fetcher,
            &AtomicBool::new(false),
            false,
            16.0,
            &|_| {},
        )
        .unwrap();
        assert!(official
            .join("versions")
            .join(&loader_id)
            .join(format!("{loader_id}.json"))
            .is_file());
        assert!(custom
            .join("versions")
            .join(&loader_id)
            .join(format!("{loader_id}.json"))
            .is_file());
        assert_eq!(
            profile_version(&official, "launcher_profiles.json"),
            loader_id
        );
        assert_eq!(
            profile_version(&custom, "TlauncherProfiles.json"),
            loader_id
        );
        assert_ne!(loader_id, older);
    }

    #[test]
    fn existing_loader_ids_stay_with_their_own_root() {
        let tmp = tempdir().unwrap();
        let pack = tmp.path().join("pack");
        let official = tmp.path().join("official");
        let custom = tmp.path().join("custom");
        fs::create_dir_all(&pack).unwrap();
        prepare_game(&official);
        prepare_game(&custom);
        let older = "fabric-loader-0.16.0-26.3";
        let newer = "fabric-loader-0.18.0-26.3";
        write_fabric(&official, older);
        write_fabric(&custom, newer);
        fs::write(custom.join("TlauncherProfiles.json"), br#"{"profiles":{}}"#).unwrap();
        let (manifest, fetcher, _) = test_manifest_and_fetcher();
        let mut target = split_target(&pack, &official, true);
        target.launcher_roots = vec![official.clone(), custom.clone()];
        target.profiles.official = Some(official.clone());
        target.profiles.tlauncher = Some(custom.clone());
        let mut plan = basic_plan(&manifest);
        plan.loader = None;
        run_install(
            &target,
            &plan,
            &manifest,
            &fetcher,
            &AtomicBool::new(false),
            false,
            16.0,
            &|_| {},
        )
        .unwrap();
        assert_eq!(profile_version(&official, "launcher_profiles.json"), older);
        assert_eq!(profile_version(&custom, "TlauncherProfiles.json"), newer);
        assert!(!official
            .join("versions")
            .join(newer)
            .join(format!("{newer}.json"))
            .is_file());
        assert!(!custom
            .join("versions")
            .join(older)
            .join(format!("{older}.json"))
            .is_file());
        let manifest_id = &manifest.loader.id;
        assert!(!official
            .join("versions")
            .join(manifest_id)
            .join(format!("{manifest_id}.json"))
            .is_file());
    }
}
