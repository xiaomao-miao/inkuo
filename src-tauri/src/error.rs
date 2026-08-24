//! Application-wide error type.
//!
//! Goal: every Tauri command returns `Result<T, AppError>`, every internal
//! `Result<_, SubError>` can be lifted into `AppError` via `?` thanks to
//! `#[from]` conversions. Sub-modules can keep their domain-specific enums
//! (`ToolError`, `KnowledgeCommandError`, etc.) for ergonomic construction;
//! their `From` impls live at the bottom of this file.
//!
//! Serialisation: AppError serialises as a plain string via `serde`'s
//! default behaviour for enums containing `String` variants. The frontend
//! already handles string error messages, so no protocol change is
//! required — every old `AppCommandError::Foo(s)` variant is preserved
//! 1-to-1 below.

use thiserror::Error;

#[derive(Debug, Clone, Error, serde::Serialize, ts_rs::TS)]
#[ts(export, export_to = "AppError.ts")]
pub enum AppError {
    // ── Documents ─────────────────────────────────────────────────────────
    #[error("Failed to read document: {0}")]
    ReadDocument(String),
    #[error("Failed to parse document: {0}")]
    ParseDocument(String),
    #[error("Failed to write document: {0}")]
    WriteDocument(String),

    // ── Office files (Word / Excel / PPTX) ────────────────────────────────
    #[error("Failed to read office file: {0}")]
    ReadOfficeFile(String),
    #[error("Failed to serialize office document: {0}")]
    SerializeOfficeDocument(String),
    #[error("Failed to write office file: {0}")]
    WriteOfficeFile(String),

    // ── Workspace paths / file system ─────────────────────────────────────
    #[error("Failed to list directory: {0}")]
    ListDirectory(String),
    #[error("Failed to watch directory: {0}")]
    WatchDirectory(String),
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
    #[error("Invalid workspace path: {0}")]
    InvalidWorkspacePath(String),

    // ── Backups ───────────────────────────────────────────────────────────
    #[error("Failed to create backup directory: {0}")]
    CreateBackupDirectory(String),
    #[error("Failed to create backup: {0}")]
    CreateBackup(String),

    // ── Settings ──────────────────────────────────────────────────────────
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
    #[error("Invalid config path")]
    InvalidConfigPath,

    // ── AI / streaming ────────────────────────────────────────────────────
    #[error("AI edit failed: {0}")]
    AIEdit(String),
    #[error("AI connection test failed: {0}")]
    TestAIConnection(String),

    // ── Snapshots ─────────────────────────────────────────────────────────
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
    #[error("Snapshot write failed: {0}")]
    SnapshotWriteFailed(String),
    #[error("Snapshot read failed: {0}")]
    SnapshotReadFailed(String),
}

// ── Conversions from existing sub-module error enums ────────────────────────
//
// Each sub-module keeps its own typed error for ergonomic construction at
// call sites, but lifts into `AppError` via `From` so that any `?`-returning
// function across module boundaries gets a unified type for free.

impl From<crate::snapshots::SnapshotError> for AppError {
    fn from(err: crate::snapshots::SnapshotError) -> Self {
        use crate::snapshots::SnapshotError as E;
        match err {
            E::Io(_) => AppError::SnapshotWriteFailed(err.to_string()),
            E::Json(_) => AppError::SnapshotCorrupt(err.to_string()),
            E::InvalidWorkspacePath(p) => AppError::InvalidWorkspacePath(p),
            E::InvalidSnapshotPath(path) => AppError::SnapshotCorrupt(path),
            E::SnapshotNotFound(id) => AppError::SnapshotNotFound(id),
            E::SnapshotCorrupt(msg) => AppError::SnapshotCorrupt(msg),
            E::SettingsRead(msg) => AppError::ReadSettings(msg),
            E::BackupFailed(msg) => AppError::CreateBackup(msg),
            E::FileWrite(msg) => AppError::SnapshotWriteFailed(msg),
        }
    }
}

impl From<crate::cloud::CloudError> for AppError {
    fn from(err: crate::cloud::CloudError) -> Self {
        AppError::AIConfig(err.to_string())
    }
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        AppError::WriteDocument(err.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(err: serde_json::Error) -> Self {
        AppError::ParseSettings(err.to_string())
    }
}
