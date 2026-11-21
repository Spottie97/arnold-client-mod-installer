use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::backup::{
    create_snapshot, list_snapshots as list_snapshots_on_disk, rollback as rollback_on_disk,
    uninstall_tracked, SnapshotMeta, UninstallResult,
};
use crate::config::{
    apply_properties, format_setting_value, update_property_keys, ConfigSwitch, LITE_MARKER,
    MANAGED_HEADER,
};
use crate::detect::{default_candidates, detect_at, live_minecraft_processes, Detection};
use crate::errors::{AppError, ErrorPayload};
use crate::fetch::UreqFetcher;
use crate::import::{adopt_import, detect_import, ImportOffer};
use crate::install::{repair_tracked, run_install, InstallTarget, ProgressEvent};
use crate::launcher::{
    choose_play_target, detect_launchers as probe_launchers, profile_destinations, LauncherPrefs,
};
use crate::local_source::{LocalAwareFetcher, LOCAL_SOURCE_BANNER};
use crate::log::{build_summary, diagnostics_text, export_install_log, local_export_stamp};
use crate::manifest::{
    active_manifest_public_key, component_by_id, default_cache_dir, lite_preset,
    load_bundled_manifest, Manifest, ManifestLoad, SNOW_LITE_PRESET,
};
use crate::pack_dirs::{self, CopyItem, ResolvedDir};
use crate::packs::{self, BA_PLUS_PACK_ID};
use crate::plan::{build_plan, InstallPlan, Selection};
use crate::prefs::{load_prefs, save_prefs, Prefs};
use crate::progress::{ProgressThrottle, PROGRESS_MIN_GAP};
use crate::quality::{apply_quality_change, QualityLevel};
use crate::scan::{probe_hardware, recommend, Recommendation};
use crate::state::{load_state, InstalledState};
use crate::updater::{check_update_sync, UpdateStatus};

pub struct AppState {
    pub game_dir: Mutex<Option<PathBuf>>,
    pub cancel_flag: Mutex<Arc<AtomicBool>>,
    /// True while `install` or `repair` is on the worker. A second call is rejected
    /// so it cannot replace the running job's cancel flag.
    pub install_running: AtomicBool,
    pub manifest: Mutex<Option<ManifestLoad>>,
    pub packs: Mutex<Option<packs::PackIndexLoad>>,
    pub selected_pack: Mutex<String>,
    pub local_source: Mutex<Option<PathBuf>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

impl AppState {
    pub fn new() -> Self {
        Self::with_local_source(None)
    }

    pub fn with_local_source(local_source: Option<PathBuf>) -> Self {
        let prefs = load_prefs().unwrap_or_default();
        let selected = prefs
            .selected_pack
            .clone()
            .unwrap_or_else(|| BA_PLUS_PACK_ID.to_string());
        Self {
            game_dir: Mutex::new(prefs.last_game_dir),
            cancel_flag: Mutex::new(Arc::new(AtomicBool::new(false))),
            install_running: AtomicBool::new(false),
            manifest: Mutex::new(None),
            packs: Mutex::new(None),
            selected_pack: Mutex::new(selected),
            local_source: Mutex::new(local_source),
        }
    }
}

fn lock_err() -> AppError {
    AppError::State("lock".into())
}

fn selected_pack_id(state: &AppState) -> String {
    state
        .selected_pack
        .lock()
        .map(|guard| guard.clone())
        .unwrap_or_else(|_| BA_PLUS_PACK_ID.to_string())
}

fn launcher_root_of(state: &AppState) -> PathBuf {
    state
        .game_dir
        .lock()
        .ok()
        .and_then(|guard| guard.clone())
        .or_else(|| default_candidates().into_iter().next())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn resolved_of(state: &AppState) -> Result<ResolvedDir, AppError> {
    let prefs = load_prefs().unwrap_or_default();
    let id = selected_pack_id(state);
    let root = launcher_root_of(state);
    Ok(pack_dirs::resolve_pack_dir(
        &prefs,
        &id,
        &root,
        &default_candidates(),
    ))
}

/// Install, repair and other writes refuse until the player picks keep or fresh.
fn pack_dir_of(state: &AppState) -> Result<PathBuf, AppError> {
    let resolved = resolved_of(state)?;
    if let Some(choice) = &resolved.needs_choice {
        return Err(packs::folder_choice_error(
            choice.root.display().to_string(),
        ));
    }
    Ok(resolved.path)
}

/// Reads may look at an existing install so the choice panel can describe it.
/// They do not create or move anything.
fn game_dir_of(state: &AppState) -> Result<PathBuf, AppError> {
    let resolved = resolved_of(state)?;
    if let Some(choice) = resolved.needs_choice {
        return Ok(choice.root);
    }
    Ok(resolved.path)
}

fn local_source_dir(state: &AppState) -> Option<PathBuf> {
    state.local_source.lock().ok().and_then(|g| g.clone())
}

fn make_fetcher(state: &AppState) -> LocalAwareFetcher<UreqFetcher> {
    let root = if crate::local_source::is_enabled() {
        local_source_dir(state)
    } else {
        None
    };
    LocalAwareFetcher::new(UreqFetcher::new(), root)
}

fn require_local_folder(state: &AppState) -> Result<(), AppError> {
    if crate::local_source::is_enabled() && local_source_dir(state).is_none() {
        return Err(AppError::State(
            "TEST BUILD: choose the folder that has manifest.json".into(),
        ));
    }
    Ok(())
}

fn pending_local_load() -> Result<ManifestLoad, AppError> {
    let manifest = load_bundled_manifest()?;
    Ok(ManifestLoad {
        manifest,
        source: "bundled".into(),
        offline: false,
        message: Some(format!(
            "{LOCAL_SOURCE_BANNER}. Choose the folder that has manifest.json."
        )),
        local_source: true,
        local_source_dir: None,
    })
}

fn remember_index(state: &AppState, loaded: packs::PackIndexLoad) {
    if let Ok(mut guard) = state.packs.lock() {
        *guard = Some(loaded);
    }
}

fn clear_manifest_cache(state: &AppState) {
    if let Ok(mut guard) = state.manifest.lock() {
        *guard = None;
    }
}

fn index_of(state: &AppState) -> Result<packs::PackIndexLoad, AppError> {
    if let Ok(guard) = state.packs.lock() {
        if let Some(loaded) = guard.clone() {
            return Ok(loaded);
        }
    }
    let key = active_manifest_public_key()?;
    let loaded = if let Some(dir) = local_source_dir(state) {
        packs::load_local_index(&dir, key)?
    } else if crate::local_source::is_enabled() {
        packs::PackIndexLoad {
            index: packs::builtin_index(),
            source: "bundled".into(),
            message: Some(LOCAL_SOURCE_BANNER.into()),
        }
    } else {
        packs::load_best_index(&UreqFetcher::new(), &default_cache_dir(), key)
    };
    remember_index(state, loaded.clone());
    Ok(loaded)
}

fn pack_entry<'a>(
    loaded: &'a packs::PackIndexLoad,
    id: &str,
) -> Result<&'a packs::PackEntry, AppError> {
    loaded
        .index
        .packs
        .iter()
        .find(|pack| pack.id == id)
        .ok_or_else(|| packs::pack_manifest_error(format!("unknown pack {id}")))
}

