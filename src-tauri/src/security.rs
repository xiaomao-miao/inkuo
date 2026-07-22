//! Workspace boundary validation + related security helpers.
//!
//! Centralises the "is this path inside the workspace?" check that was
//! historically duplicated across agent tools, file utilities, and a
//! future front-end of path-validating commands. Keeping it in one place
//! means a regression here is impossible to miss in code review.

use std::path::Path;

/// Validates that `path` (string form) lies within `workspace`. Returns
/// `Ok(())` when no workspace is configured (e.g. during early startup
/// or unit tests that don't pin the workspace). All path comparisons
/// happen on canonicalised absolute paths so a relative request or a
/// symlink cannot bypass the sandbox.
///
/// **This does NOT check whether `path` exists** — callers should pair it
/// with a separate existence check for read paths. For write paths the
/// not-yet-exists case is the common one (about-to-create).
///
/// Security considerations:
///
/// 1. We canonicalise both the workspace root and the requested path so
///    a symlink inside the workspace can't accidentally point at
///    `/etc/passwd` and be considered "inside".
/// 2. When `path` doesn't exist (typical for write operations) we
///    canonicalise the parent directory instead — that's the directory
///    we will actually write into. Without this check, a request for
///    `/workspace/../../etc/passwd` would canonicalize to `/etc/passwd`,
///    fail the parent comparison, and be rejected — which is correct.
///    The legacy implementation bailed out with `return Ok(())` here,
///    which is a CVE-tier path-traversal bug.
pub fn validate_workspace_path(path: &str, workspace: &Option<String>) -> Result<(), SecurityError> {
    let Some(workspace_root) = workspace else {
        return Ok(());
    };

    let canonical_workspace = match std::fs::canonicalize(workspace_root) {
        Ok(p) => p,
        Err(_) => {
            return Err(SecurityError::PathValidation(
                format!("Workspace path does not exist: {}", workspace_root),
            ));
        }
    };

    let canonical_requested = match std::fs::canonicalize(Path::new(path)) {
        Ok(p) => p,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Path doesn't exist yet — this is OK for write operations.
            // Validate that the *resolved* parent directory lives inside
            // the workspace, so a path like `/workspace/../../etc/passwd`
            // (whose canonicalized parent is `/etc`) can't sneak through.
            let parent = Path::new(path).parent().ok_or_else(|| {
                SecurityError::PathValidation(format!(
                    "Cannot determine parent directory for path: {}",
                    path
                ))
            })?;

            let canonical_parent = std::fs::canonicalize(parent).map_err(|err| {
                SecurityError::PathValidation(format!(
                    "Path parent is inaccessible: {} ({})",
                    path, err
                ))
            })?;

            if !canonical_parent.starts_with(&canonical_workspace) {
                return Err(SecurityError::PathValidation(format!(
                    "Path '{}' is outside the workspace directory '{}'. Access is denied for security reasons.",
                    path, workspace_root
                )));
            }
            return Ok(());
        }
        Err(e) => {
            return Err(SecurityError::PathValidation(format!(
                "Path is inaccessible: {} ({})",
                path, e
            )));
        }
    };

    if !canonical_requested.starts_with(&canonical_workspace) {
        return Err(SecurityError::PathValidation(format!(
            "Path '{}' is outside the workspace directory '{}'. Access is denied for security reasons.",
            path, workspace_root
        )));
    }

    Ok(())
}

/// Errors emitted by the workspace validator. Kept lightweight — the
/// caller wraps it into its own error type (e.g. `ToolError` for agent
/// tools, `AppError` for tauri commands) at the boundary.
#[derive(Debug, Clone, thiserror::Error)]
pub enum SecurityError {
    #[error("{0}")]
    PathValidation(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_path_traversal() {
        let dir = std::env::temp_dir().join("inkuo_security_test");
        let _ = std::fs::create_dir_all(&dir);
        let workspace = dir.to_string_lossy().to_string();

        // `/workspace/../something_else` should escape.
        let traversal = format!("{}/../escaped", workspace);
        let err = validate_workspace_path(&traversal, &Some(workspace.clone())).unwrap_err();
        assert!(matches!(err, SecurityError::PathValidation(_)));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn accepts_existing_inside_path() {
        let dir = std::env::temp_dir().join("inkuo_security_test_inside");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.txt");
        std::fs::write(&file, "ok").unwrap();
        let workspace = dir.to_string_lossy().to_string();

        validate_workspace_path(file.to_str().unwrap(), &Some(workspace.clone()))
            .expect("in-workspace file should validate");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn accepts_nonexistent_in_workspace_parent() {
        let dir = std::env::temp_dir().join("inkuo_security_test_write");
        std::fs::create_dir_all(&dir).unwrap();
        let workspace = dir.to_string_lossy().to_string();

        // Inside the workspace, doesn't exist yet — write case.
        let target = format!("{}/new.txt", workspace);
        validate_workspace_path(&target, &Some(workspace.clone()))
            .expect("non-existent in-workspace file should validate");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_workspace_is_permissive() {
        // Without a workspace boundary, all paths are accepted (used by
        // tests + the early-startup path).
        validate_workspace_path("/anything/at/all.txt", &None).unwrap();
    }
}
