//! inkuo - AI Document Editor
//!
//! Rust backend core module handling:
//! - Document parsing and serialization
//! - Diff engine
//! - AI provider adapters
//! - File system operations
//! - Agent tool calling

mod cloud;
mod backup;
mod document;
mod diff;
mod feature_toggles;
mod runtime_state;
mod app_handle;
mod ai;
mod ai_config;
mod commands;
mod commands_stream;
mod commands_agent;
mod commands_plan;
mod commands_cloud;
mod streaming;
mod openai_stream;
mod file_watcher;
mod inline_complete;
mod office;
mod snapshots;
pub mod agent;
pub mod knowledge;

pub use document::*;
pub use diff::*;
pub use ai::*;
use std::panic;
use tauri::Manager;

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

    // Build ONE CloudClient, then `.clone()` it before handing each
    // copy to Tauri's `.manage`. Because `CloudClient` internally
    // holds the mutable account behind an `Arc<Mutex<...>>`, the
    // two managed copies share state: a write through one is visible
    // to the other. This is what lets the startup hydrate (which
    // targets the `tauri::State<CloudClient>` instance) be picked up
    // by `AppState.cloud` when the agent chat path resolves an
    // AIConfig — without this link they diverge and the agent path
    // surfaces a confusing "not logged in" error in cloud mode.
    let cloud = cloud::CloudClient::new();

    tauri::Builder::default()
        .manage(commands::AppState::new(cloud.clone()))
        .manage(cloud)
        .manage(file_watcher::FileWatcherState::new())
        .setup(|app| {
            // Stash the live AppHandle in a process-global registry so
            // consumers constructed before this hook fires (e.g.
            // WebSearchTool's placeholder path) can lazy-fetch it
            // instead of permanently erroring with "AppHandle missing".
            app_handle::set_app_handle(app.handle().clone());

            tauri::async_runtime::spawn(async {
                crate::backup::init_backup_cleanup_task();
            });

            // Load the shared workspace-snapshot store from disk once at
            // startup. All subsequent reads/writes happen in-memory; the
            // disk file is updated atomically whenever a snapshot changes.
            commands::init_workspace_snapshots(&app.handle());

            // Background cleanup for file-content snapshots: scans
            // `~/.inkuo/snapshots/` every few minutes for orphan directories
            // (e.g. half-deleted snapshots) and prunes them.
            snapshots::init_snapshot_cleanup_task();

            // Register shared vector store cache so both KB commands and agent tools
            // use the same cache, avoiding WAL lock conflicts.
            knowledge::commands::register_shared_stores();

            // Re-hydrate the in-process CloudClient from the persisted
            // settings so the first chat / web_search call after a restart
            // doesn't have to wait for the frontend to re-push the account.
            // Without this, the user has to log in twice on every cold start:
            // once for the persisted settings, and once again because the
            // Rust-side CloudClient starts empty.
            let app_handle_for_hydrate = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let settings = match commands::get_settings_cached() {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::warn!(
                            "startup hydrate: could not read settings cache ({}); \
                             cloud account will stay uninitialised until the \
                             frontend pushes it",
                            e
                        );
                        return;
                    }
                };
                if let Some(account) = settings.cloud.account {
                    let cloud = app_handle_for_hydrate.state::<cloud::CloudClient>();
                    cloud.set_account(Some(account.clone())).await;
                    tracing::info!(
                        user_id = %account.user_id,
                        "startup hydrate: CloudClient restored from persisted settings"
                    );
                }
            });

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
            commands_stream::ai_edit_stream,
            commands_stream::ai_stream_cancel,
            commands_agent::ai_agent_stream,
            commands_agent::ai_agent_cancel,
            commands_agent::get_available_tools,
            commands_agent::answer_ask_user,
            commands_plan::plan_save,
            commands_plan::plan_read,
            commands_plan::plan_delete,
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
            commands::create_workspace_snapshot_cmd,
            commands::list_workspace_snapshots_cmd,
            commands::delete_workspace_snapshot_cmd,
            commands::preview_workspace_snapshot_restore_cmd,
            commands::restore_workspace_snapshot_cmd,
            commands::collect_workspace_empty_dirs_cmd,
            commands::collect_workspace_files_cmd,
            commands::read_file_bytes_cmd,
            commands::read_snapshot_file_cmd,
            commands_cloud::cloud_register,
            commands_cloud::cloud_login,
            commands_cloud::cloud_logout,
            commands_cloud::cloud_fetch_models,
            commands_cloud::cloud_fetch_account,
            commands_cloud::cloud_persist_account,
        ])
        .run(tauri::generate_context!())
        .expect("error while running inkuo application");
}
