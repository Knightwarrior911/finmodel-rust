//! Settings + OpenRouter model listing commands.
//!
//! Settings (API key, chosen model) persist to a JSON file in the app config dir.
//! The raw key is never sent to the frontend — only a `has_key` boolean.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::Manager;

use crate::error::{AppError, AppResult};

fn default_model() -> String {
    "anthropic/claude-sonnet-4".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Settings {
    #[serde(default)]
    pub openrouter_api_key: String,
    #[serde(default = "default_model")]
    pub model: String,
}

fn settings_path(app: &tauri::AppHandle) -> AppResult<PathBuf> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| AppError::Config(format!("no config dir: {e}")))?;
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("settings.json"))
}

pub fn read_settings(app: &tauri::AppHandle) -> Settings {
    match settings_path(app) {
        Ok(p) if p.exists() => std::fs::read_to_string(&p)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default(),
        _ => Settings {
            openrouter_api_key: String::new(),
            model: default_model(),
        },
    }
}

/// Return `{ has_key, model }` — never the raw key.
#[tauri::command(rename_all = "snake_case")]
pub fn load_settings(app: tauri::AppHandle) -> AppResult<String> {
    let s = read_settings(&app);
    Ok(serde_json::json!({
        "has_key": !s.openrouter_api_key.trim().is_empty(),
        "model": s.model,
    })
    .to_string())
}

/// Save settings. A blank `api_key` keeps the existing one (so the frontend can
/// send blank to change only the model). A blank `model` keeps the existing one.
#[tauri::command(rename_all = "snake_case")]
pub fn save_settings(app: tauri::AppHandle, api_key: String, model: String) -> AppResult<String> {
    let mut s = read_settings(&app);
    if !api_key.trim().is_empty() {
        s.openrouter_api_key = api_key.trim().to_string();
    }
    if !model.trim().is_empty() {
        s.model = model.trim().to_string();
    }
    let p = settings_path(&app)?;
    std::fs::write(&p, serde_json::to_string_pretty(&s)?)
        .map_err(|e| AppError::Io(e.to_string()))?;
    Ok(serde_json::json!({ "ok": true }).to_string())
}

/// Fetch the live OpenRouter model catalog using the saved key.
/// Returns a JSON array of `{ id, name, context_length, pricing }`.
#[tauri::command(rename_all = "snake_case")]
pub async fn list_models(app: tauri::AppHandle) -> AppResult<String> {
    // Network fetch — run off the IPC thread.
    tauri::async_runtime::spawn_blocking(move || {
        let s = read_settings(&app);
        if s.openrouter_api_key.trim().is_empty() {
            return Err(AppError::Config(
                "No OpenRouter API key set. Add one in Settings first.".into(),
            ));
        }
        let models = fm_extract::list_openrouter_models(s.openrouter_api_key.trim())
            .map_err(|e| AppError::Engine(format!("OpenRouter model fetch failed: {e}")))?;
        serde_json::to_string(&models).map_err(|e| AppError::Engine(e.to_string()))
    })
    .await
    .map_err(|e| AppError::Engine(format!("model fetch task failed: {e}")))?
}
