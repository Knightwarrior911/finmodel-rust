//! Auto-update commands (desktop).
//!
//! Checks the GitHub Releases `latest.json` endpoint configured in
//! `tauri.conf.json` (`plugins.updater.endpoints`) for a newer build, verifies
//! its signature against the minisign `pubkey`, and installs it on request.
//! The private signing key lives outside the repo and is supplied at build time
//! via `TAURI_SIGNING_PRIVATE_KEY` — see `docs/RELEASE_CHECKLIST.md`.

use tauri_plugin_updater::UpdaterExt;

use crate::error::{AppError, AppResult};

/// Check for an available update. Returns a JSON summary the UI renders:
/// `{ available, version?, current, notes? }`. A missing release / offline
/// endpoint surfaces as an error so the caller can decide how loud to be.
#[tauri::command(rename_all = "snake_case")]
pub async fn check_for_update(app: tauri::AppHandle) -> AppResult<String> {
    let current = app.package_info().version.to_string();
    let updater = app
        .updater()
        .map_err(|e| AppError::Engine(format!("updater unavailable: {e}")))?;
    match updater.check().await {
        Ok(Some(update)) => Ok(serde_json::json!({
            "available": true,
            "version": update.version,
            "current": current,
            "notes": update.body,
        })
        .to_string()),
        Ok(None) => Ok(serde_json::json!({
            "available": false,
            "current": current,
        })
        .to_string()),
        Err(e) => Err(AppError::Engine(format!("update check failed: {e}"))),
    }
}

/// Download and install the pending update. Re-checks so it is stateless.
/// Returns `Ok("installed")` once the install completes — the relaunch is a
/// separate [`restart_app`] call the frontend makes AFTER it has rendered
/// "Restarting…", so the UI never hangs on "Downloading…" and there is no
/// response/relaunch race.
#[tauri::command(rename_all = "snake_case")]
pub async fn install_update(app: tauri::AppHandle) -> AppResult<String> {
    let updater = app
        .updater()
        .map_err(|e| AppError::Engine(format!("updater unavailable: {e}")))?;
    let update = updater
        .check()
        .await
        .map_err(|e| AppError::Engine(format!("update check failed: {e}")))?
        .ok_or_else(|| AppError::Engine("no update available to install".into()))?;
    update
        .download_and_install(|_chunk, _total| {}, || {})
        .await
        .map_err(|e| AppError::Engine(format!("update install failed: {e}")))?;
    Ok("installed".into())
}

/// Relaunch the app (terminal — never returns). Called by the frontend after a
/// successful [`install_update`] once it has shown the "Restarting…" state.
#[tauri::command(rename_all = "snake_case")]
pub fn restart_app(app: tauri::AppHandle) -> AppResult<String> {
    app.restart();
}
