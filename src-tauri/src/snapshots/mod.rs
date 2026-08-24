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

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use tauri::AppHandle;

use crate::backup::get_backup_dir;
use crate::commands::{get_settings_cached, Settings};
use crate::file_watcher::{emit_file_change, FileChangeEvent};

// ── Constants ──────────────────────────────────────────────────────────────

/// Default maximum number of snapshots retained per workspace when the
/// user has not configured a limit.
const DEFAULT_SNAPSHOT_CAP: usize = 50;
/// How often the background cleanup task scans for orphan directories.
const SNAPSHOT_CLEANUP_INTERVAL_SECS: u64 = 300;
const MAX_SNAPSHOT_ID_BYTES: usize = 128;
const MAX_RELATIVE_PATH_BYTES: usize = 4 * 1024;
const MAX_SNAPSHOTS_PER_WORKSPACE: usize = 200;
const MAX_TRACKED_FILES_PER_WORKSPACE: usize = 100_000;
const EXCLUDED_WORKSPACE_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    "target",
    "dist",
    "build",
    ".next",
    ".cache",
    ".turbo",
    "out",
    ".inkuo",
];
static SNAPSHOT_MUTATION_LOCK: once_cell::sync::Lazy<parking_lot::Mutex<()>> =
    once_cell::sync::Lazy::new(|| parking_lot::Mutex::new(()));

// ── Helpers ────────────────────────────────────────────────────────────────

/// Returns `~/.inkuo/snapshots/` (or the platform-appropriate config dir).
pub fn get_snapshots_root() -> PathBuf {
    // Use Tauri's app config dir if available (gives us the right place on
    // each OS); fall back to `dirs::config_dir()` then $USERPROFILE (Windows)
    // or $HOME (Unix).
    let mut base = dirs::config_dir()
        .unwrap_or_else(|| {
            std::env::var_os("USERPROFILE")
                .or_else(|| std::env::var_os("HOME"))
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."))
        })
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

pub fn validate_snapshot_id(snapshot_id: &str) -> Result<(), SnapshotError> {
    if snapshot_id.is_empty()
        || snapshot_id.len() > MAX_SNAPSHOT_ID_BYTES
        || !snapshot_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(SnapshotError::InvalidSnapshotPath(format!(
            "invalid snapshot id: {snapshot_id:?}"
        )));
    }
    Ok(())
}

/// Validate an IPC/manifest path before joining it below a workspace or
/// snapshot root. Absolute paths, parent traversal and platform prefixes are
/// rejected rather than normalised.
pub fn validate_relative_path(relative_path: &str) -> Result<PathBuf, SnapshotError> {
    if relative_path.is_empty()
        || relative_path.len() > MAX_RELATIVE_PATH_BYTES
        || relative_path.contains('\0')
    {
        return Err(SnapshotError::InvalidSnapshotPath(format!(
            "invalid relative path: {relative_path:?}"
        )));
    }
    let path = Path::new(relative_path);
    if path.is_absolute()
        || path.components().any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(SnapshotError::InvalidSnapshotPath(format!(
            "relative path escapes snapshot root: {relative_path:?}"
        )));
    }
    Ok(path.to_path_buf())
}

fn validate_workspace_path(workspace_path: &str) -> Result<&Path, SnapshotError> {
    if workspace_path.is_empty()
        || workspace_path.len() > 32 * 1024
        || workspace_path.contains('\0')
        || !Path::new(workspace_path).is_absolute()
    {
        return Err(SnapshotError::InvalidWorkspacePath(
            workspace_path.to_string(),
        ));
    }
    Ok(Path::new(workspace_path))
}

