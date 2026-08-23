//! Per-turn runtime state surfaced to the LLM as a system-prompt section.
//!
//! ## Why this is its own module
//!
//! Each AI turn composes its system prompt from three independent layers
//! (in order):
//!
//!   1. **Mode base prompt** — `prompts/main/agent.slim.md`. Static document
//!      describing Agent-mode behavior.
//!
//!   2. **Runtime state** — this module. A short, structured block that
//!      the LLM can rely on to know what is *actually* true this turn:
//!      the active mode, the resulting tool tier, and a one-line summary
//!      of which feature toggles are on. It is emitted unconditionally.
//!
//!   3. **Feature toggles** — `feature_toggles::enabled_fragment`.
//!      Per-toggle availability inventory plus detailed usage rules. It is
//!      emitted after this summary and must agree with it.
//!
//! The contract: if the runtime state fragment and an earlier prompt
//! section disagree, follow the runtime state. We tell the LLM this
//! directly so it doesn't fall back to guessing.
//!
//! ## Mode → tool tier
//!
//! We hardcode the mapping here (instead of introspecting the registry)
//! because the LLM doesn't have access to the registry; it needs a
//! short human label. The mapping must stay in sync with
//! `commands_agent::ai_agent_stream`'s `match mode.as_str()` arm —
//! when a new mode is added there, add a row here too.
use serde::{Deserialize, Serialize};

/// First-class mode for AI turns. The string values match the
/// `mode` field the frontend sends in `ai_agent_stream`. A new
/// mode must be added to `Mode::ALL` below so the LLM can see it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Agent,
}

impl Mode {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "agent" => Some(Self::Agent),
            _ => None,
        }
    }

    pub fn label_zh(self) -> &'static str {
        match self {
            Mode::Agent => "Agent 模式 (执行)",
        }
    }

    /// One-line description of what the LLM can and can't do in this
    /// mode. Written for the model, not for the user.
    pub fn tool_tier(self) -> &'static str {
        match self {
            Mode::Agent => {
                "Agent profile: read/write orchestration is available unless \
                 a stricter feature toggle below removes it. Optional tools \
                 exist only when their explicit toggle state says ON."
            }
        }
    }
}

/// Build the runtime-state fragment for the given active mode and
/// enabled feature toggles.
///
/// Always non-empty: even when both lists are empty, the LLM must see
/// *some* declaration of the current mode — otherwise it falls back to
/// guessing from earlier turns.
pub fn runtime_state_fragment(
    mode: Mode,
    enabled_toggles: &[super::feature_toggles::ToggleId],
) -> String {
    let mut lines: Vec<String> = Vec::new();
    let kb_strict_on = enabled_toggles
        .iter()
        .any(|toggle| matches!(toggle, super::feature_toggles::ToggleId::KbStrict));
    let web_search_on = enabled_toggles
        .iter()
        .any(|toggle| matches!(toggle, super::feature_toggles::ToggleId::WebSearch));
    let sandbox_on = enabled_toggles
        .iter()
        .any(|toggle| matches!(toggle, super::feature_toggles::ToggleId::Sandbox));

    lines.push(
        "## Runtime State (this turn)\n\
         \n\
         The block below is the authoritative declaration of the current \
         turn. If it disagrees with an earlier section of this prompt or \
         with what you remember from previous turns, follow THIS block — \
         earlier sections are static documents that may not match the \
         current mode or toggles."
            .to_string(),
    );

    lines.push(format!(
        "- **Active mode**: {} ({})",
        mode.label_zh(),
        mode.tool_tier()
    ));

    // Inline toggle summary. The full feature_toggles::enabled_fragment
    // is appended separately; this is just a one-line recap that lives
    // next to the mode declaration so the LLM can read the two together
    // in a single glance. If a prompt caching layer is added later, the
    // inventory below (which changes per toggle) and the mode line (which
    // changes per mode) can be split into different cache buckets to
    // maximise reuse.
    if enabled_toggles.is_empty() {
        lines.push(
            "- **Feature toggles**: none active. Web search is OFF; strict KB is OFF; restricted sandbox is OFF."
                .to_string(),
        );
    } else {
        let names: Vec<String> = enabled_toggles
            .iter()
            .map(|t| match t {
                super::feature_toggles::ToggleId::KbStrict => "kb_strict (严格 KB)",
                super::feature_toggles::ToggleId::WebSearch => "web_search (联网搜索)",
                super::feature_toggles::ToggleId::Sandbox if kb_strict_on => {
                    "sandbox (requested; blocked by strict KB)"
                }
                super::feature_toggles::ToggleId::Sandbox => "sandbox (安全沙盒)",
            })
            .map(|s| s.to_string())
            .collect();
        lines.push(format!(
            "- **Feature toggles active**: {}.",
            names.join(", ")
        ));
    }

    // Pin effective capability state explicitly so the LLM does not infer
    // Agent-mode writes when strict-KB has actually removed them.
    let available_writes = matches!(mode, Mode::Agent) && !kb_strict_on;
    lines.push(format!(
        "- **Write tools available**: {}. The user picked the mode above; \
         if you are unsure whether `write_file` / `edit_file` / `create_*` \
         exist right now, the answer is `{}`.",
        if available_writes { "YES" } else { "NO" },
        if available_writes { "yes" } else { "no" },
    ));
    lines.push(format!(
        "- **Web search available**: {}. `web_search` {} in the advertised tool list.",
        if web_search_on { "YES" } else { "NO" },
        if web_search_on { "is" } else { "is not" },
    ));
    lines.push(format!(
        "- **Restricted sandbox available**: {}. `run_sandbox_command` {} in the advertised tool list{}.",
        if sandbox_on && !kb_strict_on { "YES" } else { "NO" },
        if sandbox_on && !kb_strict_on { "is" } else { "is not" },
        if kb_strict_on { " because strict KB takes precedence" } else { "" },
    ));

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feature_toggles::ToggleId;

    #[test]
    fn strict_kb_runtime_state_never_claims_writes_or_sandbox() {
        let fragment =
            runtime_state_fragment(Mode::Agent, &[ToggleId::KbStrict, ToggleId::Sandbox]);
        assert!(fragment.contains("Write tools available**: NO"));
        assert!(fragment.contains("Restricted sandbox available**: NO"));
        assert!(fragment.contains("strict KB takes precedence"));
    }

    #[test]
    fn optional_capabilities_are_reported_from_actual_toggle_state() {
        let fragment =
            runtime_state_fragment(Mode::Agent, &[ToggleId::WebSearch, ToggleId::Sandbox]);
        assert!(fragment.contains("Web search available**: YES"));
        assert!(fragment.contains("Restricted sandbox available**: YES"));
    }
}