fn cached_or_load(state: &AppState) -> Result<ManifestLoad, AppError> {
    if let Ok(guard) = state.manifest.lock() {
        if let Some(loaded) = guard.clone() {
            return Ok(loaded);
        }
    }
    let key = active_manifest_public_key()?;
    let index = index_of(state)?;
    let id = selected_pack_id(state);
    let entry = pack_entry(&index, &id)?.clone();
    if packs::needs_newer_app(&entry) {
        return Err(packs::pack_manifest_error(format!(
            "{} needs a newer installer",
            entry.name
        )));
    }
    let loaded = if let Some(dir) = local_source_dir(state) {
        let mut loaded =
            packs::load_pack_manifest(&make_fetcher(state), &entry, &default_cache_dir(), key);
        crate::local_source::bind_manifest_to_folder(&mut loaded.manifest, &dir);
        loaded.local_source = true;
        loaded.local_source_dir = Some(dir.display().to_string());
        loaded
    } else if crate::local_source::is_enabled() {
        pending_local_load()?
    } else {
        packs::load_pack_manifest(&UreqFetcher::new(), &entry, &default_cache_dir(), key)
    };
    if loaded.source == "error" {
        return Err(AppError::SignatureInvalid {
            detail: loaded
                .message
                .unwrap_or_else(|| "No signed mod list is available.".into()),
        });
    }
    if let Ok(mut guard) = state.manifest.lock() {
        *guard = Some(loaded.clone());
    }
    Ok(loaded)
}

fn current_pack_name(state: &AppState) -> String {
    let id = selected_pack_id(state);
    state
        .packs
        .lock()
        .ok()
        .and_then(|guard| {
            guard.as_ref().and_then(|loaded| {
                loaded
                    .index
                    .packs
                    .iter()
                    .find(|pack| pack.id == id)
                    .map(|pack| pack.name.clone())
            })
        })
        .unwrap_or_else(|| "Better Adventures+".into())
}

fn manifest_of(state: &AppState) -> Result<Manifest, AppError> {
    Ok(cached_or_load(state)?.manifest)
}

#[tauri::command]
pub fn get_manifest(state: State<AppState>) -> Result<ManifestLoad, ErrorPayload> {
    cached_or_load(&state).map_err(|e| e.to_payload())
}

