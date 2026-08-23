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
    #[serde(rename = "sandbox")]
    Sandbox,
}

impl ToggleId {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "kb_strict" => Some(Self::KbStrict),
            "web_search" => Some(Self::WebSearch),
            "sandbox" => Some(Self::Sandbox),
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
        ToggleId::WebSearch => include_str!("../prompts/fragments/web_search.md"),
        ToggleId::Sandbox => include_str!("../prompts/fragments/sandbox.md"),
    }
}

/// Tools that the Agent's main session must NOT see when `KbStrict` is on.
/// The list deliberately mirrors `general.md`'s "Tier 1 / Tier 2 — write"
/// names so it stays stable when the registry gains new read tools.
const KB_STRICT_BLOCKED_TOOLS: &[&str] = &[
    "write_file",
    "edit_file",
    "create_dir",
    "move_file",
    "create_svg",
    "generate_image",
    "create_word_doc",
    "modify_excel",
    "create_excel",
    "create_file_entry",
    "rename_path",
    "delete_path",
    "apply_hunk",
    "apply_all_hunks",
    "batch_write_files",
    // Arbitrary process execution is intentionally not registered. Keep its
    // reserved name blocked as defence in depth, alongside the shipped
    // dependency-free allowlisted sandbox.
    "shell_run",
    "run_sandbox_command",
    "delegate_to",
    "build_knowledge_base",
    "add_knowledge_member",
    "remove_knowledge_member",
];

