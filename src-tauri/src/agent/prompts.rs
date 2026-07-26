//! Prompts module - loads system prompts from markdown files.
//!
//! Layout under `prompts/`:
//! - `main/agent.slim.md`        — slim main-agent prompt (default for Agent Mode)
//! - `ask.md`, `plan.md`, `edit.md` — mode-specific prompts (unchanged)
//! - `tool_specs/*.md`           — detailed tool specs, loaded on-demand via `get_tool_help`
//! - `subagents/*.md`            — sub-agent system prompts, used by `delegate_to`
//! - `presets/*.md`              — preset workflows (paper, project, ...)
//!
//! All prompt content is English (LLMs are tuned best on English instructions).
//! UI-visible labels are localized separately in the frontend (`toolUtils.ts`).
//!
//! Every prompt is embedded at compile time via `include_str!`.
//!
//! ## Coordination between `tools:` here and `agent.slim.md`
//!
//! The `tools:` array for the `"main"` profile is the **authoritative tool
//! registry** for the orchestrator. It must stay in sync with the Tier 1
//! table in `prompts/main/agent.slim.md §1.1` (the agent.slim.md table is
//! the human-readable form sent to the model; this array is the runtime
//! gate). When you add a tool to one, update the other in the same change.

use crate::agent::profile::AgentProfile;
use std::sync::OnceLock;

/// Compile-time catalog: every named profile we ship.
///
/// Adding a new sub-agent is a three-line change here plus one prompt file.
pub const PROFILES: &[ProfileDescriptor] = &[
    ProfileDescriptor {
        name: "main",
        label: "Main Agent",
        system_prompt: include_str!("../../prompts/main/agent.slim.md"),
        // Office tools intentionally absent — main agent must `delegate_to`
        // `office_word_expert` / `office_excel_expert` for any .docx/.xlsx
        // work. This keeps the per-request tool schema (and tokens) small
        // for the common case of pure code / Markdown work.
        tools: &[
            "read_file", "write_file", "edit_file",
            "list_dir", "glob", "grep",
            "database_search",
            // `create_svg` lets the agent author a self-contained .svg
            // file. No Office equivalent — SVG is portable to docx, the
            // web, and the in-app viewer without going through Office.
            "create_svg",
            "get_tool_help", "delegate_to",
            "update_todo",
        ],
        max_iterations: 50,
    },
    ProfileDescriptor {
        name: "office_word_expert",
        label: "Word Document Expert",
        system_prompt: include_str!("../../prompts/subagents/office_word_expert.md"),
        tools: &[
            "read_file", "write_file",
            "list_dir", "glob", "grep",
            "read_office_file", "create_word_doc", "inspect_office", "compare_word_docs",
        ],
        max_iterations: 50,
    },
    ProfileDescriptor {
        name: "office_pptx_expert",
        label: "PowerPoint Expert",
        system_prompt: include_str!("../../prompts/subagents/office_pptx_expert.md"),
        // Packs pre-existing SVGs into an editable .pptx. One tool — no
        // incremental edit API in v1; users re-author the source SVGs and
        // re-call.
        tools: &[
            "read_file", "write_file",
            "list_dir", "glob", "grep",
            "create_pptx",
        ],
        max_iterations: 50,
    },
    ProfileDescriptor {
        name: "office_excel_expert",
        label: "Excel Spreadsheet Expert",
        system_prompt: include_str!("../../prompts/subagents/office_excel_expert.md"),
        tools: &[
            "read_file", "write_file",
            "list_dir", "glob", "grep",
            "read_office_file",
            "create_excel", "modify_excel",
            "inspect_office",
        ],
        max_iterations: 50,
    },
    ProfileDescriptor {
        name: "md_writer",
        label: "Markdown Writer",
        system_prompt: include_str!("../../prompts/subagents/md_writer.md"),
        tools: &[
            "read_file", "write_file", "edit_file",
            "list_dir", "glob", "grep",
            "database_search",
        ],
        max_iterations: 50,
    },
    ProfileDescriptor {
        name: "researcher",
        label: "Researcher",
        system_prompt: include_str!("../../prompts/subagents/researcher.md"),
        tools: &[
            "read_file", "list_dir", "glob", "grep", "database_search",
        ],
        max_iterations: 50,
    },
    ProfileDescriptor {
        name: "batch_editor",
        label: "Batch Editor",
        system_prompt: include_str!("../../prompts/subagents/batch_editor.md"),
        tools: &[
            "read_file", "write_file", "edit_file",
            "list_dir", "glob", "grep",
            "read_office_file", "inspect_office",
            "create_word_doc", "modify_excel",
        ],
        max_iterations: 50,
    },
    ProfileDescriptor {
        name: "code_expert",
        label: "Code Engineering Expert",
        system_prompt: include_str!("../../prompts/subagents/code_expert.md"),
        tools: &[
            "read_file", "write_file", "edit_file",
            "list_dir", "glob", "grep",
            "database_search",
        ],
        max_iterations: 50,
    },
    ProfileDescriptor {
        name: "flowchart_expert",
        label: "Flowchart Expert",
        system_prompt: include_str!("../../prompts/subagents/flowchart_expert.md"),
        // `render_mermaid` is in-process via the `merman` crate (pure-Rust
        // mermaid.js 11.15 parity renderer, no Node.js / Chromium needed);
        // `read_file`/`write_file` cover Markdown extraction and side
        // outputs (e.g. `.mmd` source files). No delegate_to — flowchart
        // work is self-contained.
        tools: &[
            "read_file", "write_file",
            "list_dir", "glob",
            "render_mermaid",
        ],
        max_iterations: 50,
    },
    ProfileDescriptor {
        name: "word_image_expert",
        label: "Word Image Expert",
        system_prompt: include_str!("../../prompts/subagents/word_image_expert.md"),
        // Reuses `create_word_doc` with the new `image` element type. Read-only
        // inspection tools (`read_office_file`, `inspect_office`) are needed to
        // resolve element ids when inserting relative to an anchor.
        tools: &[
            "read_file",
            "list_dir", "glob", "grep",
            "read_office_file", "inspect_office",
            "create_word_doc",
        ],
        max_iterations: 50,
    },
];

