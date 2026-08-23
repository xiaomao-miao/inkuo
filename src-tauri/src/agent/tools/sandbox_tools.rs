//! Dependency-free, allowlisted virtual terminal sandbox.
//!
//! The agent can issue familiar terminal command lines, but they are parsed
//! and executed by in-process implementations shipped with the application.
//! There is no host shell, PATH lookup, interpreter, network, package manager,
//! or dependency installation surface.

use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::{validate_workspace_path, ToolDefinition, ToolError, ToolParameters};

const DEFAULT_TIMEOUT_MS: u64 = 3_000;
const MAX_TIMEOUT_MS: u64 = 10_000;
const DEFAULT_OUTPUT_CHARS: usize = 16_000;
const MAX_OUTPUT_CHARS: usize = 64_000;
const MAX_INSPECT_FILE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_TREE_ENTRIES: usize = 5_000;
const MAX_COMMAND_LINE_CHARS: usize = 4_096;
const MAX_CONCURRENT_WORKERS: usize = 2;

static SANDBOX_WORKERS: tokio::sync::Semaphore =
    tokio::sync::Semaphore::const_new(MAX_CONCURRENT_WORKERS);

pub struct SandboxCommandTool;

#[derive(Debug, Serialize)]
struct SandboxOutput {
    status: &'static str,
    command: String,
    output: Value,
    truncated: bool,
}

impl SandboxCommandTool {
    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition::new_with_label(
            "run_sandbox_command",
            "运行沙盒命令",
            "Run one command in the dependency-free inkuo virtual terminal. Use `command_line` with familiar allowlisted syntax: `wc <file>`, `jq . <json-file>`, `sha256sum <file>`, `unzip -l <archive>`, or `find <directory>`. The legacy structured `command` + `path` form is also accepted (`text_stats`, `json_validate`, `sha256`, `zip_list`, `workspace_tree`). Commands are implemented inside the app: there is no host shell, PATH lookup, interpreter, network, package manager, download, environment access, pipe, or redirection. Relative paths resolve from the active workspace and every target is canonicalized and workspace-bounded. Never ask the user to install a dependency. Use only when the Sandbox toggle is enabled.",
            ToolParameters::new(
                vec![],
                vec![
                    (
                        "command_line",
                        "string",
                        Some("Preferred virtual-terminal command line. Exactly one allowlisted command; examples: wc \"notes.txt\", jq . data.json, sha256sum report.docx, unzip -l deck.pptx, find ."),
                    ),
                    (
                        "command",
                        "string",
                        Some("Legacy structured allowlisted command: text_stats, json_validate, sha256, zip_list, or workspace_tree. Supply together with path when command_line is omitted."),
                    ),
                    (
                        "path",
                        "string",
                        Some("Legacy structured target. Absolute or workspace-relative; a directory is required for workspace_tree and a file for all other commands."),
                    ),
                    (
                        "timeout_ms",
                        "integer",
                        Some("Optional timeout in milliseconds (100-10000, default 3000)."),
                    ),
                    (
                        "max_output_chars",
                        "integer",
                        Some("Optional output ceiling in characters (1000-64000, default 16000)."),
                    ),
                ],
            ),
        )
    }

    pub async fn execute(
        &self,
        arguments: Value,
        workspace: Option<String>,
    ) -> Result<String, ToolError> {
        let workspace = workspace
            .filter(|path| !path.trim().is_empty())
            .ok_or_else(|| {
                ToolError::PathValidationError(
                    "run_sandbox_command requires a non-empty active workspace; sandbox diagnostics are disabled without one"
                        .to_string(),
                )
            })?;
        let canonical_workspace = std::fs::canonicalize(&workspace).map_err(|error| {
            ToolError::PathValidationError(format!(
                "sandbox workspace '{}' cannot be resolved: {}",
                workspace, error
            ))
        })?;
        let workspace = Some(canonical_workspace.to_string_lossy().to_string());

        let (command, path, display_command) = parse_command_request(&arguments)?;
        if !matches!(
            command.as_str(),
            "text_stats" | "json_validate" | "sha256" | "zip_list" | "workspace_tree"
        ) {
            return Err(ToolError::InvalidArguments(
                "run_sandbox_command".to_string(),
                format!(
                    "command '{}' is not allowlisted; no shell or package-manager commands are supported",
                    command
                ),
            ));
        }

        // Resolve once, validate that exact target, then give the canonical
        // path to the blocking worker. Reopening the original symlink path in
        // the worker would create a check/use race that could escape the
        // workspace after validation.
        let requested_path = PathBuf::from(&path);
        let resolved_path = if requested_path.is_absolute() {
            requested_path
        } else {
            canonical_workspace.join(requested_path)
        };
        let canonical_path = std::fs::canonicalize(&resolved_path).map_err(|error| {
            ToolError::PathValidationError(format!(
                "sandbox path '{}' cannot be resolved: {}",
                path, error
            ))
        })?;
        let canonical_path_string = canonical_path.to_string_lossy().to_string();
        validate_workspace_path(&canonical_path_string, &workspace)?;

        let timeout_ms = arguments
            .get("timeout_ms")
            .and_then(Value::as_u64)
            .unwrap_or(DEFAULT_TIMEOUT_MS)
            .clamp(100, MAX_TIMEOUT_MS);
        let max_output_chars = arguments
            .get("max_output_chars")
            .and_then(Value::as_u64)
            .map(|value| value as usize)
            .unwrap_or(DEFAULT_OUTPUT_CHARS)
            .clamp(1_000, MAX_OUTPUT_CHARS);

        // A timed-out spawn_blocking task cannot be force-cancelled safely.
        // Keep a hard worker cap and move the permit into the task so repeated
        // timeouts cannot create an unbounded background-worker backlog.
        let worker_permit = SANDBOX_WORKERS.try_acquire().map_err(|_| {
            ToolError::ExecutionError(format!(
                "sandbox is busy (maximum {} concurrent commands); retry shortly",
                MAX_CONCURRENT_WORKERS
            ))
        })?;
        let command_for_task = command.clone();
        let path_for_task = canonical_path;
        let task = tokio::task::spawn_blocking(move || {
            let _worker_permit = worker_permit;
            run_allowlisted(&command_for_task, &path_for_task)
        });
        let output = tokio::time::timeout(Duration::from_millis(timeout_ms), task)
            .await
            .map_err(|_| {
                ToolError::ExecutionError(format!(
                    "sandbox command '{}' timed out after {} ms",
                    command, timeout_ms
                ))
            })?
            .map_err(|error| {
                ToolError::ExecutionError(format!("sandbox worker failed: {}", error))
            })??;

        let (output, truncated) = truncate_json_output(output, max_output_chars);
        serde_json::to_string(&SandboxOutput {
            status: "ok",
            command: display_command,
            output,
            truncated,
        })
        .map_err(|error| ToolError::ExecutionError(error.to_string()))
    }
}