#[tauri::command]
pub fn detect(state: State<AppState>) -> Result<Detection, ErrorPayload> {
    let manifest = manifest_of(&state).map_err(|e| e.to_payload())?;
    let chosen = state.game_dir.lock().ok().and_then(|g| g.clone());
    let processes = live_minecraft_processes();
    let mut detection = detect_at(
        chosen.as_deref(),
        &manifest.minecraft,
        &manifest.loader.id,
        &processes,
        &default_candidates(),
    );
    let resolved = resolved_of(&state).map_err(|e| e.to_payload())?;
    let report = probe_launchers();
    let prefs = load_prefs().unwrap_or_default();
    let dest = profile_destinations(&report, &prefs.launchers, &resolved.path);
    detection.pack_id = selected_pack_id(&state);
    detection.pack_dir = Some(resolved.path);
    detection.launcher_root = launcher_root_of(&state);
    detection.folder_mode = if resolved.needs_choice.is_some() {
        "choice".into()
    } else {
        resolved.mode
    };
    detection.needs_folder_choice = resolved.needs_choice;
    detection.launchers = report;
    if !dest.vanilla_roots.is_empty() {
        detection.fabric.installed = dest
            .vanilla_roots
            .iter()
            .all(|root| crate::fabric::existing_fabric_for(root, &manifest.minecraft).is_some());
    }
    Ok(detection)
}

#[tauri::command]
pub fn scan_hardware(state: State<AppState>) -> Result<Recommendation, ErrorPayload> {
    let manifest = manifest_of(&state).map_err(|e| e.to_payload())?;
    Ok(recommend(&probe_hardware(), manifest.recommend.as_ref()))
}

#[tauri::command]
pub fn plan(sel: Selection, state: State<AppState>) -> Result<InstallPlan, ErrorPayload> {
    let manifest = manifest_of(&state).map_err(|e| e.to_payload())?;
    let game = game_dir_of(&state).ok();
    let installed = game
        .as_deref()
        .and_then(|dir| load_state(dir).ok().flatten());
    let detection = detect(state).ok();
    build_plan(
        &manifest,
        &sel,
        installed.as_ref(),
        detection.as_ref().is_some_and(|d| d.fabric.installed),
    )
    .map_err(|e| e.to_payload())
}

fn fresh_cancel_flag(state: &AppState) -> Arc<AtomicBool> {
    let next = Arc::new(AtomicBool::new(false));
    if let Ok(mut guard) = state.cancel_flag.lock() {
        *guard = next.clone();
    }
    next
}

struct InstallRunGuard<'a> {
    flag: &'a AtomicBool,
}

impl Drop for InstallRunGuard<'_> {
    fn drop(&mut self) {
        self.flag.store(false, Ordering::SeqCst);
    }
}

fn begin_install(state: &AppState) -> Result<InstallRunGuard<'_>, AppError> {
    state
        .install_running
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .map_err(|_| AppError::State("an install is already running".into()))?;
    Ok(InstallRunGuard {
        flag: &state.install_running,
    })
}

struct PreparedInstall<'a> {
    target: InstallTarget,
    loaded: ManifestLoad,
    flag: Arc<AtomicBool>,
    fetcher: LocalAwareFetcher<UreqFetcher>,
    _running: InstallRunGuard<'a>,
}

fn install_target_for(state: &AppState, pack_dir: &PathBuf, pack_name: &str) -> InstallTarget {
    let prefs = load_prefs().unwrap_or_default();
    let report = probe_launchers();
    let dest = profile_destinations(&report, &prefs.launchers, pack_dir);
    let id = selected_pack_id(state);
    let kept = prefs.pack_locations.get(&id) == Some(&crate::prefs::PackLocation::DefaultFolder);
    let update_legacy = id == BA_PLUS_PACK_ID
        && kept
        && dest
            .official
            .as_ref()
            .is_some_and(crate::profiles::legacy_profile_key_present);
    let mut roots = dest.vanilla_roots.clone();
    if roots.is_empty() {
        if let Some(official) = &dest.official {
            roots.push(official.clone());
        }
        if dest.also_pack_dir {
            roots.push(pack_dir.clone());
        }
    }
    InstallTarget {
        pack_dir: pack_dir.clone(),
        launcher_roots: roots,
        profiles: crate::profiles::ProfileTargets {
            official: dest.official,
            tlauncher: dest.tlauncher,
            also_pack_dir: dest.also_pack_dir,
        },
        spec: crate::profiles::ProfileSpec {
            key: pack_dirs::profile_key(&id),
            name: pack_dirs::profile_name(pack_name),
            version_id: String::new(),
            game_dir: pack_dir.clone(),
            java_args: String::new(),
            update_legacy_in_place: update_legacy,
        },
        require_vanilla: dest.require_vanilla,
        legacy_same_folder: false,
    }
}

fn prepare_install(state: &AppState) -> Result<PreparedInstall<'_>, AppError> {
    // Take the running flag before replacing the cancel flag, so a second
    // install or repair cannot steal cancellation from the job in progress.
    let running = begin_install(state)?;
    let loaded = cached_or_load(state)?;
    let game = pack_dir_of(state)?;
    let target = install_target_for(state, &game, &current_pack_name(state));
    let flag = fresh_cancel_flag(state);
    require_local_folder(state)?;
    Ok(PreparedInstall {
        target,
        loaded,
        flag,
        fetcher: make_fetcher(state),
        _running: running,
    })
}

fn progress_sink(app: AppHandle) -> impl Fn(ProgressEvent) {
    let throttle = Mutex::new(ProgressThrottle::new(PROGRESS_MIN_GAP));
    move |event| {
        let send = throttle
            .lock()
            .map(|mut gate| gate.should_emit(&event, Instant::now()))
            .unwrap_or(true);
        if send {
            let _ = app.emit("install://progress", event);
        }
    }
}

