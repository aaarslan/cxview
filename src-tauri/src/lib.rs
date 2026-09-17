#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// The modules are public so the fixture walkthrough can be exercised end to end from
// `src-tauri/tests/`; the renderer still reaches them only through the Tauri commands below.
pub mod commands;
pub mod comparison;
pub mod db;
pub mod error;
pub mod importer;
pub mod models;
pub mod patching;
pub mod provider;
pub mod repository;
pub mod validation;

use std::fs;

use commands::AppState;
use db::Database;
use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let storage = app.path().app_data_dir().unwrap_or_else(|_| {
                app.path()
                    .app_local_data_dir()
                    .expect("CXView app data directory is unavailable")
            });
            fs::create_dir_all(&storage).expect("CXView cannot create its app data directory");
            let db_path = storage.join("cxview.sqlite3");
            let database = Database::open(&db_path).expect("CXView database migration failed");
            app.manage(AppState {
                db: std::sync::Mutex::new(database),
                write_lock: std::sync::Mutex::new(()),
                storage,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_app_snapshot,
            commands::import_report,
            commands::bind_repository,
            commands::list_findings,
            commands::read_raw_locator,
            commands::get_investigation,
            commands::create_task,
            commands::get_task,
            commands::create_manual_proposal,
            commands::import_proposal,
            commands::review_patch,
            commands::apply_reviewed_patch,
            commands::undo_cxview_patch,
            commands::export_task,
            commands::investigation_prompt,
            commands::discover_validation,
            commands::run_validation,
            commands::validation_history,
            commands::save_finding_note,
            commands::save_ui_state,
            commands::provider_diagnostic,
            commands::run_codex_proposal,
            commands::compare_report_pair,
            commands::storage_info,
            commands::delete_workspace_data
        ])
        .run(tauri::generate_context!())
        .expect("error while running CXView");
}
