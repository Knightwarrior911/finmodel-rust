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
use tauri::Manager as _;

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

/// Idempotently pause (resumably interrupt) a conversation's active run. The run
/// ends `RunInterrupted` (resumable via `agent_resume`), distinct from the
/// terminal `agent_cancel`. Returns true iff a matching active run was signalled.
#[tauri::command(rename_all = "snake_case")]
pub fn agent_pause(
    registry: tauri::State<'_, ActorRegistry>,
    conversation_id: String,
    run_id: String,
) -> AppResult<bool> {
    Ok(registry.pause(&conversation_id, &run_id))
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
        .call(move |db| {
            db.events_after(&run_id, sequence)
                .map_err(|e| e.to_string())
        })
        .await
        .map_err(AppError::Engine)?;
    Ok(serde_json::to_string(&events)?)
}

/// A conversation snapshot: the active root→leaf branch (messages + ordered
/// parts), the active run (if any), and its `last_sequence` for gap-closing.
#[tauri::command(rename_all = "snake_case")]
pub async fn get_run_snapshot(app: tauri::AppHandle, conversation_id: String) -> AppResult<String> {
    let handle = store(&app)?.handle.clone();
    let json = handle
        .call(move |db| -> Result<serde_json::Value, String> {
            let branch = db
                .branch_path(&conversation_id)
                .map_err(|e| e.to_string())?;
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
pub async fn agent_resume(
    app: tauri::AppHandle,
    registry: tauri::State<'_, ActorRegistry>,
    interrupted_run_id: String,
) -> AppResult<String> {
    use crate::commands::settings::read_settings;
    let handle = store(&app)?.handle.clone();
    let rid = interrupted_run_id.clone();
    let orig = handle
        .call(move |db| db.get_run(&rid).ok().flatten())
        .await
        .ok_or_else(|| AppError::Config("run not found".into()))?;
    let conversation_id = orig.conversation_id.clone();
    // Insert the linked resumed run (validates status == interrupted).
    let new_id = actor::resume_run(
        &handle,
        &conversation_id,
        &interrupted_run_id,
        orig.model.clone(),
    )
    .await
    .ok_or_else(|| AppError::Config("run is not resumable".into()))?;
    // Rebuild the launch context from the original turn so the resumed run is
    // actually driven (not left an orphan): user text, workspace, model, tools.
    let user_msg = match orig.user_message_id.clone() {
        Some(mid) => handle
            .call(move |db| {
                db.message_parts(&mid).ok().and_then(|parts| {
                    parts
                        .iter()
                        .find(|p| p.kind == "text")
                        .and_then(|p| {
                            serde_json::from_str::<serde_json::Value>(&p.payload_json).ok()
                        })
                        .and_then(|v| v.get("text").and_then(|t| t.as_str()).map(String::from))
                })
            })
            .await
            .unwrap_or_default(),
        None => String::new(),
    };
    let conv_for_ws = conversation_id.clone();
    let ws = handle
        .call(move |db| db.conversation_workspace(&conv_for_ws).ok().flatten())
        .await;
    let workspace = ws.unwrap_or_else(|| {
        store(&app)
            .map(|s| s.default_workspace_id.clone())
            .unwrap_or_default()
    });
    let settings = read_settings(&app);
    let model = orig
        .model
        .clone()
        .unwrap_or_else(|| settings.model.trim().to_string());
    let tools_ok = settings
        .model_capability
        .as_ref()
        .map(|c| c.model_id == model && c.native_tools)
        .unwrap_or(false);
    let mode = crate::agent::modes::AgentMode::parse(orig.policy.as_deref());
    let _ = launch_run(LaunchSpec {
        app: app.clone(),
        registry: (*registry).clone(),
        handle: handle.clone(),
        conversation_id,
        run_id: new_id.clone(),
        workspace_id: workspace,
        user_msg,
        attachments: Vec::new(),
        api_key: settings.openrouter_api_key.trim().to_string(),
        model,
        tools_ok,
        policy: mode.policy(),
        mode,
    })
    .await?;
    Ok(new_id)
}

fn new_id() -> String {
    let mut bytes = [0u8; 16];
    rand::Rng::fill(&mut rand::thread_rng(), &mut bytes);
    fm_agent::ids::format_uuid_v4(bytes)
}

/// Everything the shared launch path needs to register + drive a run.
struct LaunchSpec {
    app: tauri::AppHandle,
    registry: ActorRegistry,
    handle: crate::store::StoreHandle,
    conversation_id: String,
    run_id: String,
    workspace_id: String,
    user_msg: String,
    /// (artifact_id, owner_scope) pairs staged by the composer for THIS turn.
    attachments: Vec<(String, String)>,
    api_key: String,
    model: String,
    tools_ok: bool,
    policy: fm_agent::budget::Policy,
    mode: crate::agent::modes::AgentMode,
}

/// Shared launch path for `agent_send` and `agent_resume`: register the run with
/// the [`ActorRegistry`], build the driver/context/machine, and spawn `run_turn`
/// on a RunHandle-guarded task. On registration failure the inserted run row is
/// marked failed so boot repair never sees an orphan stuck in running/preparing.
///
/// Returns an optional `model_note` for the UI when the turn was auto-routed
/// to a vision-capable model (`{ using, using_id, usual }`).
async fn launch_run(spec: LaunchSpec) -> AppResult<Option<serde_json::Value>> {
    use crate::agent::driver::{CostGuard, LiveDriver};
    use crate::agent::events::TauriEventSink;
    use crate::agent::executors::SessionContext;
    use fm_agent::machine::AgentMachine;
    use fm_agent::types::Confidentiality;

    let LaunchSpec {
        attachments,
        app,
        registry,
        handle,
        conversation_id,
        run_id,
        workspace_id,
        user_msg,
        api_key,
        mut model,
        tools_ok,
        policy,
        mode,
    } = spec;

    // ── Pre-flight: vision auto-routing + spend guard ──────────────────
    // Decided BEFORE the run registers and BEFORE attachment staging is
    // consumed, so a refused send leaves the user's chips resendable and
    // never reaches a provider (zero cost).
    let mut model_note: Option<serde_json::Value> = None;
    let mut guard = CostGuard::default();
    {
        let s = crate::commands::settings::read_settings(&app);
        guard.budget_usd = s.conversation_budget_usd;
        let openrouter =
            crate::commands::settings::is_openrouter(&s) && !api_key.trim().is_empty();
        let has_images = app
            .try_state::<crate::commands::artifacts::ArtifactRegistry>()
            .map(|reg| crate::commands::attachments::has_image_attachments(&reg, &attachments))
            .unwrap_or(false);
        let route_wanted = openrouter && has_images && s.auto_route_vision;
        // One cached catalog serves both the router and the price snapshot.
        let catalog = if route_wanted || (openrouter && guard.budget_usd > 0.0) {
            let key = api_key.clone();
            tauri::async_runtime::spawn_blocking(move || {
                crate::commands::settings::cached_openrouter_catalog(&key)
            })
            .await
            .ok()
            .and_then(|r| r.ok())
        } else {
            None
        };
        if route_wanted {
            if let Some(cat) = &catalog {
                use crate::agent::model_router::{route_for_vision, VisionRoute};
                match route_for_vision(cat, &model, s.route_price_cap_usd) {
                    VisionRoute::Route(id, name, _) => {
                        model_note = Some(serde_json::json!({
                            "using": name,
                            "using_id": id,
                            "usual": model,
                        }));
                        model = id;
                    }
                    VisionRoute::NoneAffordable => {
                        let rid = run_id.clone();
                        let now = crate::store::now_iso();
                        handle
                            .call(move |db| {
                                let _ = db.finish_run(
                                    &rid,
                                    "failed",
                                    "failed",
                                    Some("vision_unroutable"),
                                    None,
                                    &now,
                                );
                            })
                            .await;
                        return Err(AppError::Config(format!(
                            "This message has a picture, but your current model can't see images — and no image-capable model fits under your ${:.2} limit (per million tokens it writes). Raise the limit in Settings → Spending, turn off automatic switching, or pick an image-capable model from the model list.",
                            s.route_price_cap_usd
                        )));
                    }
                    // Already sees, or unknown to the catalog (custom model —
                    // never switched away from): leave the turn untouched.
                    VisionRoute::KeepCurrent | VisionRoute::CurrentUnknown => {}
                }
            }
            // Catalog unavailable (offline / provider hiccup): proceed with
            // the user's model — the provider's own error stays visible and
            // costs nothing extra; routing silently guessing would be worse.
        }
        if let Some(cat) = &catalog {
            if let Some(m) = cat.iter().find(|m| m.id == model) {
                guard.price_in_per_mtok = m.prompt_per_mtok();
                guard.price_out_per_mtok = m.completion_per_mtok();
            }
        }
        if guard.budget_usd > 0.0 {
            let conv = conversation_id.clone();
            guard.prior_spend_usd = handle
                .call(move |db| db.conversation_spend_usd(&conv).unwrap_or(0.0))
                .await;
        }
    }

    let run_handle = match registry.start_run(&conversation_id, &run_id) {
        Ok(h) => h,
        Err(e) => {
            // Do not leave an orphan in running/preparing: mark the row failed.
            let rid = run_id.clone();
            let now = crate::store::now_iso();
            handle
                .call(move |db| {
                    let _ =
                        db.finish_run(&rid, "failed", "failed", Some("start_failed"), None, &now);
                })
                .await;
            return Err(AppError::Config(e.to_string()));
        }
    };
    let cancel = run_handle.cancellation_token();
    let interrupt = run_handle.interrupt_token();
    let cfg = fm_extract::LlmConfig { api_key, model };
    // Attachments: extract text into the seed message, collect vision inputs,
    // and move PDF handles to the live conversation for analyze_pdf.
    let (user_msg, images) = if attachments.is_empty() {
        (user_msg, Vec::new())
    } else if let Some(reg) = app.try_state::<crate::commands::artifacts::ArtifactRegistry>() {
        let (blocks, images) = crate::commands::attachments::build_attachment_context(
            &reg,
            &conversation_id,
            &attachments,
        );
        let msg = if blocks.is_empty() {
            user_msg
        } else {
            format!("{user_msg}

{}", blocks.join("

"))
        };
        (msg, images)
    } else {
        (user_msg, Vec::new())
    };
    let ctx = SessionContext {
        workspace_id,
        conversation_id: conversation_id.clone(),
        run_id: run_id.clone(),
        user_msg,
        images,
        confidentiality: Confidentiality::Standard,
        cancel,
        interrupt,
    };
    let driver = LiveDriver::new(
        app.clone(),
        handle.clone(),
        cfg,
        ctx,
        tools_ok,
        registry.clone(),
        mode,
    )
    .with_cost_guard(guard);
    let sink = TauriEventSink::new(app.clone());
    let machine = AgentMachine::new(policy);
    let handle_bg = handle.clone();
    tauri::async_runtime::spawn(async move {
        // Keep the RunHandle alive for the turn so cancel/pause + RAII deregister work.
        let _guard = run_handle;
        let _outcome = actor::run_turn(
            &handle_bg,
            &sink,
            &conversation_id,
            &run_id,
            machine,
            driver,
        )
        .await;
    });
    Ok(model_note)
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
    project_id: Option<String>,
    text: String,
    attachments: Option<Vec<serde_json::Value>>,
    mode: Option<String>,
) -> AppResult<String> {
    use serde_json::json;
    // A follow-up promise in the user's text becomes a PROPOSED schedule —
    // surfaced to the UI for explicit approval, never scheduled silently
    // (Task 8.2 precision doctrine).
    let commitment = crate::agent::commitments::extract_commitment(&text);
    let text_probe = text.clone();
    let att_pairs: Vec<(String, String)> = attachments
        .unwrap_or_default()
        .iter()
        .filter_map(|a| {
            let id = a.get("artifact_id").and_then(|v| v.as_str())?;
            let scope = a.get("scope").and_then(|v| v.as_str())?;
            Some((id.to_string(), scope.to_string()))
        })
        .collect();
    let (conv_id, run_id, model_note) = send_message_inner_mode(
        app,
        (*registry).clone(),
        conversation_id,
        workspace_id,
        project_id,
        text,
        att_pairs,
        crate::agent::modes::AgentMode::parse(mode.as_deref()),
    )
    .await?;
    let mut out = json!({ "conversation_id": conv_id, "run_id": run_id });
    if let Some(note) = model_note {
        // The composer shows a quiet "reading this with X, back to Y after"
        // line — the switch is per-message, never persisted.
        out["model_note"] = note;
    }
    if let Some(c) = commitment {
        out["commitment"] = json!({ "text": c.text, "due": c.due_semantics });
    } else if crate::agent::memory::is_durable_preference(&text_probe) {
        // A standing preference ("always show figures in USD millions")
        // becomes a PROPOSAL — remembered only on the user's explicit yes.
        // (The measured unattended classifier sat below the 98% precision
        // gate; approval-gating sidesteps the gate entirely.)
        out["memory_candidate"] = json!({ "text": text_probe });
    }
    Ok(out.to_string())
}

/// The shared send path: creates the conversation/message/run rows and spawns
/// the run. Used by the `agent_send` command and the schedule tick (which has
/// no `State` wrapper).
pub(crate) async fn send_message_inner(
    app: tauri::AppHandle,
    registry: ActorRegistry,
    conversation_id: Option<String>,
    workspace_id: Option<String>,
    project_id: Option<String>,
    text: String,
    attachments: Vec<(String, String)>,
) -> AppResult<(String, String, Option<serde_json::Value>)> {
    send_message_inner_mode(
        app,
        registry,
        conversation_id,
        workspace_id,
        project_id,
        text,
        attachments,
        crate::agent::modes::AgentMode::Analyst,
    )
    .await
}

/// The shared send path with the working mode (composer mode chip).
#[allow(clippy::too_many_arguments)]
pub(crate) async fn send_message_inner_mode(
    app: tauri::AppHandle,
    registry: ActorRegistry,
    conversation_id: Option<String>,
    workspace_id: Option<String>,
    project_id: Option<String>,
    text: String,
    attachments: Vec<(String, String)>,
    mode: crate::agent::modes::AgentMode,
) -> AppResult<(String, String, Option<serde_json::Value>)> {
    use crate::commands::settings::read_settings;
    use serde_json::json;

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

    // Assign a project folder only when this call actually creates the
    // conversation row (see `created` below), so a later send in an existing
    // chat never silently re-folders it. The client pre-allocates the id, so we
    // can't rely on the param being absent.
    let project_for_insert = project_id.filter(|s| !s.trim().is_empty());
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
            // Create the conversation; `created` is true only for a fresh row
            // (UNIQUE failure → already exists). Assign the project folder only
            // then, when a project was requested.
            let created = db
                .create_conversation(&conv_for_insert, &workspace_for_insert, &title, &now)
                .is_ok();
            if created {
                if let Some(pid) = project_for_insert.as_deref() {
                    let _ = db.set_conversation_project(&conv_for_insert, Some(pid), &now);
                }
            }
            // Link the user turn under the current active leaf so multi-turn
            // conversations keep their whole root->leaf branch (history).
            let parent = db
                .active_leaf_id(&conv_for_insert)
                .map_err(|e| e.to_string())?;
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
                Some(mode.name()),
                &now,
            )
            .map_err(|e| e.to_string())?;
            Ok(())
        })
        .await
        .map_err(AppError::Engine)?;

    let tools_ok = settings
        .model_capability
        .as_ref()
        .map(|c| c.model_id == model && c.native_tools)
        .unwrap_or(false);

    let model_note = launch_run(LaunchSpec {
        app: app.clone(),
        registry,
        handle: handle.clone(),
        conversation_id: conv_id.clone(),
        run_id: run_id.clone(),
        workspace_id: workspace,
        user_msg: text,
        attachments,
        api_key: settings.openrouter_api_key.trim().to_string(),
        model,
        tools_ok,
        // Outcome modes (goal/loop) run under the WORKFLOW guard rails;
        // everything else keeps the interactive ceiling.
        policy: mode.policy(),
        mode,
    })
    .await?;

    Ok((conv_id, run_id, model_note))
}

/// ── Scheduled follow-through (Tasks 8.2/8.3, now LIVE) ────────────────────

/// ISO timestamp `secs` seconds from now (UTC).
fn iso_in_secs(secs: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    crate::store::iso_from_epoch(now + secs)
}

/// Map coarse due semantics from commitment extraction to a concrete due time.
/// Deliberately approximate for event-anchored phrases (a calendar service can
/// tighten these later); the UI labels them honestly.
pub(crate) fn due_semantics_to_secs(due: Option<&str>) -> i64 {
    const DAY: i64 = 86_400;
    match due {
        Some("tomorrow") => DAY,
        Some("next_week") => 7 * DAY,
        Some("next_quarter") => 90 * DAY,
        Some("after_next_earnings") => 35 * DAY,
        _ => 7 * DAY,
    }
}

/// Create a user-approved schedule. `due` carries the commitment's coarse
/// semantics; `recurrence` is `none | daily | weekly`.
#[tauri::command(rename_all = "snake_case")]
pub async fn schedule_create(
    app: tauri::AppHandle,
    conversation_id: Option<String>,
    prompt: String,
    due: Option<String>,
    recurrence: Option<String>,
) -> AppResult<String> {
    let prompt = prompt.trim().to_string();
    if prompt.is_empty() {
        return Err(AppError::Config("empty schedule prompt".into()));
    }
    let rec = recurrence
        .filter(|r| r == "daily" || r == "weekly")
        .map(|r| r.to_string());
    let next_due = iso_in_secs(due_semantics_to_secs(due.as_deref()));
    let id = new_id();
    let handle = store(&app)?.handle.clone();
    let sid = id.clone();
    let conv = conversation_id.filter(|c| !c.trim().is_empty());
    let nd = next_due.clone();
    let scope = serde_json::json!({ "prompt": prompt }).to_string();
    handle
        .call(move |db| -> Result<(), String> {
            db.insert_schedule(
                &sid,
                None,
                conv.as_deref(),
                "UTC",
                rec.as_deref(),
                &nd,
                &scope,
                None,
                None,
                &crate::store::now_iso(),
            )
            .map_err(|e| e.to_string())
        })
        .await
        .map_err(AppError::Engine)?;
    Ok(serde_json::json!({ "id": id, "next_due": next_due }).to_string())
}

/// Save a user-approved memory (the chat's "Remember it" chip). Scoped to
/// the workspace; kind `preference`; confidence 1.0 — the user said yes.
#[tauri::command(rename_all = "snake_case")]
pub async fn memory_add(
    app: tauri::AppHandle,
    content: String,
    workspace_id: Option<String>,
) -> AppResult<String> {
    let content = content.trim().to_string();
    if content.is_empty() {
        return Err(AppError::Config("empty memory".into()));
    }
    let app_store = store(&app)?;
    let handle = app_store.handle.clone();
    let ws = workspace_id.unwrap_or_else(|| app_store.default_workspace_id.clone());
    let public_id = new_id();
    let key = content.to_lowercase();
    let row_id = handle
        .call(move |db| {
            db.insert_memory(
                &public_id,
                "workspace",
                Some(&ws),
                None,
                "preference",
                &content,
                &key,
                0.7,
                1.0,
                "user_approved",
                None,
                &crate::store::now_iso(),
            )
            .map_err(|e| e.to_string())
        })
        .await
        .map_err(AppError::Engine)?;
    Ok(serde_json::json!({ "id": row_id }).to_string())
}

/// All non-cancelled schedules, soonest first.
#[tauri::command(rename_all = "snake_case")]
pub async fn schedules_list(app: tauri::AppHandle) -> AppResult<String> {
    let handle = store(&app)?.handle.clone();
    let rows = handle
        .call(|db| db.list_schedules().map_err(|e| e.to_string()))
        .await
        .map_err(AppError::Engine)?;
    Ok(serde_json::to_string(&rows).unwrap_or_else(|_| "[]".into()))
}

/// Cancel a schedule (user said stop).
#[tauri::command(rename_all = "snake_case")]
pub async fn schedule_cancel(app: tauri::AppHandle, id: String) -> AppResult<bool> {
    let handle = store(&app)?.handle.clone();
    handle
        .call(move |db| db.cancel_schedule(&id).map_err(|e| e.to_string()))
        .await
        .map_err(AppError::Engine)?;
    Ok(true)
}

/// One scheduler sweep over the store: claim every due schedule and hand its
/// prompt to `launch`. Recurring schedules re-arm for the next occurrence;
/// one-shots finish; a failed launch retries with a 15-minute backoff and is
/// TERMINAL after 5 attempts (never retries forever). The launcher is
/// injected so this whole decision path is tested against a real store
/// without a Tauri runtime.
pub(crate) async fn sweep_due_schedules<F, Fut>(handle: &crate::store::StoreHandle, launch: F)
where
    F: Fn(Option<String>, String) -> Fut,
    Fut: std::future::Future<Output = Result<(), String>>,
{
    loop {
        let now = crate::store::now_iso();
        let claimed = handle
            .call(move |db| db.claim_due_schedule(&now, "tick").map_err(|e| e.to_string()))
            .await;
        let Ok(Some(id)) = claimed else { break };
        let sid = id.clone();
        let row = handle
            .call(move |db| db.get_schedule(&sid).map_err(|e| e.to_string()))
            .await;
        let Ok(Some(row)) = row else {
            let sid = id.clone();
            let retry = iso_in_secs(15 * 60);
            let _ = handle
                .call(move |db| {
                    db.fail_schedule_attempt(&sid, 5, &retry).map_err(|e| e.to_string())
                })
                .await;
            continue;
        };
        let prompt = serde_json::from_str::<serde_json::Value>(&row.scope_json)
            .ok()
            .and_then(|v| v["prompt"].as_str().map(str::to_string))
            .unwrap_or_default();
        if prompt.is_empty() {
            let sid = id.clone();
            let _ = handle
                .call(move |db| {
                    db.finish_schedule(&sid, "empty_scope", &crate::store::now_iso())
                        .map_err(|e| e.to_string())
                })
                .await;
            continue;
        }
        let launched = launch(row.conversation_id.clone(), prompt).await;
        let sid = id.clone();
        match launched {
            Ok(()) => match row.recurrence.as_deref() {
                Some("daily") => {
                    let nd = iso_in_secs(86_400);
                    let _ = handle
                        .call(move |db| db.rearm_schedule(&sid, &nd).map_err(|e| e.to_string()))
                        .await;
                }
                Some("weekly") => {
                    let nd = iso_in_secs(7 * 86_400);
                    let _ = handle
                        .call(move |db| db.rearm_schedule(&sid, &nd).map_err(|e| e.to_string()))
                        .await;
                }
                _ => {
                    let _ = handle
                        .call(move |db| {
                            db.finish_schedule(&sid, "launched", &crate::store::now_iso())
                                .map_err(|e| e.to_string())
                        })
                        .await;
                }
            },
            Err(_) => {
                let retry = iso_in_secs(15 * 60);
                let _ = handle
                    .call(move |db| {
                        db.fail_schedule_attempt(&sid, 5, &retry).map_err(|e| e.to_string())
                    })
                    .await;
            }
        }
    }
}

/// The live tick: sweep with the real run launcher (called every 60s, lib.rs).
pub async fn run_due_schedules(app: &tauri::AppHandle) {
    let Ok(app_store) = store(app) else { return };
    let handle = app_store.handle.clone();
    let registry = {
        use tauri::Manager;
        app.state::<ActorRegistry>().inner().clone()
    };
    let app = app.clone();
    sweep_due_schedules(&handle, move |conv, prompt| {
        let app = app.clone();
        let registry = registry.clone();
        async move {
            send_message_inner(app, registry, conv, None, None, prompt, Vec::new())
                .await
                .map(|_| ())
                .map_err(|e| e.to_string())
        }
    })
    .await;
}

#[cfg(test)]
mod schedule_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    fn mem_handle() -> (crate::store::StoreHandle, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("fm-sched-{}", new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = crate::store::Db::open_in_memory(&dir.join("blobs")).unwrap();
        (crate::store::StoreHandle::spawn(db), dir)
    }

    fn insert(handle: &crate::store::StoreHandle, id: &str, recurrence: Option<&str>, prompt: &str) {
        let id = id.to_string();
        let rec = recurrence.map(str::to_string);
        let scope = serde_json::json!({ "prompt": prompt }).to_string();
        let fut = handle.call(move |db| {
            db.insert_schedule(
                &id, None, None, "UTC", rec.as_deref(),
                "2020-01-01T00:00:00Z", // long past due
                &scope, None, None, "2020-01-01T00:00:00Z",
            )
            .map_err(|e| e.to_string())
        });
        tauri::async_runtime::block_on(fut).unwrap();
    }

    fn state(handle: &crate::store::StoreHandle, id: &str) -> (String, i64, Option<String>) {
        let id = id.to_string();
        tauri::async_runtime::block_on(
            handle.call(move |db| db.schedule_state(&id).map_err(|e| e.to_string())),
        )
        .unwrap()
        .unwrap()
    }

    #[test]
    fn sweep_launches_oneshot_and_rearms_recurring() {
        let (handle, dir) = mem_handle();
        insert(&handle, "one", None, "check TSLA again");
        insert(&handle, "day", Some("daily"), "morning brief");
        let launched: Arc<std::sync::Mutex<Vec<String>>> = Arc::default();
        let l2 = launched.clone();
        tauri::async_runtime::block_on(sweep_due_schedules(&handle, move |_conv, prompt| {
            let l = l2.clone();
            async move {
                l.lock().unwrap().push(prompt);
                Ok(())
            }
        }));
        let mut got = launched.lock().unwrap().clone();
        got.sort();
        assert_eq!(got, vec!["check TSLA again".to_string(), "morning brief".to_string()]);
        // One-shot finished; recurring re-armed pending with a FUTURE due time
        // (a second immediate sweep launches nothing).
        assert_eq!(state(&handle, "one").0, "done");
        let (st, _, outcome) = state(&handle, "day");
        assert_eq!(st, "pending");
        assert_eq!(outcome.as_deref(), Some("launched"));
        let n = Arc::new(AtomicUsize::new(0));
        let n2 = n.clone();
        tauri::async_runtime::block_on(sweep_due_schedules(&handle, move |_c, _p| {
            let n = n2.clone();
            async move {
                n.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        }));
        assert_eq!(n.load(Ordering::SeqCst), 0, "re-armed schedule is not due yet");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn failing_launch_backs_off_and_terminates_after_five_attempts() {
        let (handle, dir) = mem_handle();
        insert(&handle, "bad", None, "doomed run");
        for attempt in 1..=5i64 {
            // Force the row due again despite the 15-minute backoff
            // (rearm resets status to pending with the given due time and
            // preserves the attempts counter — exactly the state a matured
            // backoff would reach).
            let fut = handle.call(|db| {
                db.rearm_schedule("bad", "2020-01-01T00:00:00Z").map_err(|e| e.to_string())
            });
            tauri::async_runtime::block_on(fut).unwrap();
            tauri::async_runtime::block_on(sweep_due_schedules(&handle, |_c, _p| async {
                Err("provider down".to_string())
            }));
            let (st, attempts, _) = state(&handle, "bad");
            assert_eq!(attempts, attempt);
            if attempt < 5 {
                assert_eq!(st, "pending", "retries with backoff");
            } else {
                assert_eq!(st, "failed", "TERMINAL after 5 attempts — never forever");
            }
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn due_semantics_map_is_total() {
        assert_eq!(due_semantics_to_secs(Some("tomorrow")), 86_400);
        assert_eq!(due_semantics_to_secs(Some("next_week")), 7 * 86_400);
        assert_eq!(due_semantics_to_secs(Some("garbage")), 7 * 86_400);
        assert_eq!(due_semantics_to_secs(None), 7 * 86_400);
    }
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

/// List saved memories for a workspace (defaults to the active one). Returns a
/// JSON array of `{id, kind, content, created_at}`, newest first.
#[tauri::command(rename_all = "snake_case")]
pub async fn memory_list(app: tauri::AppHandle, workspace_id: Option<String>) -> AppResult<String> {
    let app_store = store(&app)?;
    let handle = app_store.handle.clone();
    let ws = workspace_id.unwrap_or_else(|| app_store.default_workspace_id.clone());
    let rows = handle
        .call(move |db| {
            let mut out: Vec<serde_json::Value> = Vec::new();
            if let Ok(mut stmt) = db.conn().prepare(
                "SELECT id, kind, content, created_at, pinned FROM memories \
                 WHERE (workspace_id=?1 OR scope_type='global') AND valid_to IS NULL ORDER BY pinned DESC, created_at DESC LIMIT 200",
            ) {
                if let Ok(mapped) = stmt.query_map([&ws], |r| {
                    Ok(serde_json::json!({
                        "id": r.get::<_, i64>(0)?,
                        "kind": r.get::<_, String>(1)?,
                        "content": r.get::<_, String>(2)?,
                        "created_at": r.get::<_, String>(3)?,
                        "pinned": r.get::<_, i64>(4)? != 0,
                    }))
                }) {
                    out.extend(mapped.flatten());
                }
            }
            out
        })
        .await;
    serde_json::to_string(&rows).map_err(|e| AppError::Engine(e.to_string()))
}

/// Delete a saved memory by id (user-controlled forget).
#[tauri::command(rename_all = "snake_case")]
pub async fn memory_delete(app: tauri::AppHandle, id: i64) -> AppResult<String> {
    let handle = store(&app)?.handle.clone();
    let ok = handle
        .call(move |db| {
            use crate::store::memory::{MemoryRepository, SqliteMemoryRepository};
            SqliteMemoryRepository::new(db).delete(id).is_ok()
        })
        .await;
    Ok(serde_json::json!({ "ok": ok }).to_string())
}

/// Pin or unpin a saved memory (Task 7.2) — a pinned memory is protected from
/// automatic forgetting. Reversible via the same command.
#[tauri::command(rename_all = "snake_case")]
pub async fn memory_pin(app: tauri::AppHandle, id: i64, pinned: bool) -> AppResult<String> {
    let handle = store(&app)?.handle.clone();
    let ok = handle
        .call(move |db| db.set_memory_pinned(id, pinned).unwrap_or(false))
        .await;
    Ok(serde_json::json!({ "ok": ok }).to_string())
}

/// Edit a saved memory's text (Task 7.2 — user correction). Empty text is
/// rejected; use `memory_delete` to remove.
#[tauri::command(rename_all = "snake_case")]
pub async fn memory_edit(app: tauri::AppHandle, id: i64, value: String) -> AppResult<String> {
    let v = value.trim().to_string();
    if v.is_empty() {
        return Err(AppError::Config("memory text cannot be empty".into()));
    }
    let handle = store(&app)?.handle.clone();
    let now = crate::store::now_iso();
    let ok = handle
        .call(move |db| db.update_memory_value(id, &v, &now).unwrap_or(false))
        .await;
    Ok(serde_json::json!({ "ok": ok }).to_string())
}

// ── Grounding layers (global personalization + project workspace) ─────────

fn config_dir(app: &tauri::AppHandle) -> AppResult<std::path::PathBuf> {
    use tauri::Manager;
    app.path()
        .app_config_dir()
        .map_err(|e| AppError::Config(format!("no config dir: {e}")))
}

/// Read the global personalization block (`config.json`). Empty when unset.
#[tauri::command(rename_all = "snake_case")]
pub fn grounding_get_global(app: tauri::AppHandle) -> AppResult<String> {
    let dir = config_dir(&app)?;
    Ok(crate::agent::grounding::read_global(&dir).unwrap_or_default())
}

/// Persist the global personalization block to `config.json`.
#[tauri::command(rename_all = "snake_case")]
pub fn grounding_set_global(app: tauri::AppHandle, instructions: String) -> AppResult<()> {
    let dir = config_dir(&app)?;
    std::fs::create_dir_all(&dir).map_err(|e| AppError::Config(e.to_string()))?;
    let body = serde_json::json!({ "instructions": instructions.trim() });
    let text = serde_json::to_string_pretty(&body).map_err(|e| AppError::Engine(e.to_string()))?;
    std::fs::write(dir.join("config.json"), text).map_err(|e| AppError::Config(e.to_string()))?;
    Ok(())
}

/// Read a project's grounding (`projects/<id>/finmodel.md`).
#[tauri::command(rename_all = "snake_case")]
pub fn grounding_get_project(app: tauri::AppHandle, project_id: String) -> AppResult<String> {
    let dir = config_dir(&app)?;
    Ok(crate::agent::grounding::read_project(&dir, &project_id).unwrap_or_default())
}

/// Persist a project's grounding to `projects/<id>/finmodel.md`.
#[tauri::command(rename_all = "snake_case")]
pub fn grounding_set_project(
    app: tauri::AppHandle,
    project_id: String,
    instructions: String,
) -> AppResult<()> {
    let id = project_id.trim();
    if id.is_empty() {
        return Err(AppError::Config("project_id required".into()));
    }
    let dir = config_dir(&app)?;
    let path = crate::agent::grounding::project_file(&dir, id)
        .ok_or_else(|| AppError::Config("invalid project id".into()))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::Config(e.to_string()))?;
    }
    std::fs::write(&path, instructions.trim()).map_err(|e| AppError::Config(e.to_string()))?;
    Ok(())
}

// ── Projects (conversation folders) ───────────────────────────────────────

fn active_ws(app: &tauri::AppHandle) -> AppResult<String> {
    Ok(store(app)?.default_workspace_id.clone())
}

/// List projects (folders) in the active workspace as JSON `[{id,name}]`.
#[tauri::command(rename_all = "snake_case")]
pub async fn projects_list(app: tauri::AppHandle) -> AppResult<String> {
    let handle = store(&app)?.handle.clone();
    let ws = active_ws(&app)?;
    let rows = handle
        .call(move |db| db.list_projects(&ws))
        .await
        .map_err(|e| AppError::Engine(e.to_string()))?;
    let items: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|(id, name)| serde_json::json!({ "id": id, "name": name }))
        .collect();
    serde_json::to_string(&items).map_err(|e| AppError::Engine(e.to_string()))
}

/// Create a project folder in the active workspace. Returns `{id,name}`.
#[tauri::command(rename_all = "snake_case")]
pub async fn project_create(app: tauri::AppHandle, name: String) -> AppResult<String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err(AppError::Config("project name required".into()));
    }
    let handle = store(&app)?.handle.clone();
    let ws = active_ws(&app)?;
    let id = format!("proj_{:016x}{:016x}", fastrand::u64(..), fastrand::u64(..));
    let now = crate::store::now_iso();
    let (id2, name2) = (id.clone(), name.clone());
    handle
        .call(move |db| db.create_project(&id2, &ws, &name2, &now))
        .await
        .map_err(|e| AppError::Engine(e.to_string()))?;
    Ok(serde_json::json!({ "id": id, "name": name }).to_string())
}

/// Rename a project folder.
#[tauri::command(rename_all = "snake_case")]
pub async fn project_rename(app: tauri::AppHandle, id: String, name: String) -> AppResult<()> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err(AppError::Config("project name required".into()));
    }
    let handle = store(&app)?.handle.clone();
    handle
        .call(move |db| db.rename_project(&id, &name))
        .await
        .map_err(|e| AppError::Engine(e.to_string()))?;
    Ok(())
}

