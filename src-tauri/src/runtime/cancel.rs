//! Stream cancellation registry.
//!
//! Long-running streaming commands (AI edits, agent loops, inline completions)
//! need a way to be cancelled cooperatively. We expose a process-global
//! `HashSet<String>` of cancelled session ids; each in-flight stream loop
//! calls `is_stream_cancelled(session_id)` at safe points and bails out
//! early when it returns `true`.
//!
//! Why a global HashSet instead of per-session channels:
//!  - Cancellation is fire-and-forget from the user's perspective (they
//!    click "stop" and walk away), so we don't need backpressure.
//!  - The set is tiny (only entries for currently-cancelled sessions), so
//!    linear scans are fine.
//!  - A `HashSet` survives the cancellation sender dropping (the receiver
//!    loop can still observe it), whereas a oneshot channel would deadlock
//!    the second time the user cancels the same session.
//!
//! The `StreamCancelGuard` RAII wrapper exists because forgetting to call
//! `clear_stream_cancelled` after a stream completes leaks the flag into
//! the next request — every subsequent stream for the same `session_id`
//! would be misclassified as cancelled and bail out immediately. The
//! guard makes the cleanup unconditional.

use std::collections::HashSet;

use once_cell::sync::Lazy;
use parking_lot::Mutex;

pub static STREAM_CANCELLED: Lazy<Mutex<HashSet<String>>> =
    Lazy::new(|| Mutex::new(HashSet::new()));

/// True if the given session has been marked cancelled. Use this rather
/// than reaching for `STREAM_CANCELLED.lock()` directly at every call site
/// so the lock acquisition pattern stays uniform (and so we have a single
/// place to swap in a more sophisticated cancellation queue later).
pub fn is_stream_cancelled(session_id: &str) -> bool {
    STREAM_CANCELLED.lock().contains(session_id)
}

/// Mark a session as cancelled. Cheaply idempotent — the existing key
/// stays put if already present.
pub fn mark_stream_cancelled(session_id: &str) {
    STREAM_CANCELLED.lock().insert(session_id.to_string());
}

/// Drop the cancellation flag for `session_id`, returning whether one was
/// actually removed. Callers usually want to suppress the regular "done"
/// event when this returns `true`.
pub fn clear_stream_cancelled(session_id: &str) -> bool {
    STREAM_CANCELLED.lock().remove(session_id)
}

/// RAII guard that clears the cancellation flag for `session_id` on drop.
///
/// Why this exists: cancellation flags are a global `HashSet<String>`, and
/// every code path that calls `mark_stream_cancelled` MUST call
/// `clear_stream_cancelled` (or leave the flag in a way that prevents the
/// next request from being misclassified as cancelled). A `?` early-return,
/// a panic, or a refactor that adds a new error branch all silently leak
/// the flag, which then blocks every subsequent stream for that session_id.
/// The guard makes the cleanup unconditional.
///
/// Note: this guard does NOT mark the session as cancelled on creation.
/// Cancellation is set by the `ai_*_cancel` command, not by the start of a
/// stream; this guard only guarantees cleanup on the stream side.
pub struct StreamCancelGuard {
    session_id: String,
    cleared: bool,
}

impl StreamCancelGuard {
    /// Create a guard that will clear the cancellation flag for
    /// `session_id` on drop. The flag is NOT set by this constructor.
    pub fn new(session_id: &str) -> Self {
        Self {
            session_id: session_id.to_string(),
            cleared: false,
        }
    }

    /// Explicitly clear the flag without waiting for drop, returning
    /// `true` if the flag was still set when this was called. Consumes
    /// the guard so its `Drop` does not run a second clear.
    pub fn clear(mut self) -> bool {
        if !self.cleared {
            self.cleared = true;
            clear_stream_cancelled(&self.session_id)
        } else {
            false
        }
    }
}

impl Drop for StreamCancelGuard {
    fn drop(&mut self) {
        if !self.cleared {
            let _ = clear_stream_cancelled(&self.session_id);
        }
    }
}