/// Build the system-prompt fragment for the given enabled toggles.
///
/// The result has two parts, in this order:
///
///   1. **Tool availability inventory** — *always* emitted (even when no
///      toggles are on). This is the contract: the LLM sees which
///      feature tools are available *right now* and which are gated
///      off. Without this, models hallucinate ("I can't call X but I
///      can call Y") based on training data alone. We've seen the LLM
///      confidently mention `web_search` even when the user hasn't
///      enabled the toggle; surfacing the actual state in every turn
///      stops that.
///   2. **Per-toggle usage guidance** — only emitted for toggles that
///      are *on*. Off toggles don't need usage tips; the tool isn't
///      there.
///
/// The two parts are separated by `\n\n---\n\n` so the LLM treats them
/// as distinct sections and the boundary shows up clearly in logs.
pub fn enabled_fragment(enabled: &[ToggleId]) -> String {
    let inventory = availability_inventory(enabled);
    let kb_strict_on = enabled
        .iter()
        .any(|toggle| matches!(toggle, ToggleId::KbStrict));

    let guidance: String = enabled
        .iter()
        // Strict-KB removes every execution/mutation surface. Do not append
        // sandbox usage instructions that would contradict the effective
        // tool set merely because both UI switches happened to be on.
        .filter(|toggle| !(kb_strict_on && matches!(toggle, ToggleId::Sandbox)))
        .map(|t| fragment_for(*t))
        .filter(|s| !s.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n\n---\n\n");

    match (inventory.is_empty(), guidance.is_empty()) {
        (true, true) => String::new(),
        (true, false) => guidance,
        (false, true) => inventory,
        (false, false) => format!("{inventory}\n\n---\n\n{guidance}"),
    }
}

/// Tool availability inventory — describes, for every known feature
/// tool, whether it is `available` or `gated` in the current turn.
///
/// This is the section the LLM should rely on when deciding whether a
/// tool exists. It is emitted unconditionally so the contract is
/// identical across turns: the model never has to guess.
fn availability_inventory(enabled: &[ToggleId]) -> String {
    // List every feature toggle the LLM should know about, in a fixed
    // order so the diff between turns is minimal (cheaper prompt
    // caching). Each toggle's status line tells the model both the
    // capability and how the user-controlled it, because the two are
    // easy to confuse.
    let web_search_on = enabled.iter().any(|t| matches!(t, ToggleId::WebSearch));
    let kb_strict_on = enabled.iter().any(|t| matches!(t, ToggleId::KbStrict));
    let sandbox_on = enabled.iter().any(|t| matches!(t, ToggleId::Sandbox));

    let mut lines: Vec<String> = Vec::new();
    lines.push(
        "The following feature toggles are user-controlled. Their state below \
         is what is actually available in THIS turn — do not assume a tool \
         exists based on training data or earlier turns."
            .to_string(),
    );

    if kb_strict_on {
        lines.push(format!(
            "- `kb_strict` (严格 KB): ON. Write tools are NOT available; only \
             read-only search / retrieval tools are. If the user asks for an \
             edit, explain what you would have done rather than invoking a \
             write tool."
        ));
    } else {
        lines.push(format!(
            "- `kb_strict` (严格 KB): OFF. Write tools may be available \
             depending on the active mode (Ask / Plan / Agent)."
        ));
    }

    if web_search_on {
        lines.push(format!(
            "- `web_search` (联网搜索): ON. The `web_search` tool is in your \
             tool list and you may call it for real-world factual questions."
        ));
    } else {
        lines.push(format!(
            "- `web_search` (联网搜索): OFF. The `web_search` tool is NOT in \
             your tool list — do not call it. If the user asks for \
             real-world facts you cannot answer from the conversation or \
             workspace, say so and suggest they enable 联网搜索 in the \
             composer toolbar."
        ));
    }

    if sandbox_on && kb_strict_on {
        lines.push(
            "- `sandbox` (安全沙盒): REQUESTED, but unavailable because `kb_strict` takes precedence. `run_sandbox_command` is NOT in your tool list."
                .to_string(),
        );
    } else if sandbox_on {
        lines.push(
            "- `sandbox` (安全沙盒): ON. `run_sandbox_command` is available for the shipped diagnostic allowlist only; it is not a shell and cannot install or download anything."
                .to_string(),
        );
    } else {
        lines.push(
            "- `sandbox` (安全沙盒): OFF. `run_sandbox_command` is NOT in your tool list. Use normal first-class tools or ask the user to enable the sandbox toggle when a diagnostic command is genuinely needed."
                .to_string(),
        );
    }

    lines.join("\n")
}

/// Filter the registry's allowed tool set given the enabled toggles.
/// `base` is the default tool set for the active mode (e.g. read-only for
/// Ask, full for Agent). Returns the effective list to advertise to the
/// LLM.
///
/// The toggles have different semantics, so we walk the list once
/// instead of branching:
///
///   * `KbStrict` — always available in `base`; the toggle REMOVES the
///     write-tier tools. So when the toggle is *off*, the write tools
///     stay in the set; when it's *on*, we filter them out.
///   * `WebSearch` — NOT in the registry's default `base` set; the
///     toggle ADDS the `web_search` tool. So when the toggle is *off*,
///     we drop the tool from the allowlist (a no-op today because the
///     tool isn't in `base`, but defensive — once we wire it into the
///     full registry's default set, this filter keeps the invariant
///     "toggle off ⇒ tool invisible"); when it's *on*, we insert the
///     tool id.
///   * `Sandbox` — may exist in Agent mode's base set, but is hidden unless
///     explicitly enabled. Strict-KB removes it even if both toggles arrive.
pub fn effective_tool_set(base: &[String], enabled: &[ToggleId]) -> Vec<String> {
    let kb_strict_on = enabled.iter().any(|t| matches!(t, ToggleId::KbStrict));
    let web_search_on = enabled.iter().any(|t| matches!(t, ToggleId::WebSearch));
    let sandbox_on = enabled.iter().any(|t| matches!(t, ToggleId::Sandbox));

    let blocked: Option<std::collections::HashSet<&str>> = if kb_strict_on {
        Some(KB_STRICT_BLOCKED_TOOLS.iter().copied().collect())
    } else {
        None
    };

    let mut result: Vec<String> = base
        .iter()
        // Defensive filter: even if a future change adds `web_search`
        // to `base`, leaving the toggle off still hides it. Today this
        // matches nothing, but the invariant is the whole point of the
        // toggle — better to enforce it in two places than to rely on
        // every caller remembering to omit the tool.
        .filter(|name| web_search_on || name.as_str() != "web_search")
        .filter(|name| sandbox_on || name.as_str() != "run_sandbox_command")
        .filter(|name| {
            blocked
                .as_ref()
                .map(|b| !b.contains(name.as_str()))
                .unwrap_or(true)
        })
        .cloned()
        .collect();

    if web_search_on && !result.iter().any(|n| n == "web_search") {
        result.push("web_search".to_string());
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn sandbox_is_hidden_until_explicitly_enabled() {
        let base = names(&["read_file", "run_sandbox_command"]);
        assert_eq!(effective_tool_set(&base, &[]), names(&["read_file"]));
        assert_eq!(
            effective_tool_set(&base, &[ToggleId::Sandbox]),
            names(&["read_file", "run_sandbox_command"])
        );
    }

    #[test]
    fn strict_kb_wins_over_sandbox_and_all_mutating_tier_one_tools() {
        let base = names(&[
            "read_file",
            "create_dir",
            "move_file",
            "create_svg",
            "generate_image",
            "run_sandbox_command",
        ]);
        assert_eq!(
            effective_tool_set(&base, &[ToggleId::KbStrict, ToggleId::Sandbox]),
            names(&["read_file"])
        );
    }

    #[test]
    fn inventory_never_implies_sandbox_exists_when_toggle_is_off() {
        let fragment = enabled_fragment(&[]);
        assert!(fragment.contains("sandbox` (安全沙盒): OFF"));
        assert!(fragment.contains("run_sandbox_command` is NOT in your tool list"));
    }

    #[test]
    fn strict_kb_inventory_and_guidance_do_not_claim_sandbox_is_available() {
        let fragment = enabled_fragment(&[ToggleId::KbStrict, ToggleId::Sandbox]);
        assert!(fragment.contains("REQUESTED, but unavailable"));
        assert!(!fragment.contains("## Restricted Sandbox Mode"));
    }
}
