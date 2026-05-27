//! inkuo - Local-First AI Document Editor
//! 
//! Rust backend core module handling:
//! - Document parsing and serialization
//! - Diff engine
//! - AI provider adapters
//! - RAG indexing
//! - File system operations
//! - Agent tool calling

mod document;
mod diff;
mod ai;
mod rag;
mod commands;
mod commands_stream;
mod commands_agent;
mod streaming;
mod openai_stream;
pub mod agent;

pub use document::*;
pub use diff::*;
pub use ai::*;
pub use rag::*;

use std::panic;

fn setup_logging() {
    // Simple logging setup - write to stdout
    // In production, would use tracing-appender with proper guard handling
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
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
    
    // Initialize background tasks
    commands::init_backup_cleanup_task();
    
    tauri::Builder::default()
        .manage(commands::AppState::default())
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running inkuo application");
}
