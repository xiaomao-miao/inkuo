//! Prompts module - loads system prompts from markdown files
//!
//! Prompts are stored in the prompts/ directory as markdown files for easy editing.
//! They are embedded at compile time using the include_str! macro.

/// System prompt for the agent mode (full tool access)
pub fn get_agent_system_prompt() -> String {
    include_str!("../../prompts/agent.md").to_string()
}

/// System prompt for ask mode (read-only, conversational)
pub fn get_ask_system_prompt() -> String {
    include_str!("../../prompts/ask.md").to_string()
}

/// System prompt for plan mode (structured plan output only)
pub fn get_plan_system_prompt() -> String {
    include_str!("../../prompts/plan.md").to_string()
}

/// System prompt for edit mode (document editing)
pub fn get_edit_system_prompt() -> String {
    include_str!("../../prompts/edit.md").to_string()
}

/// Alias for get_ask_system_prompt (used by commands_agent.rs)
pub fn get_read_only_system_prompt() -> String {
    get_ask_system_prompt()
}