fn parse_command_request(arguments: &Value) -> Result<(String, String, String), ToolError> {
    if let Some(command_line) = arguments.get("command_line").and_then(Value::as_str) {
        let command_line = command_line.trim();
        if command_line.is_empty() || command_line.chars().count() > MAX_COMMAND_LINE_CHARS {
            return Err(ToolError::InvalidArguments(
                "run_sandbox_command".to_string(),
                format!(
                    "command_line must contain 1-{} characters",
                    MAX_COMMAND_LINE_CHARS
                ),
            ));
        }
        let tokens = tokenize_virtual_command(command_line)?;
        let invalid = || {
            ToolError::InvalidArguments(
                "run_sandbox_command".to_string(),
                "allowed syntax: wc <file>, jq . <json-file>, sha256sum <file>, unzip -l <archive>, or find <directory>"
                    .to_string(),
            )
        };
        let (command, path) = match tokens.as_slice() {
            [program, path] if matches!(program.as_str(), "wc" | "text_stats") => {
                ("text_stats", path.as_str())
            }
            [program, path] if matches!(program.as_str(), "sha256sum" | "sha256") => {
                ("sha256", path.as_str())
            }
            [program, path] if matches!(program.as_str(), "find" | "workspace_tree") => {
                ("workspace_tree", path.as_str())
            }
            [program, path] if program == "json_validate" => ("json_validate", path.as_str()),
            [program, selector, path] if program == "jq" && selector == "." => {
                ("json_validate", path.as_str())
            }
            [program, flag, path] if program == "unzip" && flag == "-l" => {
                ("zip_list", path.as_str())
            }
            [program, path] if program == "zip_list" => ("zip_list", path.as_str()),
            _ => return Err(invalid()),
        };
        return Ok((
            command.to_string(),
            path.to_string(),
            command_line.to_string(),
        ));
    }

    let command = arguments
        .get("command")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            ToolError::InvalidArguments(
                "run_sandbox_command".to_string(),
                "supply command_line, or supply both command and path".to_string(),
            )
        })?;
    let path = arguments
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            ToolError::InvalidArguments(
                "run_sandbox_command".to_string(),
                "supply command_line, or supply both command and path".to_string(),
            )
        })?;
    Ok((
        command.to_string(),
        path.to_string(),
        format!("{} {}", command, path),
    ))
}

