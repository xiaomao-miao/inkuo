//! File-tree context-menu Tauri commands.
//!
//! Pulled out of `mod.rs` because the nine handlers below (`create_file_entry`,
//! `rename_path`, `delete_path`, `copy_path`, `move_path`, `path_exists`,
//! `open_with_default_app`, `reveal_in_file_manager`, `create_new_window`)
//! plus their three request/response types (`NewEntryPayload`,
//! `CreateEntryResult`, `RenamePathResult`) form a self-contained ~280-line
//! group with no shared state beyond the Tauri [`AppHandle`] and the
//! [`AppCommandError`] alias. Splitting them out leaves `mod.rs` focused on
//! the larger document / AI / settings / snapshot surfaces.
//!
//! Public surface kept stable via `pub use context_menu::*` in `mod.rs`,
//! so callers like `lib.rs` continue to register `commands::create_file_entry`
//! etc. without learning the sub-module path.

use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

use super::AppCommandError;
use crate::file_watcher::{emit_file_change, FileChangeEvent};

// ============================================================================

// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NewEntryPayload {
    /// A regular file. `template` is the optional initial content (e.g.
    /// `# Heading` for markdown). `extension` includes the leading dot.
    File { extension: String, template: Option<String> },
    /// A directory.
    Directory,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateEntryResult {
    pub path: String,
}

#[tauri::command]
pub async fn create_file_entry(
    parent: String,
    name: String,
    payload: NewEntryPayload,
    app_handle: AppHandle,
) -> Result<CreateEntryResult, AppCommandError> {
    let trimmed_name = name.trim();
    if trimmed_name.is_empty() {
        return Err(AppCommandError::CreateEntry("名称不能为空".to_string()));
    }
    if trimmed_name.contains('/') || trimmed_name.contains('\\') {
        return Err(AppCommandError::CreateEntry("名称不能包含路径分隔符".to_string()));
    }

    let parent_path = std::path::Path::new(&parent);
    if !parent_path.exists() {
        return Err(AppCommandError::CreateEntry(format!("父目录不存在: {}", parent)));
    }
    if !parent_path.is_dir() {
        return Err(AppCommandError::CreateEntry(format!("不是目录: {}", parent)));
    }

    let target = parent_path.join(trimmed_name);
    if target.exists() {
        return Err(AppCommandError::TargetExists);
    }

    match payload {
        NewEntryPayload::Directory => {
            std::fs::create_dir_all(&target)
                .map_err(|e| AppCommandError::CreateEntry(e.to_string()))?;
        }
        NewEntryPayload::File { extension, template } => {
            let ext_clean = extension.trim_start_matches('.').to_string();
            let file_name = if ext_clean.is_empty() {
                trimmed_name.to_string()
            } else if trimmed_name.to_lowercase().ends_with(&format!(".{}", ext_clean.to_lowercase())) {
                trimmed_name.to_string()
            } else {
                format!("{}.{}", trimmed_name, ext_clean)
            };
            let final_path = parent_path.join(&file_name);
            if final_path.exists() {
                return Err(AppCommandError::TargetExists);
            }
            let content = template.unwrap_or_default();
            std::fs::write(&final_path, content)
                .map_err(|e| AppCommandError::CreateEntry(e.to_string()))?;
            let final_str = final_path.to_string_lossy().to_string();
            emit_file_change(&app_handle, FileChangeEvent::Created { path: final_str.clone() });
            return Ok(CreateEntryResult { path: final_str });
        }
    }

    let final_str = target.to_string_lossy().to_string();
    emit_file_change(&app_handle, FileChangeEvent::Created { path: final_str.clone() });
    Ok(CreateEntryResult { path: final_str })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenamePathResult {
    pub from: String,
    pub to: String,
}

#[tauri::command]
pub async fn rename_path(
    from: String,
    to: String,
    app_handle: AppHandle,
) -> Result<RenamePathResult, AppCommandError> {
    let from_path = std::path::Path::new(&from);
    if !from_path.exists() {
        return Err(AppCommandError::RenamePath(format!("源路径不存在: {}", from)));
    }
    let to_path = std::path::Path::new(&to);
    if let Some(parent) = to_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| AppCommandError::RenamePath(e.to_string()))?;
    }
    if to_path.exists() && from_path != to_path {
        return Err(AppCommandError::TargetExists);
    }
    std::fs::rename(from_path, to_path)
        .map_err(|e| AppCommandError::RenamePath(e.to_string()))?;

    // Emit both sides so caches for the old parent and new parent refresh
    // atomically.
    emit_file_change(&app_handle, FileChangeEvent::Deleted { path: from.clone() });
    emit_file_change(&app_handle, FileChangeEvent::Created { path: to.clone() });
    Ok(RenamePathResult { from, to })
}

