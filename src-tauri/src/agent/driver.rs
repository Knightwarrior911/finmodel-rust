//! Scripted / real-agent driver scaffolding for Phase C.
//!
//! [`ScriptedDriver`] drives [`crate::agent::actor::run_turn`] with a canned
//! provider transcript and a [`ToolBackend`]. It is the acceptance vehicle for
//! "two parallel reads → dependent research → synthesis → verify → terminal"
//! without a live OpenRouter key. A production OpenRouter driver will reuse the
//! same pending-call + executor seam.

use std::collections::HashMap;
use std::sync::Arc;

use fm_agent::machine::ToolCall;
use fm_agent::types::{ApprovalResponse, Risk, ToolResultEnvelope};
use parking_lot::Mutex;
use serde_json::Value;

use crate::agent::actor::{Driver, ModelOut, PreparedInfo};
use crate::agent::executors::{execute_batch, ExecuteError, SessionContext, ToolBackend};
use crate::agent::scheduler::{plan_batches, PlannedCall};
use crate::agent::tools::ToolRegistry;

/// A model-requested call waiting for `schedule_tools`.
#[derive(Clone, Debug)]
pub struct PendingCall {
    pub name: String,
    pub args: Value,
    pub risk: Risk,
}

/// Scripted provider + registry-backed tool execution.
pub struct ScriptedDriver<B: ToolBackend> {
    pub info: PreparedInfo,
    pub model_outs: Vec<ModelOut>,
    next_model: usize,
    pub verify_ok: bool,
    pub approval: ApprovalResponse,
    registry: ToolRegistry,
    backend: B,
    ctx: SessionContext,
    /// tool_call_id → pending args (populated when the scripted model "calls").
    pending: HashMap<String, PendingCall>,
    /// tool_call_id → last execution result (for assertions).
    pub results: Arc<Mutex<HashMap<String, Result<ToolResultEnvelope, ExecuteError>>>>,
    /// Observed schedule batches (for parallel-read assertions).
    pub batches: Arc<Mutex<Vec<Vec<String>>>>,
}

