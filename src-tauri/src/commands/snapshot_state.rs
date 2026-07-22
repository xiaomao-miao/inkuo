//! In-memory + on-disk state for the per-workspace snapshot store.
//!
//! Used to live inline in `commands/mod.rs` near the related Tauri
//! commands. Splitting state out makes the IPC layer (which is per-cmd
//! small) easier to grep, and gives the disk-format details one canonical
//! home rather than scattered across the `commands` module.
//!
//! The Tauri commands themselves (`save_workspace_snapshot`,
//! `list_workspace_snapshots`, `delete_workspace_snapshot`, …) still
//! live in `commands/mod.rs`; they reach the state below through the
//! `pub(crate) use` re-exports re-declared at the bottom of `mod.rs`.

use std::collections::HashMap;

use once_cell::sync::Lazy;
use parking_lot::Mutex as PlMutex;
use tauri::{AppHandle as TauriAppHandle, Manager};

use super::AppCommandError;

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
pub(crate) fn resolve_snapshots_path(app_handle: &TauriAppHandle) -> Result<std::path::PathBuf, AppCommandError> {
    if let Some(cached) = WORKSPACE_SNAPSHOTS_PATH.lock().clone() {
        return Ok(cached);
    }
    let resolved = app_handle
        .path()
        .app_config_dir()
        .map_err(|e| crate::error::AppError::InvalidWorkspaceSnapshotsPath(e.to_string()))?
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
pub(crate) fn evict_lru_if_needed() {
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
pub(crate) fn touch_snapshot(path: &str) {
    let mut snapshots = WORKSPACE_SNAPSHOTS.lock();
    if let Some(entry) = snapshots.get_mut(path) {
        entry.last_touched_at = std::time::Instant::now();
    }
}

/// Persist the in-memory map to disk atomically (write to a sibling `.tmp`
/// file then rename). Returns `Ok(())` even when there is no path yet so
/// tests / preview builds don't crash when the config dir is unavailable.
pub(crate) fn flush_snapshots_to_disk(app_handle: &TauriAppHandle) -> Result<(), AppCommandError> {
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