#[tauri::command]
pub async fn delete_path(
    path: String,
    recursive: bool,
    app_handle: AppHandle,
) -> Result<(), AppCommandError> {
    let p = std::path::Path::new(&path);
    if !p.exists() {
        // Idempotent: deleting a missing path is a no-op.
        return Ok(());
    }
    let metadata = std::fs::metadata(p)
        .map_err(|e| AppCommandError::DeletePath(e.to_string()))?;
    if metadata.is_dir() {
        if !recursive {
            return Err(AppCommandError::DeletePath(
                "目录需要启用 recursive 选项".to_string(),
            ));
        }
        std::fs::remove_dir_all(p)
            .map_err(|e| AppCommandError::DeletePath(e.to_string()))?;
    } else {
        std::fs::remove_file(p)
            .map_err(|e| AppCommandError::DeletePath(e.to_string()))?;
    }
    emit_file_change(&app_handle, FileChangeEvent::Deleted { path });
    Ok(())
}

#[tauri::command]
pub async fn copy_path(
    from: String,
    to: String,
    app_handle: AppHandle,
) -> Result<(), AppCommandError> {
    let from_path = std::path::Path::new(&from);
    if !from_path.exists() {
        return Err(AppCommandError::CopyPath(format!("源路径不存在: {}", from)));
    }
    let to_path = std::path::Path::new(&to);
    if to_path.exists() {
        return Err(AppCommandError::TargetExists);
    }
    if let Some(parent) = to_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| AppCommandError::CopyPath(e.to_string()))?;
    }

    let metadata = std::fs::metadata(from_path)
        .map_err(|e| AppCommandError::CopyPath(e.to_string()))?;
    if metadata.is_dir() {
        copy_dir_recursive(from_path, to_path)
            .map_err(|e| AppCommandError::CopyPath(e.to_string()))?;
    } else {
        std::fs::copy(from_path, to_path)
            .map_err(|e| AppCommandError::CopyPath(e.to_string()))?;
    }

    emit_file_change(&app_handle, FileChangeEvent::Created { path: to });
    Ok(())
}

#[tauri::command]
pub async fn move_path(
    from: String,
    to: String,
    app_handle: AppHandle,
) -> Result<(), AppCommandError> {
    let from_path = std::path::Path::new(&from);
    if !from_path.exists() {
        return Err(AppCommandError::MovePath(format!("源路径不存在: {}", from)));
    }
    let to_path = std::path::Path::new(&to);
    if to_path.exists() && from_path != to_path {
        return Err(AppCommandError::TargetExists);
    }
    if let Some(parent) = to_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| AppCommandError::MovePath(e.to_string()))?;
    }
    std::fs::rename(from_path, to_path)
        .map_err(|e| AppCommandError::MovePath(e.to_string()))?;
    emit_file_change(&app_handle, FileChangeEvent::Deleted { path: from });
    emit_file_change(&app_handle, FileChangeEvent::Created { path: to });
    Ok(())
}

#[tauri::command]
pub async fn path_exists(path: String) -> Result<bool, AppCommandError> {
    Ok(std::path::Path::new(&path).exists())
}

#[tauri::command]
pub async fn open_with_default_app(
    path: String,
    app_handle: AppHandle,
) -> Result<(), AppCommandError> {
    app_handle
        .opener()
        .open_path(path, None::<&str>)
        .map_err(|e| AppCommandError::OpenWithDefaultApp(e.to_string()))
}

#[tauri::command]
pub async fn reveal_in_file_manager(
    path: String,
    app_handle: AppHandle,
) -> Result<(), AppCommandError> {
    app_handle
        .opener()
        .reveal_item_in_dir(&path)
        .map_err(|e| AppCommandError::RevealInFileManager(e.to_string()))
}

#[tauri::command]
pub async fn create_new_window(app_handle: AppHandle) -> Result<(), AppCommandError> {
    tracing::info!("Creating new window");

    use tauri::WebviewWindowBuilder;
    use tauri::WebviewUrl;

    WebviewWindowBuilder::new(
        &app_handle,
        &format!("main-{}", uuid::Uuid::new_v4()),
        WebviewUrl::App("index.html".into()),
    )
    .title("inkuo")
    .inner_size(1200.0, 800.0)
    .min_inner_size(800.0, 600.0)
    // Mark this window as a "fresh" window via a global JS variable so the
    // frontend can clear the previously persisted workspace and show the
    // welcome page. We use initialization_script (a global set before the
    // page scripts run) because Tauri 2's WebviewUrl::App is a PathBuf and
    // does not propagate query strings to the webview in dev mode.
    .initialization_script("window.__INKUO_FRESH_WINDOW__ = true;")
    .build()
    .map_err(|e| AppCommandError::CreateEntry(e.to_string()))?;

    Ok(())
}

fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else if file_type.is_symlink() {
            #[cfg(unix)]
            {
                let target = std::fs::read_link(&from)?;
                std::os::unix::fs::symlink(&target, &to)?;
            }
            #[cfg(not(unix))]
            {
                // On Windows, fall back to copying the symlink target's bytes.
                std::fs::copy(&from, &to)?;
            }
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

