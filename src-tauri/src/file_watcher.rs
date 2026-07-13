//! File system watcher for detecting file changes in the workspace
//!
//! This module uses the `notify` crate's `PollWatcher` to watch for file system
//! changes and emits events to the frontend when files are created, modified,
//! or deleted. `PollWatcher` is used instead of `RecommendedWatcher` (inotify
//! on Linux) because inotify is unreliable for in-process writes and can miss
//! events on some filesystems / Docker / overlay setups. Polling is slower but
//! reliably catches every change.

use notify::{poll::PollWatcher, Config, Event, EventKind, RecursiveMode, Watcher};
use serde::Serialize;
use std::path::PathBuf;
use std::time::Duration;
use thiserror::Error;
use tauri::{AppHandle, Emitter};
use tokio::sync::{mpsc, oneshot};

/// File change event sent to the frontend
#[derive(Clone, Serialize)]
#[serde(tag = "type", content = "data")]
pub enum FileChangeEvent {
    /// A file was created
    Created { path: String },
    /// A file was modified
    Modified { path: String },
    /// A file was deleted
    Deleted { path: String },
}

/// Emit a file-change event to the frontend.
///
/// Use this from write paths (editor save, AI tools, office writes) so that
/// the file tree refreshes even when the in-process inotify watcher doesn't
/// observe the change (a known limitation on some Linux setups / same-process
/// writes).
pub fn emit_file_change(app_handle: &AppHandle, event: FileChangeEvent) {
    if let Err(error) = app_handle.emit("file-change", event) {
        tracing::warn!("Failed to emit file-change event: {}", error);
    }
}

/// File watcher state shared across the application
#[derive(Debug, Error)]
pub enum FileWatcherError {
    #[error("Failed to create watcher: {0}")]
    CreateWatcher(String),
    #[error("Failed to watch directory: {0}")]
    WatchDirectory(String),
}

/// Handle to an active watcher task, used to signal shutdown
type WatcherAbortHandle = oneshot::Sender<()>;

pub struct FileWatcherState {
    /// Serialize watch()/stop() so concurrent StrictMode-driven invokes
    /// can't race on `watcher` / `abort_handle` (which would orphan the
    /// inotify fd and silently drop all subsequent events).
    lock: parking_lot::Mutex<()>,
    /// The currently watched directory path
    watched_path: parking_lot::Mutex<Option<PathBuf>>,
    /// The Tauri app handle for emitting events
    app_handle: parking_lot::Mutex<Option<AppHandle>>,
    /// The active watcher instance (must drop to truly stop watching)
    watcher: parking_lot::Mutex<Option<PollWatcher>>,
    /// Abort handle for the event-handling task
    abort_handle: parking_lot::Mutex<Option<WatcherAbortHandle>>,
}

impl FileWatcherState {
    pub fn new() -> Self {
        Self {
            lock: parking_lot::Mutex::new(()),
            watched_path: parking_lot::Mutex::new(None),
            app_handle: parking_lot::Mutex::new(None),
            watcher: parking_lot::Mutex::new(None),
            abort_handle: parking_lot::Mutex::new(None),
        }
    }

    /// Start watching a directory for file changes
    pub fn watch(&self, path: PathBuf, app_handle: AppHandle) -> Result<(), FileWatcherError> {
        // Serialize with stop() so StrictMode-driven double-invokes
        // can't race on watcher/abort_handle.
        let _guard = self.lock.lock();

        // Stop any existing watcher first (drops old watcher + aborts old task)
        self.stop_locked();

        // Store the app handle for emitting events
        *self.app_handle.lock() = Some(app_handle.clone());

        // Clone path for the watcher
        let watch_path = path.clone();
        let watched_path = path.clone();

        // Create a channel for receiving events
        let (tx, mut rx) = mpsc::channel::<Event>(100);

        // Create the abort channel
        let (abort_tx, abort_rx) = oneshot::channel::<()>();

        // Create the watcher
        // PollWatcher is used instead of RecommendedWatcher (inotify on Linux)
        // because inotify has known reliability issues for in-process writes
        // and on some filesystems (overlayfs, Docker bind mounts). Polling
        // catches every change reliably at the cost of higher CPU/IO.
        let mut watcher = PollWatcher::new(
            move |res: Result<Event, notify::Error>| {
                if let Ok(event) = res {
                    if let Err(error) = tx.blocking_send(event) {
                        tracing::warn!("Failed to forward file watcher event: {}", error);
                    }
                }
            },
            // Tuned for snappy UI feedback when an outside editor (or the
            // sidebar's inline-create flow that forgot to refetch) lands a
            // write the watcher otherwise wouldn't see. 2s is too sluggish —
            // users perceive that as "the file tree didn't refresh until I
            // clicked 刷新". 500ms is the smallest interval at which we
            // can still tolerate polling on every watched workspace.
            Config::default().with_poll_interval(Duration::from_millis(500)),
        )
        .map_err(|e| FileWatcherError::CreateWatcher(e.to_string()))?;

        // Start watching
        watcher
            .watch(&watch_path, RecursiveMode::Recursive)
            .map_err(|e| FileWatcherError::WatchDirectory(e.to_string()))?;

        // Store the watched path
        *self.watched_path.lock() = Some(watched_path);

        // Store the watcher so it lives for the duration of the struct
        *self.watcher.lock() = Some(watcher);

        // Spawn a task to handle events
        let app_handle_clone = app_handle.clone();
        tauri::async_runtime::spawn(async move {
            tokio::select! {
                _ = abort_rx => {
                    tracing::info!("File watcher task收到了中止信号，退出");
                }
                _ = async {
                    while let Some(event) = rx.recv().await {
                        Self::handle_event(&app_handle_clone, event);
                    }
                } => {}
            }
        });

        // Store the abort handle
        *self.abort_handle.lock() = Some(abort_tx);

        tracing::info!("Started watching directory: {:?}", path);

        Ok(())
    }

    /// Currently-watched directory path, if any. Used by callers that need
    /// to confirm a `stop()` request applies to the active watcher (e.g.
    /// `commands::unwatch_directory`).
    pub fn watched_path(&self) -> Option<PathBuf> {
        self.watched_path.lock().clone()
    }

    /// Stop watching the current directory and release all resources
    pub fn stop(&self) {
        let _guard = self.lock.lock();
        self.stop_locked();
    }

    /// Internal: caller must hold `self.lock`.
    fn stop_locked(&self) {
        // Abort the event-handling task first
        if let Some(abort) = self.abort_handle.lock().take() {
            let _ = abort.send(());
        }

        // Drop the watcher to release the file system handle
        self.watcher.lock().take();

        // Clear the path
        if let Some(path) = self.watched_path.lock().take() {
            tracing::info!("Stopped watching directory: {:?}", path);
        }
    }

    /// Handle a file system event and emit to frontend
    fn handle_event(app_handle: &AppHandle, event: Event) {
        for path in event.paths {
            let path_str = path.to_string_lossy().to_string();

            // Skip hidden files and directories
            if path_str.contains("/.") || path_str.contains("\\.") {
                continue;
            }

            let change_event = match event.kind {
                EventKind::Create(_) => Some(FileChangeEvent::Created { path: path_str }),
                EventKind::Modify(_) => Some(FileChangeEvent::Modified { path: path_str }),
                EventKind::Remove(_) => Some(FileChangeEvent::Deleted { path: path_str }),
                _ => None,
            };

            if let Some(change_event) = change_event {
                emit_file_change(app_handle, change_event);
            }
        }
    }
}

impl Default for FileWatcherState {
    fn default() -> Self {
        Self::new()
    }
}
