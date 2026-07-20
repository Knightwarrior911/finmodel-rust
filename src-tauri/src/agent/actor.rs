//! The conversation turn driver: runs one [`fm_agent::AgentMachine`] to a
//! terminal, performing provider/tool/store I/O through a [`Driver`] and the
//! [`StoreHandle`], persisting each durable transition *before* broadcasting it.
//!
//! The reducer is pure; this module is the async side. Real provider/tool
//! execution arrives in Phase C as a concrete [`Driver`]; Phase B proves the
//! loop's durability/replay/resume/crash behavior with a fake driver and a real
//! (in-memory) store, exactly as the plan's fake-store acceptance requires.

use fm_agent::machine::{Action, AgentMachine, Input, ToolCall};
use fm_agent::types::{ApprovalResponse, ArtifactRef, EventKind, Plan, SourceRef, StopReason};
use fm_agent::Policy;

use crate::agent::events::{AgentEventEnvelope, EventSink};
use crate::store::{now_iso, StoreHandle};

/// What a provider request produced.
#[derive(Clone, Debug, Default)]
pub struct ModelOut {
    pub calls: Vec<ToolCall>,
    pub final_answer: bool,
    pub tokens: u64,
}

/// A workflow the driver selected during `prepare`, carried into the plan
/// payload so the UI attributes the run to a workflow (id + version).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkflowSelection {
    pub id: String,
    pub version: u32,
}

/// A control signal the pump observes at I/O boundaries. `Cancel` is a terminal
/// user stop (`RunCancelled`); `Interrupt` is a resumable pause (`RunInterrupted`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlSignal {
    Cancel,
    Interrupt,
}

/// What [`Driver::prepare`] resolved.
#[derive(Clone, Debug, Default)]
pub struct PreparedInfo {
    pub uses_tools: bool,
    pub plan_needed: bool,
    pub needs_verification: bool,
    /// The workflow selected for this run, if any (interactive turns leave None).
    pub workflow: Option<WorkflowSelection>,
    /// A policy escalation to feed the reducer before execution when a workflow
    /// (or workflow-class capability) was accepted; None keeps the launch policy.
    pub escalation: Option<Policy>,
}

/// The I/O side of a turn. Each method corresponds to an [`Action`] the reducer
/// emits; implementations perform real provider/tool/store work (Phase C) or are
/// scripted fakes (tests). Native `async fn` in traits (stable since 1.75) —
/// used only with generics here, never as `dyn`.
/// Per-batch tool execution outcome reported by the driver, so the actor can
/// emit an honest durable ToolSucceeded/ToolFailed per call (the replayed UI
/// must never render a failed tool as succeeded).
/// One tool result's UI part (card) carried out of the batch so the actor emits
/// a durable `ResultPartAdded` — the single event/replay render path (Task 2.1),
/// replacing the transitional `chat_tool` card channel.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ResultPart {
    pub tool_call_id: String,
    pub name: String,
    /// The UI card payload (the tool result's `display`).
    pub card: serde_json::Value,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ToolBatchOutcome {
    /// Conservative token charge for the batch.
    pub tokens: u64,
    /// Ids whose execution failed (validation / runtime / cancelled).
    pub failed: Vec<String>,
    /// Sources promoted from this batch's tool results (deduped by id).
    pub sources: Vec<SourceRef>,
    /// Artifacts promoted from this batch's tool results (deduped by id).
    pub artifacts: Vec<ArtifactRef>,
    /// Per-tool UI cards, in execution order, for durable `ResultPartAdded`.
    pub parts: Vec<ResultPart>,
}

/// Metadata for a queued tool call, persisted as a `tool_invocations` row before
/// execution so reload/resume can reconcile completed vs in-flight side effects.
/// `ToolSpec` owns canonical-arg / idempotency identity.
#[derive(Clone, Debug, Default)]
pub struct ToolCallMeta {
    pub name: String,
    pub risk: String,
    pub canonical_args_json: Option<String>,
    pub idempotency_hash: Option<String>,
}

