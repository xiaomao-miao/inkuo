//! Binary asset registry: a side-channel for binary data (images, files)
//! that must NOT enter the LLM's conversation history as text.
//!
//! ## Why this exists
//!
//! `read_image` used to return the raw base64-encoded image data as part
//! of the tool result. That base64 string flows into the API request
//! verbatim, which balloons the context window for even a single 1 MB
//! PNG to ~250 k tokens — far past most providers' input limits.
//!
//! The fix: the LLM never sees the bytes. Instead, `read_image` reads
//! the file into this registry and hands back a short, opaque
//! `asset_id`. The LLM then references that id inside `<image href="asset://...">`
//! or similar placeholders. When a downstream tool (e.g. `create_svg`,
//! `create_pptx`) writes the actual artifact, it resolves the
//! placeholder back to the real bytes at the very last moment — right
//! before writing the file to disk. The base64 never appears in any
//! message, tool call, or prompt.
//!
//! ## Lifecycle
//!
//! Entries are inserted by `read_image` (or, in the future, by
//! `read_file` for non-text payloads). They live until either the
//! process exits, the consumer explicitly invalidates them via
//! `take()`, or the registry's per-entry TTL expires. Currently the
//! TTL is `1 hour`, which is generous enough to span the entire
//! "tool-call round-trip → follow-up write" window even on slow
//! models, while still bounding the worst-case memory cost.

use parking_lot::{Mutex, MutexGuard};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// One binary asset held in the registry.
#[derive(Clone)]
pub struct AssetEntry {
    /// MIME type (`image/png`, `image/jpeg`, ...).
    pub mime: String,
    /// File extension (`png`, `jpg`, ...).
    pub ext: String,
    /// Raw bytes of the asset.
    pub data: Vec<u8>,
    /// When this entry was inserted. Used for TTL eviction.
    pub inserted_at: Instant,
    /// Absolute filesystem path the asset was loaded from. Stored for
    /// debug logs and diagnostics — *not* used to re-read the file.
    pub source_path: String,
    /// Canonical workspace that owned the tool call which loaded this asset.
    /// Every consumer must match this boundary before receiving the bytes.
    pub workspace_root: String,
}

/// How long an asset stays valid before the next `lookup()` opportunistically
/// evicts it. We err on the generous side: an LLM round-trip plus follow-up
/// write is bounded by minutes, not hours. One hour covers a long agent run.
const ASSET_TTL: Duration = Duration::from_secs(3600);
/// Hard process-wide bounds. `read_image` already caps one file at 20 MiB;
/// these limits prevent a long agent session from accumulating hundreds of
/// still-live assets before the one-hour TTL elapses.
const MAX_ASSET_COUNT: usize = 32;
const MAX_ASSET_BYTES: usize = 96 * 1024 * 1024;

/// Process-global registry. The agent loop is single-threaded with respect
/// to a given session, and asset operations are O(1) microseconds, so a
/// plain `Mutex` (not `RwLock`) is the right primitive — contention is
/// effectively zero.
static REGISTRY: std::sync::OnceLock<Mutex<HashMap<String, AssetEntry>>> =
    std::sync::OnceLock::new();

#[cfg(test)]
static TEST_REGISTRY_LOCK: std::sync::OnceLock<Mutex<()>> = std::sync::OnceLock::new();

fn registry() -> &'static Mutex<HashMap<String, AssetEntry>> {
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Serialize tests that mutate the process-global registry. Rust runs test
/// modules concurrently, including the multimodal bridge tests in another
/// module, so local `clear()` calls alone are not sufficient isolation.
#[cfg(test)]
pub(crate) fn test_registry_guard() -> MutexGuard<'static, ()> {
    TEST_REGISTRY_LOCK.get_or_init(|| Mutex::new(())).lock()
}

/// Insert an asset under `id`. Returns the same id so callers can chain.
pub fn insert(id: String, entry: AssetEntry) -> String {
    let mut guard = registry().lock();
    guard.insert(id.clone(), entry);
    evict_expired_and_over_budget(&mut guard, Some(&id));
    id
}

/// Look up an asset by id. Evicts (and reports `None` for) any entry
/// older than `ASSET_TTL` on the way through. Returns `None` if the id
/// is unknown or expired.
fn lookup(id: &str) -> Option<AssetEntry> {
    let mut guard = registry().lock();
    evict_expired_and_over_budget(&mut guard, None);
    guard.get(id).cloned()
}

