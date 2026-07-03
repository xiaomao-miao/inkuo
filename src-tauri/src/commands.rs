//! Tauri commands module
//!
//! Exposes Rust backend functionality to the frontend via IPC.

use std::collections::HashSet;
use std::sync::Arc;

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use thiserror::Error;

use crate::backup::{create_backup_path, get_backup_dir, request_backup_cleanup};
use crate::file_watcher::{emit_file_change, FileChangeEvent};
use crate::office;
use crate::{ai, ai_config::{self, AITestResult, AIProviderKind, TestApiConfigRequest}, diff, document, file_watcher};
use tauri_plugin_opener::OpenerExt;

pub static STREAM_CANCELLED: Lazy<Mutex<HashSet<String>>> = Lazy::new(|| Mutex::new(HashSet::new()));

/// True if the given session has been marked cancelled. Use this rather
/// than reaching for `STREAM_CANCELLED.lock()` directly at every call site
/// so the lock acquisition pattern stays uniform (and so we have a single
/// place to swap in a more sophisticated cancellation queue later).
pub fn is_stream_cancelled(session_id: &str) -> bool {
    STREAM_CANCELLED.lock().contains(session_id)
}

/// Mark a session as cancelled. Cheaply idempotent — the existing key
/// stays put if already present.
pub fn mark_stream_cancelled(session_id: &str) {
    STREAM_CANCELLED.lock().insert(session_id.to_string());
}

/// Drop the cancellation flag for `session_id`, returning whether one was
/// actually removed. Callers usually want to suppress the regular "done"
/// event when this returns `true`.
pub fn clear_stream_cancelled(session_id: &str) -> bool {
    STREAM_CANCELLED.lock().remove(session_id)
}

/// RAII guard that clears the cancellation flag for `session_id` on drop.
/// Use [`scoped_stream_cleanup`] to obtain one.
///
/// Why this exists: cancellation flags are a global `HashSet<String>`, and
/// every code path that calls `mark_stream_cancelled` MUST call
/// `clear_stream_cancelled` (or leave the flag in a way that prevents the
/// next request from being misclassified as cancelled). A `?` early-return,
/// a panic, or a refactor that adds a new error branch all silently leak
/// the flag, which then blocks every subsequent stream for that session_id.
/// The guard makes the cleanup unconditional.
///
/// Note: this guard does NOT mark the session as cancelled on creation.
/// Cancellation is set by the `ai_*_cancel` command, not by the start of a
/// stream; this guard only guarantees cleanup on the stream side.
pub struct StreamCancelGuard {
    session_id: String,
    cleared: bool,
}

impl StreamCancelGuard {
    /// Create a guard that will clear the cancellation flag for
    /// `session_id` on drop. The flag is NOT set by this constructor.
    pub fn new(session_id: &str) -> Self {
        Self {
            session_id: session_id.to_string(),
            cleared: false,
        }
    }

    /// Explicitly clear the flag without waiting for drop, returning
    /// `true` if the flag was still set when this was called. Consumes
    /// the guard so its `Drop` does not run a second clear.
    pub fn clear(mut self) -> bool {
        if !self.cleared {
            self.cleared = true;
            clear_stream_cancelled(&self.session_id)
        } else {
            false
        }
    }
}