#[allow(async_fn_in_trait)]
pub trait Driver {
    async fn prepare(&mut self) -> PreparedInfo;
    /// Build the one visible versioned plan for this turn. Never empty when the
    /// reducer entered `Planning` (no hidden planner, no empty `PlanUpdated`).
    async fn make_plan(&mut self) -> Plan;
    async fn request_model(&mut self) -> ModelOut;
    /// Ask the model to repair one malformed call, then re-request. Returns the
    /// next model output.
    async fn repair_tool_call(&mut self, tool_call_id: &str) -> ModelOut;
    /// Execute a batch of tool calls; returns tokens consumed.
    async fn schedule_tools(&mut self, batch: &[String]) -> ToolBatchOutcome;
    /// Budget-grace wrap-up: one final no-tools model call that answers from
    /// the evidence already gathered. Called only when a runaway guard tripped
    /// mid-run (`AgentMachine::in_budget_grace`) — the normal completion path
    /// already has the model's own final prose. Default: no-op (test drivers).
    async fn wrap_up(&mut self) {}
    async fn synthesize(&mut self);
    async fn verify(&mut self) -> bool;
    /// Take the verification card produced by the last `verify()`, if any, so the
    /// actor emits it as a durable `ResultPartAdded` (Task 2.1). Default: none.
    fn take_verify_card(&mut self) -> Option<serde_json::Value> {
        None
    }
    /// Drain side cards produced outside the tool path (self-check notes,
    /// advisor second looks, the turn cost line) so the actor emits them on
    /// the durable render path too. Default: none.
    fn take_side_cards(&mut self) -> Vec<serde_json::Value> {
        Vec::new()
    }
    /// Run bounded memory extraction; returns the count of rows saved so the
    /// loop emits exactly one `MemoryUpdated {count}` before the terminal event
    /// (zero → no notice). The count is required: the UI drops count-less
    /// notices, so `MemoryUpdated` without it would never render.
    async fn extract_memory(&mut self) -> usize;
    /// Block until the parked approval is resolved.
    async fn await_approval(&mut self, tool_call_id: &str) -> ApprovalResponse;
    /// Metadata for a queued tool call, used to persist its `tool_invocations`
    /// row before execution (canonical args + idempotency identity).
    fn call_meta(&self, tool_call_id: &str) -> ToolCallMeta;

