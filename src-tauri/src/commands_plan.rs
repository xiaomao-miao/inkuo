//! Plan mode persistence: write plan output to a hidden workspace-local
//! directory so the user can review/grep plans later, and tear those files
//! down when the plan is consumed (applied) or abandoned (cancelled /
//! session closed). Mirrors Cursor's `.cursor/plans/<plan-id>.md` behavior.

use crate::commands::AppCommandError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::AppHandle;

const PLANS_DIR_NAME: &str = ".inkuo";
const PLANS_SUBDIR: &str = "plans";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanFileResult {
    /// Absolute path to the written plan md file.
    pub path: String,
    /// Plan id used as filename stem (e.g. `plan-2026-07-06-235901-a3f`).
    pub plan_id: String,
}

/// Resolve `<workspace>/.inkuo/plans/`, creating the directory tree on demand.
fn resolve_plans_dir(workspace_path: &str) -> Result<PathBuf, AppCommandError> {
    let workspace = PathBuf::from(workspace_path);
    if !workspace.is_dir() {
        return Err(AppCommandError::PlanWorkspaceMissing(workspace_path.to_string()));
    }
    let dir = workspace.join(PLANS_DIR_NAME).join(PLANS_SUBDIR);
    std::fs::create_dir_all(&dir)
        .map_err(|e| AppCommandError::PlanSaveFailed(format!("create plans dir: {}", e)))?;
    Ok(dir)
}

/// Sanitize a free-form plan id so it's safe to embed in a filename. Keeps
/// alphanumerics, dash, underscore, dot; replaces anything else with `_`.
fn sanitize_plan_id(plan_id: &str) -> String {
    let cleaned: String = plan_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "plan".to_string()
    } else {
        cleaned
    }
}

#[tauri::command]
pub async fn plan_save(
    workspace_path: String,
    plan_id: String,
    content: String,
    _app: AppHandle,
) -> Result<PlanFileResult, AppCommandError> {
    tracing::info!("plan_save - plan_id: {}", plan_id);
    let dir = resolve_plans_dir(&workspace_path)?;
    let safe_id = sanitize_plan_id(&plan_id);
    let path = dir.join(format!("{}.md", safe_id));
    write_atomic(&path, &content)
        .map_err(|e| AppCommandError::PlanSaveFailed(format!("write plan md: {}", e)))?;
    Ok(PlanFileResult {
        path: path.to_string_lossy().to_string(),
        plan_id: safe_id,
    })
}

#[tauri::command]
pub async fn plan_delete(
    workspace_path: String,
    plan_id: String,
    _app: AppHandle,
) -> Result<bool, AppCommandError> {
    tracing::info!("plan_delete - plan_id: {}", plan_id);
    let dir = resolve_plans_dir(&workspace_path)?;
    let safe_id = sanitize_plan_id(&plan_id);
    let path = dir.join(format!("{}.md", safe_id));
    if !path.exists() {
        return Ok(false);
    }
    std::fs::remove_file(&path)
        .map_err(|e| AppCommandError::PlanDeleteFailed(format!("delete plan md: {}", e)))?;
    Ok(true)
}

#[tauri::command]
pub async fn plan_read(
    workspace_path: String,
    plan_id: String,
    _app: AppHandle,
) -> Result<Option<String>, AppCommandError> {
    let dir = resolve_plans_dir(&workspace_path)?;
    let safe_id = sanitize_plan_id(&plan_id);
    let path = dir.join(format!("{}.md", safe_id));
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path)
        .map_err(|e| AppCommandError::PlanReadFailed(format!("read plan md: {}", e)))?;
    Ok(Some(content))
}

/// Atomic write: write to `<path>.tmp`, then rename into place. Avoids
/// leaving a half-written plan md if the process is killed mid-write.
fn write_atomic(path: &Path, content: &str) -> std::io::Result<()> {
    let tmp = path.with_extension("md.tmp");
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}