/// Look up an asset only when its owner matches the calling session's
/// canonical workspace. Opaque UUIDs prevent guessing; this ownership check
/// prevents an id leaked across concurrent chats from crossing workspaces.
pub fn lookup_for_workspace(id: &str, workspace: Option<&str>) -> Option<AssetEntry> {
    let workspace = workspace.filter(|path| !path.trim().is_empty())?;
    let canonical_workspace = std::fs::canonicalize(workspace).ok()?;
    let entry = lookup(id)?;
    let owner = std::path::Path::new(&entry.workspace_root);
    (owner == canonical_workspace.as_path()).then_some(entry)
}

/// Remove and return an asset by id. Used when the consumer has finished
/// embedding the asset (so we can free memory instead of waiting for TTL).
/// Returns `None` if the id is unknown or already evicted.
pub fn take(id: &str) -> Option<AssetEntry> {
    let mut guard = registry().lock();
    guard.remove(id)
}

/// Drop every entry. Useful for tests and for "reset workspace" hooks.
pub fn clear() {
    let mut guard = registry().lock();
    guard.clear();
}

/// Current number of live entries (mostly for tests + diagnostics).
pub fn len() -> usize {
    let guard = registry().lock();
    guard.len()
}

/// Current decoded byte footprint (diagnostics + tests).
pub fn byte_len() -> usize {
    let guard = registry().lock();
    guard.values().map(|entry| entry.data.len()).sum()
}

fn evict_expired_and_over_budget(
    entries: &mut HashMap<String, AssetEntry>,
    newest_id: Option<&str>,
) {
    entries.retain(|_, entry| entry.inserted_at.elapsed() <= ASSET_TTL);

    loop {
        let total_bytes: usize = entries.values().map(|entry| entry.data.len()).sum();
        if entries.len() <= MAX_ASSET_COUNT && total_bytes <= MAX_ASSET_BYTES {
            break;
        }
        let oldest = entries
            .iter()
            // Prefer retaining the just-inserted asset when another entry can
            // be evicted. If it is the only oversized entry it will remain;
            // the per-file loader cap guarantees it is < MAX_ASSET_BYTES.
            .filter(|(id, _)| {
                newest_id
                    .map(|newest| id.as_str() != newest)
                    .unwrap_or(true)
            })
            .min_by_key(|(_, entry)| entry.inserted_at)
            .map(|(id, _)| id.clone())
            .or_else(|| entries.keys().next().cloned());
        let Some(oldest) = oldest else {
            break;
        };
        entries.remove(&oldest);
    }
}

/// Generate an unguessable fresh asset id. UUID v4 is already shipped by the
/// application and avoids time-derived collisions across concurrent sessions.
pub fn fresh_id() -> String {
    format!("asset-{}", uuid::Uuid::new_v4())
}

/// Build the canonical `asset://<id>` reference string the LLM should
/// emit inside `<image href="...">` (or equivalent) attributes.
pub fn reference(id: &str) -> String {
    format!("asset://{id}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(bytes: usize, age: Duration) -> AssetEntry {
        AssetEntry {
            mime: "image/png".to_string(),
            ext: "png".to_string(),
            data: vec![0; bytes],
            inserted_at: Instant::now() - age,
            source_path: "test.png".to_string(),
            workspace_root: std::env::temp_dir().to_string_lossy().to_string(),
        }
    }

    #[test]
    fn fresh_ids_are_uuid_v4() {
        let id = fresh_id();
        let parsed = uuid::Uuid::parse_str(id.strip_prefix("asset-").unwrap()).unwrap();
        assert_eq!(parsed.get_version(), Some(uuid::Version::Random));
    }

    #[test]
    fn insert_enforces_global_count_cap() {
        let _registry_guard = test_registry_guard();
        clear();
        for index in 0..(MAX_ASSET_COUNT + 5) {
            insert(
                format!("asset-{index}"),
                entry(
                    1,
                    Duration::from_millis((MAX_ASSET_COUNT + 5 - index) as u64),
                ),
            );
        }
        assert!(len() <= MAX_ASSET_COUNT);
        assert!(lookup(&format!("asset-{}", MAX_ASSET_COUNT + 4)).is_some());
        clear();
    }

    #[test]
    fn lookup_opportunistically_removes_expired_entries() {
        let _registry_guard = test_registry_guard();
        clear();
        insert(
            "expired".to_string(),
            entry(4, ASSET_TTL + Duration::from_secs(1)),
        );
        assert!(lookup("expired").is_none());
        assert_eq!(byte_len(), 0);
        clear();
    }
}
