use crate::ai::AIError;

pub fn iter_sse_data_lines(input: &str) -> impl Iterator<Item = &str> {
    input
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
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
