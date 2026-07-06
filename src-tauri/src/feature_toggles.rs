//! Feature toggle registry — the Rust-side mirror of the frontend's
//! `FeatureToggleId` enum (`src/types/index.ts`).
//!
//! Each toggle can:
//!   1. Inject an extra system-prompt fragment (layered on top of the
//!      mode's base prompt).
//!   2. Restrict the tool set available to the Agent (for the strict-KB
//!      toggle, write tools are removed).
//!
//! The send path in `commands_agent::ai_agent_stream` reads the toggles
//! the frontend enabled and asks `enabled_fragment(...)` /
//! `effective_tool_set(...)` to apply them.

use serde::{Deserialize, Serialize};

/// Toggle ids MUST match the frontend `FeatureToggleId` union. The
/// frontend sends only the ids it knows about — unknown ids are an error
/// on the Rust side because they imply a desync.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToggleId {
    #[serde(rename = "kb_strict")]
    KbStrict,
    #[serde(rename = "web_search")]
    WebSearch,
}

impl ToggleId {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "kb_strict" => Some(Self::KbStrict),
            "web_search" => Some(Self::WebSearch),
            _ => None,
        }
    }
}

/// Return the system-prompt fragment to append when this toggle is on.
/// Each fragment is layered AFTER the mode's base prompt so the toggle's
/// rules take precedence on conflicts.
fn fragment_for(toggle: ToggleId) -> &'static str {
    match toggle {
        ToggleId::KbStrict => include_str!("../prompts/fragments/kb_strict.md"),
        // Reserved for future use — the toggle exists in the UI today but
        // has no backend wiring yet. Returning an empty fragment keeps
        // the toggle a no-op on the prompt side until web search ships.
        ToggleId::WebSearch => "",
    }
}

/// Tools that the Agent's main session must NOT see when `KbStrict` is on.
/// The list deliberately mirrors `general.md`'s "Tier 1 / Tier 2 — write"
/// names so it stays stable when the registry gains new read tools.
const KB_STRICT_BLOCKED_TOOLS: &[&str] = &[
    "write_file",
    "edit_file",
    "create_word_doc",
    "modify_excel",
    "create_excel",
    "create_file_entry",
    "rename_path",
    "delete_path",
    "apply_hunk",
    "apply_all_hunks",
    "batch_write_files",
    "shell_run",
    "delegate_to",
    "build_knowledge_base",
    "add_knowledge_member",
    "remove_knowledge_member",
];

/// Build the concatenated prompt fragment to append for the given enabled
/// toggles. Returns an empty string when no toggle contributes any text
/// (the common case for unrecognized-but-on toggles like `web_search`).
pub fn enabled_fragment(enabled: &[ToggleId]) -> String {
    enabled
        .iter()
        .map(|t| fragment_for(*t))
        .filter(|s| !s.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n\n---\n\n")
}

/// Filter the registry's allowed tool set given the enabled toggles.
/// `base` is the default tool set for the active mode (e.g. read-only for
/// Ask, full for Agent). Returns the effective list to advertise to the
/// LLM.
pub fn effective_tool_set(base: &[String], enabled: &[ToggleId]) -> Vec<String> {
    if enabled.iter().any(|t| matches!(t, ToggleId::KbStrict)) {
        let blocked: std::collections::HashSet<&str> =
            KB_STRICT_BLOCKED_TOOLS.iter().copied().collect();
        return base
            .iter()
            .filter(|name| !blocked.contains(name.as_str()))
            .cloned()
            .collect();
    }
    base.to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fragment_kb_strict_is_nonempty() {
        assert!(!fragment_for(ToggleId::KbStrict).trim().is_empty());
    }

    #[test]
    fn kb_strict_blocks_write_tools_only() {
        let base = vec![
            "read_file".to_string(),
            "write_file".to_string(),
            "database_search".to_string(),
            "create_word_doc".to_string(),
            "delegate_to".to_string(),
        ];
        let filtered = effective_tool_set(&base, &[ToggleId::KbStrict]);
        assert_eq!(
            filtered,
            vec!["read_file".to_string(), "database_search".to_string()]
        );
    }

    #[test]
    fn empty_toggles_passthrough() {
        let base = vec!["read_file".to_string(), "write_file".to_string()];
        assert_eq!(effective_tool_set(&base, &[]), base);
    }
}