async fn spawn_install_worker<F>(label: &'static str, work: F) -> Result<String, ErrorPayload>
where
    F: FnOnce() -> Result<String, ErrorPayload> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|err| AppError::Io(format!("{label} worker stopped: {err}")).to_payload())?
}

#[tauri::command]
pub async fn install(
    app: AppHandle,
    plan: InstallPlan,
    replace_collisions: Option<bool>,
) -> Result<String, ErrorPayload> {
    let worker = app.clone();
    let replace = replace_collisions.unwrap_or(false);
    spawn_install_worker("install", move || {
        let state = worker.state::<AppState>();
        let prepared = prepare_install(&state).map_err(|e| e.to_payload())?;
        let ram = probe_hardware().ram_gb;
        let sink = progress_sink(worker.clone());
        run_install(
            &prepared.target,
            &plan,
            &prepared.loaded.manifest,
            &prepared.fetcher,
            prepared.flag.as_ref(),
            replace,
            ram,
            &sink,
        )
        .map_err(|e| e.to_payload())
    })
    .await
}

#[tauri::command]
pub fn cancel(state: State<AppState>) -> Result<(), ErrorPayload> {
    if let Ok(flag) = state.cancel_flag.lock() {
        flag.store(true, Ordering::SeqCst);
    }
    Ok(())
}

#[tauri::command]
pub async fn repair(app: AppHandle) -> Result<String, ErrorPayload> {
    let worker = app.clone();
    spawn_install_worker("repair", move || {
        let state = worker.state::<AppState>();
        let prepared = prepare_install(&state).map_err(|e| e.to_payload())?;
        let ram = probe_hardware().ram_gb;
        let sink = progress_sink(worker.clone());
        repair_tracked(
            &prepared.target,
            &prepared.loaded.manifest,
            &prepared.fetcher,
            prepared.flag.as_ref(),
            ram,
            &sink,
        )
        .map_err(|e| e.to_payload())
    })
    .await
}

#[tauri::command]
pub fn list_installed(state: State<AppState>) -> Result<Option<InstalledState>, ErrorPayload> {
    let game = game_dir_of(&state).map_err(|e| e.to_payload())?;
    load_state(&game).map_err(|e| e.to_payload())
}

#[tauri::command]
pub fn uninstall(
    ids: Vec<String>,
    state: State<AppState>,
) -> Result<UninstallResult, ErrorPayload> {
    let game = pack_dir_of(&state).map_err(|e| e.to_payload())?;
    let mut installed = load_state(&game)
        .map_err(|e| e.to_payload())?
        .ok_or_else(|| AppError::State("nothing is recorded in state.json".into()).to_payload())?;
    uninstall_tracked(&game, &mut installed, &ids).map_err(|e| e.to_payload())
}

#[tauri::command]
pub fn list_snapshots(state: State<AppState>) -> Result<Vec<SnapshotMeta>, ErrorPayload> {
    let game = game_dir_of(&state).map_err(|e| e.to_payload())?;
    list_snapshots_on_disk(&game).map_err(|e| e.to_payload())
}

#[tauri::command]
pub fn rollback(
    snapshot_id: String,
    state: State<AppState>,
) -> Result<InstalledState, ErrorPayload> {
    let game = pack_dir_of(&state).map_err(|e| e.to_payload())?;
    let installed = load_state(&game)
        .map_err(|e| e.to_payload())?
        .unwrap_or_else(|| InstalledState::new("installer", "unknown"));
    rollback_on_disk(&game, &snapshot_id, &installed).map_err(|e| e.to_payload())
}

#[tauri::command]
pub fn set_game_dir(path: String, state: State<AppState>) -> Result<Detection, ErrorPayload> {
    let buf = PathBuf::from(path);
    {
        let mut guard = state
            .game_dir
            .lock()
            .map_err(|_| AppError::State("game dir lock".into()).to_payload())?;
        *guard = Some(buf.clone());
    }
    let mut prefs = load_prefs().unwrap_or_default();
    prefs.last_game_dir = Some(buf);
    let _ = save_prefs(&prefs);
    detect(state)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    pub path: String,
    pub revealed: bool,
}

#[tauri::command]
pub fn export_log(state: State<AppState>) -> Result<ExportResult, ErrorPayload> {
    let game = game_dir_of(&state).ok();
    let loaded = cached_or_load(&state).ok();
    let summary = build_summary(
        game.as_deref(),
        loaded
            .as_ref()
            .map(|m| m.manifest.minecraft.as_str())
            .unwrap_or("26.3"),
        loaded
            .as_ref()
            .map(|m| m.manifest.manifest_version.as_str()),
        loaded.as_ref().map(|m| m.source.as_str()),
    );
    let path = export_install_log(game.as_deref(), None, &summary, &local_export_stamp())
        .map_err(|e| e.to_payload())?;
    let revealed = crate::process::reveal_path(&path).is_ok();
    Ok(ExportResult {
        path: path.display().to_string(),
        revealed,
    })
}

