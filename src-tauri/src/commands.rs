use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::Deserialize;
use tauri::{AppHandle, Emitter, State};

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
use crate::install::{repair_tracked, run_install, ProgressEvent};
use crate::log::{build_summary, diagnostics_text, export_log_zip};
use crate::manifest::{
    component_by_id, default_cache_dir, lite_preset, load_best_manifest, Manifest, ManifestLoad,
    SNOW_LITE_PRESET,
};
use crate::plan::{build_plan, InstallPlan, Selection};
use crate::prefs::{load_prefs, save_prefs, Prefs};
use crate::quality::{apply_quality_change, QualityLevel};
use crate::scan::{probe_hardware, recommend, Recommendation};
use crate::state::{load_state, InstalledState};
use crate::updater::{check_update_sync, UpdateStatus};

pub struct AppState {
    pub game_dir: Mutex<Option<PathBuf>>,
    pub cancel_flag: Mutex<Arc<AtomicBool>>,
    pub manifest: Mutex<Option<ManifestLoad>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

impl AppState {
    pub fn new() -> Self {
        let prefs = load_prefs().unwrap_or_default();
        Self {
            game_dir: Mutex::new(prefs.last_game_dir),
            cancel_flag: Mutex::new(Arc::new(AtomicBool::new(false))),
            manifest: Mutex::new(None),
        }
    }
}

fn lock_err() -> AppError {
    AppError::State("lock".into())
}

fn game_dir_of(state: &AppState) -> Result<PathBuf, AppError> {
    state
        .game_dir
        .lock()
        .map_err(|_| lock_err())?
        .clone()
        .or_else(|| default_candidates().into_iter().find(|p| p.is_dir()))
        .ok_or_else(|| AppError::McMissing {
            version: "26.3".into(),
            path: "(no game folder)".into(),
        })
}

fn cached_or_load(state: &AppState) -> Result<ManifestLoad, AppError> {
    if let Ok(guard) = state.manifest.lock() {
        if let Some(loaded) = guard.clone() {
            return Ok(loaded);
        }
    }
    let loaded = load_best_manifest(
        &UreqFetcher::new(),
        &default_cache_dir(),
        crate::manifest::TEST_MANIFEST_PUBLIC_KEY_HEX,
    );
    if let Ok(mut guard) = state.manifest.lock() {
        *guard = Some(loaded.clone());
    }
    Ok(loaded)
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
    Ok(detect_at(
        chosen.as_deref(),
        &manifest.minecraft,
        &manifest.loader.id,
        &processes,
        &default_candidates(),
    ))
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

#[tauri::command]
pub fn install(
    app: AppHandle,
    plan: InstallPlan,
    replace_collisions: Option<bool>,
    state: State<AppState>,
) -> Result<String, ErrorPayload> {
    let game = game_dir_of(&state).map_err(|e| e.to_payload())?;
    let loaded = cached_or_load(&state).map_err(|e| e.to_payload())?;
    let flag = {
        let next = Arc::new(AtomicBool::new(false));
        if let Ok(mut guard) = state.cancel_flag.lock() {
            *guard = next.clone();
        }
        next
    };
    let ram = probe_hardware().ram_gb;
    let replace = replace_collisions.unwrap_or(false);
    run_install(
        &game,
        &plan,
        &loaded.manifest,
        &UreqFetcher::new(),
        flag.as_ref(),
        replace,
        ram,
        &|event: ProgressEvent| {
            let _ = app.emit("install://progress", event);
        },
    )
    .map_err(|e| e.to_payload())
}

#[tauri::command]
pub fn cancel(state: State<AppState>) -> Result<(), ErrorPayload> {
    if let Ok(flag) = state.cancel_flag.lock() {
        flag.store(true, Ordering::SeqCst);
    }
    Ok(())
}

#[tauri::command]
pub fn repair(app: AppHandle, state: State<AppState>) -> Result<String, ErrorPayload> {
    let game = game_dir_of(&state).map_err(|e| e.to_payload())?;
    let loaded = cached_or_load(&state).map_err(|e| e.to_payload())?;
    let flag = {
        let next = Arc::new(AtomicBool::new(false));
        if let Ok(mut guard) = state.cancel_flag.lock() {
            *guard = next.clone();
        }
        next
    };
    repair_tracked(
        &game,
        &loaded.manifest,
        &UreqFetcher::new(),
        flag.as_ref(),
        probe_hardware().ram_gb,
        &|event: ProgressEvent| {
            let _ = app.emit("install://progress", event);
        },
    )
    .map_err(|e| e.to_payload())
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
    let game = game_dir_of(&state).map_err(|e| e.to_payload())?;
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
    let game = game_dir_of(&state).map_err(|e| e.to_payload())?;
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

#[tauri::command]
pub fn export_log(state: State<AppState>) -> Result<PathBuf, ErrorPayload> {
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
    export_log_zip(game.as_deref(), None, &summary).map_err(|e| e.to_payload())
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
    Ok(diagnostics_text(&summary, None))
}

#[tauri::command]
pub fn open_launcher() -> Result<String, ErrorPayload> {
    crate::launcher::open_minecraft_launcher().map_err(|e| e.to_payload())
}

#[tauri::command]
pub fn detect_legacy(state: State<AppState>) -> Result<ImportOffer, ErrorPayload> {
    let game = game_dir_of(&state).map_err(|e| e.to_payload())?;
    let manifest = manifest_of(&state).map_err(|e| e.to_payload())?;
    detect_import(&game, &manifest).map_err(|e| e.to_payload())
}

#[tauri::command]
pub fn confirm_import(state: State<AppState>) -> Result<InstalledState, ErrorPayload> {
    let game = game_dir_of(&state).map_err(|e| e.to_payload())?;
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
    let game = game_dir_of(&state).map_err(|e| e.to_payload())?;
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
    let game = game_dir_of(&state).map_err(|e| e.to_payload())?;
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
    let game = game_dir_of(&state).map_err(|e| e.to_payload())?;
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
        use tauri_plugin_updater::UpdaterExt;
        match _app.updater() {
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
        use tauri_plugin_updater::UpdaterExt;
        let updater = _app.updater().map_err(|e| {
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

#[tauri::command]
pub fn snapshot_now(label: String, state: State<AppState>) -> Result<SnapshotMeta, ErrorPayload> {
    let game = game_dir_of(&state).map_err(|e| e.to_payload())?;
    let installed = load_state(&game)
        .map_err(|e| e.to_payload())?
        .unwrap_or_else(|| InstalledState::new("installer", "unknown"));
    create_snapshot(&game, &installed, &label).map_err(|e| e.to_payload())
}
