//! Per-turn runtime state surfaced to the LLM as a system-prompt section.
//!
//! ## Why this is its own module
//!
//! Each AI turn composes its system prompt from three independent layers
//! (in order):
//!
//!   1. **Mode base prompt** — `prompts/ask.md` / `plan.md` / `agent.*.md`.
//!      These are static, mode-bound documents. They contain a sentence
//!      like "You operate in Ask Mode" but that's the *label* of the
//!      mode, not the *current* mode. If the user switches modes mid-
//!      session, the prompt changes wholesale — but the LLM is still
//!      carrying the previous turn's "you can write files" framing in
//!      its context, and the new prompt may take a turn to overwrite it.
//!
//!   2. **Feature toggles** — `feature_toggles::enabled_fragment`.
//!      Per-toggle availability inventory (web_search on/off, kb_strict
//!      on/off). Already implemented.
//!
//!   3. **Runtime state** — this module. A short, structured block that
//!      the LLM can rely on to know what is *actually* true this turn:
//!      the active mode, the resulting tool tier, and a one-line summary
//!      of which feature toggles are on. It is emitted unconditionally
//!      and is the last block in the system prompt, so it overrides any
//!      stale framing from earlier layers.
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

/// The three first-class modes. The string values match the
/// `mode` field the frontend sends in `ai_agent_stream`. A new
/// mode must be added to `Mode::ALL` below so the LLM can see it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Ask,
    Plan,
    Agent,
}

impl Mode {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "ask" => Some(Self::Ask),
            "plan" => Some(Self::Plan),
            "agent" => Some(Self::Agent),
            _ => None,
        }
    }

    pub fn label_zh(self) -> &'static str {
        match self {
            Mode::Ask => "Ask 模式 (问答)",
            Mode::Plan => "Plan 模式 (规划)",
            Mode::Agent => "Agent 模式 (执行)",
        }
    }

    /// One-line description of what the LLM can and can't do in this
    /// mode. Written for the model, not for the user.
    pub fn tool_tier(self) -> &'static str {
        match self {
            Mode::Ask => {
                "Read-only: only retrieval tools (read_file, list_dir, glob, grep, \
                 read_office_file, database_search, etc.) are available. You cannot \
                 write, edit, delete, or execute side effects."
            }
            Mode::Plan => {
                "Read-only, same tool set as Ask. Use the read tools to understand \
                 the workspace, then produce a structured plan via the `create_plan` \
                 tool. You will not execute the plan; the user runs it manually in \
                 Agent mode."
            }
            Mode::Agent => {
                "Full access: read + write tools (write_file, edit_file, create_*, \
                 shell_run, delegate_to, etc.) are all available. Web search, when \
                 the toggle is on, is also available."
            }
        }
    }
}

/// Build the runtime-state fragment for the given active mode and
/// enabled feature toggles.
///
/// Always non-empty: even when both lists are empty, the LLM must see
/// *some* declaration of the current mode — otherwise it falls back to
/// guessing from earlier turns, and the user-visible bug is "I switched
/// to Ask but the AI still talks like it can write files".
pub fn runtime_state_fragment(mode: Mode, enabled_toggles: &[super::feature_toggles::ToggleId]) -> String {
    let mut lines: Vec<String> = Vec::new();

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
            "- **Feature toggles**: none active. Web search is OFF; strict KB is OFF."
                .to_string(),
        );
    } else {
        let names: Vec<String> = enabled_toggles
            .iter()
            .map(|t| match t {
                super::feature_toggles::ToggleId::KbStrict => "kb_strict (严格 KB)",
                super::feature_toggles::ToggleId::WebSearch => "web_search (联网搜索)",
            })
            .map(|s| s.to_string())
            .collect();
        lines.push(format!(
            "- **Feature toggles active**: {}.",
            names.join(", ")
        ));
    }

    // Pin the tool tier explicitly so the LLM does not improvise. The
    // mapping mirrors `commands_agent::ai_agent_stream` — see that
    // match arm for the source of truth.
    let available_writes = matches!(mode, Mode::Agent);
    lines.push(format!(
        "- **Write tools available**: {}. The user picked the mode above; \
         if you are unsure whether `write_file` / `edit_file` / `create_*` \
         exist right now, the answer is `{}`.",
        if available_writes { "YES" } else { "NO" },
        if available_writes { "yes" } else { "no" },
    ));

    lines.push(format!(
        "- **Read tools available**: YES in every mode (Ask / Plan / Agent)."
    ));

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feature_toggles::ToggleId;

    #[test]
    fn fragment_mentions_active_mode() {
        let f = runtime_state_fragment(Mode::Ask, &[]);
        assert!(f.contains("Ask 模式"));
        assert!(f.contains("(问答)"));
    }

    #[test]
    fn fragment_marks_writes_unavailable_in_ask_and_plan() {
        for mode in [Mode::Ask, Mode::Plan] {
            let f = runtime_state_fragment(mode, &[]);
            assert!(
                f.contains("**Write tools available**: NO"),
                "mode {mode:?} should disable write tools; got:\n{f}"
            );
            assert!(
                f.contains("answer is `no`"),
                "mode {mode:?} should pin the no-writes answer; got:\n{f}"
            );
        }
    }

    #[test]
    fn fragment_marks_writes_available_in_agent() {
        let f = runtime_state_fragment(Mode::Agent, &[]);
        assert!(f.contains("**Write tools available**: YES"));
        assert!(f.contains("answer is `yes`"));
    }

    #[test]
    fn fragment_lists_active_toggles_by_name() {
        let f = runtime_state_fragment(Mode::Agent, &[ToggleId::WebSearch]);
        assert!(f.contains("web_search (联网搜索)"));

        let f = runtime_state_fragment(Mode::Agent, &[ToggleId::KbStrict, ToggleId::WebSearch]);
        assert!(f.contains("kb_strict"));
        assert!(f.contains("web_search"));
    }

    #[test]
    fn fragment_handles_no_toggles() {
        let f = runtime_state_fragment(Mode::Ask, &[]);
        assert!(f.contains("none active"));
    }

    #[test]
    fn fragment_is_never_empty() {
        let all_modes = [Mode::Ask, Mode::Plan, Mode::Agent];
        for mode in all_modes {
            for toggles in [
                &[][..],
                &[ToggleId::WebSearch][..],
                &[ToggleId::KbStrict][..],
                &[ToggleId::KbStrict, ToggleId::WebSearch][..],
            ] {
                let f = runtime_state_fragment(mode, toggles);
                assert!(
                    !f.trim().is_empty(),
                    "runtime_state_fragment must be non-empty for mode={mode:?} toggles={toggles:?}"
                );
            }
        }
    }

    #[test]
    fn fragment_overrides_earlier_instructions() {
        // The "if it disagrees" preamble is the entire point — make
        // sure it shows up in every variant.
        for mode in [Mode::Ask, Mode::Plan, Mode::Agent] {
            let f = runtime_state_fragment(mode, &[]);
            assert!(
                f.contains("follow THIS block"),
                "fragment for {mode:?} should declare itself authoritative; got:\n{f}"
            );
        }
    }
}