#[tauri::command]
pub fn reveal_path(path: String) -> Result<(), ErrorPayload> {
    crate::process::reveal_path(std::path::Path::new(&path)).map_err(|e| e.to_payload())
}

#[tauri::command]
pub fn copy_diagnostics(state: State<AppState>) -> Result<String, ErrorPayload> {
    let game = game_dir_of(&state).ok();
    let loaded = cached_or_load(&state).ok();
    let summary = build_summary(
        game.as_deref(),
        "26.3",
        loaded
            .as_ref()
            .map(|m| m.manifest.manifest_version.as_str()),
        loaded.as_ref().map(|m| m.source.as_str()),
    );
    Ok(diagnostics_text(&summary, Some(&diagnostics_extra(&state))))
}

fn diagnostics_extra(state: &AppState) -> String {
    let resolved = resolved_of(state).ok();
    let report = probe_launchers();
    let index_source = state
        .packs
        .lock()
        .ok()
        .and_then(|guard| guard.as_ref().map(|loaded| loaded.source.clone()))
        .unwrap_or_else(|| "not loaded".into());
    format!(
        "pack: {}\npack dir: {}\nfolder mode: {}\nlaunchers: official={} other={}\nindex: {index_source}",
        selected_pack_id(state),
        resolved
            .as_ref()
            .map(|item| item.path.display().to_string())
            .unwrap_or_else(|| "(unresolved)".into()),
        resolved
            .as_ref()
            .map(|item| item.mode.as_str())
            .unwrap_or("unknown"),
        report.official.is_some(),
        report.tlauncher.is_some(),
    )
}

#[tauri::command]
pub fn detect_launcher() -> crate::launcher::LauncherAvailability {
    crate::launcher::availability()
}

#[tauri::command]
pub fn open_launcher(app: AppHandle) -> crate::launcher::LauncherOpenResult {
    open_selected(&app)
}

fn open_selected(app: &AppHandle) -> crate::launcher::LauncherOpenResult {
    let state = app.state::<AppState>();
    let report = probe_launchers();
    let prefs = load_prefs().unwrap_or_default();
    let name = current_pack_name(&state);
    let Some(kind) = choose_play_target(&report, &prefs.launchers) else {
        return crate::launcher::LauncherOpenResult {
            opened: false,
            note: crate::launcher::launcher_note(&name, "Minecraft Launcher"),
        };
    };
    let result = crate::launcher::open(kind, &report, &name);
    if crate::launcher::should_exit_after_launch(result.opened) {
        exit_app(app);
    }
    result
}

#[tauri::command]
pub fn finish(app: AppHandle) {
    exit_app(&app);
}

/// Stop the process. `app.exit` asks the runtime to leave; `std::process::exit`
/// then ends it so a webview host cannot keep the exe up. There is no
/// close confirmation, tray icon, or window handler that can cancel this.
fn exit_app(app: &AppHandle) -> ! {
    app.exit(0);
    std::process::exit(0);
}

#[tauri::command]
pub fn detect_legacy(state: State<AppState>) -> Result<ImportOffer, ErrorPayload> {
    let game = game_dir_of(&state).map_err(|e| e.to_payload())?;
    let manifest = manifest_of(&state).map_err(|e| e.to_payload())?;
    detect_import(&game, &manifest).map_err(|e| e.to_payload())
}

#[tauri::command]
pub fn confirm_import(state: State<AppState>) -> Result<InstalledState, ErrorPayload> {
    let game = pack_dir_of(&state).map_err(|e| e.to_payload())?;
    let manifest = manifest_of(&state).map_err(|e| e.to_payload())?;
    adopt_import(&game, &manifest, true).map_err(|e| e.to_payload())
}

#[tauri::command]
pub fn get_prefs() -> Result<Prefs, ErrorPayload> {
    load_prefs().map_err(|e| e.to_payload())
}

#[tauri::command]
pub fn set_tour_done() -> Result<Prefs, ErrorPayload> {
    let mut prefs = load_prefs().map_err(|e| e.to_payload())?;
    prefs.tour_done = true;
    save_prefs(&prefs).map_err(|e| e.to_payload())?;
    Ok(prefs)
}

#[tauri::command]
pub fn get_config(id: String, state: State<AppState>) -> Result<serde_json::Value, ErrorPayload> {
    let manifest = manifest_of(&state).map_err(|e| e.to_payload())?;
    let component = component_by_id(&manifest, &id)
        .ok_or_else(|| AppError::ManifestSchema(format!("unknown component {id}")).to_payload())?;
    let Some(config) = &component.config else {
        return Ok(serde_json::json!({ "id": id, "hasConfig": false }));
    };
    let game = game_dir_of(&state).map_err(|e| e.to_payload())?;
    let path = game.join(&config.file);
    let existing = std::fs::read_to_string(&path).ok();
    let lite_detected = existing
        .as_deref()
        .is_some_and(|text| text.lines().next().is_some_and(|l| l.trim() == LITE_MARKER));
    Ok(serde_json::json!({
        "id": id,
        "hasConfig": true,
        "file": config.file,
        "exists": existing.is_some(),
        "text": existing.clone().unwrap_or_default(),
        "liteDetected": lite_detected,
        "presetId": SNOW_LITE_PRESET,
        "settings": config.settings,
    }))
}