impl<B: ToolBackend> ScriptedDriver<B> {
    pub fn new(backend: B, ctx: SessionContext) -> Self {
        ScriptedDriver {
            info: PreparedInfo {
                uses_tools: true,
                plan_needed: true,
                needs_verification: true,
            },
            model_outs: Vec::new(),
            next_model: 0,
            verify_ok: true,
            approval: ApprovalResponse::ApproveOnce,
            registry: ToolRegistry::builtin(),
            backend,
            ctx,
            pending: HashMap::new(),
            results: Arc::new(Mutex::new(HashMap::new())),
            batches: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Register the args that accompany a scripted [`ToolCall`] id.
    pub fn seed_pending(&mut self, id: &str, name: &str, args: Value, risk: Risk) {
        self.pending.insert(
            id.into(),
            PendingCall {
                name: name.into(),
                args,
                risk,
            },
        );
    }

    fn take_model(&mut self) -> ModelOut {
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
}

impl<B: ToolBackend> Driver for ScriptedDriver<B> {
    async fn prepare(&mut self) -> PreparedInfo {
        self.info
    }
    async fn make_plan(&mut self) {}
    async fn request_model(&mut self) -> ModelOut {
        self.take_model()
    }
    async fn repair_tool_call(&mut self, _tool_call_id: &str) -> ModelOut {
        self.take_model()
    }
    async fn schedule_tools(&mut self, batch: &[String]) -> u64 {
        self.batches.lock().push(batch.to_vec());

        // Re-plan with the scheduler so write/dependent calls serialize even if
        // the reducer handed us a flat id list.
        let planned: Vec<PlannedCall> = batch
            .iter()
            .filter_map(|id| {
                let p = self.pending.get(id)?;
                Some(PlannedCall {
                    tool_call_id: id.clone(),
                    risk: p.risk,
                    depends_on: vec![],
                })
            })
            .collect();
        let waves = if planned.is_empty() {
            vec![batch.to_vec()]
        } else {
            plan_batches(&planned)
        };

        let mut total = 0u64;
        for wave in waves {
            let calls: Vec<(String, String, Value)> = wave
                .iter()
                .filter_map(|id| {
                    let p = self.pending.get(id)?;
                    Some((id.clone(), p.name.clone(), p.args.clone()))
                })
                .collect();
            let mut wave_results = HashMap::new();
            total = total.saturating_add(execute_batch(
                &self.registry,
                &self.backend,
                &calls,
                &self.ctx,
                &mut wave_results,
            ));
            self.results.lock().extend(wave_results);
        }
        total
    }
    async fn synthesize(&mut self) {}
    async fn verify(&mut self) -> bool {
        self.verify_ok
    }
    async fn extract_memory(&mut self) {}
    async fn await_approval(&mut self, _tool_call_id: &str) -> ApprovalResponse {
        self.approval
    }
}

/// Helper to build a read-only [`ToolCall`].
pub fn ro_call(id: &str, name: &str) -> ToolCall {
    ToolCall {
        tool_call_id: id.into(),
        name: name.into(),
        risk: Risk::ReadOnly,
        needs_approval: false,
        args_valid: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::actor::run_turn;
    use crate::agent::events::{AgentEventEnvelope, EventSink};
    use crate::agent::executors::{quote_card, FakeBackend, SessionContext};
    use crate::store::{now_iso, Db, StoreHandle};
    use fm_agent::machine::AgentMachine;
    use fm_agent::types::EventKind;
    use fm_agent::Policy;
    use serde_json::json;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct TempDir(PathBuf);
    impl TempDir {
        fn new(tag: &str) -> Self {
            static N: AtomicU64 = AtomicU64::new(0);
            let n = N.fetch_add(1, Ordering::Relaxed);
            let p = std::env::temp_dir().join(format!(
                "fmdrv-{tag}-{}-{}",
                std::process::id(),
                n
            ));
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

    #[derive(Clone, Default)]
    struct CollectSink {
        events: Arc<Mutex<Vec<AgentEventEnvelope>>>,
    }
    impl EventSink for CollectSink {
        fn emit(&self, env: &AgentEventEnvelope) {
            self.events.lock().push(env.clone());
        }
    }

    fn setup() -> (TempDir, StoreHandle, CollectSink, String) {
        let td = TempDir::new("drv");
        let db = Db::open_in_memory(&td.0.join("blobs")).unwrap();
        let now = now_iso();
        db.create_workspace("w", "W", "deal", "standard", "", true, &now)
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
        (td, store, CollectSink::default(), "r1".into())
    }

    #[tokio::test]
    async fn parallel_reads_then_research_verify_terminal_order() {
        let (_td, store, sink, run) = setup();
        let backend = FakeBackend::new()
            .seed_ok("get_quote", "NVDA 120", quote_card("NVDA", 120.0))
            .seed_ok(
                "list_filings",
                "filings",
                json!({
                    "type":"filings",
                    "ticker":"NVDA",
                    "url":"https://www.sec.gov/cgi-bin/browse-edgar?action=getcompany&CIK=0001045810"
                }),
            )
            .seed_ok(
                "research",
                "earnings digest",
                json!({"type":"research_answer","answer":{"summary":{"text":"beat"}}}),
            );

        let ctx = SessionContext::test_ctx("c1", "NVDA earnings review");
        let mut driver = ScriptedDriver::new(backend, ctx);
        let results = driver.results.clone();
        let batches = driver.batches.clone();
        driver.seed_pending("q1", "get_quote", json!({"ticker":"NVDA"}), Risk::ReadOnly);
        driver.seed_pending(
            "f1",
            "list_filings",
            json!({"ticker":"NVDA"}),
            Risk::ReadOnly,
        );
        driver.seed_pending(
            "r1call",
            "research",
            json!({"query":"NVDA earnings beat miss guidance"}),
            Risk::ReadOnly,
        );
        // Round 1: two independent reads (one batch). Round 2: research.
        // Round 3: final answer.
        driver.model_outs = vec![
            ModelOut {
                calls: vec![ro_call("q1", "get_quote"), ro_call("f1", "list_filings")],
                final_answer: false,
                tokens: 40,
            },
            ModelOut {
                calls: vec![ro_call("r1call", "research")],
                final_answer: false,
                tokens: 60,
            },
            ModelOut {
                calls: vec![],
                final_answer: true,
                tokens: 30,
            },
        ];

        let m = AgentMachine::new(Policy::INTERACTIVE);
        let out = run_turn(&store, &sink, "c1", &run, m, driver).await;
        assert_eq!(out.event, EventKind::RunCompleted);
        assert!(!out.partial);

        let kinds: Vec<EventKind> = sink.events.lock().iter().map(|e| e.event.kind).collect();
        assert!(kinds.contains(&EventKind::PlanUpdated));
        assert!(kinds.contains(&EventKind::ToolStarted));
        assert!(kinds.contains(&EventKind::ToolSucceeded));
        assert!(kinds.contains(&EventKind::AssistantCheckpoint));
        assert_eq!(*kinds.last().unwrap(), EventKind::RunCompleted);

        let starts: Vec<String> = sink
            .events
            .lock()
            .iter()
            .filter(|e| e.event.kind == EventKind::ToolStarted)
            .filter_map(|e| {
                e.event
                    .payload
                    .get("tool_call_id")?
                    .as_str()
                    .map(|s| s.to_string())
            })
            .collect();
        assert!(starts.contains(&"q1".to_string()));
        assert!(starts.contains(&"f1".to_string()));
        assert!(starts.contains(&"r1call".to_string()));
        // First schedule batch is the two parallel reads.
        let batches = batches.lock();
        assert!(batches.len() >= 2);
        assert_eq!(batches[0].len(), 2);
        assert!(batches[0].contains(&"q1".to_string()) && batches[0].contains(&"f1".to_string()));
        assert_eq!(batches[1], vec!["r1call".to_string()]);

        let results = results.lock();
        assert!(results.get("q1").unwrap().is_ok());
        assert!(results.get("f1").unwrap().is_ok());
        assert!(results.get("r1call").unwrap().is_ok());
        assert!(results
            .get("f1")
            .unwrap()
            .as_ref()
            .unwrap()
            .sources
            .iter()
            .any(|s| s.canonical_uri.contains("sec.gov")));
    }

    #[tokio::test]
    async fn scripted_driver_records_single_parallel_batch() {
        let (_td, store, sink, run) = setup();
        let backend = FakeBackend::new()
            .seed_ok("get_quote", "AAPL 190", quote_card("AAPL", 190.0))
            .seed_ok("get_news", "headlines", json!({"type":"news"}));
        let ctx = SessionContext::test_ctx("c1", "AAPL quote + news");
        let mut driver = ScriptedDriver::new(backend, ctx);
        let results = driver.results.clone();
        let batches = driver.batches.clone();
        driver.seed_pending("a", "get_quote", json!({"ticker":"AAPL"}), Risk::ReadOnly);
        driver.seed_pending("b", "get_news", json!({"query":"AAPL"}), Risk::ReadOnly);
        driver.info.plan_needed = false;
        driver.info.needs_verification = false;
        driver.model_outs = vec![
            ModelOut {
                calls: vec![ro_call("a", "get_quote"), ro_call("b", "get_news")],
                final_answer: false,
                tokens: 20,
            },
            ModelOut {
                calls: vec![],
                final_answer: true,
                tokens: 10,
            },
        ];
        let m = AgentMachine::new(Policy::INTERACTIVE);
        let out = run_turn(&store, &sink, "c1", &run, m, driver).await;
        assert_eq!(out.event, EventKind::RunCompleted);

        let batches = batches.lock();
        assert_eq!(batches.len(), 1, "two reads → one schedule batch");
        assert_eq!(batches[0].len(), 2);

        let results = results.lock();
        assert!(results
            .get("a")
            .unwrap()
            .as_ref()
            .unwrap()
            .summary
            .contains("AAPL"));
        assert!(results.get("b").unwrap().is_ok());
    }
}
