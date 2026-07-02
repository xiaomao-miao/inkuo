//! Agent profile definitions.
//!
//! A profile is a self-contained bundle that drives an `AgentSession`:
//! - system prompt content
//! - allowed tool set (filtered view of the global `ToolRegistry`)
//! - max iteration cap
//!
//! Profiles make sub-agent dispatch trivial: pick a profile, build a
//! filtered registry + session from it, run, return the result.

use serde::{Deserialize, Serialize};

/// A self-contained agent configuration consumed by the agent loop.
///
/// Lives at run-time as an owned struct so sub-agent dispatch can construct
/// fresh instances without lifetime gymnastics. Compile-time descriptors
/// in `prompts.rs` build these on demand via `resolve_profile`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentProfile {
    /// Profile identifier (e.g. `"main"`, `"office_word_expert"`).
    pub name: &'static str,
    /// Human-readable label for UI display.
    pub label: &'static str,
    /// Full system prompt text.
    pub system_prompt: String,
    /// Tool names this profile is allowed to see/call.
    /// An empty list means "see all tools" (used by the main profile).
    pub allowed_tools: Vec<String>,
    /// Hard cap on iterations inside this profile.
    pub max_iterations: usize,
}

impl AgentProfile {
    /// Builder helper: override allowed_tools.
    pub fn with_tools(mut self, tools: Vec<String>) -> Self {
        self.allowed_tools = tools;
        self
    }

    /// Builder helper: override max iterations.
    pub fn with_max_iterations(mut self, n: usize) -> Self {
        self.max_iterations = n;
        self
    }
}