/// Delete a project folder; its conversations become loose (unassigned).
#[tauri::command(rename_all = "snake_case")]
pub async fn project_delete(app: tauri::AppHandle, id: String) -> AppResult<()> {
    let handle = store(&app)?.handle.clone();
    handle
        .call(move |db| db.delete_project(&id))
        .await
        .map_err(|e| AppError::Engine(e.to_string()))?;
    Ok(())
}

/// Assign a conversation to a project folder, or clear it with `project_id=null`.
#[tauri::command(rename_all = "snake_case")]
pub async fn conversation_set_project(
    app: tauri::AppHandle,
    conversation_id: String,
    project_id: Option<String>,
) -> AppResult<()> {
    let handle = store(&app)?.handle.clone();
    let now = crate::store::now_iso();
    handle
        .call(move |db| db.set_conversation_project(&conversation_id, project_id.as_deref(), &now))
        .await
        .map_err(|e| AppError::Engine(e.to_string()))?;
    Ok(())
}

// ── Skills (SKILL.md library) ─────────────────────────────────────────────

/// List skills as JSON `[{name, description, state, use_count, source_version}]`.
/// Lifecycle state (Task 7.2/7.3) is overlaid best-effort; a skill with no
/// lifecycle row (or an unavailable store) defaults to `active`.
#[tauri::command(rename_all = "snake_case")]
pub async fn skills_list(app: tauri::AppHandle) -> AppResult<String> {
    let dir = config_dir(&app)?;
    let skills = crate::agent::skills::list_skills(&dir);
    let states: std::collections::HashMap<String, (String, i64, i64)> = match store(&app) {
        Ok(st) => {
            let handle = st.handle.clone();
            let names: Vec<String> = skills.iter().map(|s| s.name.clone()).collect();
            handle
                .call(move |db| {
                    let mut m = std::collections::HashMap::new();
                    for n in names {
                        if let Ok(Some((state, uses, ver, _sup))) = db.skill_lifecycle_state(&n) {
                            m.insert(n, (state, uses, ver));
                        }
                    }
                    m
                })
                .await
        }
        Err(_) => std::collections::HashMap::new(),
    };
    let items: Vec<serde_json::Value> = skills
        .into_iter()
        .map(|s| {
            let (state, uses, ver) = states
                .get(&s.name)
                .cloned()
                .unwrap_or_else(|| ("active".to_string(), 0, 1));
            serde_json::json!({
                "name": s.name,
                "description": s.description,
                "state": state,
                "use_count": uses,
                "source_version": ver,
            })
        })
        .collect();
    serde_json::to_string(&items).map_err(|e| AppError::Engine(e.to_string()))
}

