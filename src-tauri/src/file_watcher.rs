//! File system watcher for detecting file changes in the workspace.
//!
//! The OS watcher uses [`notify_debouncer_full`] (built on top of `notify`'s
//! recommended backend — `PollWatcher` is intentionally avoided because polling
//! is the very thing the debouncer is meant to replace). The debouncer batches
//! bursts of low-level events that the kernel reports for a single user-visible
//! change (e.g. `truncate` + `write` + `close-write` for one save) into one
//! quiet window of activity.
//!
//! What we send to the frontend is **not** a per-file event. We drain the
//! debounced batch into a `HashSet<String>` of directory paths, then emit a
//! single `dirs-changed` payload listing every parent directory that needs to
//! be re-listed. The frontend debounces per-directory and re-fetches only the
//! affected caches. This avoids two long-standing failure modes:
//!   * a single save firing dozens of OS events → IPC saturation + UI flicker;
//!   * a multi-directory burst collapsing to one refresh because of a flat
//!     single-instance debounce.
//!
//! `emit_file_change(...)` is a separate helper used by `commands::*` and
//! `snapshots` to publish a single, semantic change (create / rename / delete)
//! for changes the OS watcher may not observe reliably. Those call sites stay
//! untouched — they speak the old `file-change` event on purpose so the
//! watcher's queueing logic cannot accidentally drop in-app mutations.
//!
//! Path normalisation on the receiving end (`src/utils/path.ts::normalizeDirPath`)
//! collapses both `/` and `\` separators, so emitting the native separator
//! (as the OS reports it) is fine and matches the previous behaviour.

use notify_debouncer_full::{new_debouncer, DebounceEventResult, Debouncer, RecommendedCache};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use parking_lot::Mutex;
use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;
use thiserror::Error;
use tauri::{AppHandle, Emitter};

/// Quiet window used by `notify-debouncer-full`. Tuned to coalesce the typical
/// "truncate + write + close" pattern of a single editor save while still
/// feeling immediate to the user. 200 ms matches Coffee-CLI and sits inside
/// the 100–300 ms band used by VS Code / cmdr.
const DEBOUNCE_WINDOW_MS: u64 = 200;

/// One debouncer tick: every parent directory whose listing may have changed
/// during the latest quiet window. Emitted as a single Tauri event.
#[derive(Clone, Serialize)]
pub struct DirsChangedPayload {
    pub dirs: Vec<String>,
}

/// Per-file event kept for the legacy `file-change` channel that the in-app
/// mutation commands (`create_file_entry`, `rename_path`, `delete_path`, …)
/// still emit. The OS watcher does NOT publish this anymore — it only
/// publishes `dirs-changed`.
#[derive(Clone, Serialize)]
#[serde(tag = "type", content = "data")]
pub enum FileChangeEvent {
    Created { path: String },
    Modified { path: String },
    Deleted { path: String },
}

/// Emit a single semantic `file-change` event. Used by write paths so the
/// tree refreshes even when the in-process OS watcher misses the change.
pub fn emit_file_change(app_handle: &AppHandle, event: FileChangeEvent) {
    if let Err(error) = app_handle.emit("file-change", event) {
        tracing::warn!("Failed to emit file-change event: {}", error);
    }
}

/// File watcher state shared across the application.
#[derive(Debug, Error)]
pub enum FileWatcherError {
    #[error("Failed to create watcher: {0}")]
    CreateWatcher(String),
    #[error("Failed to watch directory: {0}")]
    WatchDirectory(String),
}

/// Returns true when `path` lives under a hidden segment (i.e. some path
/// component starts with `.`).
///
/// Replaces the previous `path_str.contains("/.") || path_str.contains("\\.")`
/// check, which incorrectly dropped events for legitimate directories like
/// `v1.2/` or `2026.archive`.
fn is_under_hidden_segment(path: &Path) -> bool {
    for component in path.components() {
        let raw = component.as_os_str();
        if raw.is_empty() {
            continue;
        }
        // Skip the root / prefix component (e.g. `/` on Unix, `C:\` on
        // Windows) so the path root itself isn't mistaken for a hidden
        // segment.
        if matches!(component, std::path::Component::Prefix(_) | std::path::Component::RootDir) {
            continue;
        }
        if raw.to_string_lossy().starts_with('.') {
            return true;
        }
    }
    false
}

pub struct FileWatcherState {
    /// Serialize `watch()` / `stop()` so StrictMode-driven double-invokes
    /// can't race on the debouncer / abort handle.
    lock: Mutex<()>,
    /// The currently watched directory path.
    watched_path: Mutex<Option<PathBuf>>,
    /// The Tauri app handle for emitting events.
    app_handle: Mutex<Option<AppHandle>>,
    /// The active debouncer instance. Dropping it stops all watching.
    debouncer: Mutex<Option<Debouncer<RecommendedWatcher, RecommendedCache>>>,
}

impl FileWatcherState {
    pub fn new() -> Self {
        Self {
            lock: Mutex::new(()),
            watched_path: Mutex::new(None),
            app_handle: Mutex::new(None),
            debouncer: Mutex::new(None),
        }
    }

