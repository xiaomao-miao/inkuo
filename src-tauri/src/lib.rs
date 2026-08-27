//! inkuo - AI Document Editor
//!
//! Rust backend core module handling:
//! - Document parsing and serialization
//! - Diff engine
//! - AI provider adapters
//! - File system operations
//! - Agent tool calling

pub mod cloud;
pub mod backup;
pub mod document;
pub mod diff;
pub mod error;
pub mod feature_toggles;
pub mod fs_utils;
pub mod runtime_state;
pub mod runtime;
pub mod security;
pub mod settings_state;
pub mod app_handle;
pub mod frontend_diag;
pub mod commands;
mod ai;
mod ai_config;
mod commands_stream;
mod commands_agent;
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
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Minimum Windows build we will even try to launch on. WebView2 itself
/// only ships runtime support down to Win10 1507 / build 10240. We use
/// that as the floor; the installer enforces the same via
/// `MinimumWindowsVersion`. (Previously we used 17763, but `GetVersionExW`
/// is unreliable: on Win10/11 hosts where the .exe is missing a manifest,
/// Windows runs the binary under a legacy compat shim that reports build
/// 9200 regardless of the real OS, which produced false "unsupported OS"
/// failures for normal users.)
const MIN_WINDOWS_BUILD: u32 = 10_240;

/// Return a per-user directory outside the source tree for runtime data.
/// `dirs::data_local_dir` maps to `%LOCALAPPDATA%` on Windows and
/// `$XDG_DATA_HOME` (usually `~/.local/share`) on Linux.
pub(crate) fn app_data_dir() -> std::path::PathBuf {
    dirs::data_local_dir()
        .or_else(dirs::data_dir)
        .unwrap_or_else(std::env::temp_dir)
        .join("com.inkuo.app")
}

/// Path of the on-disk startup log. Always written regardless of the
/// build profile so a release build that crashes silently still leaves
/// evidence behind for the user to copy back.
///
/// Layout: `%LOCALAPPDATA%\com.inkuo.app\startup.log` (Tauri's
/// default per-app data dir on Windows).
fn startup_log_path() -> std::path::PathBuf {
    app_data_dir().join("startup.log")
}

/// Append a single timestamped line to the startup log. We use
/// `OpenOptions::append` so each launch adds to the file rather than
/// truncating it — keeping the previous crash context visible.
///
/// Failures are intentionally swallowed: writing the log must never
/// crash the host process. If disk is full or the dir is read-only we
/// still want the application to try to start.
fn log_startup(line: impl AsRef<str>) {
    use std::io::Write;
    let path = startup_log_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let pid = std::process::id();
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(f, "[ts={timestamp} pid={pid}] {}", line.as_ref());
    }
}

/// Read the OSVERSIONINFOEXW-style build number on Windows.
///
/// Uses `ntdll!RtlGetVersion` rather than `kernel32!GetVersionExW` because
/// `GetVersionExW` is subject to the application-compatibility shim:
/// without an embedded application manifest, Windows lies to the caller
/// and reports build 9200 (Win8) even on a real Win10/11 host. This was
/// the root cause of our "PREFLIGHT FAILED: build 9200" false negative on
/// otherwise healthy systems.
///
/// `RtlGetVersion` is an internal ntdll export that is exempt from the
/// shim — it returns the kernel's view of the OS, which is what we want.
#[cfg(windows)]
fn windows_build_number() -> Option<u32> {
    use std::ffi::c_void;
    #[repr(C)]
    struct OsVersionInfoEx {
        os_version_info_size: u32,
        major_version: u32,
        minor_version: u32,
        build_number: u32,
        platform_id: u32,
        version: [u16; 128],
        service_pack_major: u16,
        service_pack_minor: u16,
        suite_mask: u16,
        product_type: u8,
        reserved: u8,
    }
    impl OsVersionInfoEx {
        const fn zeroed() -> Self {
            Self {
                os_version_info_size: 0,
                major_version: 0,
                minor_version: 0,
                build_number: 0,
                platform_id: 0,
                version: [0u16; 128],
                service_pack_major: 0,
                service_pack_minor: 0,
                suite_mask: 0,
                product_type: 0,
                reserved: 0,
            }
        }
    }
    extern "system" {
        // RtlGetVersion is exported from ntdll.dll and is exempt from the
        // application compatibility shim. Signature matches the public
        // MSDN documentation: returns an NTSTATUS (0 on success).
        fn RtlGetVersion(lp_version_information: *mut c_void) -> i32;
    }
    let mut info = OsVersionInfoEx::zeroed();
    info.os_version_info_size = std::mem::size_of::<OsVersionInfoEx>() as u32;
    // SAFETY: pointer is to a stack-allocated, properly-sized struct with
    // `os_version_info_size` already set. RtlGetVersion only reads.
    let status = unsafe { RtlGetVersion(&mut info as *mut _ as *mut c_void) };
    if status != 0 {
        None
    } else {
        Some(info.build_number)
    }
}