fn tokenize_virtual_command(command_line: &str) -> Result<Vec<String>, ToolError> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut escaped = false;

    let mut characters = command_line.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\0' || (character.is_control() && !character.is_whitespace()) {
            return Err(ToolError::InvalidArguments(
                "run_sandbox_command".to_string(),
                "command_line contains a control character".to_string(),
            ));
        }
        if escaped {
            current.push(character);
            escaped = false;
            continue;
        }
        if character == '\\' && quote != Some('\'') {
            // Preserve ordinary Windows path separators. Backslash acts as
            // an escape only for whitespace, quotes, or another backslash.
            if characters
                .peek()
                .is_some_and(|next| next.is_whitespace() || matches!(next, '\\' | '\'' | '"'))
            {
                escaped = true;
            } else {
                current.push(character);
            }
            continue;
        }
        if let Some(active_quote) = quote {
            if character == active_quote {
                quote = None;
            } else {
                current.push(character);
            }
            continue;
        }
        if matches!(character, '\'' | '"') {
            quote = Some(character);
        } else if character.is_whitespace() {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
        } else if matches!(character, '|' | '&' | ';' | '<' | '>' | '`') {
            return Err(ToolError::InvalidArguments(
                "run_sandbox_command".to_string(),
                "pipes, chaining, redirection, and shell expansion are not available in the virtual terminal"
                    .to_string(),
            ));
        } else {
            current.push(character);
        }
    }
    if escaped || quote.is_some() {
        return Err(ToolError::InvalidArguments(
            "run_sandbox_command".to_string(),
            "command_line has an unfinished escape or quote".to_string(),
        ));
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    Ok(tokens)
}

fn validate_regular_file(path: &Path) -> Result<std::fs::Metadata, ToolError> {
    let metadata = std::fs::metadata(path)
        .map_err(|error| ToolError::IoError(format!("{}: {}", path.display(), error)))?;
    if !metadata.is_file() {
        return Err(ToolError::InvalidArguments(
            "run_sandbox_command".to_string(),
            format!("{} is not a regular file", path.display()),
        ));
    }
    if metadata.len() > MAX_INSPECT_FILE_BYTES {
        return Err(ToolError::ExecutionError(format!(
            "{} is {} bytes; sandbox inspection limit is {} bytes",
            path.display(),
            metadata.len(),
            MAX_INSPECT_FILE_BYTES
        )));
    }
    Ok(metadata)
}

fn run_allowlisted(command: &str, path: &Path) -> Result<Value, ToolError> {
    match command {
        "text_stats" => text_stats(path),
        "json_validate" => json_validate(path),
        "sha256" => sha256(path),
        "zip_list" => zip_list(path),
        "workspace_tree" => workspace_tree(path),
        _ => unreachable!("command was checked against the allowlist"),
    }
}

fn text_stats(path: &Path) -> Result<Value, ToolError> {
    let metadata = validate_regular_file(path)?;
    let text = std::fs::read_to_string(path).map_err(|error| {
        ToolError::ExecutionError(format!("text_stats requires a UTF-8 text file: {}", error))
    })?;
    let lines = if text.is_empty() {
        0
    } else {
        text.lines().count()
    };
    let words = text.split_whitespace().count();
    let characters = text.chars().count();
    Ok(json!({
        "path": path,
        "bytes": metadata.len(),
        "lines": lines,
        "words": words,
        "characters": characters,
    }))
}

fn json_validate(path: &Path) -> Result<Value, ToolError> {
    validate_regular_file(path)?;
    let file = File::open(path)
        .map_err(|error| ToolError::IoError(format!("{}: {}", path.display(), error)))?;
    let value: Value = serde_json::from_reader(file).map_err(|error| {
        ToolError::ExecutionError(format!(
            "invalid JSON at line {}, column {}: {}",
            error.line(),
            error.column(),
            error
        ))
    })?;
    let (kind, entries) = match &value {
        Value::Null => ("null", 0),
        Value::Bool(_) => ("boolean", 1),
        Value::Number(_) => ("number", 1),
        Value::String(_) => ("string", 1),
        Value::Array(values) => ("array", values.len()),
        Value::Object(values) => ("object", values.len()),
    };
    Ok(json!({
        "path": path,
        "valid": true,
        "root_type": kind,
        "root_entries": entries,
    }))
}

