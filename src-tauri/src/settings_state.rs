//! Settings struct + on-disk cache + helpers.
//!
//! Owns the canonical `Settings` shape (whatever lives at
//! `~/Library/Application Support/inkuo/settings.json`) plus the in-memory
//! cache used by `get_settings_cached`.
//!
//! Why a separate module:
//!  - `commands/mod.rs` is a 2 000-line god file that needs a cycle-free
//!    place to share `Settings` + the cache between itself, `snapshots`,
//!    `agent_loop`, `ai_config`, and the various tool impls.
//!  - Having a single owner of the schema stops drift — every change goes
//!    through this file.
//!
//! Tauri commands (`get_settings`, `save_settings`, `test_api_config`) still
//! live in `crate::commands` for now; this module is purely data + cache.

use std::io::Write;

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::ai_config::AIProviderKind;
use crate::error::AppError;

pub type SettingsError = AppError;

/// Cached settings to avoid repeated disk reads. Updated whenever
/// `update_settings_cache` is called.
static SETTINGS_CACHE: Lazy<Mutex<Option<Settings>>> = Lazy::new(|| Mutex::new(None));

// ── Schema ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiConfig {
    pub id: String,
    pub name: String,
    pub provider: AIProviderKind,
    pub base_url: String,
    pub api_key: Option<String>,
    pub model: String,
    pub is_default: bool,
    pub enabled: bool,
    pub temperature: f32,
    pub max_tokens: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SnapshotSettings {
    #[serde(default = "default_snapshot_max_count")]
    pub max_count: usize,
    #[serde(default = "default_snapshot_auto_baseline")]
    pub auto_baseline: bool,
}

fn default_snapshot_max_count() -> usize {
    50
}

fn default_snapshot_auto_baseline() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSearchProviderConfig {
    #[serde(default)]
    pub id: String,
    /// Optional user-provided API key / appid / token. When `None` the
    /// backend may fall back to a built-in default (subject to rate
    /// limits).
    #[serde(default)]
    pub api_key: Option<String>,
    /// Optional override for the provider's endpoint.
    #[serde(default)]
    pub base_url: Option<String>,
    /// Per-provider kill switch.
    #[serde(default = "default_web_search_provider_enabled")]
    pub enabled: bool,
}

fn default_web_search_provider_enabled() -> bool {
    true
}