#[tauri::command]
pub fn set_config(
    id: String,
    lite: bool,
    state: State<AppState>,
) -> Result<serde_json::Value, ErrorPayload> {
    let manifest = manifest_of(&state).map_err(|e| e.to_payload())?;
    let component = component_by_id(&manifest, &id)
        .ok_or_else(|| AppError::ManifestSchema(format!("unknown component {id}")).to_payload())?;
    let config = component
        .config
        .as_ref()
        .ok_or_else(|| AppError::ManifestSchema(format!("{id} has no config")).to_payload())?;
    let game = pack_dir_of(&state).map_err(|e| e.to_payload())?;
    let path = game.join(&config.file);
    let existing = std::fs::read_to_string(&path).ok();
    let mut defaults = BTreeMap::new();
    for (key, spec) in &config.settings {
        defaults.insert(key.clone(), format_setting_value(&spec.default));
    }
    let mut lite_over = BTreeMap::new();
    if let Some(preset) = lite_preset(config) {
        for (key, value) in &preset.overrides {
            lite_over.insert(key.clone(), format_setting_value(value));
        }
    }
    let switch = if lite {
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
        std::fs::create_dir_all(parent).map_err(|e| AppError::from(e).to_payload())?;
    }
    std::fs::write(&path, outcome.content.as_bytes())
        .map_err(|e| AppError::from(e).to_payload())?;
    Ok(serde_json::json!({
        "skippedSwitch": outcome.skipped_switch,
        "addedKeys": outcome.added_keys,
        "rewritten": outcome.rewritten,
        "presetId": SNOW_LITE_PRESET,
    }))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigKeyUpdate {
    pub id: String,
    pub values: BTreeMap<String, String>,
}

#[tauri::command]
pub fn set_config_keys(
    update: ConfigKeyUpdate,
    state: State<AppState>,
) -> Result<serde_json::Value, ErrorPayload> {
    let manifest = manifest_of(&state).map_err(|e| e.to_payload())?;
    let component = component_by_id(&manifest, &update.id).ok_or_else(|| {
        AppError::ManifestSchema(format!("unknown component {}", update.id)).to_payload()
    })?;
    let config = component.config.as_ref().ok_or_else(|| {
        AppError::ManifestSchema(format!("{} has no config", update.id)).to_payload()
    })?;
    let game = pack_dir_of(&state).map_err(|e| e.to_payload())?;
    let path = game.join(&config.file);
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let next = update_property_keys(&existing, &update.values);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::from(e).to_payload())?;
    }
    std::fs::write(&path, next.as_bytes()).map_err(|e| AppError::from(e).to_payload())?;
    Ok(serde_json::json!({ "ok": true, "file": config.file }))
}

#[tauri::command]
pub fn set_quality(level: String, state: State<AppState>) -> Result<InstalledState, ErrorPayload> {
    let quality = QualityLevel::parse(&level)
        .ok_or_else(|| AppError::ManifestSchema(format!("unknown quality {level}")).to_payload())?;
    let game = pack_dir_of(&state).map_err(|e| e.to_payload())?;
    let manifest = manifest_of(&state).map_err(|e| e.to_payload())?;
    let installed = load_state(&game)
        .map_err(|e| e.to_payload())?
        .ok_or_else(|| {
            AppError::State("install a kit before changing quality".into()).to_payload()
        })?;
    apply_quality_change(&game, &manifest, quality, &installed).map_err(|e| e.to_payload())
}

#[tauri::command]
pub async fn check_update(_app: AppHandle) -> Result<UpdateStatus, ErrorPayload> {
    #[cfg(feature = "updater")]
    {
        match crate::updater::configured_updater(&_app) {
            Ok(updater) => match updater.check().await {
                Ok(Some(update)) => Ok(UpdateStatus {
                    available: true,
                    version: Some(update.version),
                    reason: "A newer installer is on GitHub Releases.".into(),
                    endpoint: crate::updater::LATEST_JSON_URL.to_string(),
                    enabled: true,
                }),
                Ok(None) => Ok(UpdateStatus {
                    available: false,
                    version: None,
                    reason: "This installer is current.".into(),
                    endpoint: crate::updater::LATEST_JSON_URL.to_string(),
                    enabled: true,
                }),
                Err(err) => Ok(UpdateStatus {
                    available: false,
                    version: None,
                    reason: err.to_string(),
                    endpoint: crate::updater::LATEST_JSON_URL.to_string(),
                    enabled: true,
                }),
            },
            Err(_) => Ok(crate::updater::disabled_status()),
        }
    }
    #[cfg(not(feature = "updater"))]
    {
        Ok(check_update_sync())
    }
}

