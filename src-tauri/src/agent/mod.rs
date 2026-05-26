//! Agent module - Full tool calling capability
//!
//! This module implements the agent loop that allows the AI to:
//! - Call tools (read_file, write_file, edit_file, etc.)
//! - See tool results
//! - Continue reasoning and calling more tools
//! - Provide final responses

mod tools;
mod agent_loop;

pub use tools::*;
pub use agent_loop::*;
