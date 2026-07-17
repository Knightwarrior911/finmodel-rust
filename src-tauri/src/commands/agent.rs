//! Tauri command surface for the unified agent loop (Phase B: control + query;
//! `agent_send` spawns a LiveDriver turn (approval fail-closed; memory capture deferred).
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

fn new_id() -> String {
    let mut bytes = [0u8; 16];
    rand::Rng::fill(&mut rand::thread_rng(), &mut bytes);
    fm_agent::ids::format_uuid_v4(bytes)
}

/// Start a unified agent turn. Creates (or appends to) a SQLite conversation,
/// registers the run with [`ActorRegistry`], and drives [`LiveDriver`] to a
/// terminal on a background task. Returns `{ conversation_id, run_id }`.
///
/// First-pass notes:
/// - Approval is fail-closed (Deny) until `agent_approve` parks a oneshot.
/// - Memory auto-capture is off (returns 0) until Phase E quality gates.
/// - Legacy JSON `chat_send` remains the UI default until Phase G cutover.
#[tauri::command(rename_all = "snake_case")]
pub async fn agent_send(
    app: tauri::AppHandle,
    registry: tauri::State<'_, ActorRegistry>,
    conversation_id: Option<String>,
    workspace_id: Option<String>,
    text: String,
) -> AppResult<String> {
    use crate::agent::driver::LiveDriver;
    use crate::agent::events::TauriEventSink;
    use crate::agent::executors::SessionContext;
    use crate::commands::settings::read_settings;
    use fm_agent::budget::Policy;
    use fm_agent::machine::AgentMachine;
    use fm_agent::types::Confidentiality;
    use serde_json::json;
    use tokio_util::sync::CancellationToken;

    let text = text.trim().to_string();
    if text.is_empty() {
        return Err(AppError::Config("empty message".into()));
    }

    // No key is allowed: LiveDriver routes to the FallbackDispatcher.
    let settings = read_settings(&app);

    let app_store = store(&app)?;
    let handle = app_store.handle.clone();
    let workspace = workspace_id
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| app_store.default_workspace_id.clone());

    let conv_id = conversation_id
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(new_id);
    let run_id = new_id();
    let user_msg_id = new_id();
    let user_part_id = new_id();
    let title: String = text.chars().take(60).collect();

    let conv_for_insert = conv_id.clone();
    let run_for_insert = run_id.clone();
    let model = settings.model.trim().to_string();
    let workspace_for_insert = workspace.clone();
    let text_for_insert = text.clone();
    let model_for_insert = model.clone();
    handle
        .call(move |db| -> Result<(), String> {
            let now = crate::store::now_iso();
            // Create conversation if this is a new id (UNIQUE failure → already exists).
            let _ = db.create_conversation(&conv_for_insert, &workspace_for_insert, &title, &now);
            // Link the user turn under the current active leaf so multi-turn
            // conversations keep their whole root->leaf branch (history).
            let parent = db.active_leaf_id(&conv_for_insert).map_err(|e| e.to_string())?;
            db.insert_message(
                &user_msg_id,
                &conv_for_insert,
                parent.as_deref(),
                "user",
                None,
                "complete",
                &now,
            )
            .map_err(|e| e.to_string())?;
            let payload = json!({ "text": text_for_insert }).to_string();
            db.insert_part(
                &user_part_id,
                &user_msg_id,
                0,
                "text",
                &payload,
                Some(&text_for_insert),
            )
            .map_err(|e| e.to_string())?;
            db.set_active_leaf(&conv_for_insert, &user_msg_id, &now)
                .map_err(|e| e.to_string())?;
            db.insert_run(
                &run_for_insert,
                &conv_for_insert,
                Some(&user_msg_id),
                None,
                "running",
                "preparing",
                Some(&model_for_insert),
                None,
                &now,
            )
            .map_err(|e| e.to_string())?;
            Ok(())
        })
        .await
        .map_err(AppError::Engine)?;

    let run_handle = registry
        .start_run(&conv_id, &run_id)
        .map_err(|e| AppError::Config(e.to_string()))?;
    let cancel: CancellationToken = run_handle.cancellation_token();

    let cfg = fm_extract::LlmConfig {
        api_key: settings.openrouter_api_key.trim().to_string(),
        model: model.clone(),
    };
    let tools_ok = settings
        .model_capability
        .as_ref()
        .map(|c| c.model_id == model && c.native_tools)
        .unwrap_or(false);

    let ctx = SessionContext {
        workspace_id: workspace,
        conversation_id: conv_id.clone(),
        run_id: run_id.clone(),
        user_msg: text,
        confidentiality: Confidentiality::Standard,
        cancel,
    };

    let driver = LiveDriver::new(app.clone(), handle.clone(), cfg, ctx, tools_ok, (*registry).clone());
    let sink = TauriEventSink::new(app.clone());
    let machine = AgentMachine::new(Policy::INTERACTIVE);
    let conv_bg = conv_id.clone();
    let run_bg = run_id.clone();
    let handle_bg = handle.clone();

    tauri::async_runtime::spawn(async move {
        // Keep the RunHandle alive for the turn so cancel + RAII deregister work.
        let _guard = run_handle;
        let _outcome =
            actor::run_turn(&handle_bg, &sink, &conv_bg, &run_bg, machine, driver).await;
    });

    Ok(json!({
        "conversation_id": conv_id,
        "run_id": run_id,
    })
    .to_string())
}

/// Resolve a parked approval for a run (Approve once / Deny / Create new
/// version). First answer wins; a missing/late waiter simply returns false.
#[tauri::command(rename_all = "snake_case")]
pub fn agent_approve(
    registry: tauri::State<'_, ActorRegistry>,
    run_id: String,
    interaction_id: Option<String>,
    response: String,
) -> AppResult<bool> {
    let _ = interaction_id; // durable ApprovalResolved carries the tool_call_id
    let resp = match response.as_str() {
        "approve_once" => fm_agent::types::ApprovalResponse::ApproveOnce,
        "create_new_version" => fm_agent::types::ApprovalResponse::CreateNewVersion,
        _ => fm_agent::types::ApprovalResponse::Deny,
    };
    Ok(registry.resolve_approval(&run_id, resp))
}