#[tauri::command]
pub async fn download_and_install(_app: AppHandle) -> Result<(), ErrorPayload> {
    #[cfg(feature = "updater")]
    {
        let updater = crate::updater::configured_updater(&_app).map_err(|e| {
            AppError::NotImplemented {
                what: format!("self-update: {e}"),
            }
            .to_payload()
        })?;
        let update = updater
            .check()
            .await
            .map_err(|e| AppError::Io(e.to_string()).to_payload())?;
        let Some(update) = update else {
            return Ok(());
        };
        update
            .download_and_install(|_, _| {}, || {})
            .await
            .map_err(|e| AppError::Io(e.to_string()).to_payload())?;
        return Ok(());
    }
    #[cfg(not(feature = "updater"))]
    {
        Err(AppError::NotImplemented {
            what: "self-update (no updater key in this build)".into(),
        }
        .to_payload())
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalSourceInfo {
    pub enabled: bool,
    pub dir: Option<String>,
    pub banner: Option<String>,
    pub needs_folder: bool,
}

fn local_source_info(state: &AppState) -> LocalSourceInfo {
    let enabled = crate::local_source::is_enabled();
    let dir = local_source_dir(state).map(|p| p.display().to_string());
    LocalSourceInfo {
        enabled,
        dir: dir.clone(),
        banner: enabled.then(|| LOCAL_SOURCE_BANNER.to_string()),
        needs_folder: enabled && dir.is_none(),
    }
}

fn store_local_source(state: &AppState, dir: PathBuf) -> Result<LocalSourceInfo, AppError> {
    if !crate::local_source::is_enabled() {
        return Err(AppError::NotImplemented {
            what: "local-source test build".into(),
        });
    }
    if !dir.is_dir() {
        return Err(AppError::State(format!(
            "TEST BUILD: not a folder: {}",
            dir.display()
        )));
    }
    let dir = dir.canonicalize()?;
    {
        let mut guard = state.local_source.lock().map_err(|_| lock_err())?;
        *guard = Some(dir);
    }
    clear_manifest_cache(state);
    if let Ok(mut guard) = state.packs.lock() {
        *guard = None;
    }
    Ok(local_source_info(state))
}

#[tauri::command]
pub fn get_local_source(state: State<AppState>) -> LocalSourceInfo {
    local_source_info(&state)
}

#[tauri::command]
pub fn pick_local_source(state: State<AppState>) -> Result<LocalSourceInfo, ErrorPayload> {
    #[cfg(feature = "local-source")]
    {
        let Some(dir) = crate::local_source::pick_folder() else {
            return Ok(local_source_info(&state));
        };
        store_local_source(&state, dir).map_err(|e| e.to_payload())
    }
    #[cfg(not(feature = "local-source"))]
    {
        let _ = state;
        Err(AppError::NotImplemented {
            what: "local-source test build".into(),
        }
        .to_payload())
    }
}

#[tauri::command]
pub fn set_local_source(
    dir: String,
    state: State<AppState>,
) -> Result<LocalSourceInfo, ErrorPayload> {
    store_local_source(&state, PathBuf::from(dir)).map_err(|e| e.to_payload())
}

#[tauri::command]
pub fn snapshot_now(label: String, state: State<AppState>) -> Result<SnapshotMeta, ErrorPayload> {
    let game = pack_dir_of(&state).map_err(|e| e.to_payload())?;
    let installed = load_state(&game)
        .map_err(|e| e.to_payload())?
        .unwrap_or_else(|| InstalledState::new("installer", "unknown"));
    create_snapshot(&game, &installed, &label).map_err(|e| e.to_payload())
}

#[tauri::command]
pub fn list_packs(state: State<AppState>) -> Result<packs::PackIndexLoad, ErrorPayload> {
    index_of(&state).map_err(|e| e.to_payload())
}

#[tauri::command]
pub fn select_pack(id: String, state: State<AppState>) -> Result<Prefs, ErrorPayload> {
    let index = index_of(&state).map_err(|e| e.to_payload())?;
    let entry = pack_entry(&index, &id).map_err(|e| e.to_payload())?;
    if packs::needs_newer_app(entry) {
        return Err(
            packs::pack_manifest_error(format!("{} needs a newer installer", entry.name))
                .to_payload(),
        );
    }
    {
        let mut guard = state
            .selected_pack
            .lock()
            .map_err(|_| AppError::State("pack lock".into()).to_payload())?;
        *guard = id.clone();
    }
    clear_manifest_cache(&state);
    let mut prefs = load_prefs().unwrap_or_default();
    prefs.selected_pack = Some(id);
    save_prefs(&prefs).map_err(|e| e.to_payload())?;
    Ok(prefs)
}

#[tauri::command]
pub fn choose_pack_folder(choice: String, state: State<AppState>) -> Result<Prefs, ErrorPayload> {
    let id = selected_pack_id(&state);
    let resolved = resolved_of(&state).map_err(|e| e.to_payload())?;
    let legacy = resolved.needs_choice.map(|item| item.root);
    let mut prefs = load_prefs().unwrap_or_default();
    match choice.as_str() {
        "keep" => {
            let root = legacy.clone().unwrap_or_else(|| launcher_root_of(&state));
            prefs
                .pack_locations
                .insert(id, crate::prefs::PackLocation::DefaultFolder);
            prefs.last_game_dir = Some(root.clone());
            if let Ok(mut guard) = state.game_dir.lock() {
                *guard = Some(root);
            }
        }
        "fresh" => {
            prefs
                .pack_locations
                .insert(id, crate::prefs::PackLocation::OwnFolder);
            if prefs.last_game_dir.is_none() {
                if let Some(root) = legacy {
                    prefs.last_game_dir = Some(root.clone());
                    if let Ok(mut guard) = state.game_dir.lock() {
                        *guard = Some(root);
                    }
                }
            }
        }
        _ => {
            return Err(AppError::State("choose keep or fresh".into()).to_payload());
        }
    }
    save_prefs(&prefs).map_err(|e| e.to_payload())?;
    Ok(prefs)
}

#[tauri::command]
pub async fn copy_from_old_folder(
    app: AppHandle,
    item: String,
) -> Result<pack_dirs::CopyReport, ErrorPayload> {
    let item = CopyItem::parse(&item)
        .ok_or_else(|| AppError::State("copy options, servers, or saves".into()).to_payload())?;
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let _running = begin_install(&state).map_err(|e| e.to_payload())?;
        let pack = pack_dir_of(&state).map_err(|e| e.to_payload())?;
        let prefs = load_prefs().unwrap_or_default();
        let old = prefs.last_game_dir.ok_or_else(|| {
            AppError::State("no previous Minecraft folder to copy from".into()).to_payload()
        })?;
        pack_dirs::copy_from_old(&old, &pack, item).map_err(|e| e.to_payload())
    })
    .await
    .map_err(|err| AppError::Io(format!("copy worker stopped: {err}")).to_payload())?
}

