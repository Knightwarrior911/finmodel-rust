//! The conversation turn driver: runs one [`fm_agent::AgentMachine`] to a
//! terminal, performing provider/tool/store I/O through a [`Driver`] and the
//! [`StoreHandle`], persisting each durable transition *before* broadcasting it.
//!
//! The reducer is pure; this module is the async side. Real provider/tool
//! execution arrives in Phase C as a concrete [`Driver`]; Phase B proves the
//! loop's durability/replay/resume/crash behavior with a fake driver and a real
//! (in-memory) store, exactly as the plan's fake-store acceptance requires.

use fm_agent::machine::{Action, AgentMachine, Input, ToolCall};
use fm_agent::types::{ApprovalResponse, EventKind, StopReason};

use crate::agent::events::{AgentEventEnvelope, EventSink};
use crate::store::{now_iso, StoreHandle};

/// What a provider request produced.
#[derive(Clone, Debug, Default)]
pub struct ModelOut {
    pub calls: Vec<ToolCall>,
    pub final_answer: bool,
    pub tokens: u64,
}

/// What [`Driver::prepare`] resolved.
#[derive(Clone, Copy, Debug)]
pub struct PreparedInfo {
    pub uses_tools: bool,
    pub plan_needed: bool,
    pub needs_verification: bool,
}

/// The I/O side of a turn. Each method corresponds to an [`Action`] the reducer
/// emits; implementations perform real provider/tool/store work (Phase C) or are
/// scripted fakes (tests). Native `async fn` in traits (stable since 1.75) —
/// used only with generics here, never as `dyn`.
#[allow(async_fn_in_trait)]
pub trait Driver {
    async fn prepare(&mut self) -> PreparedInfo;
    async fn make_plan(&mut self);
    async fn request_model(&mut self) -> ModelOut;
    /// Ask the model to repair one malformed call, then re-request. Returns the
    /// next model output.
    async fn repair_tool_call(&mut self, tool_call_id: &str) -> ModelOut;
    /// Execute a batch of tool calls; returns tokens consumed.
    async fn schedule_tools(&mut self, batch: &[String]) -> u64;
    async fn synthesize(&mut self);
    async fn verify(&mut self) -> bool;
    /// Run bounded memory extraction; returns whether any rows were saved (so
    /// the loop emits exactly one `MemoryUpdated` before the terminal event).
    async fn extract_memory(&mut self) -> bool;
    /// Block until the parked approval is resolved.
    async fn await_approval(&mut self, tool_call_id: &str) -> ApprovalResponse;
}

/// The terminal outcome of a turn plus the durable envelopes emitted (for tests
/// and live/replay equality checks).
#[derive(Debug)]
pub struct TurnOutcome {
    pub event: EventKind,
    pub stop: StopReason,
    pub partial: bool,
    pub durable_events: Vec<AgentEventEnvelope>,
}

fn kind_str(kind: EventKind) -> String {
    serde_json::to_value(kind)
        .ok()
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "unknown".into())
}

fn phase_str(phase: fm_agent::types::AgentPhase) -> String {
    serde_json::to_value(phase)
        .ok()
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "unknown".into())
}

/// Persist a durable event (monotonic sequence) then broadcast it. Persist-
/// before-broadcast makes the store authoritative; a dropped ephemeral delta can
/// never strand the UI, and replay reproduces this exact stream.
async fn emit_durable(
    store: &StoreHandle,
    sink: &dyn EventSink,
    conversation_id: &str,
    run_id: &str,
    kind: EventKind,
    payload: serde_json::Value,
) -> AgentEventEnvelope {
    let now = now_iso();
    let run = run_id.to_string();
    let ks = kind_str(kind);
    let payload_str = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".into());
    let event_id = new_uuid();
    let eid = event_id.clone();
    let now2 = now.clone();
    let seq = store
        .call(move |db| {
            db.append_event(&eid, &run, &ks, &payload_str, &now2)
                .unwrap_or(-1)
        })
        .await;
    let env = AgentEventEnvelope::durable(
        event_id,
        conversation_id.to_string(),
        run_id.to_string(),
        seq,
        kind,
        payload,
        now,
    );
    sink.emit(&env);
    env
}

