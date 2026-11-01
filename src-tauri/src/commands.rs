use std::path::PathBuf;
use std::sync::Mutex;

use tauri::{AppHandle, Emitter, State};

use crate::backup::{
    create_snapshot, list_snapshots as list_snapshots_on_disk, rollback as rollback_on_disk,
    uninstall_tracked, SnapshotMeta, UninstallResult,
};
use crate::config::{
    apply_properties, format_setting_value, ConfigSwitch, LITE_MARKER, MANAGED_HEADER,
};
use crate::detect::{default_candidates, detect_at, live_minecraft_processes, Detection};
use crate::errors::{AppError, ErrorPayload};
use crate::manifest::{load_bundled_manifest, Manifest};
use crate::plan::{build_plan, InstallPlan, Selection};
use crate::scan::{probe_hardware, recommend, Recommendation};
use crate::state::{load_state, InstalledState};

pub struct AppState {
    pub game_dir: Mutex<Option<PathBuf>>,
}

fn game_dir_of(state: &AppState) -> Result<PathBuf, AppError> {
    state
        .game_dir
        .lock()
        .map_err(|_| AppError::State("game dir lock".into()))?
        .clone()
        .or_else(|| default_candidates().into_iter().find(|p| p.is_dir()))
        .ok_or_else(|| AppError::McMissing {
            version: "26.3".into(),
            path: "(no game folder)".into(),
        })
}

#[tauri::command]
pub fn get_manifest() -> Result<Manifest, ErrorPayload> {
    load_bundled_manifest().map_err(|e| e.to_payload())
}

#[tauri::command]
pub fn detect(state: State<AppState>) -> Result<Detection, ErrorPayload> {
    let manifest = load_bundled_manifest().map_err(|e| e.to_payload())?;
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
pub fn scan_hardware() -> Result<Recommendation, ErrorPayload> {
    let manifest = load_bundled_manifest().map_err(|e| e.to_payload())?;
    Ok(recommend(&probe_hardware(), manifest.recommend.as_ref()))
}

#[tauri::command]
pub fn plan(sel: Selection, state: State<AppState>) -> Result<InstallPlan, ErrorPayload> {
    let manifest = load_bundled_manifest().map_err(|e| e.to_payload())?;
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
pub fn install(app: AppHandle, plan: InstallPlan) -> Result<String, ErrorPayload> {
    let run_id = format!("run-{}", crate::state::now_stamp());
    let _ = app.emit(
        "install://progress",
        serde_json::json!({
            "runId": run_id,
            "step": "stub",
            "status": "stubbed",
            "percent": 0,
            "message": "Install transaction is stubbed in Phase 1. The UI uses the mock runner.",
            "kit": plan.kit,
        }),
    );
    Err(AppError::NotImplemented {
        what: "install transaction".into(),
    }
    .to_payload())
}

#[tauri::command]
pub fn cancel(_run: String) -> Result<(), ErrorPayload> {
    Err(AppError::NotImplemented {
        what: "install cancel".into(),
    }
    .to_payload())
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
        *guard = Some(buf);
    }
    detect(state)
}

#[tauri::command]
pub fn export_log() -> Result<PathBuf, ErrorPayload> {
    crate::log::export_log(None).map_err(|e| e.to_payload())
}

#[tauri::command]
pub fn open_launcher() -> Result<(), ErrorPayload> {
    Err(AppError::NotImplemented {
        what: "open Minecraft Launcher".into(),
    }
    .to_payload())
}

#[tauri::command]
pub fn get_config(id: String, state: State<AppState>) -> Result<serde_json::Value, ErrorPayload> {
    let manifest = load_bundled_manifest().map_err(|e| e.to_payload())?;
    let component = crate::manifest::component_by_id(&manifest, &id)
        .ok_or_else(|| AppError::ManifestSchema(format!("unknown component {id}")).to_payload())?;
    let Some(config) = &component.config else {
        return Ok(serde_json::json!({ "id": id, "hasConfig": false }));
    };
    let game = game_dir_of(&state).map_err(|e| e.to_payload())?;
    let path = game.join(&config.file);
    let existing = std::fs::read_to_string(&path).ok();
    Ok(serde_json::json!({
        "id": id,
        "hasConfig": true,
        "file": config.file,
        "exists": existing.is_some(),
        "text": existing.unwrap_or_default(),
    }))
}

#[tauri::command]
pub fn set_config(
    id: String,
    lite: bool,
    state: State<AppState>,
) -> Result<serde_json::Value, ErrorPayload> {
    let manifest = load_bundled_manifest().map_err(|e| e.to_payload())?;
    let component = crate::manifest::component_by_id(&manifest, &id)
        .ok_or_else(|| AppError::ManifestSchema(format!("unknown component {id}")).to_payload())?;
    let config = component
        .config
        .as_ref()
        .ok_or_else(|| AppError::ManifestSchema(format!("{id} has no config")).to_payload())?;
    let game = game_dir_of(&state).map_err(|e| e.to_payload())?;
    let path = game.join(&config.file);
    let existing = std::fs::read_to_string(&path).ok();
    let mut defaults = std::collections::BTreeMap::new();
    for (key, spec) in &config.settings {
        defaults.insert(key.clone(), format_setting_value(&spec.default));
    }
    let mut lite_over = std::collections::BTreeMap::new();
    if let Some(preset) = config.presets.get("lite") {
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
    let marker = config
        .presets
        .get("lite")
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
    }))
}

#[tauri::command]
pub fn check_update() -> Result<serde_json::Value, ErrorPayload> {
    Ok(serde_json::json!({
        "available": false,
        "reason": "Updater is stubbed in Phase 1. No signing keys are shipped."
    }))
}

#[tauri::command]
pub fn download_and_install() -> Result<(), ErrorPayload> {
    Err(AppError::NotImplemented {
        what: "self-update".into(),
    }
    .to_payload())
}

#[tauri::command]
pub fn snapshot_now(label: String, state: State<AppState>) -> Result<SnapshotMeta, ErrorPayload> {
    let game = game_dir_of(&state).map_err(|e| e.to_payload())?;
    let installed = load_state(&game)
        .map_err(|e| e.to_payload())?
        .unwrap_or_else(|| InstalledState::new("installer", "unknown"));
    create_snapshot(&game, &installed, &label).map_err(|e| e.to_payload())
}
