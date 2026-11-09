//! OUTPOST — a local-first chess analysis studio.

pub mod ai;
pub mod analysis;
pub mod chess;
pub mod commands;
pub mod db;
pub mod engine;
pub mod error;
pub mod games;
pub mod import;
pub mod paths;
pub mod secrets;
pub mod selftest;
pub mod tactics;

use commands::{AppState, Jobs};
use std::sync::Arc;
use tauri::Manager;

fn init_logging() {
    use tracing_subscriber::{fmt, prelude::*, EnvFilter};
    let filter = || EnvFilter::try_from_env("OUTPOST_LOG").unwrap_or_else(|_| "warn".into());
    // Warnings and errors also go to <data>/outpost.log (truncated when it passes 1 MB),
    // so problems seen in the window can be diagnosed afterwards.
    let file = paths::Paths::resolve().ok().and_then(|p| {
        let path = p.data.join("outpost.log");
        if std::fs::metadata(&path).is_ok_and(|m| m.len() > 1 << 20) {
            let _ = std::fs::remove_file(&path);
        }
        std::fs::OpenOptions::new().create(true).append(true).open(path).ok()
    });
    let registry = tracing_subscriber::registry().with(fmt::layer().with_filter(filter()));
    match file {
        Some(file) => registry
            .with(fmt::layer().with_ansi(false).with_writer(std::sync::Mutex::new(file)).with_filter(filter()))
            .init(),
        None => registry.init(),
    }
}

pub fn run() {
    init_logging();

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(|app| {
            let paths = paths::Paths::resolve()?;
            let db = Arc::new(db::Db::open(&paths.db)?);
            match tauri::async_runtime::block_on(engine::builtin::ensure_registered(&db)) {
                Ok(Some(record)) => tracing::info!("built-in engine: {} at {}", record.name, record.path),
                Ok(None) => tracing::warn!("no built-in engine in this build; run scripts/build-engine"),
                Err(e) => tracing::warn!("built-in engine did not start: {e:#}"),
            }
            let http = import::client()?;
            app.manage(AppState { paths, db, engines: engine::EngineManager::default(), http, jobs: Jobs::default() });
            // The window starts hidden so a self-test run never appears on screen.
            if selftest::report_path().is_none() {
                if let Some(window) = app.get_webview_window("main") {
                    window.show()?;
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::settings_get,
            commands::settings_set,
            commands::job_cancel,
            commands::fen_check,
            commands::pgn_parse,
            commands::file_read_text,
            commands::file_write_text,
            commands::file_read_image,
            commands::engines_list,
            commands::engines_detect,
            commands::engine_add,
            commands::engine_remove,
            commands::engine_set_options,
            commands::engine_set_default,
            commands::engine_restart,
            commands::engine_shutdown,
            commands::analysis_start,
            commands::analysis_stop,
            commands::review_start,
            commands::review_get,
            commands::games_list,
            commands::game_get,
            commands::games_import_pgn,
            commands::game_save,
            commands::game_delete,
            commands::games_clear_history,
            commands::game_favorite,
            commands::accounts_list,
            commands::games_history,
            commands::game_import_selected,
            commands::chesscom_import,
            commands::lichess_import,
            commands::lichess_import_game,
            commands::tactics_stats,
            commands::tactics_themes,
            commands::tactic_next,
            commands::tactics_for_game,
            commands::tactic_check,
            commands::tactic_attempt,
            commands::tactics_scan,
            commands::ai_providers,
            commands::ai_status,
            commands::ai_key_set,
            commands::ai_key_delete,
            commands::ai_ask,
            commands::ai_check_moves,
            commands::chats_list,
            commands::chat_messages,
            commands::chat_delete,
            selftest::selftest_info,
            selftest::selftest_log,
            selftest::selftest_done,
            selftest::log_frontend,
        ])
        .build(tauri::generate_context!())
        .expect("error while building OUTPOST");

    app.run(|handle, event| {
        if let tauri::RunEvent::Exit = event {
            // Engines get `stop` + `quit`, and are killed if they ignore it.
            if let Some(state) = handle.try_state::<AppState>() {
                tauri::async_runtime::block_on(state.engines.shutdown_all());
            }
        }
    });
}
