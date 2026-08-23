//! Agent module - Full tool calling capability

mod agent_helpers;
mod agent_loop;
mod multimodal;
mod profile;
mod prompts;
pub mod tools;

pub use agent_loop::*;
pub use multimodal::{
    push_visual_inspection_bounded, resolve_image_attachment_groups, resolve_image_attachments,
    validate_image_request_budget, visual_inspection_from_asset,
    visual_inspections_from_tool_output, FrontendImageAttachment, ImageAttachment, ImageDetail,
    MultimodalError, VisualInspectionInput,
};
pub use profile::AgentProfile;
pub use prompts::{
    find_profile, find_tool_spec, get_agent_system_prompt, get_edit_system_prompt, list_profiles,
    resolve_profile,
};
pub use tools::*;