fn workspace_destination(workspace: &Path, relative_path: &str) -> Result<PathBuf, SnapshotError> {
    let relative = validate_relative_path(relative_path)?;
    let mut current = workspace.to_path_buf();
    if let Some(parent) = relative.parent() {
        for component in parent.components() {
            current.push(component.as_os_str());
            if let Ok(metadata) = fs::symlink_metadata(&current) {
                if metadata.file_type().is_symlink() {
                    return Err(SnapshotError::InvalidSnapshotPath(format!(
                        "workspace path crosses a symlink: {}",
                        current.display()
                    )));
                }
            }
        }
    }
    let destination = workspace.join(relative);
    if fs::symlink_metadata(&destination)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false)
    {
        return Err(SnapshotError::InvalidSnapshotPath(format!(
            "workspace destination is a symlink: {}",
            destination.display()
        )));
    }
    Ok(destination)
}

fn should_descend_workspace_entry(entry: &walkdir::DirEntry) -> bool {
    entry.depth() == 0
        || !entry.file_type().is_dir()
        || entry
            .file_name()
            .to_str()
            .map(|name| !EXCLUDED_WORKSPACE_DIRS.contains(&name))
            .unwrap_or(true)
}

fn validate_snapshot_path_layout(
    files: &std::collections::HashSet<PathBuf>,
    directories: &std::collections::HashSet<PathBuf>,
) -> Result<(), SnapshotError> {
    for file in files {
        if path_contains_excluded_directory(file) {
            return Err(SnapshotError::InvalidSnapshotPath(format!(
                "file is inside an excluded workspace directory: {}",
                file.display()
            )));
        }
        if directories.contains(file) {
            return Err(SnapshotError::InvalidSnapshotPath(format!(
                "path is both a file and directory: {}",
                file.display()
            )));
        }
    }

    for directory in directories {
        if path_contains_excluded_directory(directory) {
            return Err(SnapshotError::InvalidSnapshotPath(format!(
                "excluded workspace directory cannot be snapshotted: {}",
                directory.display()
            )));
        }
    }

    for path in files.iter().chain(directories.iter()) {
        let mut ancestor = path.parent();
        while let Some(parent) = ancestor {
            if parent.as_os_str().is_empty() {
                break;
            }
            if files.contains(parent) {
                return Err(SnapshotError::InvalidSnapshotPath(format!(
                    "file path is an ancestor of another snapshot path: {}",
                    parent.display()
                )));
            }
            ancestor = parent.parent();
        }
    }
    Ok(())
}

fn path_contains_excluded_directory(path: &Path) -> bool {
    path.components().any(|component| match component {
        Component::Normal(name) => name
            .to_str()
            .map(|name| EXCLUDED_WORKSPACE_DIRS.contains(&name))
            .unwrap_or(false),
        _ => false,
    })
}

/// Full path to a specific snapshot directory.
pub fn snapshot_dir(workspace_path: &str, snapshot_id: &str) -> Result<PathBuf, SnapshotError> {
    validate_workspace_path(workspace_path)?;
    validate_snapshot_id(snapshot_id)?;
    Ok(get_snapshots_root()
        .join(workspace_hash(workspace_path))
        .join(snapshot_id))
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
    #[error("Invalid snapshot path: {0}")]
    InvalidSnapshotPath(String),
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
        DEFAULT_SNAPSHOT_CAP
    } else {
        cap.min(MAX_SNAPSHOTS_PER_WORKSPACE as u64) as usize
    }
}

// ── Index helpers ──────────────────────────────────────────────────────────

fn index_path(workspace_path: &str) -> PathBuf {
    get_snapshots_root()
        .join(workspace_hash(workspace_path))
        .join("index.json")
}

fn load_index(workspace_path: &str) -> Result<WorkspaceSnapshotIndex, SnapshotError> {
    validate_workspace_path(workspace_path)?;
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
    index
        .snapshots
        .retain(|entry| validate_snapshot_id(&entry.id).is_ok());
    index.snapshots.truncate(MAX_SNAPSHOTS_PER_WORKSPACE);
    Ok(index)
}

