//! Tauri commands for the cloud-mode UI flow.
//!
//! These are thin wrappers around `cloud::CloudClient` that the React
//! frontend invokes. The frontend persists the resulting `CloudAccount`
//! into `Settings.cloud.account` so the Rust-side config-builders can
//! route chat traffic to the cloud when `Settings.cloud.cloud_mode_enabled`
//! is `true`.

use crate::cloud::{CloudAccount, CloudAccountInfo, CloudClient, CloudError, CloudModelEntry};
use crate::commands::{AppCommandError, Settings};
use std::path::PathBuf;
use tauri::State;

fn map_err(e: CloudError) -> AppCommandError {
    AppCommandError::AIConfig(format!("cloud: {}", e))
}

#[tauri::command]
pub async fn cloud_register(
    base_url: String,
    invite_code: String,
    email: String,
    password: String,
    cloud: State<'_, CloudClient>,
) -> Result<CloudAccount, AppCommandError> {
    cloud
        .register(&base_url, &invite_code, &email, &password)
        .await
        .map_err(map_err)
}

#[tauri::command]
pub async fn cloud_login(
    base_url: String,
    email: String,
    password: String,
    cloud: State<'_, CloudClient>,
) -> Result<CloudAccount, AppCommandError> {
    cloud
        .login(&base_url, &email, &password)
        .await
        .map_err(map_err)
}

#[tauri::command]
pub async fn cloud_logout(cloud: State<'_, CloudClient>) -> Result<(), AppCommandError> {
    cloud.logout().await;
    Ok(())
}

#[tauri::command]
pub async fn cloud_fetch_models(
    cloud: State<'_, CloudClient>,
) -> Result<Vec<CloudModelEntry>, AppCommandError> {
    cloud.fetch_models().await.map_err(map_err)
}

#[tauri::command]
pub async fn cloud_fetch_account(
    cloud: State<'_, CloudClient>,
) -> Result<CloudAccountInfo, AppCommandError> {
    cloud.fetch_account().await.map_err(map_err)
}

/// Persist the current in-memory `CloudAccount` back into `Settings.cloud.account`
/// on disk. The frontend calls this after a successful register/login and after
/// any token refresh so the Rust side always reads fresh credentials from disk.
#[tauri::command]
pub async fn cloud_persist_account(
    settings: Settings,
    cloud: State<'_, CloudClient>,
) -> Result<(), AppCommandError> {
    let account = cloud.current().await;
    let mut updated = settings;
    updated.cloud.account = account;
    write_settings(&updated).map_err(|e| AppCommandError::AIConfig(e))
}

/// Atomic, fsync'd write of the settings file. Delegates to the shared helper
/// in `commands` so the document write path, the `save_settings` command, and
/// the cloud persist path all use the same write-temp-then-rename pattern.
fn write_settings(settings: &Settings) -> Result<(), String> {
    let path: PathBuf = crate::commands::get_settings_path();
    let content = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    crate::commands::atomic_write_settings(&path, &content).map_err(|e| e.to_string())
}