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

/// Serialises clone + disk commit so two windows cannot race and let an older
/// snapshot payload overwrite a newer one.
static WORKSPACE_SNAPSHOT_FLUSH_LOCK: Lazy<PlMutex<()>> = Lazy::new(|| PlMutex::new(()));

/// Hard cap on how many workspace snapshots we keep in memory + on disk.
/// 200 is generous (each entry is small JSON: tabs + AI session summaries)
/// while still preventing the file from growing without bound.
pub const MAX_WORKSPACE_SNAPSHOTS: usize = 200;
const MAX_WORKSPACE_SNAPSHOT_KEY_BYTES: usize = 32 * 1024;
const MAX_WORKSPACE_SNAPSHOT_VALUE_BYTES: usize = 8 * 1024 * 1024;
const MAX_WORKSPACE_SNAPSHOT_STORE_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Clone)]
pub struct SnapshotEntry {
    pub value: serde_json::Value,
    pub last_touched_at: std::time::Instant,
}

/// Path to the on-disk JSON file. Resolved at runtime via Tauri's path API so
/// it lands in the platform-correct config directory for the running app.
pub static WORKSPACE_SNAPSHOTS_PATH: once_cell::sync::Lazy<PlMutex<Option<std::path::PathBuf>>> =
    once_cell::sync::Lazy::new(|| PlMutex::new(None));

pub(crate) fn validate_workspace_snapshot_key(path: &str) -> Result<(), AppCommandError> {
    if path.is_empty()
        || path.len() > MAX_WORKSPACE_SNAPSHOT_KEY_BYTES
        || path.contains('\0')
        || !std::path::Path::new(path).is_absolute()
    {
        return Err(AppCommandError::InvalidWorkspacePath(path.to_string()));
    }
    Ok(())
}

pub(crate) fn validate_workspace_snapshot(
    path: &str,
    snapshot: &serde_json::Value,
) -> Result<(), AppCommandError> {
    validate_workspace_snapshot_key(path)?;
    let encoded_len = serde_json::to_vec(snapshot)
        .map_err(|e| AppCommandError::WriteWorkspaceSnapshots(format!("serialize: {}", e)))?
        .len();
    if encoded_len > MAX_WORKSPACE_SNAPSHOT_VALUE_BYTES {
        return Err(AppCommandError::WriteWorkspaceSnapshots(format!(
            "snapshot payload too large: {} bytes (max {})",
            encoded_len, MAX_WORKSPACE_SNAPSHOT_VALUE_BYTES
        )));
    }
    Ok(())
}

fn encoded_store_entry_bytes(
    path: &str,
    snapshot: &serde_json::Value,
) -> Result<u64, AppCommandError> {
    let key_bytes = serde_json::to_vec(path)
        .map_err(|e| AppCommandError::WriteWorkspaceSnapshots(format!("serialize key: {e}")))?
        .len() as u64;
    let value_bytes = serde_json::to_vec(snapshot)
        .map_err(|e| AppCommandError::WriteWorkspaceSnapshots(format!("serialize value: {e}")))?
        .len() as u64;
    key_bytes
        .checked_add(value_bytes)
        .and_then(|bytes| bytes.checked_add(1)) // ':'
        .ok_or_else(|| AppCommandError::WriteWorkspaceSnapshots("snapshot size overflow".into()))
}

/// Validate and insert a snapshot without allowing the in-memory store to grow
/// beyond the same bound enforced for its on-disk representation.
pub(crate) fn upsert_workspace_snapshot(
    path: String,
    snapshot: serde_json::Value,
) -> Result<(), AppCommandError> {
    validate_workspace_snapshot(&path, &snapshot)?;
    let mut snapshots = WORKSPACE_SNAPSHOTS.lock();
    let eviction_candidate = if !snapshots.contains_key(&path)
        && snapshots.len() >= MAX_WORKSPACE_SNAPSHOTS
    {
        snapshots
            .iter()
            .min_by_key(|(_, entry)| entry.last_touched_at)
            .map(|(existing_path, _)| existing_path.clone())
    } else {
        None
    };
    let mut encoded_bytes = 2u64; // opening + closing object braces
    let mut entry_count = 0u64;

    for (existing_path, entry) in snapshots.iter() {
        if existing_path == &path || eviction_candidate.as_ref() == Some(existing_path) {
            continue;
        }
        encoded_bytes = encoded_bytes
            .checked_add(encoded_store_entry_bytes(existing_path, &entry.value)?)
            .ok_or_else(|| {
                AppCommandError::WriteWorkspaceSnapshots("snapshot size overflow".into())
            })?;
        entry_count += 1;
    }
    encoded_bytes = encoded_bytes
        .checked_add(encoded_store_entry_bytes(&path, &snapshot)?)
        .and_then(|bytes| bytes.checked_add(entry_count)) // commas between entries
        .ok_or_else(|| AppCommandError::WriteWorkspaceSnapshots("snapshot size overflow".into()))?;
    if encoded_bytes > MAX_WORKSPACE_SNAPSHOT_STORE_BYTES {
        return Err(AppCommandError::WriteWorkspaceSnapshots(format!(
            "snapshot store too large: {} bytes (max {})",
            encoded_bytes, MAX_WORKSPACE_SNAPSHOT_STORE_BYTES
        )));
    }

    if let Some(victim) = eviction_candidate {
        snapshots.remove(&victim);
        tracing::info!(
            "Evicted workspace snapshot for {} (LRU, cap={})",
            victim,
            MAX_WORKSPACE_SNAPSHOTS
        );
    }
    snapshots.insert(
        path,
        SnapshotEntry {
            value: snapshot,
            last_touched_at: std::time::Instant::now(),
        },
    );
    Ok(())
}

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

    if let Ok(metadata) = std::fs::metadata(&path) {
        if metadata.len() > MAX_WORKSPACE_SNAPSHOT_STORE_BYTES {
            tracing::warn!(
                "Workspace snapshots file is too large ({} bytes > {}); ignoring it",
                metadata.len(),
                MAX_WORKSPACE_SNAPSHOT_STORE_BYTES
            );
            return;
        }
    }

    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
        Err(e) => {
            tracing::warn!("Failed to read workspace snapshots ({}): {}", path.display(), e);
            return;
        }
    };
    if content.len() as u64 > MAX_WORKSPACE_SNAPSHOT_STORE_BYTES {
        tracing::warn!(
            "Workspace snapshots file grew beyond the size limit while reading; ignoring it"
        );
        return;
    }

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
        .filter(|(path, value)| validate_workspace_snapshot(path, value).is_ok())
        .take(MAX_WORKSPACE_SNAPSHOTS)
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
    if snapshots.len() <= MAX_WORKSPACE_SNAPSHOTS {
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
    let _flush_guard = WORKSPACE_SNAPSHOT_FLUSH_LOCK.lock();
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

    let content = serde_json::to_string(&on_disk)
        .map_err(|e| AppCommandError::WriteWorkspaceSnapshots(format!("serialize: {}", e)))?;
    if content.len() as u64 > MAX_WORKSPACE_SNAPSHOT_STORE_BYTES {
        return Err(AppCommandError::WriteWorkspaceSnapshots(format!(
            "snapshot store too large: {} bytes (max {})",
            content.len(), MAX_WORKSPACE_SNAPSHOT_STORE_BYTES
        )));
    }

    crate::fs_utils::atomic_write(&path, content.as_bytes())
        .map_err(|e| AppCommandError::WriteWorkspaceSnapshots(format!("atomic write: {}", e)))?;
    Ok(())
}
