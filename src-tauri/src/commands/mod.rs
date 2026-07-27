//! Tauri commands module
//!
//! Exposes Rust backend functionality to the frontend via IPC.
//!
//! ## Module layout
//!
//! | File | Responsibility |
//! |------|----------------|
//! | `mod.rs` (~1 070 lines) | Document / AI / settings / office / workspace-snapshot command handlers + the shared `AppCommandError` alias. |
//! | `context_menu.rs` (~300 lines) | File-tree context-menu handlers (`create_file_entry`, `rename_path`, `delete_path`, `copy_path`, `move_path`, `path_exists`, `open_with_default_app`, `reveal_in_file_manager`, `create_new_window`) plus their request/response types. Re-exported through `mod.rs` so `lib.rs` can keep registering `commands::create_file_entry` etc. |
//!
//! The cancellation helpers (`is_stream_cancelled`, `StreamCancelGuard`,
//! etc.) used to live here. They moved to `crate::runtime::cancel` so that
//! `agent_loop.rs` and other downstream modules can reach them without
//! importing this file (which would otherwise form a cycle once snapshots
//! or settings helpers are also pulled out).

use std::io::Write;
use std::sync::Arc;

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

pub(crate) mod context_menu;

/// Alias used by snapshot-state and the snapshot Tauri commands so
/// existing call sites can stay short and intent-revealing.
pub(crate) use tauri::AppHandle as TauriAppHandle;

pub use crate::runtime::cancel::{
    clear_stream_cancelled, is_stream_cancelled, mark_stream_cancelled, StreamCancelGuard,
};

/// Legacy alias kept so existing `AppCommandError::Foo(...)` call sites
/// continue to compile. The real enum lives in `crate::error::AppError` —
/// see that module for the `From` impls that bridge the sub-module errors.
pub type AppCommandError = crate::error::AppError;

// Re-export the context-menu handlers so `lib.rs` can keep registering
// `commands::create_file_entry` / `commands::rename_path` / etc. without
// learning the sub-module path. The implementations live in `context_menu.rs`.
pub use context_menu::*;

// Re-export the `Settings` schema + cache helpers from `settings_state` so
// that the existing `crate::commands::Settings` /
// `crate::commands::get_settings_cached` / etc. call sites keep compiling
// unchanged while the canonical implementation lives in its own module.
pub use crate::settings_state::{
    atomic_write_settings, clear_settings_cache_account, get_chunk_overlap, get_chunk_size,
    get_embedding_model, get_settings_cached, get_settings_path, get_web_search_settings,
    patch_settings_cache_account, read_settings_from_disk, settings_cache_populated,
    update_settings_cache, ApiConfig, CloudSettings, Settings, SnapshotSettings,
    WebSearchProviderConfig, WebSearchSettings,
};

pub mod logging;
pub(crate) mod snapshot_state;

pub use logging::{frontend_log, frontend_log_path, FrontendLogPayload};

// Re-export the snapshot state + helpers so existing
// `crate::commands::WORKSPACE_SNAPSHOTS` /
// `crate::commands::SnapshotEntry` /
// `crate::commands::init_workspace_snapshots` call sites continue to
// compile unchanged while the canonical implementation lives in
// `snapshot_state`.
pub(crate) use snapshot_state::{
    evict_lru_if_needed, flush_snapshots_to_disk, init_workspace_snapshots,
    resolve_snapshots_path, touch_snapshot, SnapshotEntry, MAX_WORKSPACE_SNAPSHOTS,
    WORKSPACE_SNAPSHOTS, WORKSPACE_SNAPSHOTS_PATH,
};

use crate::backup::{create_backup_path, get_backup_dir, request_backup_cleanup};
use crate::file_watcher::{emit_file_change, FileChangeEvent};
use crate::office;
use crate::{ai, ai_config::{self, AITestResult, TestApiConfigRequest}, diff, document, file_watcher};
use tauri_plugin_opener::OpenerExt;

pub struct AppState {
    /// Async resolver that produces a fresh `AIConfig` on demand.
    /// Replaces the previous `Arc<RwLock<AIConfig>>` cache, which
    /// silently went stale when the cloud access token rotated.
    pub ai_config: Arc<ai_config::AIConfigResolver>,
    /// Handle to the process-global CloudClient. Cloned from the
    /// same instance that lib.rs passes to `.manage(CloudClient)`,
    /// so the two managed copies share the inner account state via
    /// the existing `Arc<Mutex<...>>` inside CloudClient. Both the
    /// startup hydrate and the agent-path `build_input_ai_config_async`
    /// therefore see the same logged-in account.
    pub cloud: crate::cloud::CloudClient,
}