/// Read a skill's full SKILL.md content (for editing). Empty if absent.
#[tauri::command(rename_all = "snake_case")]
pub fn skills_get(app: tauri::AppHandle, name: String) -> AppResult<String> {
    if !crate::agent::skills::is_valid_name(&name) {
        return Err(AppError::Config("invalid skill name".into()));
    }
    let dir = config_dir(&app)?;
    let path = dir.join("skills").join(format!("{}.md", name.trim()));
    Ok(std::fs::read_to_string(path).unwrap_or_default())
}

/// Save a skill (`content` = full SKILL.md; frontmatter `name` must equal `name`).
/// A saved/edited skill is recorded `active` in the lifecycle store (Task 7.3),
/// best-effort so a store failure never blocks the save.
#[tauri::command(rename_all = "snake_case")]
pub async fn skills_save(app: tauri::AppHandle, name: String, content: String) -> AppResult<()> {
    let dir = config_dir(&app)?;
    crate::agent::skills::save_skill(&dir, &name, &content).map_err(AppError::Config)?;
    if let Ok(st) = store(&app) {
        let handle = st.handle.clone();
        let n = name.clone();
        let now = crate::store::now_iso();
        let _ = handle.call(move |db| db.upsert_skill(&n, 1, &now)).await;
    }
    Ok(())
}

