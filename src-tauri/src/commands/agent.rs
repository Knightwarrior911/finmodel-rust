//! Tauri command surface for the unified agent loop (Phase B: control + query;
//! `agent_send` lands in Phase C with the real provider/tool driver).
//!
//! These commands are the race-free attach/reload contract: the UI registers the
//! event listener, loads a snapshot with `last_sequence`, then calls
//! [`get_run_events_after`] to close the snapshot/subscription gap. Cancellation
//! and resume are idempotent and route through the [`ActorRegistry`] (the sole
//! active-run authority) and the store actor.

use serde::Serialize;

use crate::agent::{actor, registry::ActorRegistry};
use crate::error::{AppError, AppResult};
use crate::store::AppStore;

fn store<'a>(app: &'a tauri::AppHandle) -> AppResult<tauri::State<'a, AppStore>> {
    use tauri::Manager;
    app.try_state::<AppStore>()
        .ok_or_else(|| AppError::Config("store not initialized".into()))
}

/// Idempotently cancel a specific conversation's active run. Returns true iff a
/// matching active run was found and signalled.
#[tauri::command(rename_all = "snake_case")]
pub fn agent_cancel(
    registry: tauri::State<'_, ActorRegistry>,
    conversation_id: String,
    run_id: String,
) -> AppResult<bool> {
    Ok(registry.cancel(&conversation_id, &run_id))
}

/// All active `(conversation_id, run_id)` pairs, as JSON.
#[tauri::command(rename_all = "snake_case")]
pub fn list_active_runs(registry: tauri::State<'_, ActorRegistry>) -> AppResult<String> {
    #[derive(Serialize)]
    struct ActiveRun {
        conversation_id: String,
        run_id: String,
    }
    let runs: Vec<ActiveRun> = registry
        .active_runs()
        .into_iter()
        .map(|(c, r)| ActiveRun {
            conversation_id: c,
            run_id: r,
        })
        .collect();
    Ok(serde_json::to_string(&runs)?)
}

/// Durable run events strictly after `sequence` — closes the snapshot/
/// subscription gap on attach/reload.
#[tauri::command(rename_all = "snake_case")]
pub async fn get_run_events_after(
    app: tauri::AppHandle,
    run_id: String,
    sequence: i64,
) -> AppResult<String> {
    let handle = store(&app)?.handle.clone();
    let events = handle
        .call(move |db| db.events_after(&run_id, sequence).map_err(|e| e.to_string()))
        .await
        .map_err(AppError::Engine)?;
    Ok(serde_json::to_string(&events)?)
}

/// A conversation snapshot: the active root→leaf branch (messages + ordered
/// parts), the active run (if any), and its `last_sequence` for gap-closing.
#[tauri::command(rename_all = "snake_case")]
pub async fn get_run_snapshot(
    app: tauri::AppHandle,
    conversation_id: String,
) -> AppResult<String> {
    let handle = store(&app)?.handle.clone();
    let json = handle
        .call(move |db| -> Result<serde_json::Value, String> {
            let branch = db.branch_path(&conversation_id).map_err(|e| e.to_string())?;
            let mut messages = Vec::new();
            for m in &branch {
                let parts = db.message_parts(&m.id).map_err(|e| e.to_string())?;
                messages.push(serde_json::json!({ "message": m, "parts": parts }));
            }
            // The most recent run for this conversation (active or last terminal).
            let run = db
                .latest_run_for_conversation(&conversation_id)
                .map_err(|e| e.to_string())?;
            let last_sequence = run.as_ref().map(|r| r.last_sequence).unwrap_or(0);
            Ok(serde_json::json!({
                "conversation_id": conversation_id,
                "messages": messages,
                "run": run,
                "last_sequence": last_sequence,
            }))
        })
        .await
        .map_err(AppError::Engine)?;
    Ok(serde_json::to_string(&json)?)
}

/// Resume an interrupted run: creates and returns a NEW run id linked by
/// `resumed_from_run_id`, seeded from the last complete boundary. Never reuses a
/// partially executed side effect; never reopens a terminal run.
#[tauri::command(rename_all = "snake_case")]
pub async fn agent_resume(app: tauri::AppHandle, interrupted_run_id: String) -> AppResult<String> {
    let handle = store(&app)?.handle.clone();
    // Resolve the conversation from the interrupted run.
    let rid = interrupted_run_id.clone();
    let conversation_id = handle
        .call(move |db| db.get_run(&rid).map(|o| o.map(|r| r.conversation_id)))
        .await
        .map_err(|e| AppError::Engine(e.to_string()))?
        .ok_or_else(|| AppError::Config("run not found".into()))?;
    let new_id = actor::resume_run(&handle, &conversation_id, &interrupted_run_id, None)
        .await
        .ok_or_else(|| AppError::Config("run is not resumable".into()))?;
    Ok(new_id)
}
