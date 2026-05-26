use crate::ai::AIError;

/// Try to take one complete SSE event from the buffer.
///
/// SSE events are delimited by a blank line ("\n\n" or "\r\n\r\n").
/// Returns (event_text, remaining_buffer).
pub fn take_next_sse_event(buffer: &str) -> Option<(String, String)> {
    // Prefer CRLF delimiter if present.
    if let Some(idx) = buffer.find("\r\n\r\n") {
        let event = buffer[..idx].to_string();
        let rest = buffer[idx + 4..].to_string();
        return Some((event, rest));
    }

    if let Some(idx) = buffer.find("\n\n") {
        let event = buffer[..idx].to_string();
        let rest = buffer[idx + 2..].to_string();
        return Some((event, rest));
    }

    None
}

/// Iterate `data:` payload lines from a full SSE event block.
///
/// Handles both `data: ...` and `data:...` forms, and ignores other fields.
pub fn iter_sse_event_data_lines(event: &str) -> impl Iterator<Item = &str> {
    event.lines().filter_map(|line| {
        let trimmed = line.trim_end();
        if let Some(v) = trimmed.strip_prefix("data:") {
            Some(v.trim_start())
        } else {
            None
        }
    })
}

pub fn extract_openai_delta_content(json_str: &str) -> Result<Option<String>, AIError> {
    if json_str.trim() == "[DONE]" {
        return Ok(None);
    }

    let v: serde_json::Value = serde_json::from_str(json_str)
        .map_err(|e| AIError::InvalidResponse(format!("Invalid SSE json: {}", e)))?;

    let delta = &v["choices"][0]["delta"];
    let content = delta["content"].as_str().map(|s| s.to_string());

    Ok(content)
}