#[tauri::command]
pub fn set_use_default_folder(enabled: bool) -> Result<Prefs, ErrorPayload> {
    let mut prefs = load_prefs().map_err(|e| e.to_payload())?;
    prefs.use_default_folder = enabled;
    save_prefs(&prefs).map_err(|e| e.to_payload())?;
    Ok(prefs)
}

#[tauri::command]
pub fn switch_shared_pack(
    to_id: String,
    state: State<AppState>,
) -> Result<pack_dirs::SwitchReport, ErrorPayload> {
    let _running = begin_install(&state).map_err(|e| e.to_payload())?;
    let resolved = resolved_of(&state).map_err(|e| e.to_payload())?;
    if resolved.needs_choice.is_some() || resolved.mode != "default" {
        return Err(AppError::State(
            "pack switching only applies when packs share the Minecraft folder".into(),
        )
        .to_payload());
    }
    let index = index_of(&state).map_err(|e| e.to_payload())?;
    let entry = pack_entry(&index, &to_id)
        .map_err(|e| e.to_payload())?
        .clone();
    let key = active_manifest_public_key().map_err(|e| e.to_payload())?;
    let loaded =
        packs::load_pack_manifest(&make_fetcher(&state), &entry, &default_cache_dir(), key);
    if loaded.source == "error" {
        return Err(AppError::SignatureInvalid {
            detail: loaded.message.unwrap_or_else(|| "pack list failed".into()),
        }
        .to_payload());
    }
    let from_id = selected_pack_id(&state);
    let processes = live_minecraft_processes();
    let report = pack_dirs::switch_shared_pack(
        &resolved.path,
        &from_id,
        &to_id,
        &loaded.manifest,
        &processes,
    )
    .map_err(|e| e.to_payload())?;
    {
        let mut guard = state
            .selected_pack
            .lock()
            .map_err(|_| AppError::State("pack lock".into()).to_payload())?;
        *guard = to_id.clone();
    }
    clear_manifest_cache(&state);
    let mut prefs = load_prefs().unwrap_or_default();
    prefs.selected_pack = Some(to_id);
    let _ = save_prefs(&prefs);
    Ok(report)
}

#[tauri::command]
pub fn detect_launchers() -> crate::launcher::LauncherReport {
    probe_launchers()
}

#[tauri::command]
pub fn set_launcher_prefs(prefs: LauncherPrefs) -> Result<Prefs, ErrorPayload> {
    let mut stored = load_prefs().map_err(|e| e.to_payload())?;
    stored.launchers = prefs;
    save_prefs(&stored).map_err(|e| e.to_payload())?;
    Ok(stored)
}

#[tauri::command]
pub fn play(app: AppHandle) -> crate::launcher::LauncherOpenResult {
    open_selected(&app)
}

#[tauri::command]
pub fn open_credit_link(which: String) -> Result<(), ErrorPayload> {
    let url = packs::credit_url(&which).map_err(|e| e.to_payload())?;
    crate::process::open_url(url).map_err(|e| e.to_payload())
}

#[cfg(all(test, feature = "local-source"))]
mod tests {
    use super::*;

    #[test]
    fn pending_local_load_uses_bundled_ba_plus_list() {
        let loaded = pending_local_load().unwrap();
        assert_eq!(loaded.source, "bundled");
        assert_eq!(loaded.manifest.manifest_version, "2026.11.7");
        assert!(loaded.local_source);
        assert!(loaded.local_source_dir.is_none());
    }
}