fn new_uuid() -> String {
    let mut bytes = [0u8; 16];
    rand::Rng::fill(&mut rand::thread_rng(), &mut bytes);
    fm_agent::ids::format_uuid_v4(bytes)
}

/// Drive `machine` to a terminal, persisting+broadcasting durable events. The
/// caller has already created the `agent_runs` row (status `running`).
pub async fn run_turn<D: Driver>(
    store: &StoreHandle,
    sink: &dyn EventSink,
    conversation_id: &str,
    run_id: &str,
    mut machine: AgentMachine,
    mut driver: D,
) -> TurnOutcome {
    let mut events = Vec::new();

    events.push(
        emit_durable(
            store,
            sink,
            conversation_id,
            run_id,
            EventKind::RunStarted,
            serde_json::json!({}),
        )
        .await,
    );

    let mut prev_phase = machine.phase();
    let mut action = machine.start();

    loop {
        // Execute the current action; produce the next reducer input.
        let input: Input = match action {
            Action::Prepare => {
                let info = driver.prepare().await;
                Input::Prepared {
                    uses_tools: info.uses_tools,
                    plan_needed: info.plan_needed,
                    needs_verification: info.needs_verification,
                }
            }
            Action::MakePlan => {
                driver.make_plan().await;
                events.push(
                    emit_durable(
                        store,
                        sink,
                        conversation_id,
                        run_id,
                        EventKind::PlanUpdated,
                        serde_json::json!({}),
                    )
                    .await,
                );
                Input::PlanReady
            }
            Action::RequestModel => {
                let m = driver.request_model().await;
                Input::ModelResponded {
                    calls: m.calls,
                    final_answer: m.final_answer,
                    tokens: m.tokens,
                }
            }
            Action::RepairToolCall { tool_call_id } => {
                let m = driver.repair_tool_call(&tool_call_id).await;
                Input::ModelResponded {
                    calls: m.calls,
                    final_answer: m.final_answer,
                    tokens: m.tokens,
                }
            }
            Action::ScheduleTools { batch } => {
                for id in &batch {
                    events.push(
                        emit_durable(
                            store,
                            sink,
                            conversation_id,
                            run_id,
                            EventKind::ToolStarted,
                            serde_json::json!({ "tool_call_id": id }),
                        )
                        .await,
                    );
                }
                let tokens = driver.schedule_tools(&batch).await;
                for id in &batch {
                    events.push(
                        emit_durable(
                            store,
                            sink,
                            conversation_id,
                            run_id,
                            EventKind::ToolSucceeded,
                            serde_json::json!({ "tool_call_id": id }),
                        )
                        .await,
                    );
                }
                Input::ToolsCompleted { tokens }
            }
            Action::RequestApproval { tool_call_id } => {
                events.push(
                    emit_durable(
                        store,
                        sink,
                        conversation_id,
                        run_id,
                        EventKind::ApprovalRequested,
                        serde_json::json!({ "tool_call_id": tool_call_id }),
                    )
                    .await,
                );
                let resp = driver.await_approval(&tool_call_id).await;
                events.push(
                    emit_durable(
                        store,
                        sink,
                        conversation_id,
                        run_id,
                        EventKind::ApprovalResolved,
                        serde_json::json!({ "tool_call_id": tool_call_id, "response": resp }),
                    )
                    .await,
                );
                Input::ApprovalResolved { response: resp }
            }
            Action::Synthesize => {
                driver.synthesize().await;
                events.push(
                    emit_durable(
                        store,
                        sink,
                        conversation_id,
                        run_id,
                        EventKind::AssistantCheckpoint,
                        serde_json::json!({}),
                    )
                    .await,
                );
                Input::Synthesized
            }
            Action::Verify => {
                let ok = driver.verify().await;
                Input::Verified { ok }
            }
            Action::ExtractMemory => {
                // Saved rows emit exactly one MemoryUpdated BEFORE the terminal
                // event; a timeout/failed capture returns false and adds no
                // notice (plan capture policy + event-order acceptance).
                if driver.extract_memory().await {
                    events.push(
                        emit_durable(
                            store,
                            sink,
                            conversation_id,
                            run_id,
                            EventKind::MemoryUpdated,
                            serde_json::json!({}),
                        )
                        .await,
                    );
                }
                Input::MemoryDone
            }
            Action::Emit { event, stop, partial } => {
                // Persist the terminal event and finalize the run row.
                let env = emit_durable(
                    store,
                    sink,
                    conversation_id,
                    run_id,
                    event,
                    serde_json::json!({ "stop": stop, "partial": partial }),
                )
                .await;
                events.push(env);
                let run = run_id.to_string();
                let status = terminal_status(event).to_string();
                let phase = phase_str(machine.phase());
                let stop_json = serde_json::to_string(&stop).ok();
                store
                    .call(move |db| {
                        let _ = db.finish_run(
                            &run,
                            &status,
                            &phase,
                            stop_json.as_deref(),
                            None,
                            &now_iso(),
                        );
                    })
                    .await;
                return TurnOutcome {
                    event,
                    stop,
                    partial,
                    durable_events: events,
                };
            }
            Action::Wait => {
                // Reducer is idle awaiting external input we don't model here.
                break;
            }
        };

        action = machine.next(input);

        // Emit a PhaseChanged when the reducer advanced to a new phase.
        let now_phase = machine.phase();
        if now_phase != prev_phase && !now_phase.is_terminal() {
            events.push(
                emit_durable(
                    store,
                    sink,
                    conversation_id,
                    run_id,
                    EventKind::PhaseChanged,
                    serde_json::json!({ "phase": phase_str(now_phase) }),
                )
                .await,
            );
            prev_phase = now_phase;
        }
    }

    // Unreachable in normal flow (every path Emits a terminal); guard anyway.
    TurnOutcome {
        event: EventKind::RunFailed,
        stop: StopReason::Error("loop exited without terminal".into()),
        partial: false,
        durable_events: events,
    }
}

