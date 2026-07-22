//! Process-wide runtime singletons and registries.
//!
//! Submodules in this directory hold things that need to be reachable from
//! anywhere in the crate without a circular dependency back through
//! `commands.rs`:
//!
//!  - `cancel` — the AI stream cancellation set + RAII guard.

pub mod cancel;