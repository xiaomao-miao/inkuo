//! Process-wide registry of the Tauri `AppHandle`.
//!
//! ## Why
//!
//! A few subsystems (`WebSearchTool`, late-binding agent tools, any
//! future IPC bridge that wants to emit events) need access to the
//! running `AppHandle` but are constructed *before* the Tauri builder
//! hands it out — or before any code path has the chance to call a
//! setter on them. The previous design relied on a per-tool `Option<
//! AppHandle>` populated eagerly by the tool registry's `setup`
//! hook, and a missed hook meant the tool returned "AppHandle
//! missing" forever.
//!
//! This module gives those consumers a single, process-global source
//! of truth for the live `AppHandle`, populated exactly once during
//! `lib.rs::run`'s setup closure. Consumers should treat the return
//! value as **fallback-only** — when possible, prefer the `AppHandle`
//! already in scope (Tauri commands, agent calls).
//!
//! ## Threading
//!
//! `OnceLock<AppHandle>` is `Send + Sync`; cloning the stored handle is
//! cheap (it's `Arc` underneath). Reads are wait-free after the first
//! write.

use std::sync::OnceLock;
use tauri::AppHandle;

/// Singleton storage. Set exactly once at startup; never cleared.
static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();

/// Register the running app's handle. Idempotent: a second call is a
/// no-op so the setup closure can be re-entered safely during dev-mode
/// hot reload. Returns `true` if the registration actually took effect
/// (i.e. this is the first call), `false` otherwise.
pub fn set_app_handle(handle: AppHandle) -> bool {
    APP_HANDLE.set(handle).is_ok()
}

/// Returns a clone of the running app's handle, or `None` if startup
/// has not yet completed. Intended as a *fallback* for code paths that
/// lost the original handle (e.g. `WebSearchTool` constructed before
/// `setup` ran). Prefer the handle that's already in scope whenever
/// possible.
pub fn current_app_handle() -> Option<AppHandle> {
    APP_HANDLE.get().cloned()
}

