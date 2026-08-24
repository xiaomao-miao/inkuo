//! Backup management utilities

use sha2::{Digest, Sha256};
use std::collections::HashMap;
use tokio::sync::mpsc;

/// Get the backup directory path (e.g. ~/.inkuo/backups/)
pub fn get_backup_dir() -> std::path::PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("inkuo")
        .join("backups")
}

/// Create a backup file path based on the original file path.
/// Uses a hash of the original path to avoid collisions and stores in ~/.inkuo/backups/
pub fn create_backup_path(original_path: &str) -> std::path::PathBuf {
    let mut hasher = Sha256::new();
    hasher.update(original_path.as_bytes());
    // Use 16 hex chars for the hash (64 bits) so collisions on the cleanup
    // key are astronomically unlikely even for a million files.
    let hash = hex::encode(&hasher.finalize()[..8]);

    let filename = std::path::Path::new(original_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    let backup_dir = get_backup_dir();
    let timestamp = format!(
        "{}_{}",
        chrono::Utc::now().timestamp_millis(),
        uuid::Uuid::new_v4()
    );

    // Layout: `{safename}__bak-{hash}__{timestamp}.bak`
    //
    // The leading `__bak-{hash}__` block is delimited by the unique `__bak-`
    // prefix and the trailing `__` separator, so `cleanup_old_backups` can
    // recover the hash even when the user-supplied filename contains
    // underscores. Older versions stored `{filename}_{hash}_{timestamp}.bak`
    // which ambiguated the hash boundary for any input filename that itself
    // contained an underscore; we keep the new format going forward.
    backup_dir.join(format!(
        "{filename}__bak-{hash}__{timestamp}.bak"
    ))
}

/// Extract the per-file hash out of a backup filename produced by
/// `create_backup_path`. Returns `None` for any name that doesn't match the
/// expected `__bak-<hash>__` layout — callers must treat `None` as "unknown
/// key" rather than falling back to a hand-rolled substring scan.
pub fn parse_backup_filename(filename: &str) -> Option<&str> {
    let marker = "__bak-";
    let marker_start = filename.rfind(marker)? + marker.len();
    let rest = &filename[marker_start..];
    let hash_end = rest.find("__")?;
    let hash = &rest[..hash_end];
    if hash.len() == 16 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Some(hash)
    } else {
        None
    }
}

/// Clean up old backup files, keeping only the most recent N backups per original file.
pub fn cleanup_old_backups(max_backups_per_file: usize) {
    let backup_dir = get_backup_dir();

    if !backup_dir.exists() {
        return;
    }

    let mut backups_by_hash: HashMap<String, Vec<std::path::PathBuf>> = HashMap::new();

    if let Ok(entries) = std::fs::read_dir(&backup_dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.extension().map(|e| e == "bak").unwrap_or(false) {
                if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
                    if let Some(hash) = parse_backup_filename(filename) {
                        backups_by_hash
                            .entry(hash.to_string())
                            .or_default()
                            .push(path);
                    }
                }
            }
        }
    }

    for (_, mut backups) in backups_by_hash {
        backups.sort_by(|a, b| {
            std::fs::metadata(b).and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
                .cmp(&std::fs::metadata(a).and_then(|m| m.modified())
                    .unwrap_or(std::time::SystemTime::UNIX_EPOCH))
        });

        for backup in backups.into_iter().skip(max_backups_per_file) {
            let _ = std::fs::remove_file(backup);
        }
    }
}

/// Channel for backup cleanup requests
pub static BACKUP_CLEANUP_TX: std::sync::LazyLock<
    parking_lot::Mutex<Option<mpsc::Sender<()>>>,
> = std::sync::LazyLock::new(|| parking_lot::Mutex::new(None));

/// Initialize background backup cleanup task on the active async runtime
pub fn init_backup_cleanup_task() {
    let (tx, mut rx) = mpsc::channel::<()>(32);

    tauri::async_runtime::spawn(async move {
        let mut pending_cleanups: Vec<tokio::time::Instant> = Vec::new();
        let cleanup_interval = tokio::time::Duration::from_secs(60);
        let debounce_duration = tokio::time::Duration::from_secs(30);

        loop {
            tokio::select! {
                _ = rx.recv() => {
                    pending_cleanups.push(tokio::time::Instant::now() + debounce_duration);
                }
                _ = tokio::time::sleep(cleanup_interval) => {
                    if let Some(next_cleanup) = pending_cleanups.first() {
                        if tokio::time::Instant::now() >= *next_cleanup {
                            cleanup_old_backups(10);
                            pending_cleanups.clear();
                        }
                    }
                }
            }
        }
    });

    *BACKUP_CLEANUP_TX.lock() = Some(tx);
}

/// Request a backup cleanup (will be debounced)
pub fn request_backup_cleanup() {
    if let Some(tx) = BACKUP_CLEANUP_TX.lock().as_ref() {
        let _ = tx.try_send(());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hash_after_user_supplied_filename() {
        assert_eq!(
            parse_backup_filename("quarterly_report.docx__bak-0123456789abcdef__123456.bak"),
            Some("0123456789abcdef")
        );
        assert_eq!(parse_backup_filename("not-a-backup.bak"), None);
        assert_eq!(
            parse_backup_filename("file__bak-not-hex__________123.bak"),
            None
        );
    }
}
