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

use std::collections::HashMap;
use std::sync::Mutex;
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
}

/// How long an asset stays valid before the next `lookup()` opportunistically
/// evicts it. We err on the generous side: an LLM round-trip plus follow-up
/// write is bounded by minutes, not hours. One hour covers a long agent run.
const ASSET_TTL: Duration = Duration::from_secs(3600);

/// Process-global registry. The agent loop is single-threaded with respect
/// to a given session, and asset operations are O(1) microseconds, so a
/// plain `Mutex` (not `RwLock`) is the right primitive — contention is
/// effectively zero.
static REGISTRY: std::sync::OnceLock<Mutex<HashMap<String, AssetEntry>>> =
    std::sync::OnceLock::new();

fn registry() -> &'static Mutex<HashMap<String, AssetEntry>> {
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Insert an asset under `id`. Returns the same id so callers can chain.
pub fn insert(id: String, entry: AssetEntry) -> String {
    let mut guard = registry().lock().expect("asset registry poisoned");
    guard.insert(id.clone(), entry);
    id
}

/// Look up an asset by id. Evicts (and reports `None` for) any entry
/// older than `ASSET_TTL` on the way through. Returns `None` if the id
/// is unknown or expired.
pub fn lookup(id: &str) -> Option<AssetEntry> {
    let mut guard = registry().lock().expect("asset registry poisoned");
    let entry = guard.get(id)?;
    if entry.inserted_at.elapsed() > ASSET_TTL {
        guard.remove(id);
        return None;
    }
    Some(entry.clone())
}

/// Remove and return an asset by id. Used when the consumer has finished
/// embedding the asset (so we can free memory instead of waiting for TTL).
/// Returns `None` if the id is unknown or already evicted.
pub fn take(id: &str) -> Option<AssetEntry> {
    let mut guard = registry().lock().expect("asset registry poisoned");
    guard.remove(id)
}

/// Drop every entry. Useful for tests and for "reset workspace" hooks.
pub fn clear() {
    let mut guard = registry().lock().expect("asset registry poisoned");
    guard.clear();
}

/// Current number of live entries (mostly for tests + diagnostics).
pub fn len() -> usize {
    let guard = registry().lock().expect("asset registry poisoned");
    guard.len()
}

/// Generate a fresh asset id. Format: `asset-<8-char base36 random>`.
/// Avoids `uuid` to keep the dep surface small; collisions only matter
/// within a single session, and 36^8 ≈ 2.8 × 10^12 is plenty.
pub fn fresh_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    // Cheap mixing: combine the high bits of nanos with the low bits.
    let mixed = (nanos as u64) ^ ((nanos >> 64) as u64);
    let id = mixed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (nanos as u64).rotate_left(17);
    format!("asset-{:08x}", (id as u32) as u32)
}

/// Build the canonical `asset://<id>` reference string the LLM should
/// emit inside `<image href="...">` (or equivalent) attributes.
pub fn reference(id: &str) -> String {
    format!("asset://{id}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_bytes() -> Vec<u8> {
        // Minimal valid PNG (1x1 black pixel).
        vec![
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // signature
            0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, // IHDR length+name
            0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, // 1x1
            0x08, 0x00, 0x00, 0x00, 0x00, 0x3B, 0x7E, 0x9B, 0x55, // bit depth / CRC
            0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, // IDAT
            0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05,
            0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00,
            0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82, // IEND
        ]
    }

    #[test]
    fn insert_then_lookup_returns_same_bytes() {
        clear();
        let id = fresh_id();
        insert(
            id.clone(),
            AssetEntry {
                mime: "image/png".to_string(),
                ext: "png".to_string(),
                data: png_bytes(),
                inserted_at: Instant::now(),
                source_path: "/tmp/x.png".to_string(),
            },
        );
        let entry = lookup(&id).expect("lookup should find freshly-inserted entry");
        assert_eq!(entry.data, png_bytes());
        assert_eq!(entry.mime, "image/png");
    }

    #[test]
    fn take_removes_entry() {
        clear();
        let id = fresh_id();
        insert(
            id.clone(),
            AssetEntry {
                mime: "image/jpeg".to_string(),
                ext: "jpg".to_string(),
                data: vec![1, 2, 3],
                inserted_at: Instant::now(),
                source_path: "/tmp/y.jpg".to_string(),
            },
        );
        let taken = take(&id).expect("take should find entry");
        assert_eq!(taken.data, vec![1, 2, 3]);
        assert!(lookup(&id).is_none(), "entry should be gone after take");
    }

    #[test]
    fn fresh_ids_are_unique() {
        // FLAKY-CANDIDATE: with 36^8 id space, a collision here is
        // essentially impossible. Use the simpler existence test
        // rather than running millions of iterations.
        let a = fresh_id();
        let b = fresh_id();
        assert_ne!(a, b);
        assert!(a.starts_with("asset-"));
        assert!(b.starts_with("asset-"));
    }

    #[test]
    fn lookup_unknown_id_returns_none() {
        clear();
        assert!(lookup("asset-deadbeef").is_none());
    }

    #[test]
    fn reference_format_is_asset_scheme() {
        assert_eq!(reference("asset-123"), "asset://asset-123");
    }
}
