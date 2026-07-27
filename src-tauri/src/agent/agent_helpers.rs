//! Free-standing helpers for `agent_loop.rs`.
//!
//! Pulled out of `agent_loop.rs` because none of these functions need
//! the `AgentExecutor` state:
//!   - `parse_tool_call_message` is a pure JSON parser
//!   - `DeltaResponse` / `DeltaToolCall` / `DeltaFunction` structs live
//!     alongside the parsers that produce them.
//!
//! The `parse_tool_call_message` helper is the only side-effecting-free
//! helper left; the rest used to be plan-related and have been removed
//! along with plan mode.

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