/// Delete a skill by name.
#[tauri::command(rename_all = "snake_case")]
pub async fn agents_list(app: tauri::AppHandle) -> AppResult<String> {
    use tauri::Manager;
    let dir = app.path().app_config_dir().map_err(|e| AppError::Config(e.to_string()))?;
    let agents = crate::agent::agents::list_agents(&dir);
    serde_json::to_string(&agents).map_err(|e| AppError::Config(e.to_string()))
}

#[tauri::command]
pub fn agents_get(app: tauri::AppHandle, name: String) -> AppResult<String> {
    use tauri::Manager;
    let dir = app.path().app_config_dir().map_err(|e| AppError::Config(e.to_string()))?;
    crate::agent::agents::get_agent_md(&dir, &name)
        .ok_or_else(|| AppError::Config(format!("agent `{name}` not found")))
}

#[tauri::command]
pub async fn agents_save(app: tauri::AppHandle, name: String, content: String) -> AppResult<()> {
    use tauri::Manager;
    let dir = app.path().app_config_dir().map_err(|e| AppError::Config(e.to_string()))?;
    crate::agent::agents::save_agent(&dir, &name, &content)
        .map(|_| ())
        .map_err(AppError::Config)
}

#[tauri::command]
pub fn agents_delete(app: tauri::AppHandle, name: String) -> AppResult<()> {
    use tauri::Manager;
    let dir = app.path().app_config_dir().map_err(|e| AppError::Config(e.to_string()))?;
    crate::agent::agents::delete_agent(&dir, &name).map_err(AppError::Config)
}

