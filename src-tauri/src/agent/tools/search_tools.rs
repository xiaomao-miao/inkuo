//! Search tools: list_dir, glob, grep

use glob::Pattern;
use serde_json::Value;
use std::path::{Path, PathBuf};

use super::{ToolDefinition, ToolError, ToolParameters, validate_workspace_path};

const MAX_GLOB_DEPTH: usize = 20;
const GLOB_MAX_RESULTS: usize = 1000;

pub struct ListDirTool;

impl ListDirTool {
    pub fn new() -> Self { Self }
    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition::new_with_label(
            "list_dir",
            "列出目录",
            "List the contents of a directory.",
            ToolParameters::new(
                vec!["path"],
                vec![
                    ("path", "string", Some("Absolute path to the directory to list")),
                ],
            ),
        )
    }
    pub async fn execute(&self, arguments: Value, workspace: Option<String>) -> Result<String, ToolError> {
        let path = arguments["path"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("list_dir".to_string(), "path must be a string".into()))?;

        validate_workspace_path(path, &workspace)?;

        let mut entries = Vec::new();

        let mut dir = tokio::fs::read_dir(path)
            .await
            .map_err(|e| ToolError::IoError(format!("Failed to read directory {}: {}", path, e)))?;

        while let Some(entry) = dir.next_entry().await.map_err(|e| ToolError::IoError(e.to_string()))? {
            let file_type = entry.file_type().await.map_err(|e| ToolError::IoError(e.to_string()))?;
            let name = entry.file_name().to_string_lossy().to_string();

            let entry_type = if file_type.is_dir() {
                "[DIR]"
            } else if file_type.is_symlink() {
                "[SYMLINK]"
            } else {
                "[FILE]"
            };

            entries.push(format!("{} {}", entry_type, name));
        }

        entries.sort();
        entries.insert(0, format!("Contents of '{}':", path));
        Ok(entries.join("\n"))
    }
}

impl Default for ListDirTool {
    fn default() -> Self { Self::new() }
}

pub struct GlobTool;

impl GlobTool {
    pub fn new() -> Self { Self }
    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition::new_with_label(
            "glob",
            "查找文件",
            "Find all files matching a glob pattern.",
            ToolParameters::new(
                vec!["pattern", "base_dir"],
                vec![
                    ("pattern", "string", Some("Glob pattern to match (e.g., '**/*.rs', 'src/**/*.{ts,tsx}')")),
                    ("base_dir", "string", Some("Base directory to search from")),
                ],
            ),
        )
    }
    pub async fn execute(&self, arguments: Value, workspace: Option<String>) -> Result<String, ToolError> {
        let pattern = arguments["pattern"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("glob".to_string(), "pattern must be a string".into()))?;

        let base_dir = arguments["base_dir"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("glob".to_string(), "base_dir must be a string".into()))?;

        validate_workspace_path(base_dir, &workspace)?;

        let base = Path::new(base_dir);
        let pattern_for_match = Pattern::new(pattern)
            .map_err(|e| ToolError::ExecutionError(format!("Invalid glob pattern: {}", e)))?;

        let mut files: Vec<String> = Vec::new();
        let mut dirs_to_visit: Vec<PathBuf> = vec![base.to_path_buf()];

        while let Some(current_dir) = dirs_to_visit.pop() {
            if files.len() >= GLOB_MAX_RESULTS {
                files.push(format!("[... truncated: {} files omitted ...]", files.len()));
                break;
            }

            let mut dir = match tokio::fs::read_dir(&current_dir).await {
                Ok(d) => d,
                Err(_) => continue, // Skip directories we can't read
            };

            while let Some(entry) = dir.next_entry().await.map_err(|e| ToolError::IoError(e.to_string()))? {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().to_string();

                // Skip hidden files and common exclusions
                if name.starts_with('.') || name == "node_modules" || name == "target" {
                    continue;
                }

                let file_type = match entry.file_type().await {
                    Ok(ft) => ft,
                    Err(_) => continue,
                };

                // Skip symlinks to avoid loops (especially Steam's wine paths)
                if file_type.is_symlink() {
                    continue;
                }

                if file_type.is_dir() {
                    // Only descend if within depth limit
                    if current_dir.components().count() < base.components().count() + MAX_GLOB_DEPTH {
                        dirs_to_visit.push(path.clone());
                    }
                } else if file_type.is_file() {
                    // For non-absolute patterns, match against the relative path from base_dir
                    let rel_path: String = if let Ok(stripped) = path.strip_prefix(base_dir) {
                        stripped.to_string_lossy().trim_start_matches('/').to_string()
                    } else {
                        path.to_string_lossy().trim_start_matches('/').to_string()
                    };

                    if pattern_for_match.matches(&rel_path) {
                        files.push(rel_path);
                    }
                }
            }
        }

        files.sort();
        if files.is_empty() {
            Ok(format!("No files matching pattern '{}' found in '{}'", pattern, base_dir))
        } else {
            Ok(format!("Found {} file(s):\n{}", files.len(), files.join("\n")))
        }
    }
}

impl Default for GlobTool {
    fn default() -> Self { Self::new() }
}

pub struct GrepTool;

