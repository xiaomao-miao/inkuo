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
    let hash = hex::encode(&hasher.finalize()[..8]);

    let filename = std::path::Path::new(original_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    let backup_dir = get_backup_dir();
    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");

    backup_dir.join(format!("{}_{}_{}.bak", filename, hash, timestamp))
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
                    if let Some(last_underscore) = filename.rfind('_') {
                        if let Some(second_last) = filename[..last_underscore].rfind('_') {
                            let hash = &filename[second_last + 1..last_underscore];
                            backups_by_hash
                                .entry(hash.to_string())
                                .or_default()
                                .push(path);
                        }
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
