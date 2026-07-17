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
    /// Count of rows scripted memory extraction reports as saved.
    pub memory_saved: usize,
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
            memory_saved: 0,
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
    async fn extract_memory(&mut self) -> usize {
        self.memory_saved
    }
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

/// Map an accumulated provider stream into a reducer [`ModelOut`], classifying
/// each complete tool call's risk / approval need / argument validity through
/// the [`ToolRegistry`]. This is the exact seam a live OpenRouter driver's
/// `request_model` uses: SSE → accumulator → typed reducer input. Path- and
/// confidentiality-based approval refinement happens later in the executor /
/// security layer; this classifies the base risk the reducer partitions on.
pub fn model_out_from_stream(
    registry: &ToolRegistry,
    acc: &crate::agent::provider::StreamAccumulator,
) -> ModelOut {
    let mut calls = Vec::new();
    for c in acc.complete_calls() {
        let args: Value = serde_json::from_str(&c.arguments).unwrap_or(Value::Null);
        let args_valid = registry.validate_call(&c.name, &args).is_ok();
        let spec = registry.get(&c.name);
        let risk = spec.map(|s| s.risk).unwrap_or(Risk::ReadOnly);
        // Unknown tools fail closed: the reducer already drops `!args_valid`
        // calls, but never let an unrecognized name be classified as auto-run.
        let needs_approval = spec.map(|s| !s.risk.auto_runs()).unwrap_or(true);
        calls.push(ToolCall {
            tool_call_id: c.id.clone(),
            name: c.name.clone(),
            risk,
            needs_approval,
            args_valid,
        });
    }
    let final_answer = calls.is_empty();
    let tokens = acc
        .meta
        .usage
        .as_ref()
        .and_then(|u| u.get("total_tokens"))
        .and_then(|t| t.as_u64())
        .unwrap_or(0);
    ModelOut {
        calls,
        final_answer,
        tokens,
    }
}

/// Production OpenRouter-backed driver. Reuses the proven SSE path, the
/// registry/`ToolBackend` executor seam, and [`model_out_from_stream`].
///
/// First-pass policy:
/// - `await_approval` **Deny**s (fail closed) — Export/Overwrite never auto-run.
/// - `extract_memory` returns 0 until Phase E quality gates land.
/// - Tool execution runs inside `spawn_blocking` so blocking cores cannot pin
///   a tokio worker for the duration of a finmodel call.
pub struct LiveDriver {
    app: tauri::AppHandle,
    cfg: fm_extract::LlmConfig,
    registry: ToolRegistry,
    ctx: SessionContext,
    messages: Vec<Value>,
    tools: Vec<Value>,
    pending: HashMap<String, PendingCall>,
    /// Last assistant prose (for synthesize persistence / final-answer path).
    pub last_content: String,
    started: std::time::Instant,
    deadline: std::time::Duration,
    store: crate::store::StoreHandle,
}

impl LiveDriver {
    pub fn new(
        app: tauri::AppHandle,
        store: crate::store::StoreHandle,
        cfg: fm_extract::LlmConfig,
        ctx: SessionContext,
        tools_enabled: bool,
    ) -> Self {
        let messages = crate::commands::chat::seed_agent_messages(&ctx.user_msg);
        let tools = if tools_enabled {
            crate::commands::chat::tool_schemas()
        } else {
            Vec::new()
        };
        LiveDriver {
            app,
            cfg,
            registry: ToolRegistry::builtin(),
            ctx,
            messages,
            tools,
            pending: HashMap::new(),
            last_content: String::new(),
            started: std::time::Instant::now(),
            deadline: std::time::Duration::from_secs(120),
            store,
        }
    }

    fn remaining(&self) -> std::time::Duration {
        self.deadline.saturating_sub(self.started.elapsed())
    }

