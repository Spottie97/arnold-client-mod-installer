pub mod backup;
pub mod commands;
pub mod config;
pub mod detect;
pub mod download;
pub mod errors;
pub mod fabric;
pub mod install;
pub mod log;
pub mod manifest;
pub mod plan;
pub mod scan;
pub mod state;

use commands::AppState;
use std::sync::Mutex;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState {
            game_dir: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_manifest,
            commands::detect,
            commands::scan_hardware,
            commands::plan,
            commands::install,
            commands::cancel,
            commands::list_installed,
            commands::uninstall,
            commands::list_snapshots,
            commands::rollback,
            commands::set_game_dir,
            commands::export_log,
            commands::open_launcher,
            commands::get_config,
            commands::set_config,
            commands::check_update,
            commands::download_and_install,
            commands::snapshot_now,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
