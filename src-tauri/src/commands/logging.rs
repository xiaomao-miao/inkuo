//! Frontend diagnostic bridge.
//!
//! `frontend_log` is the IPC sink the injected init-script uses (see
//! `lib.rs`).  We deliberately route to a *file* under app_data_dir
//! rather than stdout because the release exe on Windows is built with
//! `windows_subsystem = "windows"` and has no console attached, and on
//! Linux `/dev/null` swallowing the output would defeat the whole point.
//!
//! Safe under concurrent webviews: writes are serialised behind a Mutex;
//! we never block the IPC thread for more than a single small write +
//! flush.

use std::io::Write;

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde::Deserialize;

static FRONTEND_LOG_FILE: Lazy<Mutex<Option<std::path::PathBuf>>> =
    Lazy::new(|| Mutex::new(None));

pub fn frontend_log_path() -> std::path::PathBuf {
    let mut p = crate::app_data_dir();
    let _ = std::fs::create_dir_all(&p);
    p.push("frontend-console.log");
    p
}

#[derive(Debug, Deserialize)]
pub struct FrontendLogPayload {
    pub level: String,
    pub message: String,
    /// Optional URL where the message originated (best-effort).
    #[serde(default)]
    pub url: Option<String>,
    /// Optional stack trace (for errors / rejections).
    #[serde(default)]
    pub stack: Option<String>,
}

#[tauri::command]
pub async fn frontend_log(payload: FrontendLogPayload) -> Result<(), String> {
    let path = {
        let mut guard = FRONTEND_LOG_FILE.lock();
        guard.get_or_insert_with(frontend_log_path).clone()
    };

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);

    let line = format!(
        "[{}] [{}] [{}] {}{}\n",
        ts,
        payload.level.to_uppercase(),
        payload
            .url
            .clone()
            .unwrap_or_else(|| "<unknown>".to_string()),
        payload.message,
        payload
            .stack
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(|s| format!("\n  stack: {}", s))
            .unwrap_or_default(),
    );

    // Mirror to tracing so the same line shows up in stdout/file log too.
    let trimmed = line.trim_end();
    match payload.level.as_str() {
        "error" => tracing::error!(target: "frontend", "{}", trimmed),
        "warn" => tracing::warn!(target: "frontend", "{}", trimmed),
        _ => tracing::info!(target: "frontend", "{}", trimmed),
    }

    // Append to the file.  Best-effort — never propagate IO failures back
    // into the frontend (would create an error log loop).
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = f.write_all(line.as_bytes());
        let _ = f.flush();
    }
    Ok(())
}