#[tauri::command]
pub fn skills_delete(app: tauri::AppHandle, name: String) -> AppResult<()> {
    let dir = config_dir(&app)?;
    crate::agent::skills::delete_skill(&dir, &name).map_err(AppError::Config)?;
    Ok(())
}

/// Restore a stale/archived skill to `active` so it re-enters default context
/// (Task 7.2/7.3). Reversible: skills are never silently lost.
#[tauri::command(rename_all = "snake_case")]
pub async fn skill_restore(app: tauri::AppHandle, name: String) -> AppResult<()> {
    let handle = store(&app)?.handle.clone();
    let now = crate::store::now_iso();
    handle
        .call(move |db| db.restore_skill(&name, &now))
        .await
        .map_err(|e| AppError::Engine(e.to_string()))?;
    Ok(())
}

/// Self-evolution: ask the model to abstract a solved task (its `transcript`)
/// into a reusable SKILL.md draft. Returns the draft for the user to review and
/// save; the model generalizes the steps (not a static template).
#[tauri::command(rename_all = "snake_case")]
pub async fn skill_suggest(app: tauri::AppHandle, transcript: String) -> AppResult<String> {
    let transcript = transcript.trim().to_string();
    if transcript.is_empty() {
        return Err(AppError::Config("nothing to abstract into a skill".into()));
    }
    let settings = crate::commands::settings::read_settings(&app);
    let api_key = settings.openrouter_api_key.trim().to_string();
    if api_key.is_empty() {
        return Err(AppError::Config(
            "add an API key in Settings to generate a skill".into(),
        ));
    }
    let model = settings.model.trim().to_string();
    // Honor the configured provider (Settings.base_url), not a hardcoded endpoint.
    let chat_url = crate::commands::settings::chat_completions_url(&settings);
    const SYS: &str = "You turn a solved financial-analyst task into a reusable SKILL.md playbook. Output ONLY the SKILL.md, nothing else. Exact format: a line with `---`, then `name:` (kebab-case, <=40 chars), then `description:` (one line describing WHEN to use this), then a line with `---`, then generalized numbered steps. Generalize away specifics (tickers, years, company names) into instructions that name the app's tools where relevant (get_financials, benchmark_peers, research, read_filing, build_model, get_quote). Keep it under 15 lines.";
    let draft = tokio::task::spawn_blocking(move || {
        crate::commands::settings::complete_once(&api_key, &model, &chat_url, SYS, &transcript, 700)
    })
    .await
    .map_err(|e| AppError::Engine(e.to_string()))?
    .map_err(AppError::Engine)?;
    let d = draft.trim();
    let d = d
        .strip_prefix("```markdown")
        .or_else(|| d.strip_prefix("```md"))
        .or_else(|| d.strip_prefix("```"))
        .unwrap_or(d);
    let d = d.strip_suffix("```").unwrap_or(d);
    Ok(d.trim().to_string())
}