impl AppState {
    /// Constructor used by `lib.rs::run` so the same `CloudClient`
    /// can both be `tauri::manage()`-ed and stored on `AppState`.
    /// Tauri's manager receives a clone; the inner `Arc<Mutex<...>>`
    /// in `CloudClient` keeps both copies in sync.
    pub fn new(cloud: crate::cloud::CloudClient) -> Self {
        let cloud_for_resolver = cloud.clone();
        Self {
            ai_config: Arc::new(ai_config::AIConfigResolver::new(cloud_for_resolver)),
            cloud,
        }
    }
}

impl Default for AppState {
    /// Used by unit tests that don't share state with a running Tauri
    /// app. The production path builds via `AppState::new(...)` in
    /// `lib.rs::run` so the two managed CloudClient copies share
    /// their inner account state; `default()` here produces a fresh,
    /// standalone CloudClient, which is exactly the semantics tests
    /// expect (no leakage between test cases).
    fn default() -> Self {
        Self::new(crate::cloud::CloudClient::new())
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ReadDocumentResult {
    pub document: document::Document,
    pub content: String,
    pub mtime: i64,  // Unix timestamp in milliseconds
}

/// Payload returned by `read_file_for_viewer` for binary file rendering.
#[derive(Debug, Serialize, Deserialize)]
pub struct ViewerFilePayload {
    pub path: String,
    pub size: u64,
    /// Best-effort MIME type derived from the file extension
    /// (e.g. `image/png`, `application/pdf`).
    pub mime: String,
    /// Coarse-grained `file_kind` classification (matches `FileEntry.file_kind`).
    pub file_kind: String,
    /// Raw file bytes encoded as base64. Frontend decodes via
    /// `Uint8Array.from(atob(...), c => c.charCodeAt(0))` or uses the
    /// `data:` URL directly for `<img>` / `<video>` / `<audio>`.
    pub data_base64: String,
}

#[tauri::command]
pub async fn read_document(path: String) -> Result<ReadDocumentResult, AppCommandError> {
    tracing::info!("Reading document: {}", path);

    let content = std::fs::read_to_string(&path)
        .map_err(|e| AppCommandError::ReadDocument(e.to_string()))?;

    let mtime = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .map(|t| t.duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as i64)
        .unwrap_or(0);

    let doc = document::Document::from_markdown(&content, &path)
        .map_err(|e| AppCommandError::ParseDocument(e.to_string()))?;

    Ok(ReadDocumentResult { document: doc, content, mtime })
}

#[tauri::command]
pub async fn write_document(
    path: String,
    content: String,
    app_handle: AppHandle,
) -> Result<(), AppCommandError> {
    tracing::info!("Writing document: {}", path);

    if std::path::Path::new(&path).exists() {
        let backup_dir = get_backup_dir();
        std::fs::create_dir_all(&backup_dir)
            .map_err(|e| AppCommandError::CreateBackupDirectory(e.to_string()))?;

        let backup_path = create_backup_path(&path);
        std::fs::copy(&path, &backup_path)
            .map_err(|e| AppCommandError::CreateBackup(e.to_string()))?;

        request_backup_cleanup();
    }

    // Atomic write: write to a temp file, then rename (POSIX guarantees atomicity).
    // The temp filename includes a process-unique suffix so concurrent writes to the
    // same path don't race on the temp file (which would cause one writer's bytes to
    // be clobbered mid-flush). On any error after the temp file is created, we remove
    // it so we don't leave stale `.tmp` siblings behind.
    let path_obj = std::path::Path::new(&path);
    let unique_suffix = format!(
        "{}.{}.tmp",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    let temp_path = path_obj.with_extension(
        path_obj
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| format!("{}.{}", e, unique_suffix))
            .unwrap_or_else(|| unique_suffix.clone())
    );

    if let Err(error) = std::fs::write(&temp_path, &content) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(AppCommandError::WriteDocument(error.to_string()));
    }

    if let Err(error) = std::fs::rename(&temp_path, &path) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(AppCommandError::WriteDocument(error.to_string()));
    }

    // Inotify inside the same process is not always reliable for in-process
    // writes (atomic rename, kernel delivery timing). Emit explicitly so the
    // file tree always refreshes.
    emit_file_change(&app_handle, FileChangeEvent::Modified { path });

    Ok(())
}

#[derive(Debug, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "FileEntry.ts")]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    /// Coarse-grained classification driving the editor + icon mapping.
    /// Possible values: `word`, `excel`, `image`, `pdf`, `code`, `config`,
    /// `data`, `markdown`, `text`, `binary`, `audio`, `video`, `archive`.
    /// The frontend keeps `is_markdown` for backwards compatibility — it is
    /// `true` iff `file_kind == "markdown" || file_kind == "text"`.
    pub file_kind: String,
    /// Kept for backwards compatibility — true if the file is a markdown
    /// document. Prefer `file_kind`.
    pub is_markdown: bool,
}

