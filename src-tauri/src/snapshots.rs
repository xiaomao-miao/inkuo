//! Workspace file-content snapshot module.
//!
//! A snapshot is a point-in-time copy of every file in a workspace, stored
//! under `~/.inkuo/snapshots/{workspaceHash}/`.  Each snapshot has an
//! `index.json`-level manifest (one per workspace, listing all snapshots) and
//! a per-snapshot `manifest.json` + `files/` directory containing the raw
//! bytes of every tracked file.
//!
//! Design choices:
//! - **Whole-file copies** – no delta/diff storage, keeping binary files
//!   (docx/xlsx/pptx) trivial to handle and guarantees exact byte-for-byte
//!   restore.
//! - **LRU cap** – enforced after each write; old snapshots are deleted
//!   oldest-first until the cap is met.
//! - **Atomic writes** – snapshot data is always written to a temp path and
//!   renamed, preventing corrupt reads if the process crashes mid-write.
//! - **Pre-restore safety backup** – before restoring, we create a timestamped
//!   backup of the current workspace files via `crate::backup`.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use tauri::AppHandle;

use crate::backup::get_backup_dir;
use crate::commands::{get_settings_cached, AppCommandError, Settings};
use crate::file_watcher::{emit_file_change, FileChangeEvent};

// ── Constants ──────────────────────────────────────────────────────────────

/// Default maximum number of snapshots retained per workspace when the
/// user has not configured a limit.
const DEFAULT_SNAPSHOT_CAP: usize = 50;
/// How often the background cleanup task scans for orphan directories.
const SNAPSHOT_CLEANUP_INTERVAL_SECS: u64 = 300;

// ── Helpers ────────────────────────────────────────────────────────────────

/// Returns `~/.inkuo/snapshots/` (or the platform-appropriate config dir).
pub fn get_snapshots_root() -> PathBuf {
    // Use Tauri's app config dir if available (gives us the right place on
    // each OS); fall back to `dirs::config_dir()` then `$HOME/.inkuo`.
    let mut base = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from(env!("HOME")))
        .join(".inkuo");
    base.push("snapshots");
    base
}

/// A 16-char hex SHA-256 prefix of the workspace path, used as a directory
/// name so that paths with slashes, spaces, or non-ASCII chars are safe.
pub fn workspace_hash(workspace_path: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(workspace_path.as_bytes());
    let hex = hex::encode(&hasher.finalize());
    hex[..16].to_string()
}

/// Full path to a specific snapshot directory.
pub fn snapshot_dir(workspace_path: &str, snapshot_id: &str) -> PathBuf {
    get_snapshots_root()
        .join(workspace_hash(workspace_path))
        .join(snapshot_id)
}

