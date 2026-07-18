//! Phase B actor tests: durable persist-then-broadcast, monotonic sequences,
//! one terminal event, live/replay equality, resume linkage, and crash repair.
//! Uses a scripted fake [`Driver`] and a real in-memory store + collecting sink.

use super::*;
use crate::agent::events::AgentEventEnvelope;
use crate::store::{Db, StoreHandle};
use fm_agent::machine::{AgentMachine, ToolCall};
use fm_agent::types::Risk;
use fm_agent::types::{ArtifactRef, Plan, PlanStep, PlanStepStatus};
use fm_agent::Policy;
use parking_lot::Mutex;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

struct TempDir(PathBuf);
impl TempDir {
    fn new(tag: &str) -> Self {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let p = std::env::temp_dir().join(format!("fmactor-{tag}-{}-{}", std::process::id(), n));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Collects emitted envelopes.
#[derive(Clone, Default)]
struct CollectSink {
    events: Arc<Mutex<Vec<AgentEventEnvelope>>>,
}
impl EventSink for CollectSink {
    fn emit(&self, env: &AgentEventEnvelope) {
        self.events.lock().push(env.clone());
    }
}

/// A scripted driver: `plan` describes the turn shape.
struct FakeDriver {
    info: PreparedInfo,
    /// Sequence of model outputs to return on successive request_model calls.
    model_outs: Vec<ModelOut>,
    next_model: usize,
    verify_ok: bool,
    approval: ApprovalResponse,
    memory_saved: usize,
    elapsed_ms: u64,
    control: Option<ControlSignal>,
    batch_artifacts: Vec<ArtifactRef>,
    batch_parts: Vec<ResultPart>,
}
impl FakeDriver {
    fn direct() -> Self {
        FakeDriver {
            info: PreparedInfo {
                uses_tools: false,
                plan_needed: false,
                needs_verification: false,
                workflow: None,
                escalation: None,
            },
            model_outs: vec![],
            next_model: 0,
            verify_ok: true,
            approval: ApprovalResponse::ApproveOnce,
            memory_saved: 0,
            elapsed_ms: 0,
            control: None,
            batch_artifacts: Vec::new(),
            batch_parts: Vec::new(),
        }
    }
}
impl Driver for FakeDriver {
    async fn prepare(&mut self) -> PreparedInfo {
        self.info.clone()
    }
    async fn make_plan(&mut self) -> Plan {
        Plan {
            objective: "fake plan".into(),
            assumptions: Vec::new(),
            steps: vec![PlanStep {
                id: "s1".into(),
                label: "step".into(),
                status: PlanStepStatus::Pending,
            }],
            version: 1,
        }
    }
    async fn request_model(&mut self) -> ModelOut {
        let out = self
            .model_outs
            .get(self.next_model)
            .cloned()
            .unwrap_or(ModelOut {
                calls: vec![],
                final_answer: true,
                tokens: 10,
            });
        self.next_model += 1;
        out
    }
    async fn repair_tool_call(&mut self, _id: &str) -> ModelOut {
        self.request_model().await
    }
    async fn schedule_tools(&mut self, _batch: &[String]) -> ToolBatchOutcome {
        ToolBatchOutcome {
            tokens: 20,
            failed: vec![],
            sources: Vec::new(),
            artifacts: self.batch_artifacts.clone(),
            parts: self.batch_parts.clone(),
        }
    }
    async fn synthesize(&mut self) {}
    async fn verify(&mut self) -> bool {
        self.verify_ok
    }
    async fn extract_memory(&mut self) -> usize {
        self.memory_saved
    }
    async fn await_approval(&mut self, _id: &str) -> ApprovalResponse {
        self.approval
    }
    fn elapsed_ms(&self) -> u64 {
        self.elapsed_ms
    }
    fn control_signal(&self) -> Option<ControlSignal> {
        self.control
    }
    fn call_meta(&self, _id: &str) -> ToolCallMeta {
        ToolCallMeta {
            name: "get_quote".into(),
            risk: "read_only".into(),
            ..Default::default()
        }
    }
}

fn ro_call(id: &str) -> ToolCall {
    ToolCall {
        tool_call_id: id.into(),
        name: "get_quote".into(),
        risk: Risk::ReadOnly,
        needs_approval: false,
        args_valid: true,
    }
}

fn setup() -> (TempDir, StoreHandle, CollectSink, String) {
    let td = TempDir::new("run");
    let db = Db::open_in_memory(&td.0.join("blobs")).unwrap();
    let now = now_iso();
    db.create_workspace("w", "W", "deal", "confidential", "", true, &now)
        .unwrap();
    db.create_conversation("c1", "w", "t", &now).unwrap();
    db.insert_run(
        "r1",
        "c1",
        None,
        None,
        "running",
        "preparing",
        None,
        None,
        &now,
    )
    .unwrap();
    let store = StoreHandle::spawn(db);
    (td, store, CollectSink::default(), "r1".to_string())
}

/// Assert the collected durable stream equals the store's replayed stream.
async fn assert_live_equals_replay(store: &StoreHandle, sink: &CollectSink, run_id: &str) {
    let live = sink.events.lock().clone();
    let run = run_id.to_string();
    let replay = store
        .call(move |db| db.events_after(&run, 0).unwrap())
        .await;
    assert_eq!(live.len(), replay.len(), "live vs replay length");
    for (l, r) in live.iter().zip(replay.iter()) {
        assert_eq!(l.sequence, Some(r.sequence), "sequence mismatch");
        assert_eq!(
            super::kind_str(l.event.kind),
            r.kind,
            "kind mismatch at seq {}",
            r.sequence
        );
    }
    // Strictly monotonic sequences 1..=n.
    for (i, r) in replay.iter().enumerate() {
        assert_eq!(
            r.sequence,
            (i as i64) + 1,
            "sequences must be dense & monotonic"
        );
    }
    // Exactly one terminal event.
    let terminals = live.iter().filter(|e| e.is_terminal()).count();
    assert_eq!(terminals, 1, "exactly one terminal event");
    assert!(live.last().unwrap().is_terminal(), "terminal is last");
}

#[tokio::test]
async fn direct_answer_turn_persists_and_replays_equally() {
    let (_td, store, sink, run) = setup();
    let m = AgentMachine::new(Policy::INTERACTIVE);
    let out = run_turn(&store, &sink, "c1", &run, m, FakeDriver::direct()).await;
    assert_eq!(out.event, EventKind::RunCompleted);
    assert!(!out.partial);
    assert_live_equals_replay(&store, &sink, &run).await;
    // Run row finalized.
    let r = run.clone();
    let row = store.call(move |db| db.get_run(&r).unwrap().unwrap()).await;
    assert_eq!(row.status, "completed");
}

#[tokio::test]
async fn tool_turn_emits_tool_and_terminal_events_in_order() {
    let (_td, store, sink, run) = setup();
    let mut driver = FakeDriver::direct();
    driver.info = PreparedInfo {
        uses_tools: true,
        plan_needed: false,
        needs_verification: true,
        ..Default::default()
    };
    // First model call: one read tool; second call: final answer.
    driver.model_outs = vec![
        ModelOut {
            calls: vec![ro_call("q1")],
            final_answer: false,
            tokens: 30,
        },
        ModelOut {
            calls: vec![],
            final_answer: true,
            tokens: 40,
        },
    ];
    let m = AgentMachine::new(Policy::INTERACTIVE);
    let out = run_turn(&store, &sink, "c1", &run, m, driver).await;
    assert_eq!(out.event, EventKind::RunCompleted);
    let kinds: Vec<EventKind> = sink.events.lock().iter().map(|e| e.event.kind).collect();
    assert!(kinds.contains(&EventKind::ToolStarted));
    assert!(kinds.contains(&EventKind::ToolSucceeded));
    // ToolStarted precedes ToolSucceeded.
    let s = kinds
        .iter()
        .position(|k| *k == EventKind::ToolStarted)
        .unwrap();
    let d = kinds
        .iter()
        .position(|k| *k == EventKind::ToolSucceeded)
        .unwrap();
    assert!(s < d);
    assert_live_equals_replay(&store, &sink, &run).await;
}

#[tokio::test]
async fn tool_result_emits_durable_result_part() {
    // Task 2.1: the batch's UI card rides a durable ResultPartAdded (the single
    // event/replay render path), not just the transitional chat_tool channel.
    let (_td, store, sink, run) = setup();
    let mut driver = FakeDriver::direct();
    driver.info = PreparedInfo {
        uses_tools: true,
        ..Default::default()
    };
    driver.model_outs = vec![
        ModelOut {
            calls: vec![ro_call("q1")],
            final_answer: false,
            tokens: 30,
        },
        ModelOut {
            calls: vec![],
            final_answer: true,
            tokens: 40,
        },
    ];
    driver.batch_parts = vec![ResultPart {
        tool_call_id: "q1".into(),
        name: "get_quote".into(),
        card: serde_json::json!({ "type": "quote", "ticker": "AAPL", "price": 190.0 }),
    }];
    let m = AgentMachine::new(Policy::INTERACTIVE);
    let _ = run_turn(&store, &sink, "c1", &run, m, driver).await;
    let rp = sink
        .events
        .lock()
        .iter()
        .find(|e| e.event.kind == EventKind::ResultPartAdded)
        .cloned()
        .expect("a ResultPartAdded event");
    assert!(rp.event.kind.is_durable());
    assert_eq!(rp.event.payload["name"], "get_quote");
    assert_eq!(rp.event.payload["card"]["ticker"], "AAPL");
    // Durable → survives replay identically.
    assert_live_equals_replay(&store, &sink, &run).await;
}

#[tokio::test]
async fn approval_turn_requests_and_resolves() {
    let (_td, store, sink, run) = setup();
    let mut driver = FakeDriver::direct();
    driver.info = PreparedInfo {
        uses_tools: true,
        plan_needed: false,
        needs_verification: false,
        ..Default::default()
    };
    let write = ToolCall {
        tool_call_id: "w1".into(),
        name: "export_excel".into(),
        risk: Risk::Export,
        needs_approval: true,
        args_valid: true,
    };
    driver.model_outs = vec![
        ModelOut {
            calls: vec![write],
            final_answer: false,
            tokens: 10,
        },
        ModelOut {
            calls: vec![],
            final_answer: true,
            tokens: 10,
        },
    ];
    driver.approval = ApprovalResponse::ApproveOnce;
    let m = AgentMachine::new(Policy::INTERACTIVE);
    let out = run_turn(&store, &sink, "c1", &run, m, driver).await;
    assert_eq!(out.event, EventKind::RunCompleted);
    let kinds: Vec<EventKind> = sink.events.lock().iter().map(|e| e.event.kind).collect();
    assert!(kinds.contains(&EventKind::ApprovalRequested));
    assert!(kinds.contains(&EventKind::ApprovalResolved));
    assert_live_equals_replay(&store, &sink, &run).await;
}

#[tokio::test]
async fn unverified_after_repair_completes_partial() {
    let (_td, store, sink, run) = setup();
    let mut driver = FakeDriver::direct();
    driver.info = PreparedInfo {
        uses_tools: true,
        plan_needed: false,
        needs_verification: true,
        ..Default::default()
    };
    driver.model_outs = vec![ModelOut {
        calls: vec![],
        final_answer: true,
        tokens: 10,
    }];
    driver.verify_ok = false; // both verify attempts fail
    let m = AgentMachine::new(Policy::INTERACTIVE);
    let out = run_turn(&store, &sink, "c1", &run, m, driver).await;
    assert_eq!(out.event, EventKind::RunCompleted);
    assert!(out.partial, "unverified claim -> partial answer");
    assert!(matches!(out.stop, StopReason::UnverifiedClaim));
    assert_live_equals_replay(&store, &sink, &run).await;
}

#[tokio::test]
async fn crash_repair_then_resume_creates_linked_run() {
    let (_td, store, _sink, run) = setup();
    // Simulate a crash: the run is left 'running' with a couple of events.
    let r = run.clone();
    store
        .call(move |db| {
            db.append_event("e1", &r, "run_started", "{}", &now_iso())
                .unwrap();
            db.append_event("e2", &r, "phase_changed", "{}", &now_iso())
                .unwrap();
        })
        .await;
    // Restart repair.
    let repaired = store
        .call(|db| db.repair_interrupted_runs(&now_iso()).unwrap())
        .await;
    assert_eq!(repaired, 1);
    let r2 = run.clone();
    let row = store
        .call(move |db| db.get_run(&r2).unwrap().unwrap())
        .await;
    assert_eq!(row.status, "interrupted");
    // Resume returns a NEW run linked by resumed_from_run_id.
    let new_id = resume_run(&store, "c1", &run, None).await.expect("resume");
    assert_ne!(new_id, run);
    let nid = new_id.clone();
    let new_row = store
        .call(move |db| db.get_run(&nid).unwrap().unwrap())
        .await;
    assert_eq!(new_row.resumed_from_run_id.as_deref(), Some(run.as_str()));
    assert_eq!(new_row.status, "running");
    // A terminal (non-interrupted) run cannot be resumed.
    let nid2 = new_id.clone();
    store
        .call(move |db| {
            db.finish_run(
                &nid2,
                "completed",
                "completed",
                Some("end_turn"),
                None,
                &now_iso(),
            )
            .unwrap();
        })
        .await;
    assert!(resume_run(&store, "c1", &new_id, None).await.is_none());
}

#[tokio::test]
async fn memory_updated_precedes_single_terminal_when_saved() {
    let (_td, store, sink, run) = setup();
    let mut driver = FakeDriver::direct();
    driver.memory_saved = 2;
    let m = AgentMachine::new(Policy::INTERACTIVE);
    let out = run_turn(&store, &sink, "c1", &run, m, driver).await;
    assert_eq!(out.event, EventKind::RunCompleted);
    let kinds: Vec<EventKind> = sink.events.lock().iter().map(|e| e.event.kind).collect();
    // Exactly one MemoryUpdated, and it precedes the single terminal.
    assert_eq!(
        kinds
            .iter()
            .filter(|k| **k == EventKind::MemoryUpdated)
            .count(),
        1
    );
    let mem = kinds
        .iter()
        .position(|k| *k == EventKind::MemoryUpdated)
        .unwrap();
    let term = kinds
        .iter()
        .position(|k| *k == EventKind::RunCompleted)
        .unwrap();
    assert!(mem < term, "MemoryUpdated must precede RunCompleted");
    // The count rides the payload — the UI drops count-less notices.
    let mem_env = sink
        .events
        .lock()
        .iter()
        .find(|e| e.event.kind == EventKind::MemoryUpdated)
        .cloned()
        .unwrap();
    assert_eq!(mem_env.event.payload["count"], serde_json::json!(2));
    assert_eq!(kinds.iter().filter(|k| k.is_terminal()).count(), 1);
    assert_live_equals_replay(&store, &sink, &run).await;
}

#[tokio::test]
async fn no_memory_notice_when_capture_saves_nothing() {
    let (_td, store, sink, run) = setup();
    let driver = FakeDriver::direct(); // memory_saved defaults 0
    let m = AgentMachine::new(Policy::INTERACTIVE);
    let out = run_turn(&store, &sink, "c1", &run, m, driver).await;
    assert_eq!(out.event, EventKind::RunCompleted);
    let kinds: Vec<EventKind> = sink.events.lock().iter().map(|e| e.event.kind).collect();
    assert!(
        !kinds.contains(&EventKind::MemoryUpdated),
        "timeout/empty capture adds no memory notice"
    );
    assert_eq!(kinds.iter().filter(|k| k.is_terminal()).count(), 1);
}

// ---- Phase 0: lifecycle-truthfulness pins (fail on the pre-edit pump, which
// never fed Tick/Cancel/Interrupt/WorkflowAccepted to the reducer). ----

#[tokio::test]
async fn control_cancel_yields_cancelled_not_completed() {
    let (_td, store, sink, run) = setup();
    let mut driver = FakeDriver::direct();
    driver.info.uses_tools = true; // reach a real I/O boundary
    driver.control = Some(ControlSignal::Cancel);
    let m = AgentMachine::new(Policy::INTERACTIVE);
    let out = run_turn(&store, &sink, "c1", &run, m, driver).await;
    assert_eq!(out.event, EventKind::RunCancelled);
    assert_ne!(out.event, EventKind::RunCompleted);
    assert_live_equals_replay(&store, &sink, &run).await;
}

#[tokio::test]
async fn control_interrupt_yields_interrupted_resumable() {
    let (_td, store, sink, run) = setup();
    let mut driver = FakeDriver::direct();
    driver.info.uses_tools = true;
    driver.control = Some(ControlSignal::Interrupt);
    let m = AgentMachine::new(Policy::INTERACTIVE);
    let out = run_turn(&store, &sink, "c1", &run, m, driver).await;
    // Interrupt is a distinct, resumable terminal — never Cancelled or Completed.
    assert_eq!(out.event, EventKind::RunInterrupted);
    // The run row is left "interrupted" so agent_resume can relaunch it.
    let r = run.clone();
    let row = store.call(move |db| db.get_run(&r).unwrap().unwrap()).await;
    assert_eq!(row.status, "interrupted");
    assert!(resume_run(&store, "c1", &run, None).await.is_some());
}

#[tokio::test]
async fn boundary_tick_trips_deadline() {
    let (_td, store, sink, run) = setup();
    let mut driver = FakeDriver::direct();
    driver.info.uses_tools = true;
    driver.elapsed_ms = Policy::INTERACTIVE.deadline_ms + 1;
    let m = AgentMachine::new(Policy::INTERACTIVE);
    let out = run_turn(&store, &sink, "c1", &run, m, driver).await;
    assert_eq!(out.event, EventKind::RunBudgetLimited);
}

#[tokio::test]
async fn workflow_escalation_raises_the_deadline_ceiling() {
    let (_td, store, sink, run) = setup();
    let mut driver = FakeDriver::direct();
    driver.info.uses_tools = true;
    // Past the INTERACTIVE deadline but well under the escalated WORKFLOW one.
    driver.elapsed_ms = Policy::INTERACTIVE.deadline_ms + 1;
    driver.info.escalation = Some(Policy::WORKFLOW);
    let m = AgentMachine::new(Policy::INTERACTIVE);
    let out = run_turn(&store, &sink, "c1", &run, m, driver).await;
    // Escalation preserved usage AND raised the ceiling: the run completes
    // rather than tripping the old 120s interactive deadline.
    assert_eq!(out.event, EventKind::RunCompleted);
}

#[tokio::test]
async fn plan_update_carries_a_non_empty_plan() {
    let (_td, store, sink, run) = setup();
    let mut driver = FakeDriver::direct();
    driver.info.uses_tools = true;
    driver.info.plan_needed = true;
    let m = AgentMachine::new(Policy::INTERACTIVE);
    let _ = run_turn(&store, &sink, "c1", &run, m, driver).await;
    let plan = sink
        .events
        .lock()
        .iter()
        .find(|e| e.event.kind == EventKind::PlanUpdated)
        .cloned()
        .expect("a PlanUpdated event");
    // No empty PlanUpdated: objective + at least one step must ride the payload.
    assert_eq!(
        plan.event.payload["objective"],
        serde_json::json!("fake plan")
    );
    assert_eq!(
        plan.event.payload["steps"].as_array().map(|a| a.len()),
        Some(1)
    );
}

#[tokio::test]
async fn plan_steps_advance_through_tool_and_synthesis_transitions() {
    // Task 3.2 live: the pump revises the plan as real transitions happen — the
    // step goes Pending -> Running (tool batch begins) -> Done (batch completes),
    // each a whole re-emitted PlanUpdated with a rising version. Never time-based.
    let (_td, store, sink, run) = setup();
    let mut driver = FakeDriver::direct();
    driver.info.uses_tools = true;
    driver.info.plan_needed = true;
    driver.model_outs = vec![
        ModelOut {
            calls: vec![ro_call("q1")],
            final_answer: false,
            tokens: 30,
        },
        ModelOut {
            calls: vec![],
            final_answer: true,
            tokens: 40,
        },
    ];
    let m = AgentMachine::new(Policy::INTERACTIVE);
    let _ = run_turn(&store, &sink, "c1", &run, m, driver).await;
    let statuses: Vec<String> = sink
        .events
        .lock()
        .iter()
        .filter(|e| e.event.kind == EventKind::PlanUpdated)
        .map(|e| {
            e.event.payload["steps"][0]["status"]
                .as_str()
                .unwrap_or("")
                .to_string()
        })
        .collect();
    // At least three revisions: pending -> running -> done, in order.
    assert!(statuses.contains(&"pending".to_string()), "{statuses:?}");
    assert!(statuses.contains(&"running".to_string()), "{statuses:?}");
    assert!(statuses.contains(&"done".to_string()), "{statuses:?}");
    let p = statuses.iter().position(|s| s == "pending").unwrap();
    let r = statuses.iter().position(|s| s == "running").unwrap();
    let d = statuses.iter().position(|s| s == "done").unwrap();
    assert!(p < r && r < d, "{statuses:?}");
    // Versions strictly increase across revisions.
    let versions: Vec<u64> = sink
        .events
        .lock()
        .iter()
        .filter(|e| e.event.kind == EventKind::PlanUpdated)
        .map(|e| e.event.payload["version"].as_u64().unwrap_or(0))
        .collect();
    assert!(versions.windows(2).all(|w| w[1] > w[0]), "{versions:?}");
    assert_live_equals_replay(&store, &sink, &run).await;
}

#[tokio::test]
async fn tool_batch_persists_invocation_rows() {
    let (_td, store, sink, run) = setup();
    let mut driver = FakeDriver::direct();
    driver.info = PreparedInfo {
        uses_tools: true,
        plan_needed: false,
        needs_verification: false,
        ..Default::default()
    };
    driver.model_outs = vec![
        ModelOut {
            calls: vec![ro_call("q1")],
            final_answer: false,
            tokens: 30,
        },
        ModelOut {
            calls: vec![],
            final_answer: true,
            tokens: 10,
        },
    ];
    let m = AgentMachine::new(Policy::INTERACTIVE);
    let _ = run_turn(&store, &sink, "c1", &run, m, driver).await;
    let r = run.clone();
    let rows: Vec<(String, String)> = store
        .call(move |db| {
            let mut stmt = db
                .conn()
                .prepare("SELECT tool_name, status FROM tool_invocations WHERE run_id=?1")
                .unwrap();
            let mapped = stmt
                .query_map([&r], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .unwrap();
            mapped.filter_map(|x| x.ok()).collect()
        })
        .await;
    assert_eq!(rows.len(), 1, "one invocation row persisted");
    assert_eq!(rows[0].0, "get_quote");
    assert_eq!(rows[0].1, "succeeded");
}

#[tokio::test]
async fn checkpoint_payload_is_typed_not_empty() {
    let (_td, store, sink, run) = setup();
    let mut driver = FakeDriver::direct();
    driver.info = PreparedInfo {
        uses_tools: true,
        plan_needed: false,
        needs_verification: true,
        ..Default::default()
    };
    driver.model_outs = vec![ModelOut {
        calls: vec![],
        final_answer: true,
        tokens: 10,
    }];
    let m = AgentMachine::new(Policy::INTERACTIVE);
    let _ = run_turn(&store, &sink, "c1", &run, m, driver).await;
    let ckpt = sink
        .events
        .lock()
        .iter()
        .find(|e| e.event.kind == EventKind::AssistantCheckpoint)
        .cloned()
        .expect("a checkpoint event");
    assert_eq!(
        ckpt.event.payload["checkpoint_version"],
        serde_json::json!(1)
    );
    assert!(ckpt.event.payload.get("phase").is_some());
    assert!(ckpt.event.payload.get("completed_invocation_ids").is_some());
    assert!(ckpt.event.payload.get("budget").is_some());
}

#[tokio::test]
async fn resumed_run_when_driven_emits_new_events() {
    let (_td, store, sink, run) = setup();
    // Interrupt the original run (leaves status "interrupted").
    let mut d = FakeDriver::direct();
    d.info.uses_tools = true;
    d.control = Some(ControlSignal::Interrupt);
    let out = run_turn(
        &store,
        &sink,
        "c1",
        &run,
        AgentMachine::new(Policy::INTERACTIVE),
        d,
    )
    .await;
    assert_eq!(out.event, EventKind::RunInterrupted);
    // Resume creates a linked run; DRIVING it (what agent_resume's launch_run
    // does) must produce fresh durable events, not just a row.
    let new_id = resume_run(&store, "c1", &run, None)
        .await
        .expect("resumable");
    let sink2 = CollectSink::default();
    let out2 = run_turn(
        &store,
        &sink2,
        "c1",
        &new_id,
        AgentMachine::new(Policy::INTERACTIVE),
        FakeDriver::direct(),
    )
    .await;
    assert_eq!(out2.event, EventKind::RunCompleted);
    let nid = new_id.clone();
    let evs = store
        .call(move |db| db.events_after(&nid, 0).unwrap())
        .await;
    assert!(evs.iter().any(|e| e.kind == "run_started"));
    assert!(evs.iter().any(|e| e.kind == "run_completed"));
}

#[tokio::test]
async fn batch_artifacts_persist_and_emit_artifact_created() {
    let (_td, store, sink, run) = setup();
    let mut driver = FakeDriver::direct();
    driver.info = PreparedInfo {
        uses_tools: true,
        plan_needed: false,
        needs_verification: false,
        ..Default::default()
    };
    driver.model_outs = vec![
        ModelOut {
            calls: vec![ro_call("m1")],
            final_answer: false,
            tokens: 20,
        },
        ModelOut {
            calls: vec![],
            final_answer: true,
            tokens: 10,
        },
    ];
    driver.batch_artifacts = vec![ArtifactRef {
        id: "art-xyz".into(),
        kind: "workbook".into(),
        label: "AAPL model".into(),
        mime: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet".into(),
        version: 1,
        sha256: "abc".into(),
    }];
    let m = AgentMachine::new(Policy::INTERACTIVE);
    let _ = run_turn(&store, &sink, "c1", &run, m, driver).await;
    // Typed ArtifactCreated event carries the opaque id (no raw path).
    let ev = sink
        .events
        .lock()
        .iter()
        .find(|e| e.event.kind == EventKind::ArtifactCreated)
        .cloned()
        .expect("an ArtifactCreated event");
    assert_eq!(ev.event.payload["id"], serde_json::json!("art-xyz"));
    assert_eq!(ev.event.payload["label"], serde_json::json!("AAPL model"));
    // The artifact row was persisted (survives reload).
    let r = run.clone();
    let count: i64 = store
        .call(move |db| {
            db.conn()
                .query_row(
                    "SELECT count(*) FROM artifacts WHERE run_id=?1 AND id='art-xyz'",
                    [&r],
                    |row| row.get(0),
                )
                .unwrap_or(0)
        })
        .await;
    assert_eq!(count, 1);
}

#[tokio::test]
async fn emitted_payloads_deserialize_into_typed_structs() {
    use crate::agent::events::payloads;
    let (_td, store, sink, run) = setup();
    let mut driver = FakeDriver::direct();
    driver.info = PreparedInfo {
        uses_tools: true,
        plan_needed: true,
        needs_verification: false,
        ..Default::default()
    };
    driver.model_outs = vec![
        ModelOut {
            calls: vec![ro_call("q1")],
            final_answer: false,
            tokens: 20,
        },
        ModelOut {
            calls: vec![],
            final_answer: true,
            tokens: 10,
        },
    ];
    driver.memory_saved = 1;
    driver.batch_artifacts = vec![ArtifactRef {
        id: "art-1".into(),
        kind: "workbook".into(),
        label: "AAPL".into(),
        mime: "x".into(),
        version: 1,
        sha256: "h".into(),
    }];
    let m = AgentMachine::new(Policy::INTERACTIVE);
    let _ = run_turn(&store, &sink, "c1", &run, m, driver).await;
    // Every structured payload the actor ACTUALLY emitted must deserialize into
    // its typed struct — catches drift between emit_durable json!() and the type.
    let evs = sink.events.lock().clone();
    for e in &evs {
        let p = e.event.payload.clone();
        match e.event.kind {
            EventKind::PlanUpdated => {
                serde_json::from_value::<payloads::PlanUpdated>(p).expect("PlanUpdated");
            }
            EventKind::ToolStarted | EventKind::ToolSucceeded | EventKind::ToolFailed => {
                serde_json::from_value::<payloads::ToolEvent>(p).expect("ToolEvent");
            }
            EventKind::ArtifactCreated => {
                serde_json::from_value::<payloads::ArtifactCreated>(p).expect("ArtifactCreated");
            }
            EventKind::MemoryUpdated => {
                serde_json::from_value::<payloads::MemoryUpdated>(p).expect("MemoryUpdated");
            }
            EventKind::PhaseChanged => {
                serde_json::from_value::<payloads::PhaseChanged>(p).expect("PhaseChanged");
            }
            EventKind::RunCompleted
            | EventKind::RunFailed
            | EventKind::RunCancelled
            | EventKind::RunInterrupted
            | EventKind::RunBudgetLimited => {
                serde_json::from_value::<payloads::Terminal>(p).expect("Terminal");
            }
            _ => {}
        }
    }
    // Ensure we actually exercised the structured kinds.
    assert!(evs.iter().any(|e| e.event.kind == EventKind::PlanUpdated));
    assert!(evs
        .iter()
        .any(|e| e.event.kind == EventKind::ArtifactCreated));
    assert!(evs.iter().any(|e| e.event.kind == EventKind::MemoryUpdated));
}

// ---- Phase 5.2: child delegation supervisor (real run_turn child + delivery) ----

#[tokio::test]
async fn child_delegation_runs_and_delivers_once() {
    use crate::agent::child::{run_child_delegation, ChildDispatch};
    let (_td, store, sink, parent) = setup();
    let delivered = Arc::new(Mutex::new(Vec::<String>::new()));
    let d = delivered.clone();
    let ids = ChildDispatch {
        delegation_id: "d1".into(),
        child_run_id: "child1".into(),
    };
    let m = AgentMachine::new(Policy::INTERACTIVE);
    let out = run_child_delegation(
        &store,
        &sink,
        "c1",
        &parent,
        Some("tc1".into()),
        0,
        ids,
        "{\"task\":\"peer comps\"}".into(),
        m,
        FakeDriver::direct(),
        move |res: String| async move {
            d.lock().push(res);
            true
        },
    )
    .await
    .unwrap();
    assert_eq!(out.event, EventKind::RunCompleted);
    // The child actually ran: run_turn finalized its run row.
    let row = store
        .call(|db| db.get_run("child1").unwrap().unwrap())
        .await;
    assert_eq!(row.status, "completed");
    // The parent append ran exactly once and the result is no longer pending.
    assert_eq!(delivered.lock().len(), 1);
    let p = parent.clone();
    let undel = store
        .call(move |db| db.undelivered_completed_delegations(&p).unwrap())
        .await;
    assert!(undel.is_empty());
    let p = parent.clone();
    assert_eq!(
        store
            .call(move |db| db.delegations_in_status(&p, "succeeded").unwrap())
            .await,
        1
    );
}

#[tokio::test]
async fn failed_parent_append_releases_claim_for_redelivery() {
    use crate::agent::child::{run_child_delegation, ChildDispatch};
    let (_td, store, sink, parent) = setup();
    let ids = ChildDispatch {
        delegation_id: "d1".into(),
        child_run_id: "child1".into(),
    };
    let m = AgentMachine::new(Policy::INTERACTIVE);
    let out = run_child_delegation(
        &store,
        &sink,
        "c1",
        &parent,
        None,
        0,
        ids,
        "{}".into(),
        m,
        FakeDriver::direct(),
        |_res: String| async { false }, // parent append fails
    )
    .await
    .unwrap();
    assert_eq!(out.event, EventKind::RunCompleted);
    // A failed append releases the claim → still undelivered (redeliverable).
    let p = parent.clone();
    let undel = store
        .call(move |db| db.undelivered_completed_delegations(&p).unwrap())
        .await;
    assert_eq!(undel.len(), 1);
}

#[tokio::test]
async fn child_depth_limit_blocks_grandchildren() {
    use crate::agent::child::{run_child_delegation, ChildDispatch, ChildError, MAX_CHILD_DEPTH};
    let (_td, store, sink, parent) = setup();
    let ids = ChildDispatch {
        delegation_id: "d1".into(),
        child_run_id: "child1".into(),
    };
    let m = AgentMachine::new(Policy::INTERACTIVE);
    let err = run_child_delegation(
        &store,
        &sink,
        "c1",
        &parent,
        None,
        MAX_CHILD_DEPTH,
        ids,
        "{}".into(),
        m,
        FakeDriver::direct(),
        |_res: String| async { true },
    )
    .await
    .unwrap_err();
    assert_eq!(err, ChildError::DepthExceeded);
    // Nothing persisted when depth-blocked: no delegation, no child run.
    let p = parent.clone();
    assert_eq!(
        store
            .call(move |db| db.delegations_in_status(&p, "queued").unwrap())
            .await,
        0
    );
    assert!(store
        .call(|db| db.get_run("child1").unwrap())
        .await
        .is_none());
}

#[tokio::test]
async fn cancelled_child_is_not_delivered() {
    use crate::agent::child::{run_child_delegation, ChildDispatch};
    let (_td, store, sink, parent) = setup();
    let mut driver = FakeDriver::direct();
    driver.info.uses_tools = true; // reach a real I/O boundary
    driver.control = Some(ControlSignal::Cancel);
    let delivered = Arc::new(Mutex::new(0u32));
    let d = delivered.clone();
    let ids = ChildDispatch {
        delegation_id: "d1".into(),
        child_run_id: "child1".into(),
    };
    let m = AgentMachine::new(Policy::INTERACTIVE);
    let out = run_child_delegation(
        &store,
        &sink,
        "c1",
        &parent,
        None,
        0,
        ids,
        "{}".into(),
        m,
        driver,
        move |_res: String| async move {
            *d.lock() += 1;
            true
        },
    )
    .await
    .unwrap();
    assert_eq!(out.event, EventKind::RunCancelled);
    // Cancelled → finished but never claimed/delivered (no parent append).
    assert_eq!(*delivered.lock(), 0);
    let p = parent.clone();
    assert_eq!(
        store
            .call(move |db| db.delegations_in_status(&p, "cancelled").unwrap())
            .await,
        1
    );
    let p = parent.clone();
    assert!(store
        .call(move |db| db.undelivered_completed_delegations(&p).unwrap())
        .await
        .is_empty());
}

#[tokio::test]
async fn comps_fan_out_delivers_each_child_once() {
    // Prove peer-comps fan-out (Task 5.4): several one-level children run
    // CONCURRENTLY, sharing the StoreHandle + delivery CAS, and each delivers its
    // single result exactly once with none left pending.
    use crate::agent::child::{run_child_delegation, ChildDispatch};
    let (_td, store, sink, parent) = setup();
    let delivered = Arc::new(Mutex::new(Vec::<String>::new()));
    let (d0, d1, d2) = (delivered.clone(), delivered.clone(), delivered.clone());
    let f0 = run_child_delegation(
        &store,
        &sink,
        "c1",
        &parent,
        Some("tc0".into()),
        0,
        ChildDispatch {
            delegation_id: "d0".into(),
            child_run_id: "child0".into(),
        },
        "{\"peer\":\"AAPL\"}".into(),
        AgentMachine::new(Policy::INTERACTIVE),
        FakeDriver::direct(),
        move |res: String| async move {
            d0.lock().push(res);
            true
        },
    );
    let f1 = run_child_delegation(
        &store,
        &sink,
        "c1",
        &parent,
        Some("tc1".into()),
        0,
        ChildDispatch {
            delegation_id: "d1".into(),
            child_run_id: "child1".into(),
        },
        "{\"peer\":\"MSFT\"}".into(),
        AgentMachine::new(Policy::INTERACTIVE),
        FakeDriver::direct(),
        move |res: String| async move {
            d1.lock().push(res);
            true
        },
    );
    let f2 = run_child_delegation(
        &store,
        &sink,
        "c1",
        &parent,
        Some("tc2".into()),
        0,
        ChildDispatch {
            delegation_id: "d2".into(),
            child_run_id: "child2".into(),
        },
        "{\"peer\":\"GOOG\"}".into(),
        AgentMachine::new(Policy::INTERACTIVE),
        FakeDriver::direct(),
        move |res: String| async move {
            d2.lock().push(res);
            true
        },
    );
    // Drive all three concurrently.
    let (a, b, c) = tokio::join!(f0, f1, f2);
    assert_eq!(a.unwrap().event, EventKind::RunCompleted);
    assert_eq!(b.unwrap().event, EventKind::RunCompleted);
    assert_eq!(c.unwrap().event, EventKind::RunCompleted);
    // Every peer child delivered its result exactly once; none left pending.
    assert_eq!(delivered.lock().len(), 3);
    let p = parent.clone();
    assert_eq!(
        store
            .call(move |db| db.delegations_in_status(&p, "succeeded").unwrap())
            .await,
        3
    );
    let p = parent.clone();
    assert!(store
        .call(move |db| db.undelivered_completed_delegations(&p).unwrap())
        .await
        .is_empty());
}

// ---- Phase 8.3: due-schedule sweep (claim → run → finalize/retry/fail) ----

#[tokio::test]
async fn due_schedules_run_and_finalize() {
    use crate::agent::scheduler::run_due_schedules;
    let (_td, store, _sink, _parent) = setup();
    // Two schedules due in the past.
    store
        .call(|db| {
            db.insert_schedule(
                "s1",
                None,
                Some("c1"),
                "UTC",
                None,
                "2020-01-01T00:00:00Z",
                "{}",
                None,
                None,
                "2020-01-01T00:00:00Z",
            )
            .unwrap();
            db.insert_schedule(
                "s2",
                None,
                Some("c1"),
                "UTC",
                None,
                "2020-01-01T00:00:00Z",
                "{}",
                None,
                None,
                "2020-01-01T00:00:00Z",
            )
            .unwrap();
        })
        .await;
    let ran = Arc::new(Mutex::new(Vec::<String>::new()));
    let r = ran.clone();
    let sweep = run_due_schedules(
        &store,
        "2020-06-01T00:00:00Z",
        "sched",
        3,
        "2020-12-01T00:00:00Z",
        move |id| {
            let r = r.clone();
            async move {
                r.lock().push(id);
                true
            }
        },
    )
    .await;
    assert_eq!(sweep.done.len(), 2);
    assert!(sweep.retried.is_empty() && sweep.failed.is_empty());
    assert_eq!(ran.lock().len(), 2);
    // Both finalized done → nothing due remains claimable.
    let none = store
        .call(|db| db.claim_due_schedule("2020-06-01T00:00:00Z", "x").unwrap())
        .await;
    assert!(none.is_none());
}

#[tokio::test]
async fn failed_schedule_retries_then_fails_terminally() {
    use crate::agent::scheduler::run_due_schedules;
    let (_td, store, _sink, _parent) = setup();
    store
        .call(|db| {
            db.insert_schedule(
                "s1",
                None,
                Some("c1"),
                "UTC",
                None,
                "2020-01-01T00:00:00Z",
                "{}",
                None,
                None,
                "2020-01-01T00:00:00Z",
            )
            .unwrap();
        })
        .await;
    // First sweep: the follow-up fails → attempt 1, retried with a future due.
    let sweep1 = run_due_schedules(
        &store,
        "2020-06-01T00:00:00Z",
        "sched",
        2,
        "2020-06-01T12:00:00Z",
        |_id| async { false },
    )
    .await;
    assert_eq!(sweep1.retried, vec!["s1".to_string()]);
    assert!(sweep1.failed.is_empty());
    let st = store
        .call(|db| db.schedule_state("s1").unwrap().unwrap())
        .await;
    assert_eq!(st.0, "pending");
    assert_eq!(st.1, 1);
    // Second sweep past the retry due: fails again → attempts 2 == max → failed.
    let sweep2 = run_due_schedules(
        &store,
        "2020-06-02T00:00:00Z",
        "sched",
        2,
        "2020-06-03T00:00:00Z",
        |_id| async { false },
    )
    .await;
    assert_eq!(sweep2.failed, vec!["s1".to_string()]);
    let st = store
        .call(|db| db.schedule_state("s1").unwrap().unwrap())
        .await;
    assert_eq!(st.0, "failed");
    assert_eq!(st.1, 2);
}

// ---- Phase 4.3: stale-approval expiry denies the parked oneshot (never hangs) ----

#[tokio::test]
async fn stale_approval_expiry_denies_parked_waiter() {
    use crate::agent::approvals::expire_and_deny_stale_approvals;
    use crate::agent::registry::ActorRegistry;
    use fm_agent::types::ApprovalResponse;
    let (_td, store, _sink, run) = setup(); // run == "r1"
                                            // A pending approval created in the past, and a driver parked on its oneshot.
    store
        .call(|db| {
            db.insert_pending(
                "p1",
                "r1",
                Some("tc1"),
                "approval",
                "{}",
                "2020-01-01T00:00:00Z",
            )
            .unwrap()
        })
        .await;
    let registry = ActorRegistry::new();
    let rx = registry.park_approval(&run);
    // The sweep (cutoff after the row) expires it AND denies the parked waiter.
    let denied = expire_and_deny_stale_approvals(
        &store,
        &registry,
        "2020-06-01T00:00:00Z",
        "2020-06-01T00:00:00Z",
    )
    .await
    .unwrap();
    assert_eq!(denied, vec!["r1".to_string()]);
    // The waiter resolves to Deny promptly — await_approval never hangs.
    assert_eq!(rx.await.unwrap(), ApprovalResponse::Deny);
    // The pending row is now expired (fail-closed), not still pending.
    let unresolved = store.call(|db| db.unresolved_pending("r1").unwrap()).await;
    assert!(unresolved.is_empty());
}