#[cfg(not(windows))]
fn windows_build_number() -> Option<u32> {
    None
}

/// Quick sanity check before we hand control to `tauri::Builder`.
///
/// Returns `Ok(())` when the host is acceptable, `Err(reason)` with a
/// human-readable explanation when it is not. We deliberately keep this
/// dependency-free so it runs even if every later Tauri init step fails.
fn preflight_os_check() -> Result<(), String> {
    #[cfg(windows)]
    {
        match windows_build_number() {
            None => {
                // GetVersionExW failed. Don't silently fall through;
                // refuse to launch because we have no way to know whether
                // WebView2 will start. The installer already enforces
                // the same constraint via `MinimumWindowsVersion`, so
                // reaching this branch in practice would mean someone
                // shipped us an unsupported host (e.g. server SKU with
                // an exotic manifest).
                return Err(format!(
                    "Could not determine Windows build number. \
                     InkUO requires Windows 10 (build {MIN_WINDOWS_BUILD}+) \
                     or Windows 11."
                ));
            }
            Some(build) if build < MIN_WINDOWS_BUILD => Err(format!(
                "InkUO requires Windows 10 (build {MIN_WINDOWS_BUILD}) or later. \
                 Detected Windows build {build}. \
                 Please upgrade your operating system and reinstall."
            )),
            Some(_) => Ok(()),
        }
    }
    #[cfg(not(windows))]
    {
        Ok(())
    }
}

// ── Logging ────────────────────────────────────────────────────────────────────

fn setup_logging() {
    // Write tracing output both to stdout (useful in `cargo run` /
    // `pnpm tauri dev`) and to a rolling file under
    // `%LOCALAPPDATA%\com.inkuo.app\inkuo.log`. The file path is
    // logged once at startup so a support engineer can ask the user
    // to paste it without guessing.
    let log_path = startup_log_path().with_file_name("inkuo.log");
    if let Some(parent) = log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let stdout_layer = tracing_subscriber::fmt::layer()
        .with_target(true)
        .with_writer(std::io::stdout);
    // Two layers stacked on the registry: stdout + (optional) file.
    // We build the file layer first because `Option<L>` is itself a
    // valid Layer, so the registry stays homogeneous regardless of
    // whether the log file could be opened.
    let file_layer = match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        Ok(f) => Some(
            tracing_subscriber::fmt::layer()
                .with_target(true)
                .with_ansi(false)
                .with_writer(std::sync::Mutex::new(f)),
        ),
        Err(e) => {
            eprintln!("warning: failed to open tracing log file {log_path:?}: {e}");
            None
        }
    };
    /// Reads `RUST_LOG` env var to control log levels. Defaults to a
    /// noisy-but-actionable profile: warnings from third-party crates,
    /// info from inkuo crates. Set `RUST_LOG=trace` to see everything.
    fn console_env_filter() -> EnvFilter {
        EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("warn,inkuo_lib=info"))
    }
    let subscriber = tracing_subscriber::registry()
        .with(console_env_filter())
        .with(stdout_layer)
        .with(file_layer);
    // `try_init` instead of `init` so a test harness that already
    // installed a global subscriber doesn't poison us; we still get
    // stdout + (maybe) file in normal launches.
    let _ = subscriber.try_init();
    log_startup(format!(
        "logging initialized; per-launch startup log = {}, \
         rolling tracing log = {}",
        startup_log_path().display(),
        log_path.display()
    ));

    // Panic hook: log every panic to both stderr AND the startup log
    // so a release-mode user without a console can still find the
    // cause after the fact.
    panic::set_hook(Box::new(move |info| {
        let loc = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_else(|| "<unknown>".to_string());
        let msg = if let Some(s) = info.payload().downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "<non-string panic payload>".to_string()
        };
        eprintln!("Application panic at {loc}: {msg}");
        log_startup(format!("PANIC at {loc}: {msg}"));
    }));
}