    fn seed_pending_from_acc(&mut self, acc: &crate::agent::provider::StreamAccumulator) {
        self.pending.clear();
        for c in acc.complete_calls() {
            let args: Value = serde_json::from_str(&c.arguments).unwrap_or(Value::Null);
            let risk = self
                .registry
                .get(&c.name)
                .map(|s| s.risk)
                .unwrap_or(Risk::ReadOnly);
            self.pending.insert(
                c.id.clone(),
                PendingCall {
                    name: c.name.clone(),
                    args,
                    risk,
                },
            );
        }
    }

    fn append_assistant_tool_calls(&mut self, acc: &crate::agent::provider::StreamAccumulator) {
        let calls: Vec<Value> = acc
            .complete_calls()
            .into_iter()
            .map(|c| {
                serde_json::json!({
                    "id": c.id,
                    "type": "function",
                    "function": { "name": c.name, "arguments": c.arguments },
                })
            })
            .collect();
        if calls.is_empty() {
            return;
        }
        let content = if acc.content.is_empty() {
            Value::Null
        } else {
            Value::String(acc.content.clone())
        };
        self.messages.push(serde_json::json!({
            "role": "assistant",
            "content": content,
            "tool_calls": calls,
        }));
    }
}

impl Driver for LiveDriver {
    async fn prepare(&mut self) -> PreparedInfo {
        PreparedInfo {
            uses_tools: !self.tools.is_empty(),
            plan_needed: false,
            needs_verification: false,
        }
    }

    async fn make_plan(&mut self) {}

    async fn request_model(&mut self) -> ModelOut {
        if self.ctx.cancel.is_cancelled() {
            return ModelOut {
                calls: vec![],
                final_answer: true,
                tokens: 0,
            };
        }
        let remaining = self.remaining();
        if remaining.is_zero() {
            self.last_content =
                "(stopped: chat deadline elapsed — try a shorter question or another model)".into();
            return ModelOut {
                calls: vec![],
                final_answer: true,
                tokens: 0,
            };
        }

        let mut tools = self.tools.clone();
        let req = crate::commands::chat::build_chat_request(
            &self.cfg.model,
            &self.messages,
            &tools,
            true,
        );
        let app = self.app.clone();
        let conv = self.ctx.conversation_id.clone();
        let run = self.ctx.run_id.clone();
        let cfg = self.cfg.clone();
        let cancel = self.ctx.cancel.clone();

        let result = crate::commands::chat::stream_completion_for_agent(
            &app, &conv, &run, &cfg, &req, &cancel, remaining,
        )
        .await;

        match result {
            Ok(acc) => {
                self.last_content = acc.content.clone();
                self.seed_pending_from_acc(&acc);
                if !acc.complete_calls().is_empty() {
                    self.append_assistant_tool_calls(&acc);
                } else if !acc.content.trim().is_empty() {
                    self.messages.push(serde_json::json!({
                        "role": "assistant",
                        "content": acc.content,
                    }));
                }
                model_out_from_stream(&self.registry, &acc)
            }
            Err(e) if e == "tools_unsupported" && !tools.is_empty() => {
                // Drop tools and retry once as a direct answer.
                self.tools.clear();
                tools.clear();
                self.messages.push(serde_json::json!({
                    "role": "system",
                    "content": "(tools unavailable for this model — answer directly without tools)",
                }));
                let req = crate::commands::chat::build_chat_request(
                    &self.cfg.model,
                    &self.messages,
                    &tools,
                    true,
                );
                match crate::commands::chat::stream_completion_for_agent(
                    &app, &conv, &run, &cfg, &req, &cancel, self.remaining(),
                )
                .await
                {
                    Ok(acc) => {
                        self.last_content = acc.content.clone();
                        if !acc.content.trim().is_empty() {
                            self.messages.push(serde_json::json!({
                                "role": "assistant",
                                "content": acc.content,
                            }));
                        }
                        model_out_from_stream(&self.registry, &acc)
                    }
                    Err(err) => {
                        self.last_content = format!(
                            "⚠ the model request failed — check your API key and model in Settings. ({err})"
                        );
                        ModelOut {
                            calls: vec![],
                            final_answer: true,
                            tokens: 0,
                        }
                    }
                }
            }
            Err(e) => {
                self.last_content = format!(
                    "⚠ the model request failed — check your API key and model in Settings. ({e})"
                );
                ModelOut {
                    calls: vec![],
                    final_answer: true,
                    tokens: 0,
                }
            }
        }
    }

