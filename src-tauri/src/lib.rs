//! inkuo - Local-First AI Document Editor
//!
//! Rust backend core module handling:
//! - Document parsing and serialization
//! - Diff engine
//! - AI provider adapters
//! - File system operations
//! - Agent tool calling

mod backup;
mod document;
mod diff;
mod ai;
mod ai_config;
mod commands;
mod commands_stream;
mod commands_agent;
mod streaming;
mod openai_stream;
mod file_watcher;
mod inline_complete;
mod office;
pub mod agent;
pub mod knowledge;

pub use document::*;
pub use diff::*;
pub use ai::*;
use std::panic;

fn setup_logging() {
    // Simple logging setup - write to stdout
    // In production, would use tracing-appender with proper guard handling
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_target(true)
        .init();

    // Set up panic hook to log panics
    panic::set_hook(Box::new(move |info| {
        eprintln!("Application panic: {:?}", info);
    }));
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    setup_logging();

    tracing::info!("Starting inkuo v{}", env!("CARGO_PKG_VERSION"));

    tauri::Builder::default()
        .manage(commands::AppState::default())
        .manage(file_watcher::FileWatcherState::new())
        .setup(|app| {
            tauri::async_runtime::spawn(async {
                crate::backup::init_backup_cleanup_task();
            });

            // Load the shared workspace-snapshot store from disk once at
            // startup. All subsequent reads/writes happen in-memory; the
            // disk file is updated atomically whenever a snapshot changes.
            commands::init_workspace_snapshots(&app.handle());

            // Register shared vector store cache so both KB commands and agent tools
            // use the same cache, avoiding WAL lock conflicts.
            knowledge::commands::register_shared_stores();
            Ok(())
        })
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_os::init())
        .invoke_handler(tauri::generate_handler![
            commands::read_document,
            commands::write_document,
            commands::list_directory,
            commands::search_directory,
            commands::compute_diff,
            commands::ai_edit,
            commands_stream::ai_chat_stream,
            commands_stream::ai_edit_stream,
            commands_stream::ai_stream_cancel,
            commands_agent::ai_agent_stream,
            commands_agent::ai_agent_cancel,
            commands_agent::get_available_tools,
            commands::get_settings,
            commands::save_settings,
            commands::test_api_config,
            commands::watch_directory,
            commands::unwatch_directory,
            inline_complete::ai_inline_complete,
            inline_complete::ai_inline_complete_cancel,
            inline_complete::get_inline_completion_state,
            commands::read_office_file,
            commands::write_office_file,
            commands::read_office_text,
            commands::write_office_text,
            commands::read_xlsx_structured,
            commands::write_xlsx_structured,
            knowledge::commands::knowledge_build,
            knowledge::commands::knowledge_search,
            knowledge::commands::knowledge_status,
            knowledge::commands::knowledge_update,
            knowledge::commands::knowledge_clear,
            knowledge::commands::knowledge_add_members,
            knowledge::commands::knowledge_remove_members,
            knowledge::commands::knowledge_get_members,
            knowledge::commands::check_available_models,
            knowledge::commands::download_model_files,
            commands::create_file_entry,
            commands::rename_path,
            commands::delete_path,
            commands::copy_path,
            commands::move_path,
            commands::path_exists,
            commands::open_with_default_app,
            commands::reveal_in_file_manager,
            commands::create_new_window,
            commands::save_workspace_snapshot,
            commands::load_workspace_snapshot,
        ])
        .run(tauri::generate_context!())
        .expect("error while running inkuo application");
}