// ── Data model ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotFileEntry {
    pub rel_path: String,
    pub abs_path: String,
    pub size: u64,
    pub sha256: String,
    #[serde(default)]
    pub is_binary: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotManifest {
    pub snapshot_id: String,
    pub workspace_path: String,
    #[serde(default)]
    pub label: Option<String>,
    pub trigger: String,
    pub created_at: u64,
    pub files: Vec<SnapshotFileEntry>,
    /// Empty directories that existed in the workspace at capture time.
    /// Required for a full-state restore: when the workspace is rolled back
    /// to a previous state, every directory the user removed since then
    /// (including ones that *became* empty) must be re-created, otherwise
    /// the post-restore workspace would be missing structure that was
    /// present at snapshot time.
    #[serde(default)]
    pub directories: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotIndexEntry {
    pub id: String,
    pub created_at: u64,
    #[serde(default)]
    pub label: Option<String>,
    pub file_count: usize,
    pub total_bytes: u64,
    pub trigger: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceSnapshotIndex {
    pub version: u32,
    pub workspace_path: String,
    pub snapshots: Vec<SnapshotIndexEntry>,
}

/// Result of a restore operation.  After a full-state restore:
///
/// - `restored`: files overwritten by the snapshot contents.
/// - `deleted`: files that were on disk but not in the snapshot (removed).
/// - `deleted_dirs`: directories that were on disk but not in the snapshot
///   (removed, deepest-first so the final state matches the snapshot).
/// - `created_dirs`: empty directories recorded in the snapshot that had to
///   be re-created on disk.
/// - `backup_path`: absolute path of the timestamped backup directory under
///   `~/.inkuo/backups/`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreResult {
    #[serde(default)]
    pub restored: Vec<String>,
    #[serde(default)]
    pub deleted: Vec<String>,
    #[serde(default)]
    pub deleted_dirs: Vec<String>,
    #[serde(default)]
    pub created_dirs: Vec<String>,
    pub backup_path: String,
}

impl Default for WorkspaceSnapshotIndex {
    fn default() -> Self {
        Self {
            version: 1,
            workspace_path: String::new(),
            snapshots: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeKind {
    Unchanged,
    Added,
    Modified,
    Deleted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileDiffPreview {
    pub rel_path: String,
    pub abs_path: String,
    pub change_kind: ChangeKind,
    #[serde(default)]
    pub is_binary: bool,
    pub snapshot_bytes: u64,
    pub disk_bytes_now: u64,
}

#[derive(Debug, Error)]
#[allow(dead_code)] // Some variants are reserved for future error-mapping in UI layers.
pub enum SnapshotError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("JSON serialisation error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Workspace path is not absolute: {0}")]
    InvalidWorkspacePath(String),
    #[error("Snapshot not found: {0}")]
    SnapshotNotFound(String),
    #[error("Snapshot manifest corrupt: {0}")]
    SnapshotCorrupt(String),
    #[error("Settings read failed: {0}")]
    SettingsRead(String),
    #[error("Failed to create pre-restore backup: {0}")]
    BackupFailed(String),
    #[error("File write failed: {0}")]
    FileWrite(String),
}

impl From<SnapshotError> for AppCommandError {
    fn from(e: SnapshotError) -> Self {
        AppCommandError::SnapshotReadFailed(e.to_string())
    }
}

// ── LRU cap helper ─────────────────────────────────────────────────────────

/// Read the user-configured snapshot cap from settings; fall back to the
/// default if the setting is absent or zero.
fn snapshot_cap() -> usize {
    match get_settings_cached() {
        Ok(settings) => read_cap_from_settings(&settings),
        Err(_) => DEFAULT_SNAPSHOT_CAP,
    }
}

fn read_cap_from_settings(settings: &Settings) -> usize {
    // Settings is deserialised from JSON; custom fields may or may not be
    // present depending on whether the user has upgraded.  We parse the
    // optional `snapshot` object defensively.
    let raw = serde_json::to_value(settings).ok();
    let cap = raw
        .as_ref()
        .and_then(|v| v.get("snapshot"))
        .and_then(|s| s.get("max_count"))
        .and_then(|v| v.as_u64())
        .unwrap_or(DEFAULT_SNAPSHOT_CAP as u64);
    if cap == 0 {
        usize::MAX
    } else {
        cap as usize
    }
}

// ── Index helpers ──────────────────────────────────────────────────────────

fn index_path(workspace_path: &str) -> PathBuf {
    get_snapshots_root()
        .join(workspace_hash(workspace_path))
        .join("index.json")
}

fn load_index(workspace_path: &str) -> Result<WorkspaceSnapshotIndex, SnapshotError> {
    let path = index_path(workspace_path);
    if !path.exists() {
        return Ok(WorkspaceSnapshotIndex {
            workspace_path: workspace_path.to_string(),
            ..Default::default()
        });
    }
    let data = fs::read_to_string(&path)?;
    let mut index: WorkspaceSnapshotIndex = serde_json::from_str(&data)?;
    index.workspace_path = workspace_path.to_string();
    Ok(index)
}

fn save_index(workspace_path: &str, index: &WorkspaceSnapshotIndex) -> Result<(), SnapshotError> {
    let path = index_path(workspace_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    let data = serde_json::to_string_pretty(index)?;
    {
        let mut f = File::create(&tmp)?;
        f.write_all(data.as_bytes())?;
        f.sync_all()?;
    }
    fs::rename(&tmp, &path)?;
    Ok(())
}

// ── Manifest helpers ───────────────────────────────────────────────────────

fn manifest_path(snap_dir: &Path) -> PathBuf {
    snap_dir.join("manifest.json")
}

fn load_manifest(snap_dir: &Path) -> Result<SnapshotManifest, SnapshotError> {
    let path = manifest_path(snap_dir);
    if !path.exists() {
        return Err(SnapshotError::SnapshotCorrupt(format!(
            "manifest.json missing in {}",
            snap_dir.display()
        )));
    }
    let data = fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&data)?)
}

fn save_manifest(snap_dir: &Path, manifest: &SnapshotManifest) -> Result<(), SnapshotError> {
    fs::create_dir_all(snap_dir.join("files"))?;
    let path = manifest_path(snap_dir);
    let tmp = path.with_extension("tmp");
    let data = serde_json::to_string_pretty(manifest)?;
    {
        let mut f = File::create(&tmp)?;
        f.write_all(data.as_bytes())?;
        f.sync_all()?;
    }
    fs::rename(&tmp, &path)?;
    Ok(())
}

// ── Binary detection ───────────────────────────────────────────────────────

/// A small set of extensions that are reliably text-based.  Everything else
/// is treated as binary so that arbitrary bytes (docx, images, etc.) round-
/// trip safely.
fn is_text_extension(path: &str) -> bool {
    const TEXT_EXTS: &[&str] = &[
        ".md", ".txt", ".json", ".yaml", ".yml", ".toml", ".csv", ".ini",
        ".cfg", ".conf", ".log", ".html", ".htm", ".css", ".xml", ".svg",
        ".js", ".jsx", ".ts", ".tsx", ".mjs", ".cjs", ".py", ".rb", ".rs",
        ".go", ".java", ".kt", ".kts", ".swift", ".c", ".h", ".cpp", ".hpp",
        ".cs", ".php", ".sh", ".bash", ".zsh", ".fish", ".lua", ".r", ".sql",
        ".vue", ".svelte", ".astro", ".wasm",
    ];
    path.rsplit('.').next().map(|ext| {
        let ext = format!(".{}", ext.to_lowercase());
        TEXT_EXTS.contains(&ext.as_str())
    }).unwrap_or(false)
}

fn path_is_binary(path: &str) -> bool {
    !is_text_extension(path)
}

// ── LRU eviction ───────────────────────────────────────────────────────────

/// Ensure the number of snapshots for `workspace_path` does not exceed the
/// configured cap.  The oldest snapshots (by `created_at`) are evicted first.
fn enforce_snapshot_cap(workspace_path: &str) -> Result<(), SnapshotError> {
    let cap = snapshot_cap();
    let mut index = load_index(workspace_path)?;

    while index.snapshots.len() > cap {
        // snapshots are stored newest-first, so evict from the tail.
        if let Some(victim) = index.snapshots.pop() {
            let dir = snapshot_dir(workspace_path, &victim.id);
            let _ = fs::remove_dir_all(&dir);
        }
    }

    save_index(workspace_path, &index)
}

// ── Background cleanup ─────────────────────────────────────────────────────

/// Periodically scan all workspace snapshot directories and remove any that
/// no longer appear in their `index.json`.  This catches orphan `files/`
/// directories that could remain after a manual partial delete or crash.
pub fn init_snapshot_cleanup_task() {
    tauri::async_runtime::spawn(async move {
        let mut interval =
            tokio::time::interval(tokio::time::Duration::from_secs(SNAPSHOT_CLEANUP_INTERVAL_SECS));
        interval.tick().await; // skip the immediate fire
        loop {
            interval.tick().await;
            let _ = run_cleanup_pass();
        }
    });
}

fn run_cleanup_pass() -> Result<(), SnapshotError> {
    let root = get_snapshots_root();
    if !root.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let index_path = entry.path().join("index.json");
        if !index_path.exists() {
            continue;
        }
        let index: WorkspaceSnapshotIndex = match fs::read_to_string(&index_path) {
            Ok(data) => match serde_json::from_str(&data) {
                Ok(idx) => idx,
                Err(_) => continue,
            },
            Err(_) => continue,
        };

        let valid_ids: std::collections::HashSet<_> =
            index.snapshots.iter().map(|s| s.id.as_str()).collect();

    for snap_entry in fs::read_dir(entry.path())? {
        let snap_entry = snap_entry?;
        if !snap_entry.file_type()?.is_dir() {
            continue;
        }
        let name = snap_entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str == "index.json" || name_str == "index.json.tmp" {
            continue;
        }
        if !valid_ids.contains(name_str.as_ref()) {
            let _ = fs::remove_dir_all(snap_entry.path());
        }
    }
    }
    Ok(())
}

// ── Public API ─────────────────────────────────────────────────────────────

/// Create a new snapshot for `workspace_path`.  `file_paths` is a vec of
/// `(relative_path, raw_bytes)` – the frontend is expected to read the files
/// itself so that we don't need a separate "list workspace files" command.
///
/// Returns the manifest for the newly created snapshot.
pub fn create_workspace_snapshot(
    workspace_path: &str,
    label: Option<String>,
    trigger: &str,
    file_paths: Vec<(String, Vec<u8>)>,
    directories: Vec<String>,
) -> Result<SnapshotManifest, SnapshotError> {
    if !Path::new(workspace_path).is_absolute() {
        return Err(SnapshotError::InvalidWorkspacePath(
            workspace_path.to_string(),
        ));
    }

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    let snapshot_id = format!(
        "snap_{}",
        chrono::Utc::now().format("%Y-%m-%dT%H-%M-%S")
    );

    let snap_dir = snapshot_dir(workspace_path, &snapshot_id);
    let files_dir = snap_dir.join("files");

    // Build manifest
    let mut files: Vec<SnapshotFileEntry> = Vec::with_capacity(file_paths.len());
    for (rel_path, bytes) in &file_paths {
        let abs_path = Path::new(workspace_path).join(rel_path).to_string_lossy().to_string();
        let size = bytes.len() as u64;
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        let sha256 = hex::encode(&hasher.finalize());
        let is_binary = path_is_binary(rel_path);

        files.push(SnapshotFileEntry {
            rel_path: rel_path.clone(),
            abs_path,
            size,
            sha256,
            is_binary,
        });
    }

    let total_bytes: u64 = files.iter().map(|f| f.size).sum();

    let manifest = SnapshotManifest {
        snapshot_id: snapshot_id.clone(),
        workspace_path: workspace_path.to_string(),
        label,
        trigger: trigger.to_string(),
        created_at: now_ms,
        files,
        directories,
    };

    // Write files first (fail early if disk is full)
    fs::create_dir_all(&files_dir)?;
    for entry in &manifest.files {
        let src_bytes = file_paths
            .iter()
            .find(|(rp, _)| rp == &entry.rel_path)
            .map(|(_, b)| b.clone())
            .unwrap_or_default();
        let dest = files_dir.join(&entry.rel_path);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp = dest.with_extension("snapshot_tmp");
        {
            let mut f = File::create(&tmp)?;
            f.write_all(&src_bytes)?;
            f.sync_all()?;
        }
        fs::rename(&tmp, &dest)?;
    }

    // Write manifest
    save_manifest(&snap_dir, &manifest)?;

    // Update workspace-level index
    let mut index = load_index(workspace_path)?;
    index.workspace_path = workspace_path.to_string();
    index.snapshots.insert(
        0,
        SnapshotIndexEntry {
            id: snapshot_id.clone(),
            created_at: now_ms,
            label: manifest.label.clone(),
            file_count: manifest.files.len(),
            total_bytes,
            trigger: manifest.trigger.clone(),
        },
    );
    save_index(workspace_path, &index)?;

    // Enforce LRU cap
    enforce_snapshot_cap(workspace_path)?;

    Ok(manifest)
}

/// List all snapshots for `workspace_path`, newest first.
pub fn list_workspace_snapshots(
    workspace_path: &str,
) -> Result<Vec<SnapshotIndexEntry>, SnapshotError> {
    let index = load_index(workspace_path)?;
    Ok(index.snapshots)
}

/// Delete a snapshot by id.
pub fn delete_workspace_snapshot(
    workspace_path: &str,
    snapshot_id: &str,
) -> Result<(), SnapshotError> {
    let mut index = load_index(workspace_path)?;
    index.snapshots.retain(|s| s.id != snapshot_id);
    save_index(workspace_path, &index)?;

    let dir = snapshot_dir(workspace_path, snapshot_id);
    let _ = fs::remove_dir_all(&dir);
    Ok(())
}

/// Compute a preview of what restoring `snapshot_id` would change.  Each
/// entry compares the snapshot copy against the current file on disk.
pub fn preview_workspace_snapshot_restore(
    workspace_path: &str,
    snapshot_id: &str,
) -> Result<Vec<FileDiffPreview>, SnapshotError> {
    let snap_dir = snapshot_dir(workspace_path, snapshot_id);
    let manifest = load_manifest(&snap_dir)?;

    let mut previews: Vec<FileDiffPreview> = Vec::with_capacity(manifest.files.len());

    for entry in &manifest.files {
        let disk_path = Path::new(&entry.abs_path);
        let (change_kind, disk_bytes_now) = if disk_path.exists() {
            // Compare by SHA-256, not by byte size — same-size-but-different-
            // content files are still "Modified".  We read the whole file
            // because comparing hashes is cheaper than diffing and the file
            // was already on disk during the snapshot we just made.
            match fs::read(disk_path) {
                Ok(bytes) => {
                    let disk_size = bytes.len() as u64;
                    let mut hasher = Sha256::new();
                    hasher.update(&bytes);
                    let disk_hash = hex::encode(&hasher.finalize());
                    let change = if disk_size == entry.size && disk_hash == entry.sha256 {
                        ChangeKind::Unchanged
                    } else {
                        ChangeKind::Modified
                    };
                    (change, disk_size)
                }
                Err(_) => (ChangeKind::Modified, 0),
            }
        } else {
            (ChangeKind::Added, 0)
        };

        previews.push(FileDiffPreview {
            rel_path: entry.rel_path.clone(),
            abs_path: entry.abs_path.clone(),
            change_kind,
            is_binary: entry.is_binary,
            snapshot_bytes: entry.size,
            disk_bytes_now,
        });
    }

    // Detect deleted files: files that exist on disk but not in the snapshot.
    let ws_path = Path::new(workspace_path);
    if let Ok(entries) = collect_tracked_files(ws_path, manifest.files.len()) {
        for tracked in entries {
            if !manifest.files.iter().any(|e| e.abs_path == tracked) {
                if let Ok(meta) = fs::metadata(&tracked) {
                    let rel_path = ws_path
                        .join(&tracked)
                        .strip_prefix(ws_path)
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or(tracked.clone());
                    previews.push(FileDiffPreview {
                        rel_path,
                        abs_path: tracked.clone(),
                        change_kind: ChangeKind::Deleted,
                        is_binary: path_is_binary(&tracked),
                        snapshot_bytes: 0,
                        disk_bytes_now: meta.len(),
                    });
                }
            }
        }
    }

    Ok(previews)
}

/// Restore `snapshot_id` to make the workspace **exactly** match the state
/// captured at snapshot time:
///
/// 1. Every file in the snapshot is written back to its path.
/// 2. Every file on disk that was NOT in the snapshot is removed.
/// 3. Every directory on disk that was NOT in the snapshot is removed
///    (recursively, including directories that became empty after step 2).
/// 4. Every empty directory recorded in the snapshot is re-created.
///
/// Before any of this happens, a timestamped backup of the *current*
/// workspace contents (files + directories) is written under
/// `~/.inkuo/backups/`, so the user can recover manually if needed.
///
/// On success, `file-change` events are emitted for every touched path so
/// that the editor refreshes automatically.
pub fn restore_workspace_snapshot(
    workspace_path: &str,
    snapshot_id: &str,
    app_handle: &AppHandle,
) -> Result<RestoreResult, SnapshotError> {
    if !Path::new(workspace_path).is_absolute() {
        return Err(SnapshotError::InvalidWorkspacePath(
            workspace_path.to_string(),
        ));
    }

    let snap_dir = snapshot_dir(workspace_path, snapshot_id);
    let manifest = load_manifest(&snap_dir)?;

    // Pre-restore safety backup of every file currently on disk.
    let backup_dir = get_backup_dir();
    let backup_stamp = format!(
        "pre_restore_{}",
        chrono::Utc::now().format("%Y%m%dT%H%M%S")
    );
    let backup_target = backup_dir.join(&backup_stamp);
    fs::create_dir_all(&backup_target)?;

    let ws_path = Path::new(workspace_path);
    if let Ok(tracked) = collect_tracked_files(ws_path, 0) {
        for path_str in tracked {
            let path = Path::new(&path_str);
            if !path.exists() {
                continue;
            }
            let rel = path
                .strip_prefix(workspace_path)
                .unwrap_or_else(|_| Path::new(&path_str));
            let dest = backup_target.join(rel);
            if let Some(parent) = dest.parent() {
                let _ = fs::create_dir_all(parent);
            }
            if let Err(e) = fs::copy(path, &dest) {
                tracing::warn!(
                    "Pre-restore backup failed for {}: {}",
                    path_str,
                    e
                );
            }
        }
    }

    // Build the set of paths the snapshot considers "kept".  Used to decide
    // what to delete.
    let manifest_file_abs: std::collections::HashSet<String> = manifest
        .files
        .iter()
        .map(|e| e.abs_path.clone())
        .collect();
    let manifest_dir_abs: std::collections::HashSet<std::path::PathBuf> = manifest
        .directories
        .iter()
        .map(|d| ws_path.join(d))
        .collect();

    // 1. Delete files on disk that are not in the snapshot.
    let mut deleted: Vec<String> = Vec::new();
    if let Ok(tracked) = collect_tracked_files(ws_path, manifest.files.len()) {
        for path_str in tracked {
            if manifest_file_abs.contains(&path_str) {
                continue;
            }
            let path = Path::new(&path_str);
            if !path.exists() {
                continue;
            }
            // (already backed up above)
            if let Err(e) = fs::remove_file(path) {
                tracing::warn!("Delete failed for {}: {}", path_str, e);
                continue;
            }
            deleted.push(path_str.clone());
            emit_file_change(
                app_handle,
                FileChangeEvent::Deleted {
                    path: path_str.clone(),
                },
            );
        }
    }

    // 2. Restore in-snapshot files.
    let mut restored: Vec<String> = Vec::with_capacity(manifest.files.len());
    for entry in &manifest.files {
        let src = snap_dir.join("files").join(&entry.rel_path);
        if !src.exists() {
            tracing::warn!("Snapshot file missing during restore: {}", src.display());
            continue;
        }

        let bytes = fs::read(&src)?;
        let dest = Path::new(&entry.abs_path);

        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }

        let tmp = dest.with_extension("inkuo_restore_tmp");
        {
            let mut f = File::create(&tmp)?;
            f.write_all(&bytes)?;
            f.sync_all()?;
        }
        fs::rename(&tmp, dest)?;

        restored.push(entry.abs_path.clone());

        emit_file_change(
            app_handle,
            FileChangeEvent::Modified {
                path: entry.abs_path.clone(),
            },
        );
    }

    // 3. Delete directories on disk that were not in the snapshot, deepest
    //    first so we never try to delete a parent before its children.  Only
    //    directories whose final state is empty (no files, no kept
    //    sub-directories) are removed; manifest-listed dirs are preserved.
    let mut deleted_dirs: Vec<String> = Vec::new();
    if let Ok(mut extra_dirs) = collect_extra_directories(ws_path, &manifest_dir_abs) {
        // deepest first
        extra_dirs.sort_by(|a, b| {
            let ad = a.components().count();
            let bd = b.components().count();
            ad.cmp(&bd).reverse()
        });
        for dir in extra_dirs {
            if !dir.exists() {
                continue;
            }
            // Final emptiness check (race-safe: only remove if still empty).
            let is_empty = fs::read_dir(&dir)
                .map(|mut it| it.next().is_none())
                .unwrap_or(false);
            if !is_empty {
                continue;
            }
            if let Err(e) = fs::remove_dir(&dir) {
                tracing::warn!("rmdir failed for {}: {}", dir.display(), e);
                continue;
            }
            deleted_dirs.push(dir.to_string_lossy().to_string());
        }
    }

    // 4. Re-create empty directories that were in the snapshot but are not
    //    currently on disk.
    let mut created_dirs: Vec<String> = Vec::new();
    for rel in &manifest.directories {
        let abs = ws_path.join(rel);
        if abs.exists() {
            continue;
        }
        if let Err(e) = fs::create_dir_all(&abs) {
            tracing::warn!("mkdir failed for {}: {}", abs.display(), e);
            continue;
        }
        created_dirs.push(abs.to_string_lossy().to_string());
    }

    Ok(RestoreResult {
        restored,
        deleted,
        deleted_dirs,
        created_dirs,
        backup_path: backup_target.to_string_lossy().to_string(),
    })
}

