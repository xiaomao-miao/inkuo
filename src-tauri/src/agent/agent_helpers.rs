//! Free-standing helpers for `agent_loop.rs`.
//!
//! Pulled out of `agent_loop.rs` because none of these functions need
//! the `AgentExecutor` state:
//!   - `parse_tool_call_message` / `parse_sse_delta` / `parse_ollama_delta`
//!     / `parse_response` are pure JSON parsers
//!   - `generate_plan_id_for_session` / `chrono_from_timestamp` /
//!     `is_leap_year` are pure data formatters
//!   - `save_plan_to_workspace` is the only side-effecting helper —
//!     takes a plan id + body, writes a markdown file under the
//!     workspace `.plans/` dir.
//!
//! The `DeltaResponse` / `DeltaToolCall` / `DeltaFunction` structs live
//! alongside the parsers that produce them.

use serde_json::Value;

pub(crate) use super::agent_loop::{ToolCallFunction, ToolCallMessage};

#[derive(Debug)]
pub(crate) struct DeltaResponse {
    pub(crate) content: Option<String>,
    pub(crate) reasoning_content: Option<String>,
    pub(crate) tool_calls: Option<Vec<DeltaToolCall>>,
}

#[derive(Debug)]
pub(crate) struct DeltaToolCall {
    pub(crate) index: usize,
    pub(crate) id: Option<String>,
    pub(crate) function: DeltaFunction,
}

#[derive(Debug)]
pub(crate) struct DeltaFunction {
    pub(crate) name: Option<String>,
    pub(crate) arguments: Option<String>,
}

// Re-export the tool-call message types from `agent_loop` so
// `parse_tool_call_message` (which lives here) can construct them
// without depending on a private field structure.

/// Format: `plan-YYYYMMDD-HHmmss-<6-char-base36>` — stable across sessions.
pub(crate) fn generate_plan_id_for_session() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let t = chrono_from_timestamp(secs);
    let pad = |n: u64| format!("{:02}", n);
    let stamp = format!(
        "{}{}{}-{}-{}{}",
        t.0, pad(t.1), pad(t.2),
        pad(t.3), pad(t.4), pad(t.5)
    );
    let suffix = (secs % (36u64.pow(6)))
        .to_string()
        .chars()
        .map(|c| {
            let v = c.to_digit(10).unwrap_or(0);
            "0123456789abcdefghijklmnopqrstuvwxyz"
                .chars()
                .nth(v as usize)
                .unwrap_or('0')
        })
        .collect::<String>();
    format!("plan-{}-{}", stamp, suffix)
}

pub(crate) fn chrono_from_timestamp(secs: u64) -> (u64, u64, u64, u64, u64, u64) {
    // (year, month, day, hour, min, sec) using simple calendar math
    let mut rem = secs;
    let sec = rem % 60; rem /= 60;
    let min = rem % 60; rem /= 60;
    let hour = rem % 24; rem /= 24;
    // Days since epoch; approximate year/month
    let mut year = 1970u64;
    let mut year_days = 365;
    while rem >= year_days {
        rem -= year_days;
        year += 1;
        year_days = if is_leap_year(year) { 366 } else { 365 };
    }
    let mut month = 1u64;
    let month_days = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    while rem >= month_days[(month - 1) as usize] {
        rem -= month_days[(month - 1) as usize];
        if month == 2 && is_leap_year(year) && rem > 0 {
            rem -= 1;
        }
        month += 1;
    }
    let day = rem + 1;
    (year, month, day, hour, min, sec)
}

pub(crate) fn is_leap_year(year: u64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

const PLANS_DIR: &str = ".inkuo";
const PLANS_SUBDIR: &str = "plans";

/// Write plan markdown to `<workspace>/.inkuo/plans/<plan_id>.md`.
pub(crate) async fn save_plan_to_workspace(
    workspace: &str,
    plan_id: &str,
    content: &str,
) -> Result<String, String> {
    let ws_dir = std::path::Path::new(workspace);
    if !ws_dir.is_dir() {
        return Err(format!("Workspace not found: {}", workspace));
    }
    let plans_dir = ws_dir.join(PLANS_DIR).join(PLANS_SUBDIR);
    tokio::fs::create_dir_all(&plans_dir)
        .await
        .map_err(|e| format!("create plans dir: {}", e))?;

    let safe_id: String = plan_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let safe_id = if safe_id.is_empty() { "plan" } else { &safe_id };
    let path = plans_dir.join(format!("{}.md", safe_id));

    tokio::fs::write(&path, content)
        .await
        .map_err(|e| format!("write plan file: {}", e))?;

    Ok(path.to_string_lossy().to_string())
}
pub(crate) fn parse_tool_call_message(tc: &serde_json::Value) -> Result<ToolCallMessage, String> {
    let id = tc
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing or non-string `id`".to_string())?;
    let call_type = tc
        .get("type")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing or non-string `type`".to_string())?;
    let function = tc
        .get("function")
        .ok_or_else(|| "missing `function` object".to_string())?;
    let name = function
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing or non-string `function.name`".to_string())?;
    let arguments = function
        .get("arguments")
        .and_then(|v| match v {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Object(map) => Some(serde_json::to_string(map).unwrap_or_default()),
            _ => None,
        })
        .ok_or_else(|| "missing or unsupported `function.arguments`".to_string())?;

    Ok(ToolCallMessage {
        id: id.to_string(),
        call_type: call_type.to_string(),
        function: ToolCallFunction {
            name: name.to_string(),
            arguments,
        },
    })
}