fn save_index(workspace_path: &str, index: &WorkspaceSnapshotIndex) -> Result<(), SnapshotError> {
    validate_workspace_path(workspace_path)?;
    let path = index_path(workspace_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let data = serde_json::to_string_pretty(index)?;
    crate::fs_utils::atomic_write(&path, data.as_bytes())?;
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
    let data = serde_json::to_string_pretty(manifest)?;
    crate::fs_utils::atomic_write(&path, data.as_bytes())?;
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
            let dir = snapshot_dir(workspace_path, &victim.id)?;
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
    let _mutation_guard = SNAPSHOT_MUTATION_LOCK.lock();
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
    let _mutation_guard = SNAPSHOT_MUTATION_LOCK.lock();
    let workspace = validate_workspace_path(workspace_path)?;
    if file_paths.len() > MAX_TRACKED_FILES_PER_WORKSPACE
        || directories.len() > MAX_TRACKED_FILES_PER_WORKSPACE
    {
        return Err(SnapshotError::InvalidSnapshotPath(format!(
            "snapshot contains too many paths (files={}, directories={}, max={})",
            file_paths.len(),
            directories.len(),
            MAX_TRACKED_FILES_PER_WORKSPACE
        )));
    }
    let mut seen_files = std::collections::HashSet::new();
    for (rel_path, _) in &file_paths {
        let relative = validate_relative_path(rel_path)?;
        if !seen_files.insert(relative) {
            return Err(SnapshotError::InvalidSnapshotPath(format!(
                "duplicate file path: {rel_path:?}"
            )));
        }
    }
    let mut seen_directories = std::collections::HashSet::new();
    for directory in &directories {
        let relative = validate_relative_path(directory)?;
        if !seen_directories.insert(relative) {
            return Err(SnapshotError::InvalidSnapshotPath(format!(
                "duplicate directory path: {directory:?}"
            )));
        }
    }
    validate_snapshot_path_layout(&seen_files, &seen_directories)?;

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    let snapshot_id = format!("snap_{}_{}", now_ms, uuid::Uuid::new_v4());

    let snap_dir = snapshot_dir(workspace_path, &snapshot_id)?;
    let files_dir = snap_dir.join("files");

    // Build manifest
    let mut files: Vec<SnapshotFileEntry> = Vec::with_capacity(file_paths.len());
    for (rel_path, bytes) in &file_paths {
        let relative = validate_relative_path(rel_path)?;
        let abs_path = workspace.join(relative).to_string_lossy().to_string();
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
        let dest = files_dir.join(validate_relative_path(&entry.rel_path)?);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        if let Err(error) = crate::fs_utils::atomic_write(&dest, &src_bytes) {
            let _ = fs::remove_dir_all(&snap_dir);
            return Err(error.into());
        }
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
    let _mutation_guard = SNAPSHOT_MUTATION_LOCK.lock();
    validate_snapshot_id(snapshot_id)?;
    let mut index = load_index(workspace_path)?;
    index.snapshots.retain(|s| s.id != snapshot_id);
    save_index(workspace_path, &index)?;

    let dir = snapshot_dir(workspace_path, snapshot_id)?;
    let _ = fs::remove_dir_all(&dir);
    Ok(())
}

/// Compute a preview of what restoring `snapshot_id` would change.  Each
/// entry compares the snapshot copy against the current file on disk.
pub fn preview_workspace_snapshot_restore(
    workspace_path: &str,
    snapshot_id: &str,
) -> Result<Vec<FileDiffPreview>, SnapshotError> {
    let _mutation_guard = SNAPSHOT_MUTATION_LOCK.lock();
    let snap_dir = snapshot_dir(workspace_path, snapshot_id)?;
    let manifest = load_manifest(&snap_dir)?;
    if manifest.snapshot_id != snapshot_id {
        return Err(SnapshotError::SnapshotCorrupt(format!(
            "manifest id {:?} does not match requested id {:?}",
            manifest.snapshot_id, snapshot_id
        )));
    }
    let ws_path = validate_workspace_path(workspace_path)?;

    let mut previews: Vec<FileDiffPreview> = Vec::with_capacity(manifest.files.len());
    let mut manifest_file_abs = std::collections::HashSet::with_capacity(manifest.files.len());

    for entry in &manifest.files {
        let disk_path = workspace_destination(ws_path, &entry.rel_path)?;
        let disk_path_string = disk_path.to_string_lossy().to_string();
        manifest_file_abs.insert(disk_path_string.clone());
        let (change_kind, disk_bytes_now) = if disk_path.exists() {
            // Compare by SHA-256, not by byte size — same-size-but-different-
            // content files are still "Modified".  We read the whole file
            // because comparing hashes is cheaper than diffing and the file
            // was already on disk during the snapshot we just made.
            match fs::read(&disk_path) {
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
            abs_path: disk_path_string,
            change_kind,
            is_binary: entry.is_binary,
            snapshot_bytes: entry.size,
            disk_bytes_now,
        });
    }

    // Detect deleted files: files that exist on disk but not in the snapshot.
    for tracked in collect_tracked_files(ws_path)? {
        if !manifest_file_abs.contains(&tracked) {
            if let Ok(meta) = fs::metadata(&tracked) {
                let rel_path = Path::new(&tracked)
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
    let _mutation_guard = SNAPSHOT_MUTATION_LOCK.lock();
    let ws_path = validate_workspace_path(workspace_path)?;
    let snap_dir = snapshot_dir(workspace_path, snapshot_id)?;
    let manifest = load_manifest(&snap_dir)?;
    if manifest.snapshot_id != snapshot_id {
        return Err(SnapshotError::SnapshotCorrupt(format!(
            "manifest id {:?} does not match requested id {:?}",
            manifest.snapshot_id, snapshot_id
        )));
    }

    // Fully validate the manifest and snapshot payload before deleting or
    // overwriting anything in the workspace. Stored absolute paths are
    // intentionally ignored; destinations are always derived from the
    // caller's validated workspace root plus a safe relative path.
    let mut seen_files = std::collections::HashSet::new();
    let mut safe_files = Vec::with_capacity(manifest.files.len());
    for entry in &manifest.files {
        let relative = validate_relative_path(&entry.rel_path)?;
        if !seen_files.insert(relative.clone()) {
            return Err(SnapshotError::SnapshotCorrupt(format!(
                "duplicate file path in manifest: {:?}",
                entry.rel_path
            )));
        }
        let source = snap_dir.join("files").join(&relative);
        if !source.is_file() {
            return Err(SnapshotError::SnapshotCorrupt(format!(
                "snapshot file missing: {}",
                source.display()
            )));
        }
        let bytes = fs::read(&source)?;
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let hash = hex::encode(hasher.finalize());
        if bytes.len() as u64 != entry.size || hash != entry.sha256 {
            return Err(SnapshotError::SnapshotCorrupt(format!(
                "snapshot file failed integrity check: {}",
                source.display()
            )));
        }
        let destination = workspace_destination(ws_path, &entry.rel_path)?;
        if destination.is_dir() {
            return Err(SnapshotError::InvalidSnapshotPath(format!(
                "refusing to replace a directory with a snapshot file: {}",
                destination.display()
            )));
        }
        safe_files.push((entry, source, destination));
    }

    let mut seen_directories = std::collections::HashSet::new();
    let mut safe_directories = Vec::with_capacity(manifest.directories.len());
    for relative in &manifest.directories {
        let validated = validate_relative_path(relative)?;
        if !seen_directories.insert(validated) {
            return Err(SnapshotError::SnapshotCorrupt(format!(
                "duplicate directory path in manifest: {relative:?}"
            )));
        }
        let destination = workspace_destination(ws_path, relative)?;
        if fs::symlink_metadata(&destination)
            .map(|metadata| metadata.file_type().is_symlink())
            .unwrap_or(false)
        {
            return Err(SnapshotError::InvalidSnapshotPath(format!(
                "snapshot directory resolves to a symlink: {}",
                destination.display()
            )));
        }
        safe_directories.push(destination);
    }
    validate_snapshot_path_layout(&seen_files, &seen_directories)?;

    // Pre-restore safety backup of every file currently on disk.
    let backup_dir = get_backup_dir();
    let backup_stamp = format!(
        "pre_restore_{}_{}",
        chrono::Utc::now().format("%Y%m%dT%H%M%S%3f"),
        uuid::Uuid::new_v4()
    );
    let backup_target = backup_dir.join(&backup_stamp);
    let tracked_before_restore = collect_tracked_files(ws_path)
        .map_err(|error| SnapshotError::BackupFailed(error.to_string()))?;
    let empty_directories_before_restore = collect_empty_directories(ws_path, &[])
        .map_err(|error| SnapshotError::BackupFailed(error.to_string()))?;
    fs::create_dir_all(&backup_target)
        .map_err(|error| SnapshotError::BackupFailed(error.to_string()))?;

    let backup_result = (|| -> Result<(), SnapshotError> {
        for path_str in &tracked_before_restore {
            let path = Path::new(path_str);
            if !path.exists() {
                continue;
            }
            let rel = path.strip_prefix(workspace_path).map_err(|error| {
                SnapshotError::BackupFailed(format!("invalid backup path {path_str}: {error}"))
            })?;
            let dest = backup_target.join(rel);
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)
                    .map_err(|error| SnapshotError::BackupFailed(error.to_string()))?;
            }
            crate::fs_utils::atomic_copy(path, &dest).map_err(|error| {
                SnapshotError::BackupFailed(format!("failed to back up {path_str}: {error}"))
            })?;
        }
        for relative in &empty_directories_before_restore {
            let relative = validate_relative_path(relative).map_err(|error| {
                SnapshotError::BackupFailed(format!("invalid empty directory: {error}"))
            })?;
            fs::create_dir_all(backup_target.join(relative))
                .map_err(|error| SnapshotError::BackupFailed(error.to_string()))?;
        }
        Ok(())
    })();
    if let Err(error) = backup_result {
        let _ = fs::remove_dir_all(&backup_target);
        return Err(error);
    }

    // Build the set of paths the snapshot considers "kept".  Used to decide
    // what to delete.
    let manifest_file_abs: std::collections::HashSet<String> = safe_files
        .iter()
        .map(|(_, _, destination)| destination.to_string_lossy().to_string())
        .collect();
    let manifest_dir_abs: std::collections::HashSet<std::path::PathBuf> =
        safe_directories.iter().cloned().collect();

    // 1. Delete files on disk that are not in the snapshot.
    let mut deleted: Vec<String> = Vec::new();
    for path_str in tracked_before_restore {
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

    // 2. Restore in-snapshot files.
    let mut restored: Vec<String> = Vec::with_capacity(safe_files.len());
    for (_entry, src, dest) in &safe_files {
        let bytes = fs::read(src)?;

        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        crate::fs_utils::atomic_write(dest, &bytes)?;

        let restored_path = dest.to_string_lossy().to_string();
        restored.push(restored_path.clone());

        emit_file_change(
            app_handle,
            FileChangeEvent::Modified {
                path: restored_path,
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
    for abs in safe_directories {
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

/// Walk `workspace_path` recursively and return all tracked absolute file
/// paths. Derived/build directories are pruned, and an oversized workspace
/// fails as a whole rather than returning a dangerous partial deletion set.
fn collect_tracked_files(workspace_path: &Path) -> Result<Vec<String>, io::Error> {
    let mut result = Vec::new();
    if !workspace_path.exists() {
        return Ok(result);
    }
    for entry in walkdir::WalkDir::new(workspace_path)
        .into_iter()
        .filter_entry(should_descend_workspace_entry)
    {
        let entry = entry.map_err(|error| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("failed to walk workspace: {error}"),
            )
        })?;
        if entry.file_type().is_file() {
            let p = entry.path().to_str().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("workspace path is not valid UTF-8: {:?}", entry.path()),
                )
            })?;
            result.push(p.to_string());
            if result.len() > MAX_TRACKED_FILES_PER_WORKSPACE {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "workspace contains more than {} tracked files",
                        MAX_TRACKED_FILES_PER_WORKSPACE
                    ),
                ));
            }
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
        .into_iter()
        .filter_entry(|entry| {
            should_descend_workspace_entry(entry)
                && (entry.depth() == 0
                    || !entry.file_type().is_dir()
                    || entry
                        .file_name()
                        .to_str()
                        .map(|name| !skip_dirs.iter().any(|skip| skip == name))
                        .unwrap_or(true))
        })
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_dir() {
            continue;
        }
        if entry.path() == workspace_path {
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
        .into_iter()
        .filter_entry(should_descend_workspace_entry)
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_dir() {
            continue;
        }
        if entry.path() == workspace_path {
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

#[cfg(test)]
mod security_tests {
    use super::*;

    #[test]
    fn relative_snapshot_paths_cannot_escape_their_root() {
        for invalid in ["", ".", "../secret", "folder/../../secret", "/absolute"] {
            assert!(validate_relative_path(invalid).is_err(), "accepted {invalid:?}");
        }
        assert_eq!(
            validate_relative_path("reports/2026/q1.docx").unwrap(),
            PathBuf::from("reports/2026/q1.docx")
        );
    }

    #[test]
    fn snapshot_ids_are_single_safe_path_components() {
        for invalid in ["", "../other", "nested/id", "id with spaces", "id.json"] {
            assert!(validate_snapshot_id(invalid).is_err(), "accepted {invalid:?}");
        }
        assert!(validate_snapshot_id("snap_1720000000000_123e4567-e89b-12d3-a456-426614174000").is_ok());
    }

    #[test]
    fn snapshot_directory_rejects_traversal_ids() {
        let workspace = if cfg!(windows) {
            r"C:\workspace"
        } else {
            "/tmp/workspace"
        };
        assert!(snapshot_dir(workspace, "../../outside").is_err());
    }

    #[test]
    fn snapshot_layout_rejects_file_directory_conflicts() {
        let files = [PathBuf::from("reports"), PathBuf::from("reports/q1.docx")]
            .into_iter()
            .collect();
        assert!(validate_snapshot_path_layout(&files, &Default::default()).is_err());

        let files = [PathBuf::from("reports/q1.docx")].into_iter().collect();
        let directories = [PathBuf::from("reports/q1.docx")].into_iter().collect();
        assert!(validate_snapshot_path_layout(&files, &directories).is_err());

        let files = [PathBuf::from(".git/config")].into_iter().collect();
        assert!(validate_snapshot_path_layout(&files, &Default::default()).is_err());
    }

    #[test]
    fn tracked_files_never_include_derived_directories() {
        let workspace = std::env::temp_dir().join(format!(
            "inkuo_snapshot_walk_{}_{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let _ = fs::remove_dir_all(&workspace);
        fs::create_dir_all(workspace.join(".git")).unwrap();
        fs::create_dir_all(workspace.join("node_modules/pkg")).unwrap();
        fs::write(workspace.join("report.md"), b"tracked").unwrap();
        fs::write(workspace.join(".git/config"), b"must survive restore").unwrap();
        fs::write(workspace.join("node_modules/pkg/index.js"), b"derived").unwrap();

        let tracked = collect_tracked_files(&workspace).unwrap();
        assert_eq!(
            tracked,
            vec![workspace.join("report.md").to_string_lossy().to_string()]
        );
        let _ = fs::remove_dir_all(&workspace);
    }
}