impl GrepTool {
    pub fn new() -> Self { Self }
    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition::new_with_label(
            "grep",
            "搜索文本",
            "Search for lines containing a substring (case-insensitive by default) in files. \
            This tool performs literal substring matching, NOT regex. \
            For regex / advanced queries, delegate to `code_expert` which can shell out to `rg`.",
            ToolParameters::new(
                vec!["pattern", "paths"],
                vec![
                    ("pattern", "string", Some("Literal substring to search for. Not a regex.")),
                    ("paths", "array", Some("Array of file/directory paths to search in")),
                    ("case_sensitive", "boolean", Some("Whether search should be case sensitive. Default: false")),
                ],
            ),
        )
    }
    pub async fn execute(&self, arguments: Value, workspace: Option<String>) -> Result<String, ToolError> {
        let pattern = arguments["pattern"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("grep".to_string(), "pattern must be a string".into()))?;

        let paths = arguments["paths"]
            .as_array()
            .ok_or_else(|| ToolError::InvalidArguments("grep".to_string(), "paths must be an array".into()))?;

        let case_sensitive = arguments["case_sensitive"].as_bool().unwrap_or(false);

        if paths.is_empty() {
            return Err(ToolError::InvalidArguments("grep".to_string(), "paths array cannot be empty".into()));
        }

        let mut results: Vec<String> = Vec::new();
        let pattern_lower = if case_sensitive {
            pattern.to_string()
        } else {
            pattern.to_lowercase()
        };

        for path_val in paths {
            let path = path_val.as_str().ok_or_else(|| {
                ToolError::InvalidArguments("grep".to_string(), "path must be a string".into())
            })?;

            if let Err(e) = validate_workspace_path(path, &workspace) {
                results.push(format!("[Path validation error] {}", e));
                continue;
            }

            let path_obj = Path::new(path);

            if path_obj.is_file() {
                let content = tokio::fs::read_to_string(path)
                    .await
                    .map_err(|e| ToolError::IoError(format!("Failed to read {}: {}", path, e)))?;

                let search_content = if case_sensitive {
                    content.clone()
                } else {
                    content.to_lowercase()
                };

                for (line_num, line) in search_content.lines().enumerate() {
                    if line.contains(&pattern_lower) {
                        let original_line = content.lines().nth(line_num).unwrap_or("");
                        results.push(format!("{}:{}: {}", path, line_num + 1, original_line));
                    }
                }
            } else if path_obj.is_dir() {
                grep_directory_traverse(
                    path,
                    &pattern_lower,
                    case_sensitive,
                    workspace.as_deref(),
                    &mut results,
                )
                .await?;
            }
        }

        if results.is_empty() {
            Ok(format!("No matches found for '{}'", pattern))
        } else {
            Ok(format!("Found {} match(es):\n{}", results.len(), results.join("\n")))
        }
    }
}

impl Default for GrepTool {
    fn default() -> Self { Self::new() }
}

async fn grep_directory_traverse(
    dir: &str,
    pattern: &str,
    case_sensitive: bool,
    workspace: Option<&str>,
    results: &mut Vec<String>,
) -> Result<(), ToolError> {
    let mut dir_entries = tokio::fs::read_dir(dir)
        .await
        .map_err(|e| ToolError::IoError(format!("Failed to read directory {}: {}", dir, e)))?;

    while let Some(entry) = dir_entries.next_entry().await.map_err(|e| ToolError::IoError(e.to_string()))? {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        if name.starts_with('.') || name == "node_modules" || name == "target" || name == ".git" {
            continue;
        }

        // Reject symlinks outright: a symlink inside the workspace can point
        // outside it, which would bypass the workspace boundary the caller
        // expects. Skipping them entirely is the safest behavior; legitimate
        // reads of symlinked files can still go through file_tools.
        let file_type = entry.file_type().await.map_err(|e| ToolError::IoError(e.to_string()))?;
        if file_type.is_symlink() {
            continue;
        }

        let path_str = path.to_string_lossy().to_string();

        if file_type.is_dir() {
            // Defensive re-validation: the entry-level `validate_workspace_path`
            // check at the public entrypoint only sees the user-supplied path.
            // When recursing, follow-up directories may themselves be reachable
            // only via a symlink that the OS resolved for us; canonicalize and
            // confirm we're still inside the workspace before descending.
            if let Some(root) = workspace {
                if let Ok(canonical) = std::fs::canonicalize(&path) {
                    if let Ok(root_canonical) = std::fs::canonicalize(root) {
                        if !canonical.starts_with(&root_canonical) {
                            continue;
                        }
                    }
                }
            }
            Box::pin(grep_directory_traverse(
                &path_str,
                pattern,
                case_sensitive,
                workspace,
                results,
            ))
            .await?;
        } else if file_type.is_file() {
            // Try to read as text - if it fails (binary file), skip silently
            if let Ok(content) = tokio::fs::read_to_string(&path).await {
                let search_content = if case_sensitive {
                    content.clone()
                } else {
                    content.to_lowercase()
                };

                for (line_num, line) in search_content.lines().enumerate() {
                    if line.contains(pattern) {
                        let original_line = content.lines().nth(line_num).unwrap_or("");
                        results.push(format!("{}:{}: {}", path_str, line_num + 1, original_line));
                    }
                }
            }
        }
    }

    Ok(())
}
