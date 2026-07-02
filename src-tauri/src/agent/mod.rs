//! Agent module - Full tool calling capability

pub mod tools;
mod agent_loop;
mod prompts;
mod profile;

pub use tools::*;
pub use agent_loop::*;
pub use prompts::{
    find_profile, find_tool_spec, get_agent_system_prompt, get_ask_system_prompt,
    get_edit_system_prompt, get_plan_system_prompt, get_read_only_system_prompt,
    list_profiles, resolve_profile,
};
pub use profile::AgentProfile;