impl Default for WebSearchProviderConfig {
    fn default() -> Self {
        Self {
            id: "baike".to_string(),
            api_key: None,
            base_url: None,
            enabled: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSearchSettings {
    /// Master kill switch.
    #[serde(default = "default_web_search_enabled")]
    pub enabled: bool,
    /// Provider-specific configs.
    #[serde(default)]
    pub providers: Vec<WebSearchProviderConfig>,
    /// Hard cap on results per call.
    #[serde(default = "default_web_search_max_results")]
    pub max_results: usize,
    /// Routing mode for web_search: "local" / "cloud" / free-form.
    #[serde(default = "default_web_search_routing")]
    pub routing: String,
}

fn default_web_search_enabled() -> bool {
    true
}

fn default_web_search_max_results() -> usize {
    5
}

fn default_web_search_routing() -> String {
    "local".to_string()
}

impl Default for WebSearchSettings {
    fn default() -> Self {
        Self {
            enabled: default_web_search_enabled(),
            providers: vec![WebSearchProviderConfig::default()],
            max_results: default_web_search_max_results(),
            routing: default_web_search_routing(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CloudSettings {
    /// Whether cloud mode is currently active.
    #[serde(default)]
    pub cloud_mode_enabled: bool,
    /// The logged-in cloud account.
    #[serde(default)]
    pub account: Option<crate::cloud::CloudAccount>,
    /// Cached list of cloud models.
    #[serde(default)]
    pub cached_models: Vec<crate::cloud::CloudModelEntry>,
    /// Active cloud model id.
    #[serde(default)]
    pub active_cloud_model_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub theme: String,
    pub accent_color: String,
    pub editor_font_size: u32,
    pub editor_font_family: String,
    pub editor_word_wrap: bool,
    pub editor_line_numbers: bool,
    pub api_configs: Vec<ApiConfig>,
    pub active_api_config_id: Option<String>,
    pub embedding_model: String,
    pub embedding_model_path: Option<String>,
    pub chunk_size: usize,
    pub chunk_overlap: usize,
    #[serde(default)]
    pub snapshot: SnapshotSettings,
    #[serde(default)]
    pub web_search: WebSearchSettings,
    #[serde(default)]
    pub cloud: CloudSettings,
}

impl Default for Settings {
    fn default() -> Self {
        let default_api_config = ApiConfig {
            id: uuid::Uuid::new_v4().to_string(),
            name: "DeepSeek V3".to_string(),
            provider: AIProviderKind::DeepSeek,
            base_url: "https://api.deepseek.com".to_string(),
            api_key: None,
            model: "deepseek-chat".to_string(),
            is_default: true,
            enabled: true,
            temperature: 0.7,
            max_tokens: Some(4096),
        };

        Self {
            theme: "inkuo-dark".to_string(),
            accent_color: "#7C5CFF".to_string(),
            editor_font_size: 14,
            editor_font_family: "JetBrains Mono, monospace".to_string(),
            editor_word_wrap: true,
            editor_line_numbers: true,
            api_configs: vec![default_api_config.clone()],
            active_api_config_id: Some(default_api_config.id),
            embedding_model: "BAAI/bge-small-zh-v1.5".to_string(),
            embedding_model_path: None,
            chunk_size: 500,
            chunk_overlap: 50,
            snapshot: SnapshotSettings::default(),
            web_search: WebSearchSettings::default(),
            cloud: CloudSettings::default(),
        }
    }
}

// ── Cache helpers ───────────────────────────────────────────────────────────

/// Get cached settings, reading from disk only when cache is empty.
pub fn get_settings_cached() -> Result<Settings, AppError> {
    {
        let guard = SETTINGS_CACHE.lock();
        if let Some(ref settings) = *guard {
            return Ok(settings.clone());
        }
    }
    let settings = read_settings_from_disk()?;
    let mut guard = SETTINGS_CACHE.lock();
    // Re-check: another thread may have populated the cache while we were
    // reading disk. Theirs is at least as fresh as ours, so we drop ours.
    if let Some(existing) = guard.clone() {
        return Ok(existing);
    }
    *guard = Some(settings.clone());
    Ok(settings)
}

/// Update the settings cache. Does not touch disk — call `atomic_write_settings`
/// (or the `save_settings` IPC command) for that.
pub fn update_settings_cache(settings: Settings) {
    let mut guard = SETTINGS_CACHE.lock();
    *guard = Some(settings);
}

/// Patch the in-memory settings cache in-place for the cloud account.
/// **Does NOT write to disk.**
pub fn patch_settings_cache_account(account: crate::cloud::CloudAccount) {
    let mut guard = SETTINGS_CACHE.lock();
    if let Some(ref mut settings) = *guard {
        settings.cloud.account = Some(account);
    }
}

/// Clear the cached cloud account. **Does NOT write to disk.**
pub fn clear_settings_cache_account() {
    let mut guard = SETTINGS_CACHE.lock();
    if let Some(ref mut settings) = *guard {
        settings.cloud.account = None;
    }
}

/// Resolve the on-disk settings file path. Today this constructs the
/// canonical config location (`<config_dir>/inkuo/settings.json`); fall
/// back to a relative path if the OS reports no usable config dir.
pub fn get_settings_path() -> std::path::PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("inkuo")
        .join("settings.json")
}

/// Read fresh settings from disk. Loads `Settings::default()` if no file
/// exists yet. Tries a "merged defaults + overrides" pass if a strict parse
/// fails, so legacy fields can be picked up without crashing later startup.
pub fn read_settings_from_disk() -> Result<Settings, AppError> {
    let path = get_settings_path();
    if !path.exists() {
        return Ok(Settings::default());
    }
    let content = std::fs::read_to_string(&path).map_err(|e| AppError::ReadSettings(e.to_string()))?;

    match serde_json::from_str::<Settings>(&content) {
        Ok(settings) => Ok(settings),
        Err(e) => {
            tracing::warn!("Failed to parse settings ({}), trying merged format", e);

            let value: serde_json::Value = serde_json::from_str(&content)
                .map_err(|e| AppError::ParseSettings(format!("settings JSON: {}", e)))?;

            if let Some(object) = value.as_object() {
                let mut merged = serde_json::to_value(Settings::default())
                    .map_err(|e| AppError::SerializeSettings(format!("default settings: {}", e)))?;

                if let Some(merged_object) = merged.as_object_mut() {
                    for (key, value) in object {
                        merged_object.insert(key.clone(), value.clone());
                    }
                }

                if let Ok(settings) = serde_json::from_value::<Settings>(merged) {
                    return Ok(settings);
                }
            }

            Err(AppError::ParseSettings(format!(
                "settings format is invalid and no longer supports legacy single-config fields: {}",
                e
            )))
        }
    }
}

/// Write `content` (already-serialised JSON) to `path` atomically.
/// Prefer `write_settings_to_disk` — it serialises for you.
pub fn atomic_write_settings(path: &std::path::Path, content: &str) -> Result<(), AppError> {
    use std::io::Write;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| AppError::CreateConfigDirectory(e.to_string()))?;
    }

    let unique_suffix = format!(
        "{}.{}.tmp",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    let temp_path = path.with_extension(format!("json.{}", unique_suffix));

    let write_result = (|| -> std::io::Result<()> {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)?;
        f.write_all(content.as_bytes())?;
        f.flush()?;
        f.sync_all()?;
        Ok(())
    })();

    if let Err(e) = write_result {
        let _ = std::fs::remove_file(&temp_path);
        return Err(AppError::WriteSettings(format!("write temp settings: {}", e)));
    }

    if let Err(e) = std::fs::rename(&temp_path, path) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(AppError::WriteSettings(format!("rename temp settings: {}", e)));
    }

    // Best-effort sweep of any stale `*.tmp` siblings left behind by a
    // crashed previous write.
    if let Some(parent) = path.parent() {
        if let Ok(entries) = std::fs::read_dir(parent) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p == path {
                    continue;
                }
                let Some(name) = p.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                if name.starts_with("settings.json.") && name.ends_with(".tmp") {
                    let _ = std::fs::remove_file(&p);
                }
            }
        }
    }

    Ok(())
}

/// Convenience for `save_settings`: serialise + atomic write in one call.
pub fn write_settings_to_disk(settings: &Settings) -> Result<(), AppError> {
    let path = get_settings_path();
    let json = serde_json::to_string_pretty(settings)
        .map_err(|e| AppError::SerializeSettings(e.to_string()))?;
    atomic_write_settings(&path, &json)
}

/// True if there is a hot cache entry. Used by tests + boot path.
pub fn settings_cache_populated() -> bool {
    SETTINGS_CACHE.lock().is_some()
}

/// Convenience accessor for the embedding model name. Falls back to the
/// baked-in default.
pub fn get_embedding_model() -> String {
    match get_settings_cached() {
        Ok(s) => s.embedding_model,
        Err(_) => "BAAI/bge-small-zh-v1.5".to_string(),
    }
}

pub fn get_chunk_size() -> usize {
    match get_settings_cached() {
        Ok(s) => s.chunk_size,
        Err(_) => 500,
    }
}

pub fn get_chunk_overlap() -> usize {
    match get_settings_cached() {
        Ok(s) => s.chunk_overlap,
        Err(_) => 50,
    }
}

pub fn get_web_search_settings() -> WebSearchSettings {
    match get_settings_cached() {
        Ok(s) => s.web_search,
        Err(_) => WebSearchSettings::default(),
    }
}