    async fn repair_tool_call(&mut self, _tool_call_id: &str) -> ModelOut {
        // One repair = another provider round with the existing message history
        // (validation errors were already fed back as tool results).
        self.request_model().await
    }

    async fn schedule_tools(&mut self, batch: &[String]) -> u64 {
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

            let app = self.app.clone();
            let ctx = self.ctx.clone();
            let calls_owned = calls.clone();

            let (tokens, wave_results) = tokio::task::spawn_blocking(move || {
                let backend = crate::commands::chat::ChatToolBackend { app: &app };
                let registry = ToolRegistry::builtin();
                let mut results = HashMap::new();
                let tokens = execute_batch(&registry, &backend, &calls_owned, &ctx, &mut results);
                (tokens, results)
            })
            .await
            .unwrap_or_else(|_| (0, HashMap::new()));

            total = total.saturating_add(tokens);
            for (id, res) in wave_results {
                let content = match res {
                    Ok(env) => env.summary,
                    Err(e) => format!("Tool error: {e}"),
                };
                self.messages.push(serde_json::json!({
                    "role": "tool",
                    "tool_call_id": id,
                    "content": content,
                }));
            }
        }
        total
    }

    async fn synthesize(&mut self) {
        // Persist the final assistant text as an ordered message part so
        // snapshots reload the same prose the live turn produced.
        let content = self.last_content.clone();
        if content.trim().is_empty() {
            return;
        }
        let conv = self.ctx.conversation_id.clone();
        let store = self.store.clone();
        store
            .call(move |db| {
                let msg_id = {
                    let mut b = [0u8; 16];
                    rand::Rng::fill(&mut rand::thread_rng(), &mut b);
                    fm_agent::ids::format_uuid_v4(b)
                };
                let part_id = {
                    let mut b = [0u8; 16];
                    rand::Rng::fill(&mut rand::thread_rng(), &mut b);
                    fm_agent::ids::format_uuid_v4(b)
                };
                let now = crate::store::now_iso();
                let _ = db.insert_message(
                    &msg_id,
                    &conv,
                    None,
                    "assistant",
                    None,
                    "complete",
                    &now,
                );
                let payload = serde_json::json!({ "text": content }).to_string();
                let _ = db.insert_part(&part_id, &msg_id, 0, "text", &payload, Some(&content));
                let _ = db.set_active_leaf(&conv, &msg_id, &now);
            })
            .await;
    }

    async fn verify(&mut self) -> bool {
        true
    }

    async fn extract_memory(&mut self) -> usize {
        // Manual-memory-only until Phase E quality gates pass.
        0
    }

    async fn await_approval(&mut self, _tool_call_id: &str) -> ApprovalResponse {
        // Fail closed: first-pass has no UI parking / agent_approve yet.
        // Auto-run tools never reach this path; anything that does is Deny.
        ApprovalResponse::Deny
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

    #[tokio::test]
    async fn dcf_export_requests_approval_then_completes() {
        let (_td, store, sink, run) = setup();
        let backend = FakeBackend::new().seed_ok(
            "build_model",
            "built MSFT",
            json!({
                "type":"model",
                "ticker":"MSFT",
                "artifact_id":"art-0123456789abcdef0123456789abcdef",
                "label":"MSFT DCF"
            }),
        );
        let ctx = SessionContext::test_ctx("c1", "DCF MSFT then export");
        let mut driver = ScriptedDriver::new(backend, ctx);
        driver.seed_pending(
            "bm",
            "build_model",
            json!({"ticker":"MSFT"}),
            Risk::LocalCreate,
        );
        // Export outside output root requires approval (Export risk).
        driver.seed_pending(
            "ex",
            "build_model",
            json!({"ticker":"MSFT"}),
            Risk::Export,
        );
        driver.info.plan_needed = false;
        driver.info.needs_verification = true;
        driver.approval = ApprovalResponse::ApproveOnce;
        let export = ToolCall {
            tool_call_id: "ex".into(),
            name: "build_model".into(),
            risk: Risk::Export,
            needs_approval: true,
            args_valid: true,
        };
        driver.model_outs = vec![
            ModelOut {
                calls: vec![ro_call("bm", "build_model")],
                final_answer: false,
                tokens: 20,
            },
            ModelOut {
                calls: vec![export],
                final_answer: false,
                tokens: 10,
            },
            ModelOut {
                calls: vec![],
                final_answer: true,
                tokens: 10,
            },
        ];
        let m = AgentMachine::new(Policy::WORKFLOW);
        let out = run_turn(&store, &sink, "c1", &run, m, driver).await;
        assert_eq!(out.event, EventKind::RunCompleted);
        let kinds: Vec<EventKind> = sink.events.lock().iter().map(|e| e.event.kind).collect();
        assert!(kinds.contains(&EventKind::ApprovalRequested));
        assert!(kinds.contains(&EventKind::ApprovalResolved));
        // Approval must precede the export tool start.
        let apr = kinds.iter().position(|k| *k == EventKind::ApprovalRequested).unwrap();
        let starts: Vec<(usize, String)> = sink
            .events
            .lock()
            .iter()
            .enumerate()
            .filter(|(_, e)| e.event.kind == EventKind::ToolStarted)
            .filter_map(|(i, e)| Some((i, e.event.payload.get("tool_call_id")?.as_str()?.to_string())))
            .collect();
        let ex_start = starts.iter().find(|(_, id)| id == "ex").unwrap().0;
        assert!(apr < ex_start, "approval before export execution");
    }

    #[test]
    fn stream_content_only_is_final_answer() {
        let reg = ToolRegistry::builtin();
        let acc = crate::agent::provider::accumulate(&[
            r#"{"choices":[{"delta":{"content":"The margin expanded."}}]}"#,
            r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#,
            r#"{"choices":[],"usage":{"total_tokens":42}}"#,
            "[DONE]",
        ]);
        let out = model_out_from_stream(&reg, &acc);
        assert!(out.final_answer);
        assert!(out.calls.is_empty());
        assert_eq!(out.tokens, 42);
    }

    #[test]
    fn stream_parallel_reads_map_to_readonly_autorun_calls() {
        let reg = ToolRegistry::builtin();
        let acc = crate::agent::provider::accumulate(&[
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"q","function":{"name":"get_quote","arguments":"{\"ticker\":\"NVDA\"}"}}]}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":1,"id":"f","function":{"name":"list_filings","arguments":"{\"ticker\":\"NVDA\"}"}}]}}]}"#,
            r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#,
            "[DONE]",
        ]);
        let out = model_out_from_stream(&reg, &acc);
        assert!(!out.final_answer);
        assert_eq!(out.calls.len(), 2);
        for c in &out.calls {
            assert_eq!(c.risk, Risk::ReadOnly);
            assert!(!c.needs_approval);
            assert!(c.args_valid);
        }
    }

    #[test]
    fn stream_build_model_is_local_create_autorun() {
        let reg = ToolRegistry::builtin();
        let acc = crate::agent::provider::accumulate(&[
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"b","function":{"name":"build_model","arguments":"{\"ticker\":\"MSFT\"}"}}]}}]}"#,
            "[DONE]",
        ]);
        let out = model_out_from_stream(&reg, &acc);
        assert_eq!(out.calls.len(), 1);
        assert_eq!(out.calls[0].risk, Risk::LocalCreate);
        assert!(!out.calls[0].needs_approval); // new immutable version auto-runs
        assert!(out.calls[0].args_valid);
    }

    #[test]
    fn stream_invalid_args_and_unknown_tool_flag_args_invalid() {
        let reg = ToolRegistry::builtin();
        let acc = crate::agent::provider::accumulate(&[
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"x","function":{"name":"get_quote","arguments":"{\"ticker\":\"not a ticker!!\"}"}}]}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":1,"id":"y","function":{"name":"frobnicate","arguments":"{}"}}]}}]}"#,
            "[DONE]",
        ]);
        let out = model_out_from_stream(&reg, &acc);
        assert_eq!(out.calls.len(), 2);
        assert!(!out.calls[0].args_valid, "bad ticker fails semantic validation");
        assert!(!out.calls[1].args_valid, "unknown tool has no spec");
        assert_eq!(out.calls[1].risk, Risk::ReadOnly); // unknown defaults to read-only
        assert!(out.calls[1].needs_approval, "unknown tool fails closed (never auto-run)");
    }

    #[tokio::test]
    async fn earnings_golden_fixture_end_to_end() {
        // Plan the golden earnings_review workflow against the real registry.
        let reg = ToolRegistry::builtin();
        let plan = crate::agent::workflows::plan_workflow(
            "earnings_review",
            &json!({"ticker":"NVDA"}),
            &reg,
        )
        .unwrap();
        assert!(plan.needs_verification, "earnings is a numeric-finance turn");
        for t in ["list_filings", "read_filing", "get_news", "get_quote"] {
            assert!(
                plan.steps.iter().any(|s| s.tool_name == t),
                "missing required step {t}"
            );
        }

        // Drive the earnings sequence through the scripted driver + fake backend.
        let (_td, store, sink, run) = setup();
        let backend = FakeBackend::new()
            .seed_ok(
                "list_filings",
                "NVDA 10-K + 10-Q",
                json!({
                    "type":"filings","ticker":"NVDA",
                    "url":"https://www.sec.gov/cgi-bin/browse-edgar?action=getcompany&CIK=0001045810"
                }),
            )
            .seed_ok(
                "read_filing",
                "Item 7 MD&A: revenue $60,922M FY2024",
                json!({
                    "type":"filing_doc","ticker":"NVDA","form":"10-K","filing_date":"2024-02-21",
                    "url":"https://www.sec.gov/Archives/edgar/data/1045810/nvda-10k.htm"
                }),
            )
            .seed_ok("get_news", "Guidance raised for Q1", json!({"type":"news","query":"NVDA guidance"}))
            .seed_ok("get_quote", "NVDA 788.17 USD", quote_card("NVDA", 788.17));

        let ctx = SessionContext::test_ctx("c1", "NVDA earnings review beat/miss + guidance");
        let mut driver = ScriptedDriver::new(backend, ctx);
        let results = driver.results.clone();
        driver.seed_pending("lf", "list_filings", json!({"ticker":"NVDA"}), Risk::ReadOnly);
        driver.seed_pending("rf", "read_filing", json!({"ticker":"NVDA","item":"7"}), Risk::ReadOnly);
        driver.seed_pending("nw", "get_news", json!({"query":"NVDA guidance"}), Risk::ReadOnly);
        driver.seed_pending("qt", "get_quote", json!({"ticker":"NVDA"}), Risk::ReadOnly);
        // R1: two independent reads. R2: dependent read_filing + news. R3: final.
        driver.model_outs = vec![
            ModelOut {
                calls: vec![ro_call("lf", "list_filings"), ro_call("qt", "get_quote")],
                final_answer: false,
                tokens: 40,
            },
            ModelOut {
                calls: vec![ro_call("rf", "read_filing"), ro_call("nw", "get_news")],
                final_answer: false,
                tokens: 60,
            },
            ModelOut { calls: vec![], final_answer: true, tokens: 30 },
        ];
        driver.verify_ok = true;

        let m = AgentMachine::new(Policy::WORKFLOW);
        let out = run_turn(&store, &sink, "c1", &run, m, driver).await;
        assert_eq!(out.event, EventKind::RunCompleted);
        assert!(!out.partial, "verified earnings answer is not partial");

        let kinds: Vec<EventKind> = sink.events.lock().iter().map(|e| e.event.kind).collect();
        assert!(kinds.contains(&EventKind::AssistantCheckpoint));
        assert_eq!(*kinds.last().unwrap(), EventKind::RunCompleted);

        // Every required tool executed and the filing promoted a sec.gov source.
        let results = results.lock();
        for id in ["lf", "rf", "nw", "qt"] {
            assert!(results.get(id).unwrap().is_ok(), "tool {id} failed");
        }
        assert!(
            results
                .get("rf")
                .unwrap()
                .as_ref()
                .unwrap()
                .sources
                .iter()
                .any(|s| s.canonical_uri.contains("sec.gov")),
            "filing source promoted to ledger"
        );
    }

    #[tokio::test]
    async fn trading_comps_golden_fixture_end_to_end() {
        // Plan the golden trading_comps workflow against the real registry.
        let reg = ToolRegistry::builtin();
        let plan = crate::agent::workflows::plan_workflow(
            "trading_comps",
            &json!({"tickers":["NVDA","AMD"]}),
            &reg,
        )
        .unwrap();
        assert!(plan.needs_verification, "comps is a numeric-finance turn");
        for t in ["benchmark_peers", "get_quote", "list_filings"] {
            assert!(
                plan.steps.iter().any(|s| s.tool_name == t),
                "missing required step {t}"
            );
        }

        // Drive the comps sequence through the scripted driver + fake backend.
        let (_td, store, sink, run) = setup();
        let backend = FakeBackend::new()
            .seed_ok(
                "benchmark_peers",
                "NVDA vs AMD: EV/EBITDA 34.2x vs 28.9x",
                json!({"type":"benchmark","tickers":["NVDA","AMD"]}),
            )
            .seed_ok("get_quote", "NVDA 788.17 USD", quote_card("NVDA", 788.17))
            .seed_ok(
                "list_filings",
                "NVDA 10-K index",
                json!({
                    "type":"filings","ticker":"NVDA",
                    "url":"https://www.sec.gov/cgi-bin/browse-edgar?action=getcompany&CIK=0001045810"
                }),
            );

        let ctx = SessionContext::test_ctx("c1", "comps NVDA vs AMD EV/EBITDA");
        let mut driver = ScriptedDriver::new(backend, ctx);
        let results = driver.results.clone();
        driver.seed_pending("bp", "benchmark_peers", json!({"tickers":["NVDA","AMD"]}), Risk::ReadOnly);
        driver.seed_pending("qt", "get_quote", json!({"ticker":"NVDA"}), Risk::ReadOnly);
        driver.seed_pending("lf", "list_filings", json!({"ticker":"NVDA"}), Risk::ReadOnly);
        // R1: two independent reads. R2: dependent peer-pool benchmark. R3: final.
        driver.model_outs = vec![
            ModelOut {
                calls: vec![ro_call("qt", "get_quote"), ro_call("lf", "list_filings")],
                final_answer: false,
                tokens: 40,
            },
            ModelOut {
                calls: vec![ro_call("bp", "benchmark_peers")],
                final_answer: false,
                tokens: 50,
            },
            ModelOut { calls: vec![], final_answer: true, tokens: 30 },
        ];
        driver.verify_ok = true;

        let m = AgentMachine::new(Policy::WORKFLOW);
        let out = run_turn(&store, &sink, "c1", &run, m, driver).await;
        assert_eq!(out.event, EventKind::RunCompleted);
        assert!(!out.partial, "verified comps answer is not partial");

        let kinds: Vec<EventKind> = sink.events.lock().iter().map(|e| e.event.kind).collect();
        assert!(kinds.contains(&EventKind::AssistantCheckpoint));
        assert_eq!(*kinds.last().unwrap(), EventKind::RunCompleted);

        // Every required comps tool executed.
        let results = results.lock();
        for id in ["bp", "qt", "lf"] {
            assert!(results.get(id).unwrap().is_ok(), "tool {id} failed");
        }
    }
}