/// Synchronously hydrate the in-process `CloudClient` from the persisted
/// settings cache. Runs on the setup task (not in a `spawn`) so the very
/// first chat request after launch can already see the logged-in account.
///
/// Returns `Ok(())` when there is nothing to do (no persisted account) and
/// `Err` only when reading the settings cache fails — in which case the
/// caller is expected to log and continue, because the frontend can still
/// push a freshly-logged-in account via `cloud_login` / `cloud_register`.

// ── Cloud client hydration ──────────────────────────────────────────────────────

fn hydrate_cloud_client_from_settings(
    app_handle: &tauri::AppHandle,
) -> Result<(), String> {
    let settings = commands::get_settings_cached().map_err(|e| e.to_string())?;
    if let Some(account) = settings.cloud.account.clone() {
        let cloud = app_handle.state::<cloud::CloudClient>();
        let user_id = account.user_id.clone();
        let expires_at = account.access_expires_at;
        // `set_account` is async; we block on the runtime here. Tauri's
        // async runtime is multi-threaded so this is fine — we just hand
        // off to the executor and wait.
        tauri::async_runtime::block_on(async move {
            cloud.set_account(Some(account)).await;
        });
        tracing::info!(
            user_id = %user_id,
            expires_at = %expires_at,
            "startup hydrate: CloudClient restored from persisted settings"
        );
    } else {
        tracing::info!(
            "startup hydrate: no persisted cloud account; \
             user must log in before cloud-mode chat will work"
        );
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    setup_logging();

    tracing::info!("Starting inkuo v{}", env!("CARGO_PKG_VERSION"));

    // Refuse to launch on host OSes we cannot serve. This catches the
    // path where a user manually copies the .exe onto Win7/8/8.1,
    // bypassing the MSI `MinimumWindowsVersion` guard. We bail BEFORE
    // tauri::Builder so we never spin up WebView2 on an unsupported host
    // (which would otherwise surface as a confusing crash dialog).
    if let Err(reason) = preflight_os_check() {
        log_startup(format!("PREFLIGHT FAILED: {reason}"));
        tracing::error!("startup preflight failed: {reason}");
        eprintln!("\n[inkuo] {reason}\n");
        return;
    }
    log_startup(format!(
        "preflight passed (host Windows build = {:?}); entering tauri::Builder",
        windows_build_number()
    ));

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

            // === FRONTEND DIAGNOSTIC BRIDGE ===
            //
            // Release builds hide the WebView2 DevTools behind a right-click
            // menu, and when the renderer fails silently the user can't see
            // any console output at all. The init-script + window-rebuild
            // ceremony now lives in `crate::frontend_diag`; the call site
            // here is intentionally tiny so the rest of `run()` stays
            // readable.

            crate::frontend_diag::rebuild_main_webview_with_diag(&app);

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
            //
            // We hydrate synchronously (blocking the setup task) rather than
            // firing-and-forgetting in a `spawn` because the previous async
            // shape had a race: a chat request that arrived before the hydrate
            // task finished would surface "not logged in" and force a manual
            // re-login on every cold start. Blocking here is fine — reading
            // `settings.json` is a sub-millisecond operation once the cache
            // is warm (and the cache is populated by `read_settings_from_disk`
            // before this point on the first launch).
            let hydrate_app_handle = app.handle().clone();
            if let Err(e) = hydrate_cloud_client_from_settings(&hydrate_app_handle) {
                tracing::warn!(
                    "startup hydrate: cloud account not restored ({}); \
                     the frontend can still push one via cloud_login / cloud_register",
                    e
                );
            }

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
            commands_stream::ai_ask_stream,
            commands_stream::ai_ask_cancel,
            commands_agent::ai_agent_stream,
            commands_agent::ai_agent_cancel,
            commands_agent::ai_agent_resume,
            commands_agent::get_available_tools,
            commands_agent::plugins::plugin_create_package,
            commands_agent::plugins::plugin_import,
            commands_agent::plugins::plugin_list,
            commands_agent::plugins::plugin_set_enabled,
            commands_agent::plugins::plugin_remove,
            commands_agent::plugins::plugin_export,
            commands::get_settings,
            commands::save_settings,
            commands::test_api_config,
            commands::test_image_gen_config,
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
            commands::inspect_dropped_paths,
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
            commands::read_file_for_viewer,
            commands::read_snapshot_file_cmd,
            commands_cloud::cloud_register,
            commands_cloud::cloud_login,
            commands_cloud::cloud_logout,
            commands_cloud::cloud_fetch_models,
            commands_cloud::cloud_fetch_account,
            commands_cloud::cloud_persist_account,
            commands::logging::frontend_log,
        ])
        .run(tauri::generate_context!())
        .expect("error while running inkuo application");
}