/// Classify a file by extension into a coarse-grained `file_kind` string.
///
/// Mirrors the TypeScript `detectFileKind` in `src/types/index.ts` so the
/// frontend and backend agree on the routing decision.
pub fn classify_file_kind(path: &std::path::Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    match ext.as_str() {
        "docx" | "doc" => "word",
        "xlsx" | "xls" | "xlsm" => "excel",
        "md" | "markdown" => "markdown",
        "pdf" => "pdf",
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico" | "avif" | "tif"
        | "tiff" | "svg" => "image",
        "json" | "jsonc" | "json5" | "yaml" | "yml" | "toml" | "ini" | "xml" | "env" => {
            "config"
        }
        "csv" | "tsv" => "data",
        "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "rs" | "py" | "go" | "java"
        | "kt" | "swift" | "c" | "h" | "cpp" | "cc" | "cxx" | "hpp" | "hxx" | "rb"
        | "php" | "lua" | "sh" | "bash" | "zsh" | "sql" | "graphql" | "gql" | "html"
        | "htm" | "css" | "scss" | "sass" | "less" | "vue" | "svelte" | "astro"
        | "dart" | "r" | "jl" | "pl" | "scala" | "clj" | "ex" | "exs" | "erl"
        | "hs" | "ml" | "fs" | "fsx" | "mdx" => "code",
        "txt" | "log" | "text" => "text",
        "mp3" | "wav" | "flac" | "aac" | "ogg" | "m4a" => "audio",
        "mp4" | "mov" | "mkv" | "webm" | "avi" | "m4v" => "video",
        "zip" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "7z" | "rar" | "jar" | "war" => {
            "archive"
        }
        _ => "binary",
    }
}

#[tauri::command]
pub async fn list_directory(path: String) -> Result<Vec<FileEntry>, AppCommandError> {
    tracing::trace!("Listing directory: {}", path);

    let entries = std::fs::read_dir(&path)
        .map_err(|e| AppCommandError::ListDirectory(e.to_string()))?;

    let mut files: Vec<FileEntry> = entries
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();

            if name.starts_with('.') {
                return None;
            }

            let is_dir = path.is_dir();
            let file_kind = classify_file_kind(&path);
            let is_markdown = matches!(file_kind, "markdown" | "text");

            Some(FileEntry {
                name,
                path: path.to_string_lossy().to_string(),
                is_dir,
                file_kind: file_kind.to_string(),
                is_markdown,
            })
        })
        .collect();

    files.sort_by(|a, b| {
        match (a.is_dir, b.is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        }
    });

    Ok(files)
}

#[tauri::command]
pub async fn search_directory(
    path: String,
    query: String,
) -> Result<Vec<FileEntry>, AppCommandError> {
    tracing::info!("Searching directory: {} for '{}'", path, query);

    if query.is_empty() {
        return Ok(vec![]);
    }

    use crate::fs_utils::{walk_dir_safe, WalkEntry};

    let query_lower = query.to_lowercase();
    let mut results: Vec<FileEntry> = Vec::new();

    walk_dir_safe(
        std::path::Path::new(&path),
        |entry: WalkEntry| {
            if results.len() >= 100 {
                return;
            }
            let path = entry.path;
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            if name.is_empty() {
                return;
            }

            if !name.to_lowercase().contains(&query_lower) {
                return;
            }

            let file_kind = classify_file_kind(&path);
            let is_markdown = matches!(file_kind, "markdown" | "text");

            results.push(FileEntry {
                name,
                path: path.to_string_lossy().to_string(),
                is_dir: entry.is_dir,
                file_kind: file_kind.to_string(),
                is_markdown,
            });
        },
    );

    // Sort results: directories first, then by relevance (shorter paths first).
    // Depth is measured in `Path` components so we count correctly on both
    // POSIX (`/`) and Windows (`\`) paths.
    results.sort_by(|a, b| {
        match (a.is_dir, b.is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => {
                let a_depth = std::path::Path::new(&a.path).components().count();
                let b_depth = std::path::Path::new(&b.path).components().count();
                if a_depth != b_depth {
                    a_depth.cmp(&b_depth)
                } else {
                    a.name.to_lowercase().cmp(&b.name.to_lowercase())
                }
            }
        }
    });

    Ok(results)
}

#[tauri::command]
pub async fn watch_directory(
    path: String,
    state: State<'_, file_watcher::FileWatcherState>,
    app_handle: AppHandle,
) -> Result<(), AppCommandError> {
    tracing::info!("Starting file watcher for: {}", path);
    state
        .watch(std::path::PathBuf::from(path), app_handle)
        .map_err(|error| AppCommandError::WatchDirectory(error.to_string()))
}

