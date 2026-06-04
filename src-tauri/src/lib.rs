//! inkuo - Local-First AI Document Editor
//!
//! Rust backend core module handling:
//! - Document parsing and serialization
//! - Diff engine
//! - AI provider adapters
//! - RAG indexing
//! - File system operations
//! - Agent tool calling

mod backup;
mod document;
mod diff;
mod ai;
mod rag;
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
pub use rag::*;
use tauri::Manager;

use std::panic;

fn setup_logging() {
    // Simple logging setup - write to stdout
    // In production, would use tracing-appender with proper guard handling
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_target(true)
        .init();

    // Set up panic hook to log panics
    let default_panic = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        eprintln!("Application panic: {:?}", info);
        default_panic(info);
    }));
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    setup_logging();

    tracing::info!("Starting inkuo v{}", env!("CARGO_PKG_VERSION"));

    // Spawn a background thread with Tokio runtime for backup cleanup
    std::thread::spawn(|| {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Failed to create Tokio runtime");

        rt.block_on(async {
            crate::backup::init_backup_cleanup_task();
        });
    });

    tauri::Builder::default()
        .setup(|app| {
            // Configure RAG index persistence path
            if let Some(app_data) = app.path().app_data_dir().ok() {
                let state = app.state::<commands::AppState>();
                let rag_index = state.rag_index.clone();
                let app_data_clone = app_data.clone();
                // Spawn a blocking task to configure persistence
                std::thread::spawn(move || {
                    let rt = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .expect("Failed to create Tokio runtime for RAG setup");
                    rt.block_on(async {
                        let mut index = rag_index.write().await;
                        index.set_persistence_path(app_data_clone.clone());
                        tracing::info!("RAG index configured with persistence path: {:?}", app_data_clone);
                    });
                });
            }
            Ok(())
        })
        .manage(commands::AppState::default())
        .manage(file_watcher::FileWatcherState::new())
        .manage(knowledge::commands::KnowledgeState::default())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_os::init())
        .invoke_handler(tauri::generate_handler![
            commands::read_document,
            commands::write_document,
            commands::list_directory,
            commands::compute_diff,
            commands::ai_edit,
            commands_stream::ai_chat_stream,
            commands_stream::ai_edit_stream,
            commands_stream::ai_stream_cancel,
            commands_agent::ai_agent_stream,
            commands_agent::ai_agent_cancel,
            commands_agent::get_available_tools,
            commands::search_knowledge_base,
            commands::get_settings,
            commands::save_settings,
            commands::index_workspace,
            commands::test_ai_connection,
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
            knowledge::commands::knowledge_build,
            knowledge::commands::knowledge_search,
            knowledge::commands::knowledge_status,
            knowledge::commands::knowledge_update,
            knowledge::commands::knowledge_clear,
            knowledge::commands::check_available_models,
            knowledge::commands::download_model_files,
        ])
        .run(tauri::generate_context!())
        .expect("error while running inkuo application");
}