    /// Monotonic elapsed milliseconds since the run started. The pump feeds this
    /// to the reducer as `Input::Tick` at every I/O boundary so the deadline is
    /// the authoritative post-boundary budget check.
    fn elapsed_ms(&self) -> u64;
    /// A pending control signal (user pause/stop) observed at I/O boundaries. The
    /// pump feeds `Cancel`/`Interrupt` to the reducer before accepting a normal
    /// success result, so a stop/pause preempts the next boundary promptly.
    fn control_signal(&self) -> Option<ControlSignal>;
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

/// Emit a `PlanUpdated` carrying the whole current plan (Task 3.2). The plan is
/// revised in place as tool/verification transitions complete steps — progress is
/// never inferred from elapsed time — and the complete revision is emitted whole.
async fn emit_plan(
    store: &StoreHandle,
    sink: &dyn EventSink,
    conversation_id: &str,
    run_id: &str,
    plan: &fm_agent::types::Plan,
    workflow: &Option<WorkflowSelection>,
) -> AgentEventEnvelope {
    let mut payload = serde_json::to_value(plan).unwrap_or_else(|_| serde_json::json!({}));
    if let Some(w) = workflow {
        if let Some(obj) = payload.as_object_mut() {
            obj.insert(
                "workflow".into(),
                serde_json::json!({ "id": w.id, "version": w.version }),
            );
        }
    }
    emit_durable(
        store,
        sink,
        conversation_id,
        run_id,
        EventKind::PlanUpdated,
        payload,
    )
    .await
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
    // The workflow selected in `prepare`, echoed into the plan payload.
    let mut workflow_sel: Option<WorkflowSelection> = None;
    // Plan version echoed into the checkpoint; completed invocation ids let a
    // resumed run skip already-finished idempotent calls (Phase 0.5 substrate).
    let mut plan_version: u32 = 0;
    let mut completed_invocations: Vec<String> = Vec::new();
    // The current plan (Task 3.2), captured at MakePlan and revised in place as
    // tool/verification transitions complete steps; the whole revision re-emits.
    let mut plan: Option<fm_agent::types::Plan> = None;

    loop {
        // At real I/O boundaries feed the reducer clock + control BEFORE running
        // the boundary: the deadline is the authoritative post-boundary budget
        // check, and a user Stop/Pause preempts the next boundary. A tripped
        // deadline or a control signal yields a terminal Emit we fall through to.
        if is_io_boundary(&action) {
            let ticked = machine.next(Input::Tick {
                elapsed_ms: driver.elapsed_ms(),
            });
            if matches!(ticked, Action::Emit { .. }) {
                action = ticked;
            } else if let Some(sig) = driver.control_signal() {
                action = machine.next(match sig {
                    ControlSignal::Cancel => Input::Cancel,
                    ControlSignal::Interrupt => Input::Interrupt,
                });
            }
        }
        // Execute the current action; produce the next reducer input.
        let input: Input = match action {
            Action::Prepare => {
                let info = driver.prepare().await;
                if let Some(policy) = info.escalation {
                    // Escalate the run budget before execution (returns Wait).
                    let _ = machine.next(Input::WorkflowAccepted { policy });
                }
                workflow_sel = info.workflow.clone();
                Input::Prepared {
                    uses_tools: info.uses_tools,
                    plan_needed: info.plan_needed,
                    needs_verification: info.needs_verification,
                }
            }
            Action::MakePlan => {
                let made = driver.make_plan().await;
                plan_version = made.version;
                events.push(
                    emit_plan(store, sink, conversation_id, run_id, &made, &workflow_sel).await,
                );
                plan = Some(made);
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
                let batch_id = new_uuid();
                // Promote the next pending plan step to Running as this tool batch
                // begins (Task 3.2 — a transition, not a time-based guess).
                if let Some(p) = plan.as_mut() {
                    if p.advance_active().is_some() {
                        p.version += 1;
                        plan_version = p.version;
                        events.push(
                            emit_plan(store, sink, conversation_id, run_id, p, &workflow_sel).await,
                        );
                    }
                }
                for id in &batch {
                    // Persist the invocation (status running) BEFORE execution so a
                    // crash mid-batch is reconcilable on resume.
                    let meta = driver.call_meta(id);
                    let (rid, bid, inv, name, risk, args) = (
                        run_id.to_string(),
                        batch_id.clone(),
                        id.clone(),
                        meta.name.clone(),
                        meta.risk.clone(),
                        meta.canonical_args_json.clone(),
                    );
                    store
                        .call(move |db| {
                            let _ = db.insert_tool_invocation(
                                &inv,
                                &rid,
                                None,
                                Some(&bid),
                                &name,
                                "running",
                                &risk,
                                args.as_deref(),
                                &now_iso(),
                            );
                        })
                        .await;
                    events.push(
                        emit_durable(
                            store,
                            sink,
                            conversation_id,
                            run_id,
                            EventKind::ToolStarted,
                            serde_json::json!({ "tool_call_id": id, "name": meta.name }),
                        )
                        .await,
                    );
                }
                let outcome = driver.schedule_tools(&batch).await;
                for id in &batch {
                    let failed = outcome.failed.iter().any(|f| f == id);
                    let inv = id.clone();
                    let status = if failed { "failed" } else { "succeeded" };
                    let err = if failed { Some("tool_failed") } else { None };
                    store
                        .call(move |db| {
                            let _ = db.finish_tool_invocation(&inv, status, None, err, &now_iso());
                        })
                        .await;
                    if !failed {
                        completed_invocations.push(id.clone());
                    }
                    let kind = if failed {
                        EventKind::ToolFailed
                    } else {
                        EventKind::ToolSucceeded
                    };
                    events.push(
                        emit_durable(
                            store,
                            sink,
                            conversation_id,
                            run_id,
                            kind,
                            serde_json::json!({ "tool_call_id": id }),
                        )
                        .await,
                    );
                }
                // Promote sources/artifacts from committed results before the
                // model consumes them, then emit typed ArtifactCreated events
                // (Task 1.2 steps 2 & 4). Best-effort here; dedup/normalization
                // is Phase 4.1.
                if !outcome.sources.is_empty() || !outcome.artifacts.is_empty() {
                    let conv = conversation_id.to_string();
                    let ws = store
                        .call(move |db| db.conversation_workspace(&conv).ok().flatten())
                        .await
                        .unwrap_or_default();
                    for s in &outcome.sources {
                        let (id, kind, uri) =
                            (s.id.clone(), s.kind.clone(), s.canonical_uri.clone());
                        let (title, pubr, pub_at, acc) = (
                            s.title.clone(),
                            s.publisher.clone(),
                            s.published_at.clone(),
                            s.accessed_at.clone(),
                        );
                        let wsc = ws.clone();
                        store
                            .call(move |db| {
                                let _ = db.insert_source(
                                    &id,
                                    &wsc,
                                    &kind,
                                    &uri,
                                    title.as_deref(),
                                    pubr.as_deref(),
                                    pub_at.as_deref(),
                                    acc.as_deref(),
                                    None,
                                );
                            })
                            .await;
                    }
                    for a in &outcome.artifacts {
                        let (id, kind, label, mime, ver, sha) = (
                            a.id.clone(),
                            a.kind.clone(),
                            a.label.clone(),
                            a.mime.clone(),
                            a.version as i64,
                            a.sha256.clone(),
                        );
                        let wsc = ws.clone();
                        let conv2 = conversation_id.to_string();
                        let rid = run_id.to_string();
                        store
                            .call(move |db| {
                                let _ = db.insert_artifact(
                                    &id,
                                    &wsc,
                                    Some(&conv2),
                                    Some(&rid),
                                    &kind,
                                    &label,
                                    &mime,
                                    None,
                                    ver,
                                    None,
                                    &sha,
                                    &now_iso(),
                                );
                            })
                            .await;
                        events.push(
                            emit_durable(
                                store,
                                sink,
                                conversation_id,
                                run_id,
                                EventKind::ArtifactCreated,
                                serde_json::json!({
                                    "id": a.id,
                                    "kind": a.kind,
                                    "label": a.label,
                                    "mime": a.mime,
                                    "version": a.version,
                                }),
                            )
                            .await,
                        );
                    }
                }
                // Emit a durable ResultPartAdded per tool card (Task 2.1): the
                // single event/replay render path. The card rides `display`; the
                // UI reduces this instead of the transitional `chat_tool` channel.
                for part in &outcome.parts {
                    events.push(
                        emit_durable(
                            store,
                            sink,
                            conversation_id,
                            run_id,
                            EventKind::ResultPartAdded,
                            serde_json::json!({
                                "tool_call_id": part.tool_call_id,
                                "name": part.name,
                                "card": part.card,
                            }),
                        )
                        .await,
                    );
                }
                // Mark the step(s) this batch was executing Done (Task 3.2).
                if let Some(p) = plan.as_mut() {
                    let running: Vec<String> = p
                        .steps
                        .iter()
                        .filter(|s| s.status == fm_agent::types::PlanStepStatus::Running)
                        .map(|s| s.id.clone())
                        .collect();
                    if !running.is_empty() {
                        let refs: Vec<&str> = running.iter().map(|s| s.as_str()).collect();
                        p.complete_steps(&refs);
                        plan_version = p.version;
                        events.push(
                            emit_plan(store, sink, conversation_id, run_id, p, &workflow_sel).await,
                        );
                    }
                }
                Input::ToolsCompleted {
                    tokens: outcome.tokens,
                }
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
                if machine.in_budget_grace() {
                    // A runaway guard tripped mid-run: make the wrap-up model
                    // call so the turn ends with a real answer, not a dead end.
                    driver.wrap_up().await;
                }
                driver.synthesize().await;
                // Synthesis is the terminal work step: complete every step still
                // open so the delivered plan reads fully done (Task 3.2).
                if let Some(p) = plan.as_mut() {
                    let open: Vec<String> = p
                        .steps
                        .iter()
                        .filter(|s| s.status != fm_agent::types::PlanStepStatus::Done)
                        .map(|s| s.id.clone())
                        .collect();
                    if !open.is_empty() {
                        let refs: Vec<&str> = open.iter().map(|s| s.as_str()).collect();
                        p.complete_steps(&refs);
                        plan_version = p.version;
                        events.push(
                            emit_plan(store, sink, conversation_id, run_id, p, &workflow_sel).await,
                        );
                    }
                }
                // Typed durable checkpoint: reducer phase, plan version, completed
                // invocation ids, budget snapshot, and last durable sequence.
                let last_seq = events.last().and_then(|e| e.sequence).unwrap_or(0);
                let checkpoint = serde_json::json!({
                    "checkpoint_version": 1,
                    "phase": phase_str(machine.phase()),
                    "plan_version": plan_version,
                    "completed_invocation_ids": completed_invocations,
                    "budget": machine.budget(),
                    "last_sequence": last_seq,
                });
                events.push(
                    emit_durable(
                        store,
                        sink,
                        conversation_id,
                        run_id,
                        EventKind::AssistantCheckpoint,
                        checkpoint,
                    )
                    .await,
                );
                Input::Synthesized
            }
            Action::Verify => {
                let ok = driver.verify().await;
                // Side cards (self-check, advisor, turn cost) ride the same
                // durable render path as the verification card.
                for card in driver.take_side_cards() {
                    events.push(
                        emit_durable(
                            store,
                            sink,
                            conversation_id,
                            run_id,
                            EventKind::ResultPartAdded,
                            serde_json::json!({ "card": card }),
                        )
                        .await,
                    );
                }
                // Emit the verification card on the durable render path (Task 2.1).
                if let Some(card) = driver.take_verify_card() {
                    events.push(
                        emit_durable(
                            store,
                            sink,
                            conversation_id,
                            run_id,
                            EventKind::ResultPartAdded,
                            serde_json::json!({
                                "tool_call_id": "verify",
                                "name": "verify",
                                "card": card,
                            }),
                        )
                        .await,
                    );
                }
                Input::Verified { ok }
            }
            Action::ExtractMemory => {
                // Saved rows emit exactly one MemoryUpdated {count} BEFORE the
                // terminal event; a timeout/empty capture returns 0 and adds no
                // notice (plan capture policy + event-order acceptance). The
                // count is carried in the payload — the UI drops count-less
                // notices, so an empty payload would never render.
                let saved = driver.extract_memory().await;
                if saved > 0 {
                    events.push(
                        emit_durable(
                            store,
                            sink,
                            conversation_id,
                            run_id,
                            EventKind::MemoryUpdated,
                            serde_json::json!({ "count": saved }),
                        )
                        .await,
                    );
                }
                Input::MemoryDone
            }
            Action::Emit {
                event,
                stop,
                partial,
            } => {
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

/// Real provider/tool/verification boundaries, before which the pump feeds the
/// reducer clock (`Tick`) and any control signal. Excludes `Prepare` (run just
/// started), `MakePlan` (fast, local), `RequestApproval` (parked — the deadline
/// does not run while awaiting approval), `ExtractMemory` (post-answer auxiliary),
/// and the terminal `Emit`/`Wait`.
fn is_io_boundary(action: &Action) -> bool {
    matches!(
        action,
        Action::RequestModel
            | Action::RepairToolCall { .. }
            | Action::ScheduleTools { .. }
            | Action::Synthesize
            | Action::Verify
    )
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