#[tauri::command]
pub async fn unwatch_directory(
    path: Option<String>,
    state: State<'_, file_watcher::FileWatcherState>,
) -> Result<(), AppCommandError> {
    // Frontend passes the path it previously watched. Only honour an
    // explicit stop when it matches the currently-watched path; a
    // mismatched stop request is silently ignored so a stale cleanup from
    // a previous workspace can't kill the active watcher. With no `path`
    // argument we stop unconditionally (preserves the original behaviour
    // for callers that don't know what they were watching).
    match path {
        Some(requested) => {
            let active = state
                .watched_path()
                .map(|p| p.to_string_lossy().to_string());
            if active.as_deref() == Some(requested.as_str()) {
                state.stop();
            } else {
                tracing::debug!(
                    "Ignoring unwatch_directory for {} (active watcher is {:?})",
                    requested,
                    active
                );
            }
        }
        None => state.stop(),
    }
    Ok(())
}

#[tauri::command]
pub async fn compute_diff(old_text: String, new_text: String) -> Result<diff::DiffResult, AppCommandError> {
    tracing::info!("Computing diff");
    Ok(diff::compute_diff(&old_text, &new_text))
}

#[tauri::command]
pub async fn ai_edit(
    instruction: String,
    original_text: String,
    scope: String,
    context: Vec<ai::ContextItem>,
    state: State<'_, AppState>,
) -> Result<ai::AIEditResponse, AppCommandError> {
    tracing::info!("AI edit request: {}", instruction);

    let config = state
        .ai_config
        .resolve()
        .await
        .map_err(|e| AppCommandError::AIConfig(format!("resolve AI config: {}", e)))?;
    let adapter = ai::AIProviderAdapter::new(config);

    let edit_scope = match scope.as_str() {
        "selection" => ai::EditScope::Selection,
        "paragraph" => ai::EditScope::Paragraph,
        "section" => ai::EditScope::Section,
        "document" => ai::EditScope::Document,
        _ => ai::EditScope::Selection,
    };

    let request = ai::AIEditRequest {
        instruction,
        original_text,
        scope: edit_scope,
        context,
    };

    adapter.edit(request)
        .await
        .map_err(|e| AppCommandError::AIEdit(e.to_string()))
}

/// Settings schema + cache helpers live in `crate::settings_state`. They
/// are re-exported above (see `pub use crate::settings_state::*` near the
/// top of this file) so that downstream callers can keep using
/// `crate::commands::Settings` / `crate::commands::get_settings_cached` /
/// etc. without changing every call site during the staged split.

/// Settings persistence helpers (`atomic_write_settings`,
/// `read_settings_from_disk`) and the cache helpers above are now defined
/// in `crate::settings_state`. The remaining IPC commands below operate
/// exclusively on them.

#[tauri::command]
pub async fn get_settings() -> Result<Settings, AppCommandError> {
    read_settings_from_disk()
}

#[tauri::command]
pub async fn save_settings(settings: Settings, _state: State<'_, AppState>) -> Result<(), AppCommandError> {
    let path = get_settings_path();

    let content = serde_json::to_string_pretty(&settings)
        .map_err(|e| AppCommandError::SerializeSettings(e.to_string()))?;

    atomic_write_settings(&path, &content)?;

    // Refresh the in-memory settings cache so subsequent calls to
    // `get_embedding_model()`, `get_chunk_size()`, `get_chunk_overlap()`,
    // and `get_settings_cached()` see the freshly written values without
    // re-reading the file (and without requiring a process restart, which
    // was the previous behaviour).
    crate::commands::update_settings_cache(settings.clone());

    // The AIConfigResolver reads from the cache on each call, so we
    // don't need to push a new AIConfig into a shared cell here. The
    // previous code rebuilt and stored an AIConfig in
    // `state.ai_config`, but that snapshot silently went stale when
    // the cloud access token rotated — see the Step-3 change in
    // `ai_config.rs::AIConfigResolver` for the rationale.

    Ok(())
}

pub type TestResult = AITestResult;

#[derive(Debug, serde::Deserialize)]
pub struct TestImageGenRequest {
    pub provider_id: String,
    pub api_key: Option<String>,
    pub base_url: String,
    pub model: String,
    /// Tencent Cloud `SecretId`. Optional because ollama / openai
    /// providers don't need it.
    #[serde(default)]
    pub secret_id: Option<String>,
    /// Tencent Cloud `SecretKey`. Optional for the same reason as
    /// `secret_id`.
    #[serde(default)]
    pub secret_key: Option<String>,
    /// Region hint for cloud providers (e.g. `"ap-guangzhou"` for
    /// Tencent). Defaults to the provider's compile-time default.
    #[serde(default)]
    pub region: Option<String>,
}

#[tauri::command]
pub async fn test_api_config(
    request: TestApiConfigRequest,
) -> Result<TestResult, AppCommandError> {
    tracing::info!("Testing API config: {} ({})", request.model, request.provider);
    ai_config::test_ai_connection_impl(
        request.api_key.as_deref(),
        &request.base_url,
        &request.model,
    )
    .await
    .map_err(|error| AppCommandError::TestAIConnection(error.to_string()))
}

