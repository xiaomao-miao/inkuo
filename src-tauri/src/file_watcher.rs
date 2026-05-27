//! File system watcher for detecting file changes in the workspace
//! 
//! This module uses the `notify` crate to watch for file system changes
//! and emits events to the frontend when files are created, modified, or deleted.

use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher, Event, EventKind};
use serde::Serialize;
use std::path::PathBuf;
use tokio::sync::mpsc;
use tauri::{AppHandle, Emitter};

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

/// File watcher state shared across the application
pub struct FileWatcherState {
    /// The currently watched directory path
    watched_path: parking_lot::Mutex<Option<PathBuf>>,
    /// The Tauri app handle for emitting events
    app_handle: parking_lot::Mutex<Option<AppHandle>>,
}

impl FileWatcherState {
    pub fn new() -> Self {
        Self {
            watched_path: parking_lot::Mutex::new(None),
            app_handle: parking_lot::Mutex::new(None),
        }
    }

    /// Start watching a directory for file changes
    pub fn watch(&self, path: PathBuf, app_handle: AppHandle) -> Result<(), String> {
        // Stop any existing watcher
        self.stop();

        // Store the app handle for emitting events
        *self.app_handle.lock() = Some(app_handle.clone());

        // Clone path for the watcher
        let watch_path = path.clone();
        let watched_path = path.clone();

        // Create a channel for receiving events
        let (tx, mut rx) = mpsc::channel::<Event>(100);

        // Create the watcher
        let mut watcher = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                if let Ok(event) = res {
                    let _ = tx.blocking_send(event);
                }
            },
            Config::default(),
        ).map_err(|e| format!("Failed to create watcher: {}", e))?;

        // Start watching
        watcher.watch(&watch_path, RecursiveMode::Recursive)
            .map_err(|e| format!("Failed to watch directory: {}", e))?;

        // Store the watched path
        *self.watched_path.lock() = Some(watched_path);

        // Spawn a task to handle events
        let app_handle_clone = app_handle.clone();
        tauri::async_runtime::spawn(async move {
            while let Some(event) = rx.recv().await {
                Self::handle_event(&app_handle_clone, event);
            }
        });

        tracing::info!("Started watching directory: {:?}", path);
        Ok(())
    }

    /// Stop watching the current directory
    pub fn stop(&self) {
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
                let _ = app_handle.emit("file-change", change_event);
            }
        }
    }
}

impl Default for FileWatcherState {
    fn default() -> Self {
        Self::new()
    }
}