/// Compile-time tool specs. Loaded on-demand by `get_tool_help`.
///
/// Keys are **business categories** (not tool names), so the LLM only
/// pulls in context relevant to the current task — e.g. an Excel-only
/// task loads `excel`, an .md writing task loads `markdown`, etc.
/// The spec text itself is purely internal — never rendered to users.
pub const TOOL_SPECS: &[(&str, &str)] = &[
    ("general",  include_str!("../../prompts/tool_specs/general.md")),
    ("word",     include_str!("../../prompts/tool_specs/word.md")),
    ("excel",    include_str!("../../prompts/tool_specs/excel.md")),
    ("pptx",     include_str!("../../prompts/tool_specs/pptx.md")),
    ("markdown", include_str!("../../prompts/tool_specs/markdown.md")),
    ("media",    include_str!("../../prompts/tool_specs/media.md")),
    ("svg",      include_str!("../../prompts/tool_specs/svg.md")),
];

/// Static description of a profile (compile-time constants).
pub struct ProfileDescriptor {
    pub name: &'static str,
    pub label: &'static str,
    pub system_prompt: &'static str,
    pub tools: &'static [&'static str],
    pub max_iterations: usize,
}

/// Find a profile by name. Returns None if unknown.
pub fn find_profile(name: &str) -> Option<&'static ProfileDescriptor> {
    PROFILES.iter().find(|p| p.name == name)
}

/// Look up a tool spec by key. Used by `get_tool_help`.
pub fn find_tool_spec(name: &str) -> Option<&'static str> {
    TOOL_SPECS
        .iter()
        .find(|(k, _)| *k == name)
        .map(|(_, v)| *v)
}

/// List all available profile names + labels (for `delegate_to` enum validation
/// and for UI presentation).
pub fn list_profiles() -> Vec<(&'static str, &'static str)> {
    PROFILES.iter().map(|p| (p.name, p.label)).collect()
}

/// Cached parsed profiles (parsed once, reused across sessions).
static PROFILE_CACHE: OnceLock<Vec<(&'static str, AgentProfile)>> = OnceLock::new();

/// Get or initialize the cached parsed profiles.
fn cached_profiles() -> &'static Vec<(&'static str, AgentProfile)> {
    PROFILE_CACHE.get_or_init(|| {
        PROFILES
            .iter()
            .map(|p| {
                (
                    p.name,
                    AgentProfile {
                        name: p.name,
                        label: p.label,
                        system_prompt: p.system_prompt.to_string(),
                        allowed_tools: p.tools.iter().map(|s| s.to_string()).collect(),
                        max_iterations: p.max_iterations,
                    },
                )
            })
            .collect()
    })
}

/// Resolve a profile name into a fully-owned `AgentProfile` ready to drive
/// `AgentSession::new_with_profile`.
///
/// If `override_max_iterations` is `Some`, the resolved profile's
/// `max_iterations` is replaced with that value (clamped to `[1, 200]`).
/// This is how the per-expert setting from the UI overrides the compile-time
/// default. Pass `None` to use the compile-time default.
pub fn resolve_profile(name: &str, override_max_iterations: Option<usize>) -> Option<AgentProfile> {
    let entries = cached_profiles();
    let n = override_max_iterations.unwrap_or(0).clamp(1, 200);
    entries
        .iter()
        .find(|(profile_name, _)| *profile_name == name)
        .map(|(_, p)| {
            let mut profile = p.clone();
            if override_max_iterations.is_some() && n > 0 {
                profile.max_iterations = n;
            }
            profile
        })
}

// ---------------------------------------------------------------------------
// Mode-specific prompt accessors (used by other modules).
// Kept as functions (not consts) for symmetry and future runtime override.
// ---------------------------------------------------------------------------

pub fn get_ask_system_prompt() -> String {
    include_str!("../../prompts/ask.md").to_string()
}

pub fn get_plan_system_prompt() -> String {
    include_str!("../../prompts/plan.md").to_string()
}

pub fn get_edit_system_prompt() -> String {
    include_str!("../../prompts/edit.md").to_string()
}

pub fn get_read_only_system_prompt() -> String {
    get_ask_system_prompt()
}

/// Main Agent Mode prompt. Slim by default. Kept as a function so future
/// runtime config (e.g. `settings.use_slim_agent_prompt`) can override
/// without changing call sites.
pub fn get_agent_system_prompt() -> String {
    include_str!("../../prompts/main/agent.slim.md").to_string()
}