impl Drop for StreamCancelGuard {
    fn drop(&mut self) {
        if !self.cleared {
            let _ = clear_stream_cancelled(&self.session_id);
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Error)]
pub enum AppCommandError {
    #[error("Failed to read document: {0}")]
    ReadDocument(String),
    #[error("Failed to parse document: {0}")]
    ParseDocument(String),
    #[error("Failed to create backup directory: {0}")]
    CreateBackupDirectory(String),
    #[error("Failed to create backup: {0}")]
    CreateBackup(String),
    #[error("Failed to write document: {0}")]
    WriteDocument(String),
    #[error("Failed to list directory: {0}")]
    ListDirectory(String),
    #[error("Failed to watch directory: {0}")]
    WatchDirectory(String),
    #[error("AI edit failed: {0}")]
    AIEdit(String),
    #[error("AI connection test failed: {0}")]
    TestAIConnection(String),
    #[error("Failed to read settings: {0}")]
    ReadSettings(String),
    #[error("Failed to parse settings: {0}")]
    ParseSettings(String),
    #[error("Failed to create config directory: {0}")]
    CreateConfigDirectory(String),
    #[error("Failed to serialize settings: {0}")]
    SerializeSettings(String),
    #[error("Failed to write settings: {0}")]
    WriteSettings(String),
    #[error("Invalid AI configuration: {0}")]
    AIConfig(String),
    #[error("Failed to read office file: {0}")]
    ReadOfficeFile(String),
    #[error("Failed to serialize office document: {0}")]
    SerializeOfficeDocument(String),
    #[error("Failed to write office file: {0}")]
    WriteOfficeFile(String),
    #[error("Invalid config path")]
    InvalidConfigPath,
    #[error("Failed to create file or folder: {0}")]
    CreateEntry(String),
    #[error("Failed to rename path: {0}")]
    RenamePath(String),
    #[error("Failed to delete path: {0}")]
    DeletePath(String),
    #[error("Failed to copy path: {0}")]
    CopyPath(String),
    #[error("Failed to move path: {0}")]
    MovePath(String),
    #[error("Failed to open path with default app: {0}")]
    OpenWithDefaultApp(String),
    #[error("Failed to reveal path in file manager: {0}")]
    RevealInFileManager(String),
    #[error("Target already exists")]
    TargetExists,
    #[error("Failed to read workspace snapshots: {0}")]
    ReadWorkspaceSnapshots(String),
    #[error("Failed to write workspace snapshots: {0}")]
    WriteWorkspaceSnapshots(String),
    #[error("Failed to parse workspace snapshots: {0}")]
    ParseWorkspaceSnapshots(String),
    #[error("Invalid workspace snapshots path: {0}")]
    InvalidWorkspaceSnapshotsPath(String),
    #[error("Snapshot not found: {0}")]
    SnapshotNotFound(String),
    #[error("Snapshot manifest corrupt: {0}")]
    SnapshotCorrupt(String),
    #[error("Workspace path is not absolute: {0}")]
    InvalidWorkspacePath(String),
    #[error("Snapshot write failed: {0}")]
    SnapshotWriteFailed(String),
    #[error("Snapshot read failed: {0}")]
    SnapshotReadFailed(String),
}

pub struct AppState {
    pub ai_config: Arc<tokio::sync::RwLock<ai::AIConfig>>,
}

impl Default for AppState {
    fn default() -> Self {
        let settings = read_settings_from_disk().unwrap_or_else(|error| {
            // Logged here because `Default` cannot propagate errors; we still
            // get a Settings value (defaults) so the app can start.
            tracing::warn!("Falling back to default settings at startup: {}", error);
            Settings::default()
        });

        let ai_config = ai_config::build_settings_ai_config(&settings).unwrap_or_else(|error| {
            // The persisted settings may reference a cloud provider without
            // an API key (the user hasn't filled in their credentials yet).
            // Falling back to the local Ollama default lets the app boot and
            // surface a clear error when the user actually tries to chat.
            tracing::warn!(
                "Could not build AI config from settings ({}); falling back to Ollama",
                error
            );
            ai::AIConfig::default()
        });

        Self {
            ai_config: Arc::new(tokio::sync::RwLock::new(ai_config)),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ReadDocumentResult {
    pub document: document::Document,
    pub content: String,
    pub mtime: i64,  // Unix timestamp in milliseconds
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

#[derive(Debug, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub is_markdown: bool,
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
            let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            let is_markdown = matches!(extension, "md" | "markdown" | "txt");

            Some(FileEntry {
                name,
                path: path.to_string_lossy().to_string(),
                is_dir,
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

    let query_lower = query.to_lowercase();
    let mut results: Vec<FileEntry> = Vec::new();
    let mut visited: std::collections::HashSet<std::path::PathBuf> = std::collections::HashSet::new();

    fn walk_dir(
        dir: &std::path::Path,
        query: &str,
        results: &mut Vec<FileEntry>,
        max_results: usize,
        visited: &mut std::collections::HashSet<std::path::PathBuf>,
    ) {
        if results.len() >= max_results {
            return;
        }

        // Canonicalise so symlink cycles within the workspace (e.g. a dir
        // pointing back to an ancestor) get caught the second time we try
        // to descend into them. Failing canonicalise (e.g. dangling link)
        // means the path isn't readable anyway, so we skip it.
        let canonical = match std::fs::canonicalize(dir) {
            Ok(p) => p,
            Err(_) => return,
        };
        if !visited.insert(canonical) {
            return;
        }

        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };

        for entry in entries.filter_map(|e| e.ok()) {
            if results.len() >= max_results {
                break;
            }

            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();

            if name.starts_with('.') {
                continue;
            }

            let file_type = match entry.file_type() {
                Ok(t) => t,
                Err(_) => continue,
            };
            // Skip symlinks — they can create cycles or escape the
            // workspace. We deliberately don't follow them during search;
            // users who want to search them can `read_link` themselves.
            if file_type.is_symlink() {
                continue;
            }

            let is_dir = file_type.is_dir();

            if name.to_lowercase().contains(query) {
                let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                let is_markdown = matches!(extension, "md" | "markdown" | "txt");

                results.push(FileEntry {
                    name,
                    path: path.to_string_lossy().to_string(),
                    is_dir,
                    is_markdown,
                });
            }

            if is_dir {
                walk_dir(&path, query, results, max_results, visited);
            }
        }
    }

    walk_dir(std::path::Path::new(&path), &query_lower, &mut results, 100, &mut visited);

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

    let config = state.ai_config.read().await.clone();
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiConfig {
    pub id: String,
    pub name: String,
    pub provider: AIProviderKind,
    pub base_url: String,
    pub api_key: Option<String>,
    pub model: String,
    pub is_default: bool,
    pub enabled: bool,
    pub temperature: f32,
    pub max_tokens: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SnapshotSettings {
    #[serde(default = "default_snapshot_max_count")]
    pub max_count: usize,
    #[serde(default = "default_snapshot_auto_baseline")]
    pub auto_baseline: bool,
}

fn default_snapshot_max_count() -> usize {
    50
}

fn default_snapshot_auto_baseline() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub theme: String,
    pub accent_color: String,
    pub editor_font_size: u32,
    pub editor_font_family: String,
    pub editor_word_wrap: bool,
    pub editor_line_numbers: bool,
    pub api_configs: Vec<ApiConfig>,
    pub active_api_config_id: Option<String>,
    pub embedding_model: String,
    pub embedding_model_path: Option<String>,
    pub chunk_size: usize,
    pub chunk_overlap: usize,
    #[serde(default)]
    pub snapshot: SnapshotSettings,
}

impl Default for Settings {
    fn default() -> Self {
        let default_api_config = ApiConfig {
            id: uuid::Uuid::new_v4().to_string(),
            name: "DeepSeek V3".to_string(),
            provider: AIProviderKind::DeepSeek,
            base_url: "https://api.deepseek.com".to_string(),
            api_key: None,
            model: "deepseek-chat".to_string(),
            is_default: true,
            enabled: true,
            temperature: 0.7,
            max_tokens: Some(4096),
        };

        Self {
            theme: "cursor-dark".to_string(),
            accent_color: "#7C5CFF".to_string(),
            editor_font_size: 14,
            editor_font_family: "JetBrains Mono, monospace".to_string(),
            editor_word_wrap: true,
            editor_line_numbers: true,
            api_configs: vec![default_api_config.clone()],
            active_api_config_id: Some(default_api_config.id),
            embedding_model: "BAAI/bge-small-zh-v1.5".to_string(),
            embedding_model_path: None,
            chunk_size: 500,
            chunk_overlap: 50,
            snapshot: SnapshotSettings::default(),
        }
    }
}

/// Cached settings to avoid repeated disk reads.
/// Updated whenever save_settings is called.
static SETTINGS_CACHE: Lazy<Mutex<Option<Settings>>> = Lazy::new(|| Mutex::new(None));

/// Get cached settings, reading from disk only when cache is empty.
pub fn get_settings_cached() -> Result<Settings, AppCommandError> {
    // Fast path: cache hit. We do the read inside the lock so that the
    // "warm cache" check is consistent with the eventual write to the
    // cache. Without this, two callers racing on the empty cache could
    // both see `None`, both read from disk, and the slower one would
    // overwrite the fresh result with its own copy. The result is still
    // correct (same JSON content most of the time), just wasteful.
    {
        let guard = SETTINGS_CACHE.lock();
        if let Some(ref settings) = *guard {
            return Ok(settings.clone());
        }
    }
    let settings = read_settings_from_disk()?;
    let mut guard = SETTINGS_CACHE.lock();
    // Re-check: another thread may have populated the cache while we
    // were reading the disk. Theirs is at least as fresh as ours (they
    // read the same file under the same lock-guarded path), and writing
    // ours on top would discard any mutations made after their read
    // (e.g. `update_settings_cache`).
    if let Some(existing) = guard.clone() {
        return Ok(existing);
    }
    *guard = Some(settings.clone());
    Ok(settings)
}

/// Update the settings cache when settings are saved.
pub fn update_settings_cache(settings: Settings) {
    let mut guard = SETTINGS_CACHE.lock();
    *guard = Some(settings);
}

/// Get embedding model name from settings
pub fn get_embedding_model() -> String {
    get_settings_cached()
        .map(|s| s.embedding_model)
        .unwrap_or_else(|_| "BAAI/bge-small-zh-v1.5".to_string())
}

/// Get chunk size from settings
pub fn get_chunk_size() -> usize {
    get_settings_cached()
        .map(|s| s.chunk_size)
        .unwrap_or(500)
}

/// Get chunk overlap from settings
pub fn get_chunk_overlap() -> usize {
    get_settings_cached()
        .map(|s| s.chunk_overlap)
        .unwrap_or(50)
}

fn get_settings_path() -> std::path::PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("inkuo")
        .join("settings.json")
}

pub fn read_settings_from_disk() -> Result<Settings, AppCommandError> {
    let path = get_settings_path();

    if !path.exists() {
        return Ok(Settings::default());
    }

    let content = std::fs::read_to_string(&path)
        .map_err(|e| AppCommandError::ReadSettings(e.to_string()))?;

    match serde_json::from_str::<Settings>(&content) {
        Ok(settings) => Ok(settings),
        Err(e) => {
            tracing::warn!("Failed to parse settings ({}), trying merged format", e);

            let value: serde_json::Value = serde_json::from_str(&content)
                .map_err(|e| AppCommandError::ParseSettings(format!("settings JSON: {}", e)))?;

            if let Some(object) = value.as_object() {
                let mut merged = serde_json::to_value(Settings::default())
                    .map_err(|e| AppCommandError::SerializeSettings(format!("default settings: {}", e)))?;

                if let Some(merged_object) = merged.as_object_mut() {
                    for (key, value) in object {
                        merged_object.insert(key.clone(), value.clone());
                    }
                }

                if let Ok(settings) = serde_json::from_value::<Settings>(merged) {
                    return Ok(settings);
                }
            }

            Err(AppCommandError::ParseSettings(format!(
                "settings format is invalid and no longer supports legacy single-config fields: {}",
                e
            )))
        }
    }
}

#[tauri::command]
pub async fn get_settings() -> Result<Settings, AppCommandError> {
    read_settings_from_disk()
}

#[tauri::command]
pub async fn save_settings(settings: Settings, state: State<'_, AppState>) -> Result<(), AppCommandError> {
    let path = get_settings_path();
    let config_dir = path.parent().ok_or(AppCommandError::InvalidConfigPath)?;

    std::fs::create_dir_all(config_dir)
        .map_err(|e| AppCommandError::CreateConfigDirectory(e.to_string()))?;

    let content = serde_json::to_string_pretty(&settings)
        .map_err(|e| AppCommandError::SerializeSettings(e.to_string()))?;

    std::fs::write(&path, content)
        .map_err(|e| AppCommandError::WriteSettings(e.to_string()))?;

    // Refresh the in-memory settings cache so subsequent calls to
    // `get_embedding_model()`, `get_chunk_size()`, `get_chunk_overlap()`,
    // and `get_settings_cached()` see the freshly written values without
    // re-reading the file (and without requiring a process restart, which
    // was the previous behaviour).
    crate::commands::update_settings_cache(settings.clone());

    *state.ai_config.write().await = ai_config::build_settings_ai_config(&settings)
        .map_err(|error| AppCommandError::AIConfig(error.to_string()))?;

    Ok(())
}

pub type TestResult = AITestResult;

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
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NewEntryPayload {
    /// A regular file. `template` is the optional initial content (e.g.
    /// `# Heading` for markdown). `extension` includes the leading dot.
    File { extension: String, template: Option<String> },
    /// A directory.
    Directory,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateEntryResult {
    pub path: String,
}

#[tauri::command]
pub async fn create_file_entry(
    parent: String,
    name: String,
    payload: NewEntryPayload,
    app_handle: AppHandle,
) -> Result<CreateEntryResult, AppCommandError> {
    let trimmed_name = name.trim();
    if trimmed_name.is_empty() {
        return Err(AppCommandError::CreateEntry("名称不能为空".to_string()));
    }
    if trimmed_name.contains('/') || trimmed_name.contains('\\') {
        return Err(AppCommandError::CreateEntry("名称不能包含路径分隔符".to_string()));
    }

    let parent_path = std::path::Path::new(&parent);
    if !parent_path.exists() {
        return Err(AppCommandError::CreateEntry(format!("父目录不存在: {}", parent)));
    }
    if !parent_path.is_dir() {
        return Err(AppCommandError::CreateEntry(format!("不是目录: {}", parent)));
    }

    let target = parent_path.join(trimmed_name);
    if target.exists() {
        return Err(AppCommandError::TargetExists);
    }

    match payload {
        NewEntryPayload::Directory => {
            std::fs::create_dir_all(&target)
                .map_err(|e| AppCommandError::CreateEntry(e.to_string()))?;
        }
        NewEntryPayload::File { extension, template } => {
            let ext_clean = extension.trim_start_matches('.').to_string();
            let file_name = if ext_clean.is_empty() {
                trimmed_name.to_string()
            } else if trimmed_name.to_lowercase().ends_with(&format!(".{}", ext_clean.to_lowercase())) {
                trimmed_name.to_string()
            } else {
                format!("{}.{}", trimmed_name, ext_clean)
            };
            let final_path = parent_path.join(&file_name);
            if final_path.exists() {
                return Err(AppCommandError::TargetExists);
            }
            let content = template.unwrap_or_default();
            std::fs::write(&final_path, content)
                .map_err(|e| AppCommandError::CreateEntry(e.to_string()))?;
            let final_str = final_path.to_string_lossy().to_string();
            emit_file_change(&app_handle, FileChangeEvent::Created { path: final_str.clone() });
            return Ok(CreateEntryResult { path: final_str });
        }
    }

    let final_str = target.to_string_lossy().to_string();
    emit_file_change(&app_handle, FileChangeEvent::Created { path: final_str.clone() });
    Ok(CreateEntryResult { path: final_str })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenamePathResult {
    pub from: String,
    pub to: String,
}

#[tauri::command]
pub async fn rename_path(
    from: String,
    to: String,
    app_handle: AppHandle,
) -> Result<RenamePathResult, AppCommandError> {
    let from_path = std::path::Path::new(&from);
    if !from_path.exists() {
        return Err(AppCommandError::RenamePath(format!("源路径不存在: {}", from)));
    }
    let to_path = std::path::Path::new(&to);
    if let Some(parent) = to_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| AppCommandError::RenamePath(e.to_string()))?;
    }
    if to_path.exists() && from_path != to_path {
        return Err(AppCommandError::TargetExists);
    }
    std::fs::rename(from_path, to_path)
        .map_err(|e| AppCommandError::RenamePath(e.to_string()))?;

    // Emit both sides so caches for the old parent and new parent refresh
    // atomically.
    emit_file_change(&app_handle, FileChangeEvent::Deleted { path: from.clone() });
    emit_file_change(&app_handle, FileChangeEvent::Created { path: to.clone() });
    Ok(RenamePathResult { from, to })
}

#[tauri::command]
pub async fn delete_path(
    path: String,
    recursive: bool,
    app_handle: AppHandle,
) -> Result<(), AppCommandError> {
    let p = std::path::Path::new(&path);
    if !p.exists() {
        // Idempotent: deleting a missing path is a no-op.
        return Ok(());
    }
    let metadata = std::fs::metadata(p)
        .map_err(|e| AppCommandError::DeletePath(e.to_string()))?;
    if metadata.is_dir() {
        if !recursive {
            return Err(AppCommandError::DeletePath(
                "目录需要启用 recursive 选项".to_string(),
            ));
        }
        std::fs::remove_dir_all(p)
            .map_err(|e| AppCommandError::DeletePath(e.to_string()))?;
    } else {
        std::fs::remove_file(p)
            .map_err(|e| AppCommandError::DeletePath(e.to_string()))?;
    }
    emit_file_change(&app_handle, FileChangeEvent::Deleted { path });
    Ok(())
}

#[tauri::command]
pub async fn copy_path(
    from: String,
    to: String,
    app_handle: AppHandle,
) -> Result<(), AppCommandError> {
    let from_path = std::path::Path::new(&from);
    if !from_path.exists() {
        return Err(AppCommandError::CopyPath(format!("源路径不存在: {}", from)));
    }
    let to_path = std::path::Path::new(&to);
    if to_path.exists() {
        return Err(AppCommandError::TargetExists);
    }
    if let Some(parent) = to_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| AppCommandError::CopyPath(e.to_string()))?;
    }

    let metadata = std::fs::metadata(from_path)
        .map_err(|e| AppCommandError::CopyPath(e.to_string()))?;
    if metadata.is_dir() {
        copy_dir_recursive(from_path, to_path)
            .map_err(|e| AppCommandError::CopyPath(e.to_string()))?;
    } else {
        std::fs::copy(from_path, to_path)
            .map_err(|e| AppCommandError::CopyPath(e.to_string()))?;
    }

    emit_file_change(&app_handle, FileChangeEvent::Created { path: to });
    Ok(())
}

#[tauri::command]
pub async fn move_path(
    from: String,
    to: String,
    app_handle: AppHandle,
) -> Result<(), AppCommandError> {
    let from_path = std::path::Path::new(&from);
    if !from_path.exists() {
        return Err(AppCommandError::MovePath(format!("源路径不存在: {}", from)));
    }
    let to_path = std::path::Path::new(&to);
    if to_path.exists() && from_path != to_path {
        return Err(AppCommandError::TargetExists);
    }
    if let Some(parent) = to_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| AppCommandError::MovePath(e.to_string()))?;
    }
    std::fs::rename(from_path, to_path)
        .map_err(|e| AppCommandError::MovePath(e.to_string()))?;
    emit_file_change(&app_handle, FileChangeEvent::Deleted { path: from });
    emit_file_change(&app_handle, FileChangeEvent::Created { path: to });
    Ok(())
}

#[tauri::command]
pub async fn path_exists(path: String) -> Result<bool, AppCommandError> {
    Ok(std::path::Path::new(&path).exists())
}

#[tauri::command]
pub async fn open_with_default_app(
    path: String,
    app_handle: AppHandle,
) -> Result<(), AppCommandError> {
    app_handle
        .opener()
        .open_path(path, None::<&str>)
        .map_err(|e| AppCommandError::OpenWithDefaultApp(e.to_string()))
}

#[tauri::command]
pub async fn reveal_in_file_manager(
    path: String,
    app_handle: AppHandle,
) -> Result<(), AppCommandError> {
    app_handle
        .opener()
        .reveal_item_in_dir(&path)
        .map_err(|e| AppCommandError::RevealInFileManager(e.to_string()))
}

#[tauri::command]
pub async fn create_new_window(app_handle: AppHandle) -> Result<(), AppCommandError> {
    tracing::info!("Creating new window");

    use tauri::WebviewWindowBuilder;
    use tauri::WebviewUrl;

    WebviewWindowBuilder::new(
        &app_handle,
        &format!("main-{}", uuid::Uuid::new_v4()),
        WebviewUrl::App("index.html".into()),
    )
    .title("inkuo")
    .inner_size(1200.0, 800.0)
    .min_inner_size(800.0, 600.0)
    // Mark this window as a "fresh" window via a global JS variable so the
    // frontend can clear the previously persisted workspace and show the
    // welcome page. We use initialization_script (a global set before the
    // page scripts run) because Tauri 2's WebviewUrl::App is a PathBuf and
    // does not propagate query strings to the webview in dev mode.
    .initialization_script("window.__INKUO_FRESH_WINDOW__ = true;")
    .build()
    .map_err(|e| AppCommandError::CreateEntry(e.to_string()))?;

    Ok(())
}

fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else if file_type.is_symlink() {
            #[cfg(unix)]
            {
                let target = std::fs::read_link(&from)?;
                std::os::unix::fs::symlink(&target, &to)?;
            }
            #[cfg(not(unix))]
            {
                // On Windows, fall back to copying the symlink target's bytes.
                std::fs::copy(&from, &to)?;
            }
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

// =============================================================================
// Per-workspace snapshot commands (cross-window shared state)
// =============================================================================
//
// Snapshots are kept in an in-memory `HashMap` guarded by a `Mutex`, which is
// the single source of truth at runtime. The disk file is loaded once at
// startup and written back atomically (write-to-tmp + rename) whenever a
// mutation happens. All read-modify-write operations take the same lock so
// concurrent webview windows never lose updates.

use std::collections::HashMap;

use parking_lot::Mutex as PlMutex;
use tauri::{AppHandle as TauriAppHandle, Manager};

use AppCommandError::InvalidWorkspaceSnapshotsPath;

/// Global in-memory map of workspace path → JSON snapshot. Initialised from
/// disk in `init_workspace_snapshots` during the app's `setup` phase.
///
/// To prevent unbounded growth on long-running installs, the map is bounded
/// to `MAX_WORKSPACE_SNAPSHOTS` entries. When a new write would push us over
/// the limit, the least-recently-touched entry (by `last_touched_at`) is
/// evicted. Touches happen on both read and write.
pub static WORKSPACE_SNAPSHOTS: Lazy<PlMutex<HashMap<String, SnapshotEntry>>> =
    Lazy::new(|| PlMutex::new(HashMap::new()));

/// Hard cap on how many workspace snapshots we keep in memory + on disk.
/// 200 is generous (each entry is small JSON: tabs + AI session summaries)
/// while still preventing the file from growing without bound.
pub const MAX_WORKSPACE_SNAPSHOTS: usize = 200;

#[derive(Clone)]
pub struct SnapshotEntry {
    pub value: serde_json::Value,
    pub last_touched_at: std::time::Instant,
}

/// Path to the on-disk JSON file. Resolved at runtime via Tauri's path API so
/// it lands in the platform-correct config directory for the running app.
pub static WORKSPACE_SNAPSHOTS_PATH: once_cell::sync::Lazy<PlMutex<Option<std::path::PathBuf>>> =
    once_cell::sync::Lazy::new(|| PlMutex::new(None));

/// Compute and cache the absolute path to the workspace snapshots file using
/// Tauri's app config dir. Falls back to `dirs::config_dir()` only if the
/// Tauri resolver is unavailable (which should not happen in production).
fn resolve_snapshots_path(app_handle: &TauriAppHandle) -> Result<std::path::PathBuf, AppCommandError> {
    if let Some(cached) = WORKSPACE_SNAPSHOTS_PATH.lock().clone() {
        return Ok(cached);
    }
    let resolved = app_handle
        .path()
        .app_config_dir()
        .map_err(|e| InvalidWorkspaceSnapshotsPath(e.to_string()))?
        .join("workspace_snapshots.json");
    *WORKSPACE_SNAPSHOTS_PATH.lock() = Some(resolved.clone());
    Ok(resolved)
}

/// Load the snapshot file from disk into the in-memory map. Idempotent; safe
/// to call multiple times.
pub fn init_workspace_snapshots(app_handle: &TauriAppHandle) {
    let path = match resolve_snapshots_path(app_handle) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(
                "Could not resolve workspace snapshots path ({}); snapshots will not persist",
                e
            );
            return;
        }
    };

    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
        Err(e) => {
            tracing::warn!("Failed to read workspace snapshots ({}): {}", path.display(), e);
            return;
        }
    };

    let parsed: HashMap<String, serde_json::Value> = match serde_json::from_str(&content) {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(
                "Invalid workspace snapshots JSON at {} ({}); starting fresh",
                path.display(),
                e
            );
            return;
        }
    };

    let now = std::time::Instant::now();
    let entries: HashMap<String, SnapshotEntry> = parsed
        .into_iter()
        .map(|(path, value)| {
            (
                path,
                SnapshotEntry {
                    value,
                    last_touched_at: now,
                },
            )
        })
        .collect();

    *WORKSPACE_SNAPSHOTS.lock() = entries;
}

