pub mod backup;
pub mod commands;
pub mod config;
pub mod detect;
pub mod download;
pub mod errors;
pub mod fabric;
pub mod fetch;
pub mod gpu;
pub mod import;
pub mod install;
pub mod launcher;
pub mod local_source;
pub mod log;
pub mod manifest;
pub mod plan;
pub mod prefs;
pub mod process;
pub mod quality;
pub mod release_tool;
pub mod scan;
pub mod state;
pub mod updater;

use commands::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(feature = "local-source")]
    let local_dir = match crate::local_source::resolve_startup() {
        Ok(dir) => dir,
        Err(err) => {
            eprintln!("{err}");
            None
        }
    };
    #[cfg(not(feature = "local-source"))]
    let local_dir = None;
    let builder = tauri::Builder::default().manage(AppState::with_local_source(local_dir));

    #[cfg(feature = "updater")]
    let builder = builder.plugin(tauri_plugin_updater::Builder::new().build());

    builder
        .invoke_handler(tauri::generate_handler![
            commands::get_manifest,
            commands::detect,
            commands::scan_hardware,
            commands::plan,
            commands::install,
            commands::cancel,
            commands::repair,
            commands::list_installed,
            commands::uninstall,
            commands::list_snapshots,
            commands::rollback,
            commands::set_game_dir,
            commands::export_log,
            commands::reveal_path,
            commands::copy_diagnostics,
            commands::detect_launcher,
            commands::open_launcher,
            commands::finish,
            commands::detect_legacy,
            commands::confirm_import,
            commands::get_prefs,
            commands::set_tour_done,
            commands::get_config,
            commands::set_config,
            commands::set_config_keys,
            commands::set_quality,
            commands::get_local_source,
            commands::pick_local_source,
            commands::set_local_source,
            commands::check_update,
            commands::download_and_install,
            commands::snapshot_now,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