#[tauri::command]
pub async fn test_image_gen_config(
    request: TestImageGenRequest,
) -> Result<TestResult, AppCommandError> {
    tracing::info!(
        "Testing image gen config: provider={} model={} base_url={}",
        request.provider_id,
        request.model,
        request.base_url
    );
    ai_config::test_image_gen_provider_impl(
        &request.provider_id,
        request.api_key.as_deref(),
        &request.base_url,
        &request.model,
        request.secret_id.as_deref(),
        request.secret_key.as_deref(),
        request.region.as_deref(),
    )
    .await
    .map_err(|error| AppCommandError::TestAIConnection(error.to_string()))
}

#[tauri::command]
pub async fn read_office_file(path: String) -> Result<Vec<u8>, AppCommandError> {
    tracing::info!("Reading office file: {}", path);
    std::fs::read(&path).map_err(|e| AppCommandError::ReadOfficeFile(e.to_string()))
}

#[tauri::command]
pub async fn write_office_file(
    path: String,
    data: Vec<u8>,
    app_handle: AppHandle,
) -> Result<(), AppCommandError> {
    tracing::info!("Writing office file: {}", path);
    std::fs::write(&path, &data).map_err(|e| AppCommandError::WriteOfficeFile(e.to_string()))?;
    emit_file_change(&app_handle, FileChangeEvent::Modified { path });
    Ok(())
}