fn terminal_status(event: EventKind) -> &'static str {
    match event {
        EventKind::RunCompleted => "completed",
        EventKind::RunFailed => "failed",
        EventKind::RunCancelled => "cancelled",
        EventKind::RunInterrupted => "interrupted",
        EventKind::RunBudgetLimited => "budget_limited",
        _ => "failed",
    }
}

/// Create a new run linked to an interrupted one (`resumed_from_run_id`). The
/// new run starts fresh from the last complete provider/tool boundary; it never
/// reuses a partially executed side effect. Returns the new run id.
pub async fn resume_run(
    store: &StoreHandle,
    conversation_id: &str,
    interrupted_run_id: &str,
    model: Option<String>,
) -> Option<String> {
    let conv = conversation_id.to_string();
    let from = interrupted_run_id.to_string();
    store
        .call(move |db| {
            // Only resume a genuinely terminal-interrupted run.
            let run = db.get_run(&from).ok().flatten()?;
            if run.status != "interrupted" {
                return None;
            }
            let new_id = {
                let mut b = [0u8; 16];
                rand::Rng::fill(&mut rand::thread_rng(), &mut b);
                fm_agent::ids::format_uuid_v4(b)
            };
            let now = now_iso();
            db.insert_run(
                &new_id,
                &conv,
                run.user_message_id.as_deref(),
                Some(&from),
                "running",
                "preparing",
                model.as_deref(),
                None,
                &now,
            )
            .ok()?;
            Some(new_id)
        })
        .await
}

#[cfg(test)]
mod tests;