fn sha256(path: &Path) -> Result<Value, ToolError> {
    let metadata = validate_regular_file(path)?;
    let mut file = File::open(path)
        .map_err(|error| ToolError::IoError(format!("{}: {}", path.display(), error)))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| ToolError::IoError(error.to_string()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(json!({
        "path": path,
        "bytes": metadata.len(),
        "sha256": hex::encode(hasher.finalize()),
    }))
}

fn zip_list(path: &Path) -> Result<Value, ToolError> {
    validate_regular_file(path)?;
    let mut file = File::open(path)
        .map_err(|error| ToolError::IoError(format!("{}: {}", path.display(), error)))?;
    let mut magic = [0u8; 4];
    file.read_exact(&mut magic)
        .map_err(|error| ToolError::ExecutionError(format!("cannot read ZIP header: {}", error)))?;
    if magic != *b"PK\x03\x04" && magic != *b"PK\x05\x06" && magic != *b"PK\x07\x08" {
        return Err(ToolError::ExecutionError(
            "file is not a ZIP archive (missing PK header)".to_string(),
        ));
    }
    file.rewind()
        .map_err(|error| ToolError::IoError(error.to_string()))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|error| ToolError::ExecutionError(format!("invalid ZIP: {}", error)))?;
    let total_entries = archive.len();
    let mut entries = Vec::with_capacity(total_entries.min(MAX_TREE_ENTRIES));
    for index in 0..total_entries.min(MAX_TREE_ENTRIES) {
        let member = archive
            .by_index(index)
            .map_err(|error| ToolError::ExecutionError(error.to_string()))?;
        entries.push(json!({
            "name": member.name(),
            "bytes": member.size(),
            "compressed_bytes": member.compressed_size(),
            "directory": member.is_dir(),
        }));
    }
    Ok(json!({
        "path": path,
        "entry_count": total_entries,
        "entries": entries,
        "entries_truncated": total_entries > MAX_TREE_ENTRIES,
    }))
}

fn workspace_tree(path: &Path) -> Result<Value, ToolError> {
    let metadata = std::fs::metadata(path)
        .map_err(|error| ToolError::IoError(format!("{}: {}", path.display(), error)))?;
    if !metadata.is_dir() {
        return Err(ToolError::InvalidArguments(
            "run_sandbox_command".to_string(),
            format!("{} is not a directory", path.display()),
        ));
    }
    let mut entries = Vec::new();
    for entry in walkdir::WalkDir::new(path)
        .follow_links(false)
        .sort_by_file_name()
        .into_iter()
        .filter_map(Result::ok)
        .skip(1)
        .take(MAX_TREE_ENTRIES + 1)
    {
        let relative = entry.path().strip_prefix(path).unwrap_or(entry.path());
        entries.push(json!({
            "path": relative.to_string_lossy(),
            "kind": if entry.file_type().is_dir() { "directory" } else if entry.file_type().is_file() { "file" } else { "other" },
            "bytes": entry.metadata().ok().filter(|m| m.is_file()).map(|m| m.len()),
        }));
    }
    let was_truncated = entries.len() > MAX_TREE_ENTRIES;
    entries.truncate(MAX_TREE_ENTRIES);
    Ok(json!({
        "root": path,
        "entries": entries,
        "truncated": was_truncated,
    }))
}