#[tauri::command]
pub async fn read_office_text(path: String) -> Result<OfficeFileResult, AppCommandError> {
    tracing::info!("Reading office file as text: {}", path);

    let result = office::read_office_file(std::path::Path::new(&path))
        .map_err(|e| AppCommandError::ReadOfficeFile(e.to_string()))?;

    let (file_type, text_content) = result;

    match file_type {
        office::OfficeFileType::Word(doc) => Ok(OfficeFileResult {
            file_type: "docx".to_string(),
            text_content,
            json_content: serde_json::to_string(&doc)
                .map_err(|e| AppCommandError::SerializeOfficeDocument(e.to_string()))?,
            sheet_names: None,
        }),
        office::OfficeFileType::Excel(workbook) => Ok(OfficeFileResult {
            file_type: "xlsx".to_string(),
            text_content,
            json_content: serde_json::to_string(&workbook)
                .map_err(|e| AppCommandError::SerializeOfficeDocument(e.to_string()))?,
            sheet_names: Some(workbook.sheets.iter().map(|s| s.name.clone()).collect()),
        }),
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OfficeFileResult {
    pub file_type: String,
    pub text_content: String,
    pub json_content: String,
    pub sheet_names: Option<Vec<String>>,
}

#[tauri::command]
pub async fn read_xlsx_structured(path: String) -> Result<office::XlsxWorkbook, AppCommandError> {
    tracing::info!("Reading xlsx as structured workbook: {}", path);
    let bytes = std::fs::read(&path).map_err(|e| AppCommandError::ReadOfficeFile(e.to_string()))?;
    office::read_xlsx_structured(&bytes).map_err(|e| AppCommandError::ReadOfficeFile(e.to_string()))
}

#[tauri::command]
pub async fn write_xlsx_structured(
    path: String,
    workbook: office::XlsxWorkbook,
    app_handle: AppHandle,
) -> Result<(), AppCommandError> {
    tracing::info!("Writing structured xlsx workbook: {}", path);
    let path_obj = std::path::Path::new(&path);
    office::create_xlsx_workbook(&workbook, path_obj)
        .map_err(|e| AppCommandError::WriteOfficeFile(e.to_string()))?;
    emit_file_change(&app_handle, FileChangeEvent::Modified { path });
    Ok(())
}

#[tauri::command]
pub async fn write_office_text(
    path: String,
    json_content: String,
    format: String,
    app_handle: AppHandle,
) -> Result<(), AppCommandError> {
    tracing::info!("Writing office file: {} ({})", path, format);

    let path_obj = std::path::Path::new(&path);

    office::write_office_file(path_obj, &json_content)
        .map_err(|e| AppCommandError::WriteOfficeFile(e.to_string()))?;
    emit_file_change(&app_handle, FileChangeEvent::Modified { path });
    Ok(())
}

// ============================================================================
// File-tree context-menu commands (create / rename / delete / copy / move,
// reveal in file manager, open with default app).
//
// All mutating commands emit `FileChangeEvent::Created` / `Deleted` so the
// frontend's 500ms poll + the PollWatcher in `file_watcher.rs` refresh the
// tree without any extra plumbing.


/// Save (insert or overwrite) a workspace snapshot under `path`. Triggers an
/// atomic flush to disk. Evicts the least-recently-touched entry when the
/// in-memory cache exceeds `MAX_WORKSPACE_SNAPSHOTS`.
#[tauri::command]
pub async fn save_workspace_snapshot(
    app_handle: TauriAppHandle,
    path: String,
    snapshot: serde_json::Value,
) -> Result<(), AppCommandError> {
    {
        let mut map = WORKSPACE_SNAPSHOTS.lock();
        map.insert(
            path.clone(),
            SnapshotEntry {
                value: snapshot,
                last_touched_at: std::time::Instant::now(),
            },
        );
    }
    // If we just pushed the map over the cap, drop the LRU entry before we
    // serialise so the on-disk file matches the in-memory state.
    evict_lru_if_needed();
    flush_snapshots_to_disk(&app_handle)
}

/// Load the snapshot for `path`, or `None` if none has been saved. Reading
/// counts as a "touch" so the entry's LRU timestamp is refreshed.
#[tauri::command]
pub async fn load_workspace_snapshot(
    path: String,
) -> Result<Option<serde_json::Value>, AppCommandError> {
    let value = {
        let snapshots = WORKSPACE_SNAPSHOTS.lock();
        snapshots.get(&path).map(|entry| {
            // Refresh the touch timestamp under the same lock so we never
            // race with eviction. The value is cloned before the lock drops.
            entry.value.clone()
        })
    };
    if value.is_some() {
        touch_snapshot(&path);
    }
    Ok(value)
}

// =============================================================================
// Workspace file-content snapshots
// =============================================================================
//
// These commands back the "Workspace Snapshots" UI panel: a manual +
// AI-triggered safety net that stores whole-file copies of every tracked
// document and supports preview + restore of an entire workspace.
//
// The on-disk layout is documented in `snapshots.rs`.  All public commands
// here delegate to that module.

/// Enumerate every file (recursively) under `workspace_path`, skipping
/// derived directories (node_modules, target, etc.), and return each file's
/// raw bytes base64-encoded together with its relative path.  Used by the
/// frontend when creating a workspace snapshot.
#[tauri::command]
pub async fn collect_workspace_files_cmd(
    workspace_path: String,
) -> Result<Vec<WorkspaceFilePayload>, AppCommandError> {
    let path = std::path::PathBuf::from(&workspace_path);
    if !path.is_absolute() {
        return Err(AppCommandError::InvalidWorkspacePath(workspace_path));
    }

    let skip_dirs = [
        "node_modules",
        ".git",
        "target",
        "dist",
        "build",
        ".next",
        ".cache",
        ".turbo",
        "out",
    ];

    fn walk(
        dir: &std::path::Path,
        prefix: &str,
        skip_dirs: &[&str],
        out: &mut Vec<WorkspaceFilePayload>,
    ) -> Result<(), AppCommandError> {
        let entries = std::fs::read_dir(dir).map_err(|e| AppCommandError::ReadDocument(e.to_string()))?;
        for entry in entries.flatten() {
            let file_type = match entry.file_type() {
                Ok(t) => t,
                Err(_) => continue,
            };
            let name = entry.file_name().to_string_lossy().to_string();
            let entry_path = entry.path();
            if file_type.is_dir() {
                if skip_dirs.contains(&name.as_str()) {
                    continue;
                }
                let new_prefix = if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{}/{}", prefix, name)
                };
                walk(&entry_path, &new_prefix, skip_dirs, out)?;
            } else if file_type.is_file() {
                let rel = if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{}/{}", prefix, name)
                };
                let bytes = match std::fs::read(&entry_path) {
                    Ok(b) => b,
                    Err(_) => continue, // skip unreadable files
                };
                out.push(WorkspaceFilePayload {
                    rel_path: rel,
                    content_base64: base64_encode(&bytes),
                });
            }
        }
        Ok(())
    }

    let mut out: Vec<WorkspaceFilePayload> = Vec::new();
    walk(&path, "", &skip_dirs, &mut out)?;
    Ok(out)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceFilePayload {
    pub rel_path: String,
    pub content_base64: String,
}

/// Read a single file's raw bytes (base64-encoded) so the frontend can pass
/// them to the snapshot command.  Mirrors `read_document` but works for
/// binary files too.
#[tauri::command]
pub async fn read_file_bytes_cmd(
    path: String,
) -> Result<String, AppCommandError> {
    let bytes = std::fs::read(&path)
        .map_err(|e| AppCommandError::ReadDocument(e.to_string()))?;
    Ok(base64_encode(&bytes))
}

/// Read a file's bytes and return them base64-encoded, with a best-effort
/// MIME guess derived from the file extension. Used by the workspace
/// viewers for `image`, `pdf`, `audio`, `video`, and other binary
/// formats. Caps the read at 200 MB to avoid runaway memory usage.
#[tauri::command]
pub async fn read_file_for_viewer(
    path: String,
) -> Result<ViewerFilePayload, AppCommandError> {
    const MAX_BYTES: u64 = 200 * 1024 * 1024;

    let metadata = std::fs::metadata(&path)
        .map_err(|e| AppCommandError::ReadDocument(e.to_string()))?;

    if metadata.len() > MAX_BYTES {
        return Err(AppCommandError::ReadDocument(format!(
            "file too large for in-app viewer ({} > {} bytes)",
            metadata.len(),
            MAX_BYTES
        )));
    }

    let bytes = std::fs::read(&path)
        .map_err(|e| AppCommandError::ReadDocument(e.to_string()))?;
    let mime = mime_for_path(std::path::Path::new(&path));
    let file_kind = classify_file_kind(std::path::Path::new(&path))
        .to_string();

    Ok(ViewerFilePayload {
        path,
        size: bytes.len() as u64,
        mime,
        file_kind,
        data_base64: base64_encode(&bytes),
    })
}

/// Best-effort MIME mapping for the in-app viewers. Falls back to
/// `application/octet-stream` for unknown extensions.
pub fn mime_for_path(path: &std::path::Path) -> String {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let mime = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "avif" => "image/avif",
        "tif" | "tiff" => "image/tiff",
        "svg" => "image/svg+xml",
        "pdf" => "application/pdf",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "flac" => "audio/flac",
        "aac" => "audio/aac",
        "ogg" => "audio/ogg",
        "m4a" => "audio/mp4",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        "mkv" => "video/x-matroska",
        "webm" => "video/webm",
        "avi" => "video/x-msvideo",
        "m4v" => "video/x-m4v",
        _ => "application/octet-stream",
    };
    mime.to_string()
}