/// Evict the least-recently-touched entry if the in-memory map is at
/// capacity. Acquires the `WORKSPACE_SNAPSHOTS` lock itself — callers must
/// *not* hold the lock when calling, otherwise this would deadlock against
/// itself.
fn evict_lru_if_needed() {
    let mut snapshots = WORKSPACE_SNAPSHOTS.lock();
    if snapshots.len() < MAX_WORKSPACE_SNAPSHOTS {
        return;
    }

    if let Some((victim, _)) = snapshots
        .iter()
        .min_by_key(|(_, entry)| entry.last_touched_at)
        .map(|(path, entry)| (path.clone(), entry.last_touched_at))
    {
        snapshots.remove(&victim);
        tracing::info!(
            "Evicted workspace snapshot for {} (LRU, cap={})",
            victim,
            MAX_WORKSPACE_SNAPSHOTS
        );
    }
}

/// Update the touch timestamp on a snapshot entry. No-op if the entry was
/// evicted between the caller's read and this call.
fn touch_snapshot(path: &str) {
    let mut snapshots = WORKSPACE_SNAPSHOTS.lock();
    if let Some(entry) = snapshots.get_mut(path) {
        entry.last_touched_at = std::time::Instant::now();
    }
}

/// Persist the in-memory map to disk atomically (write to a sibling `.tmp`
/// file then rename). Returns `Ok(())` even when there is no path yet so
/// tests / preview builds don't crash when the config dir is unavailable.
fn flush_snapshots_to_disk(app_handle: &TauriAppHandle) -> Result<(), AppCommandError> {
    let path = match resolve_snapshots_path(app_handle) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("Skipping workspace snapshots flush: {}", e);
            return Ok(());
        }
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| AppCommandError::CreateBackupDirectory(e.to_string()))?;
    }

    // Copy out just the values (no touch timestamps) under a brief lock so
    // the on-disk payload matches the in-memory state. Serialization runs
    // outside the lock to keep the critical section short.
    let on_disk: HashMap<String, serde_json::Value> = {
        let snapshots = WORKSPACE_SNAPSHOTS.lock();
        snapshots
            .iter()
            .map(|(path, entry)| (path.clone(), entry.value.clone()))
            .collect()
    };

    let content = serde_json::to_string_pretty(&on_disk)
        .map_err(|e| AppCommandError::WriteWorkspaceSnapshots(format!("serialize: {}", e)))?;

    // Atomic write: tmp + rename. Avoids partially-written files if the
    // process is killed mid-write.
    let tmp_path = path.with_extension("json.tmp");
    std::fs::write(&tmp_path, &content)
        .map_err(|e| AppCommandError::WriteWorkspaceSnapshots(format!("write tmp: {}", e)))?;
    std::fs::rename(&tmp_path, &path)
        .map_err(|e| AppCommandError::WriteWorkspaceSnapshots(format!("rename: {}", e)))?;
    Ok(())
}

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
    } = args;

    let mut decoded: Vec<(String, Vec<u8>)> = Vec::with_capacity(files.len());
    for f in files {
        let bytes = base64_decode(&f.content_base64)
            .map_err(|e| AppCommandError::SnapshotWriteFailed(format!("base64 decode: {}", e)))?;
        decoded.push((f.rel_path, bytes));
    }

    crate::snapshots::create_workspace_snapshot(&workspace_path, label, &trigger, decoded)
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
    delete_extra_files: bool,
    app_handle: TauriAppHandle,
) -> Result<crate::snapshots::RestoreResult, AppCommandError> {
    crate::snapshots::restore_workspace_snapshot(
        &workspace_path,
        &snapshot_id,
        delete_extra_files,
        &app_handle,
    )
    .map_err(|e| AppCommandError::SnapshotWriteFailed(e.to_string()))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trip() {
        let original = b"hello world".to_vec();
        let encoded = base64_encode(&original);
        let decoded = base64_decode(&encoded).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn base64_decode_known_vector() {
        assert_eq!(base64_decode("aGVsbG8=").unwrap(), b"hello");
        assert_eq!(base64_decode("aGVsbG8gd29ybGQ=").unwrap(), b"hello world");
        assert_eq!(base64_decode("").unwrap(), b"");
    }

    fn base64_encode(input: &[u8]) -> String {
        const ALPHA: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        let mut i = 0;
        while i + 3 <= input.len() {
            let n = ((input[i] as u32) << 16) | ((input[i + 1] as u32) << 8) | (input[i + 2] as u32);
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
}

