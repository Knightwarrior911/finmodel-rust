//! Phase B actor tests: durable persist-then-broadcast, monotonic sequences,
//! one terminal event, live/replay equality, resume linkage, and crash repair.
//! Uses a scripted fake [`Driver`] and a real in-memory store + collecting sink.

use super::*;
use crate::agent::events::AgentEventEnvelope;
use crate::store::{Db, StoreHandle};
use fm_agent::machine::{AgentMachine, ToolCall};
use fm_agent::types::Risk;
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
}
impl FakeDriver {
    fn direct() -> Self {
        FakeDriver {
            info: PreparedInfo { uses_tools: false, plan_needed: false, needs_verification: false },
            model_outs: vec![],
            next_model: 0,
            verify_ok: true,
            approval: ApprovalResponse::ApproveOnce,
            memory_saved: 0,
        }
    }
}
impl Driver for FakeDriver {
    async fn prepare(&mut self) -> PreparedInfo {
        self.info
    }
    async fn make_plan(&mut self) {}
    async fn request_model(&mut self) -> ModelOut {
        let out = self.model_outs.get(self.next_model).cloned().unwrap_or(ModelOut {
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
        ToolBatchOutcome { tokens: 20, failed: vec![] }
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
    db.create_workspace("w", "W", "deal", "confidential", "", true, &now).unwrap();
    db.create_conversation("c1", "w", "t", &now).unwrap();
    db.insert_run("r1", "c1", None, None, "running", "preparing", None, None, &now).unwrap();
    let store = StoreHandle::spawn(db);
    (td, store, CollectSink::default(), "r1".to_string())
}

/// Assert the collected durable stream equals the store's replayed stream.
async fn assert_live_equals_replay(store: &StoreHandle, sink: &CollectSink, run_id: &str) {
    let live = sink.events.lock().clone();
    let run = run_id.to_string();
    let replay = store.call(move |db| db.events_after(&run, 0).unwrap()).await;
    assert_eq!(live.len(), replay.len(), "live vs replay length");
    for (l, r) in live.iter().zip(replay.iter()) {
        assert_eq!(l.sequence, Some(r.sequence), "sequence mismatch");
        assert_eq!(super::kind_str(l.event.kind), r.kind, "kind mismatch at seq {}", r.sequence);
    }
    // Strictly monotonic sequences 1..=n.
    for (i, r) in replay.iter().enumerate() {
        assert_eq!(r.sequence, (i as i64) + 1, "sequences must be dense & monotonic");
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
    driver.info = PreparedInfo { uses_tools: true, plan_needed: false, needs_verification: true };
    // First model call: one read tool; second call: final answer.
    driver.model_outs = vec![
        ModelOut { calls: vec![ro_call("q1")], final_answer: false, tokens: 30 },
        ModelOut { calls: vec![], final_answer: true, tokens: 40 },
    ];
    let m = AgentMachine::new(Policy::INTERACTIVE);
    let out = run_turn(&store, &sink, "c1", &run, m, driver).await;
    assert_eq!(out.event, EventKind::RunCompleted);
    let kinds: Vec<EventKind> = sink.events.lock().iter().map(|e| e.event.kind).collect();
    assert!(kinds.contains(&EventKind::ToolStarted));
    assert!(kinds.contains(&EventKind::ToolSucceeded));
    // ToolStarted precedes ToolSucceeded.
    let s = kinds.iter().position(|k| *k == EventKind::ToolStarted).unwrap();
    let d = kinds.iter().position(|k| *k == EventKind::ToolSucceeded).unwrap();
    assert!(s < d);
    assert_live_equals_replay(&store, &sink, &run).await;
}

#[tokio::test]
async fn approval_turn_requests_and_resolves() {
    let (_td, store, sink, run) = setup();
    let mut driver = FakeDriver::direct();
    driver.info = PreparedInfo { uses_tools: true, plan_needed: false, needs_verification: false };
    let write = ToolCall {
        tool_call_id: "w1".into(),
        name: "export_excel".into(),
        risk: Risk::Export,
        needs_approval: true,
        args_valid: true,
    };
    driver.model_outs = vec![
        ModelOut { calls: vec![write], final_answer: false, tokens: 10 },
        ModelOut { calls: vec![], final_answer: true, tokens: 10 },
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
    driver.info = PreparedInfo { uses_tools: true, plan_needed: false, needs_verification: true };
    driver.model_outs = vec![ModelOut { calls: vec![], final_answer: true, tokens: 10 }];
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
            db.append_event("e1", &r, "run_started", "{}", &now_iso()).unwrap();
            db.append_event("e2", &r, "phase_changed", "{}", &now_iso()).unwrap();
        })
        .await;
    // Restart repair.
    let repaired = store.call(|db| db.repair_interrupted_runs(&now_iso()).unwrap()).await;
    assert_eq!(repaired, 1);
    let r2 = run.clone();
    let row = store.call(move |db| db.get_run(&r2).unwrap().unwrap()).await;
    assert_eq!(row.status, "interrupted");
    // Resume returns a NEW run linked by resumed_from_run_id.
    let new_id = resume_run(&store, "c1", &run, None).await.expect("resume");
    assert_ne!(new_id, run);
    let nid = new_id.clone();
    let new_row = store.call(move |db| db.get_run(&nid).unwrap().unwrap()).await;
    assert_eq!(new_row.resumed_from_run_id.as_deref(), Some(run.as_str()));
    assert_eq!(new_row.status, "running");
    // A terminal (non-interrupted) run cannot be resumed.
    let nid2 = new_id.clone();
    store
        .call(move |db| {
            db.finish_run(&nid2, "completed", "completed", Some("end_turn"), None, &now_iso())
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
    assert_eq!(kinds.iter().filter(|k| **k == EventKind::MemoryUpdated).count(), 1);
    let mem = kinds.iter().position(|k| *k == EventKind::MemoryUpdated).unwrap();
    let term = kinds.iter().position(|k| *k == EventKind::RunCompleted).unwrap();
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