fn truncate_json_output(value: Value, max_chars: usize) -> (Value, bool) {
    let serialized = value.to_string();
    if serialized.chars().count() <= max_chars {
        return (value, false);
    }
    let preview: String = serialized.chars().take(max_chars).collect();
    (
        json!({
            "preview": preview,
            "notice": "output truncated by sandbox max_output_chars",
        }),
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_workspace(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("inkuo_sandbox_{}_{}", name, uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn rejects_non_allowlisted_programs() {
        let dir = temp_workspace("reject");
        let tool = SandboxCommandTool;
        let error = tool
            .execute(
                json!({"command": "npm", "path": dir}),
                Some(dir.to_string_lossy().to_string()),
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains("not allowlisted"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn rejects_missing_or_empty_workspace() {
        let dir = temp_workspace("missing_workspace");
        let tool = SandboxCommandTool;
        for workspace in [None, Some(String::new()), Some("   ".to_string())] {
            let error = tool
                .execute(json!({"command": "workspace_tree", "path": dir}), workspace)
                .await
                .unwrap_err();
            assert!(error
                .to_string()
                .contains("requires a non-empty active workspace"));
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn computes_text_stats_without_external_dependencies() {
        let dir = temp_workspace("stats");
        let path = dir.join("notes.txt");
        std::fs::write(&path, "hello world\nsecond line\n").unwrap();
        let tool = SandboxCommandTool;
        let output = tool
            .execute(
                json!({"command": "text_stats", "path": path}),
                Some(dir.to_string_lossy().to_string()),
            )
            .await
            .unwrap();
        assert!(output.contains("\"lines\":2"));
        assert!(output.contains("\"words\":4"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn accepts_terminal_syntax_and_workspace_relative_quoted_paths() {
        let dir = temp_workspace("terminal_syntax");
        let path = dir.join("notes with spaces.txt");
        std::fs::write(&path, "hello virtual terminal\n").unwrap();
        let output = SandboxCommandTool
            .execute(
                json!({"command_line": "wc \"notes with spaces.txt\""}),
                Some(dir.to_string_lossy().to_string()),
            )
            .await
            .unwrap();
        let parsed: Value = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed["command"], "wc \"notes with spaces.txt\"");
        assert_eq!(parsed["output"]["words"], 3);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn rejects_shell_chaining_and_package_managers_in_terminal_syntax() {
        let dir = temp_workspace("terminal_reject");
        for command_line in ["wc notes.txt | cat", "npm install anything"] {
            let error = SandboxCommandTool
                .execute(
                    json!({"command_line": command_line}),
                    Some(dir.to_string_lossy().to_string()),
                )
                .await
                .unwrap_err();
            assert!(matches!(error, ToolError::InvalidArguments(_, _)));
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn tokenizer_preserves_windows_path_separators() {
        assert_eq!(
            tokenize_virtual_command(r#"sha256sum C:\Work\report.docx"#).unwrap(),
            vec!["sha256sum", r#"C:\Work\report.docx"#]
        );
    }

    #[tokio::test]
    async fn rejects_paths_outside_workspace() {
        let dir = temp_workspace("inside");
        let outside = temp_workspace("outside").join("secret.txt");
        std::fs::write(&outside, "secret").unwrap();
        let tool = SandboxCommandTool;
        let error = tool
            .execute(
                json!({"command": "text_stats", "path": outside}),
                Some(dir.to_string_lossy().to_string()),
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains("outside the workspace"));
        let _ = std::fs::remove_dir_all(dir);
        let _ = std::fs::remove_dir_all(outside.parent().unwrap());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn rejects_symlink_target_outside_workspace() {
        use std::os::unix::fs::symlink;

        let dir = temp_workspace("symlink_inside");
        let outside_dir = temp_workspace("symlink_outside");
        let outside = outside_dir.join("secret.txt");
        let link = dir.join("linked.txt");
        std::fs::write(&outside, "secret").unwrap();
        symlink(&outside, &link).unwrap();

        let error = SandboxCommandTool
            .execute(
                json!({"command": "text_stats", "path": link}),
                Some(dir.to_string_lossy().to_string()),
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains("outside the workspace"));

        let _ = std::fs::remove_dir_all(dir);
        let _ = std::fs::remove_dir_all(outside_dir);
    }

    #[tokio::test]
    async fn enforces_output_ceiling_without_spawning_a_process() {
        let dir = temp_workspace("output_cap");
        for index in 0..120 {
            std::fs::write(
                dir.join(format!("very-long-diagnostic-filename-{index:03}.txt")),
                "x",
            )
            .unwrap();
        }
        let tool = SandboxCommandTool;
        let output = tool
            .execute(
                json!({
                    "command": "workspace_tree",
                    "path": dir,
                    "timeout_ms": 10_000,
                    "max_output_chars": 1_000,
                }),
                Some(dir.to_string_lossy().to_string()),
            )
            .await
            .unwrap();
        let parsed: Value = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed["truncated"], true);
        assert!(parsed["output"]["notice"]
            .as_str()
            .unwrap()
            .contains("truncated"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