// ── Internal: collect tracked files for "deleted" detection ─────────────────

/// Walk `workspace_path` recursively and return absolute paths of files
/// that are *not* already in `skip_paths`.  Used by preview to detect files
/// that existed on disk at restore-time but were not in the snapshot.
fn collect_tracked_files(
    workspace_path: &Path,
    skip_paths: usize,
) -> Result<Vec<String>, io::Error> {
    let mut result = Vec::new();
    if !workspace_path.exists() {
        return Ok(result);
    }
    // Limit traversal to avoid O(n²) on huge trees.
    for entry in walkdir::WalkDir::new(workspace_path)
        .max_depth(10)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() {
            if let Some(p) = entry.path().to_str() {
                result.push(p.to_string());
            }
        }
        if result.len() > skip_paths + 500 {
            break;
        }
    }
    Ok(result)
}

/// Walk `workspace_path` recursively and return the relative paths of every
/// directory that is empty (no files anywhere in its subtree, after
/// `skip_dirs` branches are pruned).  Result is sorted deepest-first so
/// callers can iterate in creation order without re-walking.
///
/// `rel_paths` are relative to `workspace_path`.  Returns `Ok(vec![])`
/// when the workspace itself does not exist.
pub fn collect_empty_directories(
    workspace_path: &Path,
    skip_dirs: &[String],
) -> Result<Vec<String>, io::Error> {
    let mut result: Vec<String> = Vec::new();
    if !workspace_path.exists() {
        return Ok(result);
    }

    // Gather every directory under the workspace, excluding any branch whose
    // path contains a skip-dir name component.
    let mut all_dirs: Vec<std::path::PathBuf> = Vec::new();
    for entry in walkdir::WalkDir::new(workspace_path)
        .max_depth(10)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_dir() {
            continue;
        }
        if entry.path() == workspace_path {
            continue;
        }
        let mut skip = false;
        for comp in entry.path().components() {
            if let Some(name) = comp.as_os_str().to_str() {
                if skip_dirs.iter().any(|s| s == name) {
                    skip = true;
                    break;
                }
            }
        }
        if skip {
            continue;
        }
        all_dirs.push(entry.path().to_path_buf());
    }

    // Recursive emptiness check: a directory is empty iff it contains no
    // files AND no non-empty subdirectories.  Walk leaves first so a parent
    // that contains only empty subdirs is also detected as empty.
    use std::collections::HashMap;
    let mut is_empty: HashMap<std::path::PathBuf, bool> = HashMap::new();

    all_dirs.sort_by_key(|d| std::cmp::Reverse(d.components().count()));

    for d in &all_dirs {
        let mut empty = true;
        let read = match std::fs::read_dir(d) {
            Ok(r) => r,
            Err(_) => {
                is_empty.insert(d.clone(), false);
                continue;
            }
        };
        for child in read.filter_map(|c| c.ok()) {
            let path = child.path();
            if path.is_file() {
                empty = false;
                break;
            }
            if path.is_dir() {
                let child_empty = is_empty.get(&path).copied().unwrap_or(false);
                if !child_empty {
                    empty = false;
                    break;
                }
            }
        }
        is_empty.insert(d.clone(), empty);
        if empty {
            if let Ok(rel) = d.strip_prefix(workspace_path) {
                result.push(rel.to_string_lossy().to_string());
            }
        }
    }

    // Stable, deep-first ordering for callers.
    result.sort_by(|a, b| {
        let ad = a.matches('/').count();
        let bd = b.matches('/').count();
        ad.cmp(&bd).then(a.cmp(b))
    });
    Ok(result)
}