/// Read a single file's *text* content from a workspace snapshot.  Returns
/// `Ok(None)` if the file doesn't exist in the snapshot or is binary.
#[tauri::command]
pub async fn read_snapshot_file_cmd(
    workspace_path: String,
    snapshot_id: String,
    rel_path: String,
) -> Result<Option<String>, AppCommandError> {
    let path = crate::snapshots::snapshot_dir(&workspace_path, &snapshot_id)
        .join("files")
        .join(&rel_path);
    if !path.exists() {
        return Ok(None);
    }
    match std::fs::read_to_string(&path) {
        Ok(text) => Ok(Some(text)),
        Err(_) => Ok(None), // binary file
    }
}

fn base64_encode(input: &[u8]) -> String {
    const ALPHA: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let mut i = 0;
    while i + 3 <= input.len() {
        let n = ((input[i] as u32) << 16)
            | ((input[i + 1] as u32) << 8)
            | (input[i + 2] as u32);
        out.push(ALPHA[((n >> 18) & 0x3F) as usize] as char);
        out.push(ALPHA[((n >> 12) & 0x3F) as usize] as char);
        out.push(ALPHA[((n >> 6) & 0x3F) as usize] as char);
        out.push(ALPHA[(n & 0x3F) as usize] as char);
        i += 3;
    }
    let rem = input.len() - i;
    if rem == 1 {
        let n = (input[i] as u32) << 16;
        out.push(ALPHA[((n >> 18) & 0x3F) as usize] as char);
        out.push(ALPHA[((n >> 12) & 0x3F) as usize] as char);
        out.push('=');
        out.push('=');
    } else if rem == 2 {
        let n = ((input[i] as u32) << 16) | ((input[i + 1] as u32) << 8);
        out.push(ALPHA[((n >> 18) & 0x3F) as usize] as char);
        out.push(ALPHA[((n >> 12) & 0x3F) as usize] as char);
        out.push(ALPHA[((n >> 6) & 0x3F) as usize] as char);
        out.push('=');
    }
    out
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSnapshotArgs {
    pub workspace_path: String,
    pub label: Option<String>,
    #[serde(default = "default_trigger")]
    pub trigger: String,
    /// Vector of `(rel_path, base64_bytes)` tuples, sent from the frontend so
    /// the backend doesn't need a separate "enumerate workspace files"
    /// command.  Base64 keeps the IPC payload JSON-safe.
    pub files: Vec<SnapshotFilePayload>,
    /// Relative paths of empty directories that exist in the workspace at
    /// capture time.  Needed for a full-state restore — directories that
    /// became empty after the snapshot (because all their files were deleted)
    /// must be re-created to match the snapshot.  Optional for backwards
    /// compatibility with old snapshots created before this field existed.
    #[serde(default)]
    pub directories: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotFilePayload {
    pub rel_path: String,
    /// Raw file bytes, base64-encoded.
    pub content_base64: String,
}

fn default_trigger() -> String {
    "manual".to_string()
}

#[tauri::command]
pub async fn create_workspace_snapshot_cmd(
    args: CreateSnapshotArgs,
) -> Result<crate::snapshots::SnapshotManifest, AppCommandError> {
    let CreateSnapshotArgs {
        workspace_path,
        label,
        trigger,
        files,
        directories,
    } = args;

    let mut decoded: Vec<(String, Vec<u8>)> = Vec::with_capacity(files.len());
    for f in files {
        let bytes = base64_decode(&f.content_base64)
            .map_err(|e| AppCommandError::SnapshotWriteFailed(format!("base64 decode: {}", e)))?;
        decoded.push((f.rel_path, bytes));
    }

    crate::snapshots::create_workspace_snapshot(
        &workspace_path,
        label,
        &trigger,
        decoded,
        directories,
    )
    .map_err(|e| AppCommandError::SnapshotWriteFailed(e.to_string()))
}

#[tauri::command]
pub async fn list_workspace_snapshots_cmd(
    workspace_path: String,
) -> Result<Vec<crate::snapshots::SnapshotIndexEntry>, AppCommandError> {
    crate::snapshots::list_workspace_snapshots(&workspace_path)
        .map_err(|e| AppCommandError::SnapshotReadFailed(e.to_string()))
}

#[tauri::command]
pub async fn delete_workspace_snapshot_cmd(
    workspace_path: String,
    snapshot_id: String,
) -> Result<(), AppCommandError> {
    crate::snapshots::delete_workspace_snapshot(&workspace_path, &snapshot_id)
        .map_err(|e| AppCommandError::SnapshotWriteFailed(e.to_string()))
}

#[tauri::command]
pub async fn preview_workspace_snapshot_restore_cmd(
    workspace_path: String,
    snapshot_id: String,
) -> Result<Vec<crate::snapshots::FileDiffPreview>, AppCommandError> {
    crate::snapshots::preview_workspace_snapshot_restore(&workspace_path, &snapshot_id)
        .map_err(|e| AppCommandError::SnapshotReadFailed(e.to_string()))
}

#[tauri::command]
pub async fn restore_workspace_snapshot_cmd(
    workspace_path: String,
    snapshot_id: String,
    app_handle: TauriAppHandle,
) -> Result<crate::snapshots::RestoreResult, AppCommandError> {
    crate::snapshots::restore_workspace_snapshot(&workspace_path, &snapshot_id, &app_handle)
        .map_err(|e| AppCommandError::SnapshotWriteFailed(e.to_string()))
}

/// Collect the relative paths of every empty directory under
/// `workspace_path`, pruning heavy branches like `node_modules`, `.git`,
/// `.inkuo`.  Returned paths use forward slashes and are relative to the
/// workspace root.
#[tauri::command]
pub async fn collect_workspace_empty_dirs_cmd(
    workspace_path: String,
) -> Result<Vec<String>, AppCommandError> {
    let p = std::path::Path::new(&workspace_path);
    if !p.is_absolute() {
        return Err(AppCommandError::SnapshotReadFailed(format!(
            "workspace_path must be absolute: {workspace_path}"
        )));
    }
    let skip_dirs = vec![
        "node_modules".to_string(),
        ".git".to_string(),
        ".inkuo".to_string(),
    ];
    crate::snapshots::collect_empty_directories(p, &skip_dirs)
        .map_err(|e| AppCommandError::SnapshotReadFailed(e.to_string()))
}

/// Minimal base64 decoder so we don't pull in the `base64` crate just for
/// this.  Returns an error on invalid input.
fn base64_decode(input: &str) -> Result<Vec<u8>, String> {
    const ALPHA: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut lookup = [255u8; 256];
    for (i, &c) in ALPHA.iter().enumerate() {
        lookup[c as usize] = i as u8;
    }

    let cleaned: Vec<u8> = input.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    if cleaned.len() % 4 != 0 {
        return Err("length not a multiple of 4".into());
    }

    let mut out = Vec::with_capacity(cleaned.len() / 4 * 3);
    let chunks = cleaned.chunks_exact(4);
    let remainder = chunks.remainder();
    for chunk in chunks {
        let padding = chunk.iter().filter(|&&c| c == b'=').count();
        let mut vals = [0u8; 4];
        for (i, &c) in chunk.iter().enumerate() {
            let v = if c == b'=' { 0 } else { lookup[c as usize] };
            if c != b'=' && v == 255 {
                return Err(format!("invalid char {:?}", c as char));
            }
            vals[i] = v;
        }
        out.push((vals[0] << 2) | (vals[1] >> 4));
        if padding < 2 {
            out.push((vals[1] << 4) | (vals[2] >> 2));
        }
        if padding < 1 {
            out.push((vals[2] << 6) | vals[3]);
        }
    }
    if !remainder.is_empty() {
        let mut vals = [0u8; 4];
        for (i, &c) in remainder.iter().enumerate() {
            let v = if c == b'=' { 0 } else { lookup[c as usize] };
            if c != b'=' && v == 255 {
                return Err(format!("invalid char {:?}", c as char));
            }
            vals[i] = v;
        }
        let padding = remainder.iter().filter(|&&c| c == b'=').count();
        out.push((vals[0] << 2) | (vals[1] >> 4));
        if padding < 2 {
            out.push((vals[1] << 4) | (vals[2] >> 2));
        }
        if padding < 1 {
            out.push((vals[2] << 6) | vals[3]);
        }
    }
    Ok(out)
}

// =====================================================================
// Frontend diagnostic bridge lives in `commands/logging.rs`.
// =====================================================================