    /// Start watching `path` recursively. Stops any prior watcher first so
    /// `watch_directory` is idempotent.
    pub fn watch(&self, path: PathBuf, app_handle: AppHandle) -> Result<(), FileWatcherError> {
        let _guard = self.lock.lock();
        self.stop_locked();

        *self.app_handle.lock() = Some(app_handle.clone());
        *self.watched_path.lock() = Some(path.clone());

        let watch_path = path.clone();
        let tick_rate = Duration::from_millis(DEBOUNCE_WINDOW_MS);

        // The debouncer callback fires on a worker thread owned by
        // `notify-debouncer-full`. We collect every directory that may have
        // changed into a `HashSet`, then forward the dedup'd list to the
        // frontend in a single `dirs-changed` event. Stays on this thread
        // (no async), which is the debouncer's contract.
        let mut debouncer = new_debouncer(
            tick_rate,
            None,
            move |result: DebounceEventResult| match result {
                Ok(events) => {
                    let mut dirs: HashSet<String> = HashSet::new();
                    for event in events {
                        // `DebouncedEvent` derefs to `notify::Event`, so
                        // `event.kind` and `event.paths` resolve through
                        // auto-deref. notify-debouncer-full preserves the
                        // precise `EventKind` (Create / Modify / Remove)
                        // per event; we re-list every affected parent
                        // directory regardless, but `event.kind` is
                        // referenced so the import is used (and tracing
                        // could include it later).
                        let kind = event.kind;
                        for path in &event.paths {
                            if is_under_hidden_segment(path) {
                                continue;
                            }
                            tracing::trace!(?kind, path = %path.display(), "debounced fs event");
                            // The parent is what the tree needs re-listing.
                            // For a newly-created directory, both the
                            // parent AND the directory itself may need
                            // refreshing — the parent so the new entry
                            // appears, the dir so any already-expanded
                            // subtree re-lists.
                            if let Some(parent) = path.parent() {
                                if !parent.as_os_str().is_empty() {
                                    dirs.insert(parent.to_string_lossy().into_owned());
                                }
                            }
                            if path.is_dir() {
                                dirs.insert(path.to_string_lossy().into_owned());
                            }
                        }
                    }
                    if dirs.is_empty() {
                        return;
                    }
                    tracing::debug!(
                        "[watcher] emitting dirs-changed for {} dirs",
                        dirs.len()
                    );
                    let payload = DirsChangedPayload {
                        dirs: dirs.into_iter().collect(),
                    };
                    if let Err(error) = app_handle.emit("dirs-changed", payload) {
                        tracing::warn!("Failed to emit dirs-changed event: {}", error);
                    }
                }
                Err(errors) => {
                    for error in errors {
                        tracing::warn!("File watcher debouncer error: {}", error);
                    }
                }
            },
        )
        .map_err(|e| FileWatcherError::CreateWatcher(e.to_string()))?;

        debouncer
            .watch(&watch_path, RecursiveMode::Recursive)
            .map_err(|e| FileWatcherError::WatchDirectory(e.to_string()))?;

        // Cache a path snapshot from the perspective of the debouncer; the
        // debouncer's watcher takes ownership of the actual recurse handle.
        *self.debouncer.lock() = Some(debouncer);

        tracing::info!("Started watching directory: {:?}", path);
        Ok(())
    }

    /// Currently-watched directory path, if any. Used by callers that need
    /// to confirm a `stop()` request applies to the active watcher.
    pub fn watched_path(&self) -> Option<PathBuf> {
        self.watched_path.lock().clone()
    }

    /// Stop watching the current directory and release all resources.
    pub fn stop(&self) {
        let _guard = self.lock.lock();
        self.stop_locked();
    }

    fn stop_locked(&self) {
        // Dropping the debouncer stops the worker thread and releases the
        // file system handles. The `stop_locked` name is kept to preserve
        // the existing call sites in `watch()` and `stop()`.
        self.debouncer.lock().take();
        if let Some(path) = self.watched_path.lock().take() {
            tracing::info!("Stopped watching directory: {:?}", path);
        }
    }
}

impl Default for FileWatcherState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::is_under_hidden_segment;
    use std::path::Path;

    #[test]
    fn hidden_root_component_is_skipped() {
        // `/.git/HEAD` is hidden because `.git` is a segment starting with '.'.
        assert!(is_under_hidden_segment(Path::new("/.git/HEAD")));
        assert!(is_under_hidden_segment(Path::new("/root/.cache/file")));
    }

    #[test]
    fn versioned_directory_segments_are_not_hidden() {
        // `v1.2` looks hidden to a naive `contains("/.")` check but the
        // segment is named `v1.2`, not `.v1.2`.
        assert!(!is_under_hidden_segment(Path::new("/workspace/v1.2/data")));
        assert!(!is_under_hidden_segment(Path::new("/archive/2026.07/notes")));
    }

    #[test]
    fn plain_paths_are_not_hidden() {
        assert!(!is_under_hidden_segment(Path::new("/root/a.md")));
        assert!(!is_under_hidden_segment(Path::new("/root/sub/file.md")));
    }
}