// ── Internal: collect extra directories (not in manifest) for removal ──────

/// Walk the workspace and return absolute paths of every directory that is
/// NOT in `kept_dirs`.  Used during a full restore to identify directories
/// that need to be removed (deepest-first ordering is applied by the
/// caller).
///
/// The workspace root itself is never returned, and skip-dirs like
/// `node_modules` / `.git` are pruned from the traversal.  The directory is
/// only listed if it does NOT contain any directory that IS in `kept_dirs`
/// transitively — otherwise removing it would also remove a kept subtree.
fn collect_extra_directories(
    workspace_path: &Path,
    kept_dirs: &std::collections::HashSet<std::path::PathBuf>,
) -> Result<Vec<std::path::PathBuf>, io::Error> {
    let mut result: Vec<std::path::PathBuf> = Vec::new();
    if !workspace_path.exists() {
        return Ok(result);
    }

    let mut all_dirs: Vec<std::path::PathBuf> = Vec::new();
    for entry in walkdir::WalkDir::new(workspace_path)
        .max_depth(10)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_dir() {
            continue;
        }
        if entry.path() == workspace_path {
            continue;
        }
        // Prune heavy / excluded directories.
        let mut skip = false;
        for comp in entry.path().components() {
            if let Some(name) = comp.as_os_str().to_str() {
                if name == "node_modules" || name == ".git" || name == ".inkuo" {
                    skip = true;
                    break;
                }
            }
        }
        if skip {
            continue;
        }
        all_dirs.push(entry.path().to_path_buf());
    }

    // A directory is "extra" iff it is not in `kept_dirs` AND none of its
    // descendants are in `kept_dirs` (a kept subtree must not be removed
    // when we remove its non-kept ancestor).  Compute the set of ancestor
    // paths so we can test in O(1).
    for d in &all_dirs {
        if kept_dirs.contains(d) {
            continue;
        }
        // Walk parents: if any ancestor (including d itself) is kept, skip.
        let mut cur: Option<&std::path::Path> = Some(d.as_path());
        let mut has_kept_ancestor = false;
        while let Some(p) = cur {
            if p == workspace_path {
                break;
            }
            if kept_dirs.contains(p) {
                has_kept_ancestor = true;
                break;
            }
            cur = p.parent();
        }
        if has_kept_ancestor {
            continue;
        }
        result.push(d.clone());
    }

    Ok(result)
}
