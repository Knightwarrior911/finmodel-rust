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
use fm_agent::types::{ApprovalResponse, Plan, PlanStep, PlanStepStatus, Risk, ToolResultEnvelope};
use parking_lot::Mutex;
use serde_json::Value;

use crate::agent::actor::{
    ControlSignal, Driver, ModelOut, PreparedInfo, ToolBatchOutcome, ToolCallMeta,
};
use crate::agent::executors::{execute_batch, ExecuteError, SessionContext, ToolBackend};
use crate::agent::scheduler::{plan_batches, PlannedCall};
use crate::agent::tools::ToolRegistry;

/// Common English stopwords excluded from recall terms; kept small and
/// finance-neutral so OR-joined recall queries don't match every stored note.
const STOPWORDS: &[&str] = &[
    "the", "and", "for", "that", "this", "with", "you", "your", "are", "was", "were", "will",
    "would", "can", "could", "should", "have", "has", "had", "not", "but", "from", "what", "why",
    "how", "when", "where", "who", "which", "did", "does", "get", "got", "let", "our", "out",
    "its", "about", "into", "over", "than", "then", "them", "they", "any", "all", "please",
];

fn is_question_word(w: &str) -> bool {
    matches!(
        w,
        "what"
            | "why"
            | "how"
            | "when"
            | "where"
            | "who"
            | "which"
            | "did"
            | "do"
            | "does"
            | "is"
            | "are"
            | "can"
            | "could"
            | "should"
            | "would"
            | "will"
    )
}

/// Parse an explicit "remember: X" manual-save directive from a user message.
/// Returns the memory content, or None when the turn is not a save (including
/// question forms like "remember what I said?" — those are recall requests).
/// Manual capture only — automatic LLM extraction stays off until the Phase E
/// quality gate lands (decision 4).
pub(crate) fn parse_memory_directive(msg: &str) -> Option<String> {
    let t = msg.trim();
    let lower = t.to_lowercase();
    const PREFIXES: &[&str] = &[
        "remember that ",
        "remember this: ",
        "remember: ",
        "remember ",
        "note to self: ",
        "note to self ",
        "note that ",
        "note: ",
        "save to memory: ",
        "save to memory ",
        "my preference is ",
        "for future reference, ",
        "for future reference: ",
    ];
    for p in PREFIXES {
        if !lower.starts_with(p) {
            continue;
        }
        let content = t[p.len()..].trim().trim_end_matches(['.', '!', ' ']).trim();
        let n = content.chars().count();
        if !(2..=2000).contains(&n) {
            return None;
        }
        // Reject questions: "remember what I said?" is a recall request, not a
        // save. Guard on a trailing '?' and a leading question word.
        if content.trim_end().ends_with('?') {
            return None;
        }
        let first = content
            .split(|c: char| !c.is_alphanumeric())
            .find(|w| !w.is_empty())
            .unwrap_or("")
            .to_lowercase();
        if is_question_word(&first) {
            return None;
        }
        return Some(content.to_string());
    }
    None
}

/// Build a safe FTS5 MATCH query from free user text: alphanumeric non-stopword
/// tokens of length >= 3, lowercased, deduped, quoted, OR-joined. None when no
/// usable token exists — skip recall rather than risk an FTS5 syntax error on
/// raw punctuation (`?`, `'`, ...) or match every note on filler words.
pub(crate) fn fts_query(msg: &str) -> Option<String> {
    let mut seen = std::collections::HashSet::new();
    let mut terms: Vec<String> = Vec::new();
    for raw in msg.split(|c: char| !c.is_alphanumeric()) {
        let w = raw.to_lowercase();
        if w.chars().count() < 3 || STOPWORDS.contains(&w.as_str()) || !seen.insert(w.clone()) {
            continue;
        }
        terms.push(format!("\"{w}\""));
        if terms.len() >= 12 {
            break;
        }
    }
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" OR "))
    }
}

/// A model-requested call waiting for `schedule_tools`.
#[derive(Clone, Debug)]
pub struct PendingCall {
    pub name: String,
    pub args: Value,
    pub risk: Risk,
}

/// Build a [`ToolCallMeta`] from a pending call (canonical args + snake_case
/// risk), for the pre-execution `tool_invocations` row. `None` → an empty meta.
fn call_meta_from_pending(p: Option<&PendingCall>) -> ToolCallMeta {
    match p {
        Some(p) => ToolCallMeta {
            name: p.name.clone(),
            risk: serde_json::to_value(p.risk)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_default(),
            canonical_args_json: serde_json::to_string(&p.args).ok(),
            idempotency_hash: None,
        },
        None => ToolCallMeta::default(),
    }
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
    /// Elapsed ms reported to the pump clock (tests set this to trip the deadline).
    pub elapsed_ms: u64,
    /// Control signal the pump observes at boundaries (tests set cancel/interrupt).
    pub control: Option<ControlSignal>,
}

impl<B: ToolBackend> ScriptedDriver<B> {
    pub fn new(backend: B, ctx: SessionContext) -> Self {
        ScriptedDriver {
            info: PreparedInfo {
                uses_tools: true,
                plan_needed: true,
                needs_verification: true,
                workflow: None,
                escalation: None,
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
            elapsed_ms: 0,
            control: None,
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
        self.info.clone()
    }
    async fn make_plan(&mut self) -> Plan {
        Plan {
            objective: "scripted plan".into(),
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
        self.take_model()
    }
    async fn repair_tool_call(&mut self, _tool_call_id: &str) -> ModelOut {
        self.take_model()
    }
    async fn schedule_tools(&mut self, batch: &[String]) -> ToolBatchOutcome {
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
        let mut failed: Vec<String> = Vec::new();
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
            for (id, res) in &wave_results {
                if res.is_err() {
                    failed.push(id.clone());
                }
            }
            self.results.lock().extend(wave_results);
        }
        ToolBatchOutcome {
            tokens: total,
            failed,
            sources: Vec::new(),
            artifacts: Vec::new(),
            parts: Vec::new(),
        }
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
    fn elapsed_ms(&self) -> u64 {
        self.elapsed_ms
    }
    fn control_signal(&self) -> Option<ControlSignal> {
        self.control
    }
    fn call_meta(&self, tool_call_id: &str) -> ToolCallMeta {
        call_meta_from_pending(self.pending.get(tool_call_id))
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

/// Refine a write tool's risk from its real target path at proposal time, so an
/// overwrite (existing file) or export (outside the output root) routes through
/// approval (Task 4.3) instead of auto-running. `build_model` is currently the
/// only write tool; its output is `{out_dir}/{stem}_model.xlsx`. Returns None
/// when the risk can't be refined (no out_dir / non-write tool / missing args),
/// and the caller keeps the tool's base risk.
fn refine_write_risk(name: &str, args: &Value, out_dir: Option<&std::path::Path>) -> Option<Risk> {
    let root = out_dir?;
    match name {
        "build_model" => {
            let ticker = args.get("ticker")?.as_str()?;
            let target = root.join(crate::commands::model::model_filename(ticker));
            Some(crate::agent::security::classify_write_risk(root, &target))
        }
        _ => None,
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
    out_dir: Option<&std::path::Path>,
) -> ModelOut {
    let mut calls = Vec::new();
    for c in acc.complete_calls() {
        let args: Value = serde_json::from_str(&c.arguments).unwrap_or(Value::Null);
        let args_valid = registry.validate_call(&c.name, &args).is_ok();
        let spec = registry.get(&c.name);
        let base = spec.map(|s| s.risk).unwrap_or(Risk::ReadOnly);
        // Refine write-risk from the real target at proposal time so an
        // overwrite/export routes through approval (Task 4.3). Unknown tools fail
        // closed: the reducer drops `!args_valid` calls, and an unrecognized name
        // never auto-runs.
        let risk = refine_write_risk(&c.name, &args, out_dir).unwrap_or(base);
        let needs_approval = spec.map(|_| !risk.auto_runs()).unwrap_or(true);
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
    /// Tool result cards produced this turn, in execution order — persisted as
    /// durable `result` parts and mirrored live on the transitional
    /// `chat_tool` channel the existing card renderer consumes.
    turn_results: Vec<(String, Value)>,
    /// Active-run registry, used to park approvals awaiting `agent_approve`.
    registry_hub: crate::agent::registry::ActorRegistry,
    /// No-key turns: whether the FallbackDispatcher decision was already issued.
    fallback_served: bool,
    /// The workflow id selected in `prepare`, so `make_plan` can build the
    /// workflow-grounded plan from its template (Task 3.2).
    selected_workflow: Option<String>,
    /// Material numeric claims extracted from this run's tool results, verified in
    /// `verify()` against their source value before the run is badged (Task 4.2).
    run_claims: Vec<fm_agent::types::Claim>,
    /// The verification card produced by `verify()`, taken by the actor and
    /// emitted as a durable `ResultPartAdded` (Task 2.1 single render path).
    verify_card: Option<Value>,
}

/// Authoritative value for a run claim in the current verification slice (Task
/// 4.2): the claim's own source-recorded figure, parsed to a number. EPS-style
/// per-share values use the rounded-currency tolerance; other exact figures use
/// exact-quantity. An unparseable value returns `None` (→ `Unverified`; missing
/// evidence never certifies). Free fn so the verify loop is testable without an
/// `AppHandle`. Genuine recompute (fm-value metrics) is the documented next step.
fn claim_authoritative(
    claim: &fm_agent::types::Claim,
) -> Option<(f64, crate::agent::verification::MetricClass, u32)> {
    use crate::agent::verification::MetricClass;
    claim.normalized_value.parse::<f64>().ok().map(|v| {
        let class = if claim.unit.contains("/shares") {
            MetricClass::RoundedCurrency
        } else {
            MetricClass::ExactQuantity
        };
        (v, class, 0)
    })
}

/// Independent authoritative value for a claim (Task 4.2/4.4). For a derivable
/// metric it recomputes from sibling claims via an accounting identity — today
/// `gross_profit == revenue - cost_of_revenue` — so a restated/inconsistent
/// figure fails verification instead of certifying itself. `values` maps each
/// run claim_key (`{ticker}.{metric}.{period}`) to its numeric value. Falls back
/// to the direct source-recorded value when no identity applies.
fn recompute_authoritative(
    claim: &fm_agent::types::Claim,
    values: &std::collections::HashMap<String, f64>,
) -> Option<(f64, crate::agent::verification::MetricClass, u32)> {
    use crate::agent::verification::MetricClass;
    let parts: Vec<&str> = claim.claim_key.splitn(3, '.').collect();
    if parts.len() == 3 && parts[1] == "gross_profit" {
        let (ticker, period) = (parts[0], parts[2]);
        let rev = values.get(&format!("{ticker}.revenue.{period}"));
        let cost = values.get(&format!("{ticker}.cost_of_revenue.{period}"));
        if let (Some(r), Some(c)) = (rev, cost) {
            // Exact accounting identity over source-recorded quantities.
            return Some((r - c, MetricClass::ExactQuantity, 0));
        }
    }
    claim_authoritative(claim)
}

/// Drop the oldest history turns from a provider message list, preserving all
/// leading `system` layers, the latest `KEEP_LATEST` turns, and the current user
/// turn (Task 3.4 overflow fallback). After the drain, drops any leading orphaned
/// `tool` reply (whose assistant `tool_calls` message was removed) so OpenAI-style
/// providers never reject a dangling tool message. Returns whether it pruned.
fn prune_history(messages: &mut Vec<serde_json::Value>) -> bool {
    let sys_end = messages
        .iter()
        .position(|m| m.get("role").and_then(|r| r.as_str()) != Some("system"))
        .unwrap_or(messages.len());
    let keep_tail = crate::agent::context::KEEP_LATEST + 1;
    if messages.len() <= sys_end + keep_tail {
        return false;
    }
    let drop_to = messages.len() - keep_tail;
    if drop_to <= sys_end {
        return false;
    }
    messages.drain(sys_end..drop_to);
    while messages
        .get(sys_end)
        .and_then(|m| m.get("role").and_then(|r| r.as_str()))
        == Some("tool")
    {
        messages.remove(sys_end);
    }
    true
}

impl LiveDriver {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        app: tauri::AppHandle,
        store: crate::store::StoreHandle,
        cfg: fm_extract::LlmConfig,
        ctx: SessionContext,
        tools_enabled: bool,
        registry_hub: crate::agent::registry::ActorRegistry,
    ) -> Self {
        let messages = crate::commands::chat::seed_agent_messages(&ctx.user_msg);
        let tools = if tools_enabled {
            ToolRegistry::shared().agent_schemas()
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
            turn_results: Vec::new(),
            registry_hub,
            fallback_served: false,
            selected_workflow: None,
            run_claims: Vec::new(),
            verify_card: None,
        }
    }

    /// Resolve the workspace grounding layer (global personalization + project
    /// instructions) and the skill catalog block for this run (see
    /// [`crate::agent::grounding`]). Returns `(workspace_instructions, catalog)`;
    /// the caller places them in the canonical context layers (Task 3.3). Empty
    /// string / `None` when a layer is absent.
    async fn grounding_layers(&self) -> (String, Option<String>) {
        use tauri::Manager;
        let Ok(dir) = self.app.path().app_config_dir() else {
            return (String::new(), None);
        };
        let global = crate::agent::grounding::read_global(&dir);
        let conv = self.ctx.conversation_id.clone();
        let project_id = self
            .store
            .call(move |db| db.conversation_project(&conv).ok().flatten())
            .await;
        let project = project_id
            .as_deref()
            .and_then(|pid| crate::agent::grounding::read_project(&dir, pid));
        // Exclude aged-out (stale/archived) skills from the default catalog
        // (Task 7.3); hand-dropped skills with no lifecycle row stay visible.
        let inactive: std::collections::HashSet<String> = self
            .store
            .call(|db| db.inactive_skill_names().unwrap_or_default())
            .await
            .into_iter()
            .collect();
        let mut skills = crate::agent::skills::list_skills(&dir);
        skills.retain(|s| !inactive.contains(&s.name));
        let catalog = crate::agent::skills::catalog_block(&skills);
        let instructions = [global, project]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join("\n\n");
        (instructions, catalog)
    }

    /// Overflow fallback (Task 3.4): drop the oldest history turns, preserving all
    /// leading system layers, the latest `KEEP_LATEST` turns, and the current user
    /// turn. Deterministic and idempotent-safe; the pump retries a context
    /// overflow at most once, then terminates visibly. Returns whether it pruned.
    fn prune_oldest_turns(&mut self) -> bool {
        prune_history(&mut self.messages)
    }

    fn remaining(&self) -> std::time::Duration {
        self.deadline.saturating_sub(self.started.elapsed())
    }

    /// The output root the live `build_model` executor writes to, for refining
    /// write-risk (overwrite/export) at proposal time (Task 4.3). Mirrors the
    /// command's resolution via one shared helper so the two never drift.
    fn out_dir_path(&self) -> Option<std::path::PathBuf> {
        crate::commands::model::default_output_root(&self.app)
    }

    /// Transitional live UI event: same shape as the legacy `chat_tool` channel
    /// (`{name, status, card?, conversation_id, run_id}`) so the existing card
    /// renderer displays agent tool results until the parts consumer lands.
    fn emit_tool(&self, name: &str, status: &str, card: Option<&Value>) {
        use tauri::Emitter;
        let mut payload = serde_json::json!({
            "name": name,
            "status": status,
            "detail": "",
            "conversation_id": self.ctx.conversation_id,
            "run_id": self.ctx.run_id,
        });
        if let Some(c) = card {
            payload["card"] = c.clone();
        }
        let _ = self.app.emit("chat_tool", payload);
    }

    /// Fan-out banner event for the live UI: `status` is `fanout` / `fanout_done`,
    /// `count` is the number of independent calls running concurrently in a wave.
    /// Surfaces parallel work (M4) without disturbing the per-tool card flow.
    fn emit_fanout(&self, status: &str, count: usize) {
        use tauri::Emitter;
        let _ = self.app.emit(
            "chat_tool",
            serde_json::json!({
                "name": "",
                "status": status,
                "count": count,
                "conversation_id": self.ctx.conversation_id,
                "run_id": self.ctx.run_id,
            }),
        );
    }

    /// Per-child subagent lifecycle event for the task tray (M4). `status` is
    /// `running` / `done` / `error`. The parallel peer/company fan-out is modelled
    /// as real [`crate::agent::subagents::SubagentPool`] children.
    fn emit_subagent(&self, pool_id: &str, sub_id: u32, label: &str, status: &str) {
        use tauri::Emitter;
        let _ = self.app.emit(
            "agent_subagent",
            serde_json::json!({
                "pool_id": pool_id,
                "sub_id": sub_id,
                "label": label,
                "status": status,
                "conversation_id": self.ctx.conversation_id,
                "run_id": self.ctx.run_id,
            }),
        );
    }

    /// Human-readable subagent label: tool name plus its primary argument
    /// (ticker / company / query / url / cik) when present.
    fn subagent_label(name: &str, args: &Value) -> String {
        let key = ["ticker", "company", "query", "url", "cik"]
            .iter()
            .find_map(|k| args.get(*k).and_then(|v| v.as_str()))
            .map(|s| s.trim())
            .filter(|s| !s.is_empty());
        match key {
            Some(k) => format!("{name} · {k}"),
            None => name.to_string(),
        }
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

    /// Accept a completed provider stream: record content, seed pending tool
    /// calls, append the assistant message (tool_calls or prose), and derive the
    /// reducer `ModelOut`. Shared by the primary path and every retry arm so the
    /// accept logic never drifts across arms.
    fn accept_stream(&mut self, acc: &crate::agent::provider::StreamAccumulator) -> ModelOut {
        self.last_content = acc.content.clone();
        self.seed_pending_from_acc(acc);
        if !acc.complete_calls().is_empty() {
            self.append_assistant_tool_calls(acc);
        } else if !acc.content.trim().is_empty() {
            self.messages.push(serde_json::json!({
                "role": "assistant",
                "content": acc.content,
            }));
        }
        model_out_from_stream(&self.registry, acc, self.out_dir_path().as_deref())
    }
}

impl LiveDriver {
    /// No-key turn: resolve the message through the isolated FallbackDispatcher.
    /// First call issues the validated tool (or a Direct help answer); the
    /// second call (after the tool ran) closes the turn.
    fn fallback_model_out(&mut self) -> ModelOut {
        use crate::agent::fallback::{dispatch, FallbackDecision};
        if self.fallback_served {
            return ModelOut {
                calls: vec![],
                final_answer: true,
                tokens: 0,
            };
        }
        self.fallback_served = true;
        match dispatch(&self.registry, &self.ctx.user_msg) {
            FallbackDecision::Tool { name, args } => {
                let id = "fb-1".to_string();
                let risk = self
                    .registry
                    .get(&name)
                    .map(|s| s.risk)
                    .unwrap_or(Risk::ReadOnly);
                let args_valid = self.registry.validate_call(&name, &args).is_ok();
                self.pending.insert(
                    id.clone(),
                    PendingCall {
                        name: name.clone(),
                        args,
                        risk,
                    },
                );
                self.last_content = String::new();
                ModelOut {
                    calls: vec![ToolCall {
                        tool_call_id: id,
                        name,
                        risk,
                        needs_approval: !risk.auto_runs(),
                        args_valid,
                    }],
                    final_answer: false,
                    tokens: 0,
                }
            }
            FallbackDecision::Direct => {
                self.last_content =
                    "I couldn't map that to a tool. Try 'build AAPL', 'benchmark AAPL, MSFT',                      'news NVDA', 'search …', or add an OpenRouter API key in Settings for full                      natural-language chat."
                        .to_string();
                ModelOut {
                    calls: vec![],
                    final_answer: true,
                    tokens: 0,
                }
            }
        }
    }
}

impl Driver for LiveDriver {
    async fn prepare(&mut self) -> PreparedInfo {
        // Assemble provider context through the canonical layered builder
        // (Task 3.3): analyst policy → workspace grounding → recalled memories →
        // compacted branch history → current user turn. Long missions stay under
        // the model allowance via `compact_turns` inside `build_context` (latest
        // four + unresolved turns kept full). The analyst system prompt (with its
        // tool-routing guidance) is the authoritative policy layer; the skill
        // catalog rides it.
        use crate::agent::context::{self, TurnBlock};
        let conv = self.ctx.conversation_id.clone();
        let store = self.store.clone();
        let mut branch_turns: Vec<TurnBlock> = store
            .call(move |db| {
                let mut out = Vec::new();
                if let Ok(branch) = db.branch_path(&conv) {
                    for m in &branch {
                        if m.role != "user" && m.role != "assistant" {
                            continue;
                        }
                        let mut text = String::new();
                        if let Ok(parts) = db.message_parts(&m.id) {
                            for part in &parts {
                                if part.kind != "text" {
                                    continue;
                                }
                                if let Ok(v) = serde_json::from_str::<Value>(&part.payload_json) {
                                    if let Some(t) = v.get("text").and_then(|x| x.as_str()) {
                                        if !text.is_empty() {
                                            text.push('\n');
                                        }
                                        text.push_str(t);
                                    }
                                }
                            }
                        }
                        if !text.trim().is_empty() {
                            out.push(TurnBlock {
                                message_id: m.id.clone(),
                                role: m.role.clone(),
                                full_text: text,
                                summary: m.context_summary.clone(),
                                unresolved: false,
                            });
                        }
                    }
                }
                out
            })
            .await;
        // The trailing user turn is the current message; `build_context` places
        // it as the final `user` layer, so split it off the history.
        let current_turn = if branch_turns.last().map(|t| t.role.as_str()) == Some("user") {
            branch_turns
                .pop()
                .map(|t| t.full_text)
                .unwrap_or_else(|| self.ctx.user_msg.clone())
        } else {
            self.ctx.user_msg.clone()
        };
        // Recall workspace-scoped memories matching this turn (record their use).
        let mut recalled_memories: Vec<String> = Vec::new();
        {
            let ws = self.ctx.workspace_id.clone();
            let run = self.ctx.run_id.clone();
            if let Some(q) = fts_query(&self.ctx.user_msg) {
                let recalled: Vec<(i64, String, f64)> = self
                    .store
                    .call(move |db| {
                        use crate::store::memory::{
                            MemoryRepository, MemoryScope, SqliteMemoryRepository,
                        };
                        let repo = SqliteMemoryRepository::new(db);
                        let scope = MemoryScope {
                            workspace_id: Some(ws),
                            conversation_id: None,
                            global_only: false,
                        };
                        repo.search(&q, &scope)
                            .map(|v| {
                                v.into_iter()
                                    .take(5)
                                    .map(|r| (r.memory.id, r.memory.content, r.rank))
                                    .collect()
                            })
                            .unwrap_or_default()
                    })
                    .await;
                if !recalled.is_empty() {
                    recalled_memories = recalled.iter().map(|(_, c, _)| c.clone()).collect();
                    let ids: Vec<(i64, f64)> =
                        recalled.iter().map(|(id, _, r)| (*id, *r)).collect();
                    self.store
                        .call(move |db| {
                            use crate::store::memory::{MemoryRepository, SqliteMemoryRepository};
                            let mut repo = SqliteMemoryRepository::new(db);
                            for (id, rank) in ids {
                                let _ = repo.record_use(&run, id, rank);
                            }
                        })
                        .await;
                }
            }
        }
        // Analyst system prompt (authoritative policy layer) + skill catalog +
        // workspace grounding.
        let base_prompt = crate::commands::chat::seed_agent_messages("")
            .into_iter()
            .next()
            .and_then(|m| m.get("content").and_then(|c| c.as_str()).map(String::from))
            .unwrap_or_default();
        let (ws_instructions, catalog) = self.grounding_layers().await;
        let system_prompt = match catalog {
            Some(cat) => format!("{base_prompt}\n\n{cat}"),
            None => base_prompt,
        };
        let ctx_msgs = context::build_context(
            &system_prompt,
            &ws_instructions,
            None,
            &recalled_memories,
            &branch_turns,
            &[],
            &current_turn,
            None,
            96_000,
        );
        self.messages = ctx_msgs
            .into_iter()
            .map(|m| serde_json::json!({ "role": m.role, "content": m.content }))
            .collect();
        // Deterministic workflow selection (Task 3.1). A high-confidence intent
        // escalates the run to the workflow policy, turns on planning and its
        // verification requirement, and tags the plan. An under-specified ask
        // stays on the interactive policy. Never select a workflow whose required
        // tools are missing from the live registry — fall back to interactive.
        let registry = crate::agent::tools::ToolRegistry::shared();
        let selected = fm_agent::workflows::select_workflow(&self.ctx.user_msg)
            .and_then(fm_agent::workflows::workflow)
            .filter(|spec| {
                spec.required_tools
                    .iter()
                    .all(|t| registry.get(t).is_some())
            });
        self.selected_workflow = selected.as_ref().map(|s| s.id.to_string());
        if let Some(spec) = selected {
            return PreparedInfo {
                uses_tools: true,
                plan_needed: true,
                needs_verification: spec.needs_verification,
                workflow: Some(crate::agent::actor::WorkflowSelection {
                    id: spec.id.to_string(),
                    version: 1,
                }),
                escalation: Some(spec.policy),
            };
        }
        // Interactive: no workflow selected. Still verify when a claim-producing
        // tool (exact reported figures / model / comps) is enabled, so the run is
        // badged and material numbers are checked (Task 4.2/4.4). verify() is a
        // no-op when the turn produces no claims.
        let has_number_tool = self.tools.iter().any(|t| {
            matches!(
                t.pointer("/function/name").and_then(|v| v.as_str()),
                Some("get_financials") | Some("build_model") | Some("benchmark_peers")
            )
        });
        PreparedInfo {
            uses_tools: true,
            plan_needed: false,
            needs_verification: has_number_tool,
            workflow: None,
            escalation: None,
        }
    }

    async fn make_plan(&mut self) -> Plan {
        let objective: String = {
            let t: String = self.ctx.user_msg.trim().chars().take(140).collect();
            if t.is_empty() {
                "Analyze the request".to_string()
            } else {
                t
            }
        };
        // A selected workflow grounds the plan in its stable template steps
        // (Task 3.2); an interactive turn uses a minimal two-step plan.
        if let Some(id) = self.selected_workflow.clone() {
            if let Some(spec) = fm_agent::workflows::workflow(&id) {
                return spec.initial_plan(&objective);
            }
        }
        Plan {
            objective,
            assumptions: Vec::new(),
            steps: vec![
                PlanStep {
                    id: "research".into(),
                    label: "Research primary sources".into(),
                    status: PlanStepStatus::Pending,
                },
                PlanStep {
                    id: "synthesize".into(),
                    label: "Synthesize a sourced answer".into(),
                    status: PlanStepStatus::Pending,
                },
            ],
            version: 1,
        }
    }

    async fn request_model(&mut self) -> ModelOut {
        // No key: deterministic FallbackDispatcher instead of a provider call.
        if self.cfg.api_key.trim().is_empty() {
            return self.fallback_model_out();
        }
        if self.ctx.cancel.is_cancelled() {
            return ModelOut {
                calls: vec![],
                final_answer: true,
                tokens: 0,
            };
        }
        // Per-request ceiling = the reducer's remaining deadline; the adapter
        // (chat.rs) enforces it alongside connect + no-progress timeouts, and the
        // reducer's boundary Tick owns the overall deadline (Task 1.3). The former
        // temporary in-driver `remaining().is_zero()` early-return is intentionally
        // gone now that stream timing lives in the adapter.
        let remaining = self.remaining();

        let mut tools = self.tools.clone();
        let req = crate::commands::chat::build_chat_request(
            &self.cfg.model,
            &self.messages,
            &tools,
            true,
            !tools.is_empty(),
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
            Ok(acc) => self.accept_stream(&acc),
            Err(e)
                if crate::agent::provider::classify_provider_error(&e)
                    == crate::agent::provider::ProviderError::ToolIncompatible
                    && !tools.is_empty() =>
            {
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
                    false,
                );
                match crate::commands::chat::stream_completion_for_agent(
                    &app,
                    &conv,
                    &run,
                    &cfg,
                    &req,
                    &cancel,
                    self.remaining(),
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
                        model_out_from_stream(&self.registry, &acc, self.out_dir_path().as_deref())
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
            Err(e)
                if crate::agent::provider::classify_provider_error(&e)
                    == crate::agent::provider::ProviderError::ContextOverflow
                    && self.prune_oldest_turns() =>
            {
                // Context overflow: prune oldest history and retry once (Task 3.4).
                let req = crate::commands::chat::build_chat_request(
                    &self.cfg.model,
                    &self.messages,
                    &tools,
                    true,
                    !tools.is_empty(),
                );
                match crate::commands::chat::stream_completion_for_agent(
                    &app,
                    &conv,
                    &run,
                    &cfg,
                    &req,
                    &cancel,
                    self.remaining(),
                )
                .await
                {
                    Ok(acc) => self.accept_stream(&acc),
                    Err(err) => {
                        self.last_content = format!(
                            "⚠ the request exceeded the model's context window even after compaction. ({err})"
                        );
                        ModelOut {
                            calls: vec![],
                            final_answer: true,
                            tokens: 0,
                        }
                    }
                }
            }
            Err(e) if crate::agent::provider::classify_provider_error(&e).is_retryable() => {
                // Transient category (rate limit / capacity / transport / timeout):
                // retry the same request once after a short backoff (Task 6.1). A
                // second transient failure falls through to a visible stop. Failover
                // across roster profiles is the tested `request_with_retry` core.
                tokio::time::sleep(std::time::Duration::from_millis(750)).await;
                let req = crate::commands::chat::build_chat_request(
                    &self.cfg.model,
                    &self.messages,
                    &tools,
                    true,
                    !tools.is_empty(),
                );
                match crate::commands::chat::stream_completion_for_agent(
                    &app,
                    &conv,
                    &run,
                    &cfg,
                    &req,
                    &cancel,
                    self.remaining(),
                )
                .await
                {
                    Ok(acc) => self.accept_stream(&acc),
                    Err(err) => {
                        self.last_content = format!(
                            "⚠ the model request failed after a retry — the provider is rate-limited or unavailable. ({err})"
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

    async fn schedule_tools(&mut self, batch: &[String]) -> ToolBatchOutcome {
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
        let mut failed: Vec<String> = Vec::new();
        // Sources/artifacts from committed results; the actor promotes them to the
        // store and emits ArtifactCreated after the batch (Task 1.2).
        let mut batch_sources: Vec<fm_agent::types::SourceRef> = Vec::new();
        let mut batch_artifacts: Vec<fm_agent::types::ArtifactRef> = Vec::new();
        let mut batch_parts: Vec<crate::agent::actor::ResultPart> = Vec::new();
        for wave in waves {
            let calls: Vec<(String, String, Value)> = wave
                .iter()
                .filter_map(|id| {
                    let p = self.pending.get(id)?;
                    Some((id.clone(), p.name.clone(), p.args.clone()))
                })
                .collect();

            // Parallel fan-out: model each independent call as a real child
            // subagent (SubagentPool, Phase F) surfaced live in the task tray.
            let parallel = calls.len() > 1;
            let mut pool = if parallel {
                Some(crate::agent::subagents::SubagentPool::new(
                    self.ctx.run_id.clone(),
                    calls.len() as u32,
                    fm_agent::budget::Budget::new(fm_agent::Policy::default()),
                ))
            } else {
                None
            };
            // call_id -> subagent id, so a result can resolve the right child.
            let mut sub_ids: HashMap<String, u32> = HashMap::new();
            if let Some(p) = pool.as_mut() {
                self.emit_fanout("fanout", calls.len());
                for (id, name, args) in &calls {
                    let label = Self::subagent_label(name, args);
                    if let Some(h) = p.spawn(label.clone()) {
                        p.start(h.id);
                        sub_ids.insert(id.clone(), h.id);
                        self.emit_subagent(p.pool_id(), h.id, &label, "running");
                    }
                }
            }
            // Live "running…" status on the transitional UI channel.
            for (_, name, _) in &calls {
                self.emit_tool(name, "start", None);
            }

            let app = self.app.clone();
            let ctx = self.ctx.clone();
            let calls_owned = calls.clone();

            let (tokens, mut wave_results) = tokio::task::spawn_blocking(move || {
                let backend = crate::commands::chat::ChatToolBackend { app: &app };
                let registry = ToolRegistry::shared();
                let mut results = HashMap::new();
                let tokens = execute_batch(registry, &backend, &calls_owned, &ctx, &mut results);
                (tokens, results)
            })
            .await
            .unwrap_or_else(|_| (0, HashMap::new()));

            total = total.saturating_add(tokens);
            // Walk in call order (not HashMap order) so cards keep execution order.
            for (id, name, _) in &calls {
                let Some(res) = wave_results.remove(id) else {
                    continue;
                };
                match res {
                    Ok(env) => {
                        self.emit_tool(name, "done", Some(&env.display));
                        self.turn_results.push((name.clone(), env.display.clone()));
                        batch_parts.push(crate::agent::actor::ResultPart {
                            tool_call_id: id.clone(),
                            name: name.clone(),
                            card: env.display.clone(),
                        });
                        batch_sources.extend(env.sources.iter().cloned());
                        batch_artifacts.extend(env.artifacts.iter().cloned());
                        self.run_claims.extend(env.claims.iter().cloned());
                        self.messages.push(serde_json::json!({
                            "role": "tool",
                            "tool_call_id": id,
                            "content": env.summary,
                        }));
                        if let (Some(p), Some(sid)) = (pool.as_mut(), sub_ids.get(id)) {
                            p.succeed(*sid, name.clone());
                            self.emit_subagent(p.pool_id(), *sid, name, "done");
                        }
                    }
                    Err(e) => {
                        failed.push(id.clone());
                        let card = serde_json::json!({
                            "type": "error", "tool": name, "message": e.to_string(),
                        });
                        self.emit_tool(name, "error", Some(&card));
                        self.turn_results.push((name.clone(), card.clone()));
                        batch_parts.push(crate::agent::actor::ResultPart {
                            tool_call_id: id.clone(),
                            name: name.clone(),
                            card: card.clone(),
                        });
                        self.messages.push(serde_json::json!({
                            "role": "tool",
                            "tool_call_id": id,
                            "content": format!("Tool error: {e}"),
                        }));
                        if let (Some(p), Some(sid)) = (pool.as_mut(), sub_ids.get(id)) {
                            p.fail(*sid, e.to_string());
                            self.emit_subagent(p.pool_id(), *sid, name, "error");
                        }
                    }
                }
            }
            if parallel {
                self.emit_fanout("fanout_done", calls.len());
            }
        }
        {
            let mut seen = std::collections::HashSet::new();
            batch_sources.retain(|s| seen.insert(s.id.clone()));
            let mut seen = std::collections::HashSet::new();
            batch_artifacts.retain(|a| seen.insert(a.id.clone()));
        }
        ToolBatchOutcome {
            tokens: total,
            failed,
            sources: batch_sources,
            artifacts: batch_artifacts,
            parts: batch_parts,
        }
    }

    async fn synthesize(&mut self) {
        // Persist the turn as one assistant message with ordered parts:
        // tool result cards (execution order) first, then the final prose.
        // Snapshots/reload then render exactly what the live turn produced.
        let content = self.last_content.clone();
        let results = std::mem::take(&mut self.turn_results);
        if content.trim().is_empty() && results.is_empty() {
            return;
        }
        let conv = self.ctx.conversation_id.clone();
        let store = self.store.clone();
        let res: Result<(), String> = store
            .call(move |db| -> Result<(), String> {
                // Link the assistant turn under the current active leaf (the user
                // message agent_send just inserted) so the root→leaf branch is
                // user→assistant and snapshots/reload show the answer.
                let parent = db.active_leaf_id(&conv).map_err(|e| e.to_string())?;
                let mk = || {
                    let mut b = [0u8; 16];
                    rand::Rng::fill(&mut rand::thread_rng(), &mut b);
                    fm_agent::ids::format_uuid_v4(b)
                };
                let msg_id = mk();
                let now = crate::store::now_iso();
                db.insert_message(
                    &msg_id,
                    &conv,
                    parent.as_deref(),
                    "assistant",
                    None,
                    "complete",
                    &now,
                )
                .map_err(|e| e.to_string())?;
                let mut ordinal: i64 = 0;
                for (tool, card) in &results {
                    let payload = serde_json::json!({ "tool": tool, "card": card }).to_string();
                    db.insert_part(&mk(), &msg_id, ordinal, "result", &payload, None)
                        .map_err(|e| e.to_string())?;
                    ordinal += 1;
                }
                if !content.trim().is_empty() {
                    let payload = serde_json::json!({ "text": content }).to_string();
                    db.insert_part(&mk(), &msg_id, ordinal, "text", &payload, Some(&content))
                        .map_err(|e| e.to_string())?;
                }
                db.set_active_leaf(&conv, &msg_id, &now)
                    .map_err(|e| e.to_string())?;
                Ok(())
            })
            .await;
        if let Err(e) = res {
            eprintln!("agent synthesize: failed to persist assistant turn: {e}");
        }
    }

    async fn verify(&mut self) -> bool {
        // Verify this run's material numeric claims (Task 4.2/4.4). Each claim is
        // checked against its authoritative value: a derivable metric is recomputed
        // from sibling claims via an accounting identity (gross_profit == revenue -
        // cost_of_revenue) so an inconsistent figure fails; other figures verify
        // against their source-recorded value under a metric-specific tolerance. A
        // failed check rolls the run up to partial_unverified and the reducer runs
        // one repair pass before publishing partial. No claims → nothing to check.
        if self.run_claims.is_empty() {
            return true;
        }
        use crate::agent::verification::{verify_run, ClaimStatus};
        let values: std::collections::HashMap<String, f64> = self
            .run_claims
            .iter()
            .filter_map(|c| {
                c.normalized_value
                    .parse::<f64>()
                    .ok()
                    .map(|v| (c.claim_key.clone(), v))
            })
            .collect();
        let report = verify_run(&self.run_claims, |c| recompute_authoritative(c, &values));
        let verified = report
            .claims
            .iter()
            .filter(|c| c.status == ClaimStatus::Verified)
            .count();
        let card = serde_json::json!({
            "type": "verification",
            "status": report.status.badge(),
            "verified": verified,
            "total": report.claims.len(),
            "source": "SEC EDGAR XBRL",
        });
        // Stash the card; the actor emits it as a durable ResultPartAdded (2.1).
        self.verify_card = Some(card);
        !report.needs_repair()
    }

    fn take_verify_card(&mut self) -> Option<Value> {
        self.verify_card.take()
    }

    async fn extract_memory(&mut self) -> usize {
        // Manual save only: capture an explicit "remember: X" directive from the
        // user turn. Automatic LLM extraction stays off until the Phase E quality
        // gate lands (decision 4). PrecisionGate rejects secrets/paths/etc.
        let Some(content) = parse_memory_directive(&self.ctx.user_msg) else {
            return 0;
        };
        if crate::agent::memory::PrecisionGate::default()
            .check(&content)
            .is_err()
        {
            return 0;
        }
        let ws = self.ctx.workspace_id.clone();
        let run = self.ctx.run_id.clone();
        let now = crate::store::now_iso();
        let normalized_key = content
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase();
        let mut b = [0u8; 16];
        rand::Rng::fill(&mut rand::thread_rng(), &mut b);
        let public_id = fm_agent::ids::format_uuid_v4(b);
        let saved = self
            .store
            .call(move |db| {
                use crate::store::memory::{MemoryRepository, NewMemory, SqliteMemoryRepository};
                let mut repo = SqliteMemoryRepository::new(db);
                repo.insert(NewMemory {
                    public_id,
                    scope_type: "workspace".into(),
                    workspace_id: Some(ws),
                    conversation_id: None,
                    kind: "note".into(),
                    content,
                    normalized_key,
                    importance: 0.6,
                    confidence: 1.0,
                    source_type: "user_explicit".into(),
                    source_ref: Some(run),
                    now,
                })
                .is_ok()
            })
            .await;
        usize::from(saved)
    }

    async fn await_approval(&mut self, tool_call_id: &str) -> ApprovalResponse {
        // Persist a durable `pending_interactions` row BEFORE parking (Task 4.3),
        // so a walked-away approval survives restart and is deniable by the expiry
        // sweep (`agent::approvals::expire_and_deny_stale_approvals`). Without this
        // insert the sweep has no rows to act on. Store errors are non-fatal —
        // parking still proceeds and the in-driver safety timeout below applies.
        let pending_id = {
            let mut b = [0u8; 16];
            rand::Rng::fill(&mut rand::thread_rng(), &mut b);
            fm_agent::ids::format_uuid_v4(b)
        };
        {
            let pid = pending_id.clone();
            let run = self.ctx.run_id.clone();
            let tc = tool_call_id.to_string();
            let now = crate::store::now_iso();
            let req = serde_json::json!({ "tool_call_id": tool_call_id }).to_string();
            self.store
                .call(move |db| {
                    let _ = db.insert_pending(&pid, &run, Some(&tc), "approval", &req, &now);
                })
                .await;
        }

        // Park until agent_approve resolves it; deny on cancel or a 10-minute
        // safety timeout so a walked-away user can never leave a run wedged.
        let rx = self.registry_hub.park_approval(&self.ctx.run_id);
        let resp = tokio::select! {
            _ = self.ctx.cancel.cancelled() => ApprovalResponse::Deny,
            _ = tokio::time::sleep(std::time::Duration::from_secs(600)) => ApprovalResponse::Deny,
            r = rx => r.unwrap_or(ApprovalResponse::Deny),
        };

        // Record the resolution (idempotent — a no-op if the expiry sweep already
        // resolved it; first answer wins).
        {
            let pid = pending_id;
            let now = crate::store::now_iso();
            let resp_json = serde_json::json!({ "response": &resp }).to_string();
            self.store
                .call(move |db| {
                    let _ = db.resolve_pending(&pid, &resp_json, &now);
                })
                .await;
        }
        resp
    }

    fn elapsed_ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    fn control_signal(&self) -> Option<ControlSignal> {
        if self.ctx.cancel.is_cancelled() {
            Some(ControlSignal::Cancel)
        } else if self.ctx.interrupt.is_cancelled() {
            Some(ControlSignal::Interrupt)
        } else {
            None
        }
    }

    fn call_meta(&self, tool_call_id: &str) -> ToolCallMeta {
        call_meta_from_pending(self.pending.get(tool_call_id))
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

    #[test]
    fn memory_directive_parses_explicit_saves() {
        assert_eq!(
            parse_memory_directive("Remember that I prefer USD millions."),
            Some("I prefer USD millions".to_string())
        );
        assert_eq!(
            parse_memory_directive("note: DealCo target close is Q3"),
            Some("DealCo target close is Q3".to_string())
        );
        assert_eq!(
            parse_memory_directive("save to memory: always show comps as EV/EBITDA"),
            Some("always show comps as EV/EBITDA".to_string())
        );
    }

    #[test]
    fn prune_history_keeps_system_and_tail_and_drops_orphan_tool() {
        use serde_json::json;
        let sys = |c: &str| json!({ "role": "system", "content": c });
        let user = |c: &str| json!({ "role": "user", "content": c });
        let asst = |c: &str| json!({ "role": "assistant", "content": c });
        let tool = |c: &str| json!({ "role": "tool", "content": c });
        // system x2, then a long history whose drain boundary lands on a `tool`
        // reply, then the latest KEEP_LATEST+1 turns.
        let mut msgs = vec![sys("policy"), sys("workspace")];
        for i in 0..6 {
            msgs.push(asst(&format!("a{i}")));
            msgs.push(tool(&format!("t{i}")));
        }
        msgs.push(user("current question"));
        let before = msgs.len();
        assert!(super::prune_history(&mut msgs));
        assert!(msgs.len() < before);
        // Leading system layers preserved.
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[1]["role"], "system");
        // The first history message after the system block is NEVER an orphaned
        // tool reply (provider would reject it).
        assert_ne!(msgs[2]["role"], "tool");
        // The current user turn survives as the last message.
        assert_eq!(msgs.last().unwrap()["content"], "current question");
        // A second prune with nothing left to drop is a no-op.
        let mut small = vec![sys("p"), user("q")];
        assert!(!super::prune_history(&mut small));
    }
    #[test]
    fn memory_directive_rejects_questions_and_non_saves() {
        // Question forms are recall requests, not saves.
        assert_eq!(
            parse_memory_directive("remember what I said about TSLA?"),
            None
        );
        assert_eq!(parse_memory_directive("Remember how we modeled it?"), None);
        // Not a save directive at all.
        assert_eq!(parse_memory_directive("what are Tesla's 2025 sales?"), None);
        // Empty content after the prefix.
        assert_eq!(parse_memory_directive("remember: "), None);
    }

    #[test]
    fn run_claims_verify_against_their_source_value() {
        use crate::agent::executors::envelope_from_card;
        use crate::agent::verification::{verify_run, ClaimStatus};
        let card = serde_json::json!({
            "type": "financials", "ticker": "NVDA", "entity": "NVIDIA Corp",
            "fiscal_year": "2024", "period_end": "2024-01-28", "currency": "USD",
            "source": "https://sec.gov/x",
            "rows": [
                { "label": "Revenue", "value": 60922000000i64, "display": "60,922.0" },
                { "label": "Diluted EPS", "value": 11.93, "display": "11.93" }
            ]
        });
        let env = envelope_from_card("s".into(), card, fm_agent::types::Trust::Trusted, "ws-a");
        assert_eq!(env.claims.len(), 2);
        // The loop `verify()` runs: every source-recorded figure verifies against
        // itself under its metric tolerance (Task 4.2 slice) → run is Verified.
        let report = verify_run(&env.claims, claim_authoritative);
        assert_eq!(report.status, ClaimStatus::Verified);
        assert!(!report.needs_repair());
        // An unparseable claim value is Unverified — missing evidence never
        // certifies (guards the reducer's repair path against a bad extraction).
        let mut bad = env.claims.clone();
        bad[0].normalized_value = "n/a".into();
        let report = verify_run(&bad, claim_authoritative);
        assert_eq!(report.status, ClaimStatus::Unverified);
        assert!(report.needs_repair());
    }

    #[test]
    fn gross_profit_identity_catches_an_inconsistent_figure() {
        use crate::agent::executors::envelope_from_card;
        use crate::agent::verification::{verify_run, ClaimStatus};
        use std::collections::HashMap;
        // A consistent card: gross_profit == revenue - cost_of_revenue.
        let good = serde_json::json!({
            "type": "financials", "ticker": "NVDA", "entity": "NVIDIA Corp",
            "fiscal_year": "2024", "period_end": "2024-01-28", "currency": "USD",
            "source": "https://sec.gov/x",
            "rows": [
                { "label": "Revenue", "value": 60922000000i64, "display": "$60.92B" },
                { "label": "Cost of revenue", "value": 16621000000i64, "display": "$16.62B" },
                { "label": "Gross profit", "value": 44301000000i64, "display": "$44.30B" }
            ]
        });
        let env = envelope_from_card("s".into(), good, fm_agent::types::Trust::Trusted, "ws-a");
        let values: HashMap<String, f64> = env
            .claims
            .iter()
            .map(|c| (c.claim_key.clone(), c.normalized_value.parse().unwrap()))
            .collect();
        let report = verify_run(&env.claims, |c| recompute_authoritative(c, &values));
        assert_eq!(report.status, ClaimStatus::Verified, "{:?}", report.claims);

        // Restate gross_profit to an inconsistent value: the accounting identity
        // must catch it (Unverified → the run is partial, never a verified badge).
        let bad = serde_json::json!({
            "type": "financials", "ticker": "NVDA", "entity": "NVIDIA Corp",
            "fiscal_year": "2024", "period_end": "2024-01-28", "currency": "USD",
            "source": "https://sec.gov/x",
            "rows": [
                { "label": "Revenue", "value": 60922000000i64, "display": "$60.92B" },
                { "label": "Cost of revenue", "value": 16621000000i64, "display": "$16.62B" },
                { "label": "Gross profit", "value": 50000000000i64, "display": "$50.00B" }
            ]
        });
        let env = envelope_from_card("s".into(), bad, fm_agent::types::Trust::Trusted, "ws-a");
        let values: HashMap<String, f64> = env
            .claims
            .iter()
            .map(|c| (c.claim_key.clone(), c.normalized_value.parse().unwrap()))
            .collect();
        let report = verify_run(&env.claims, |c| recompute_authoritative(c, &values));
        assert_eq!(report.status, ClaimStatus::Unverified);
        assert!(report.needs_repair());
        let gp = report
            .claims
            .iter()
            .find(|c| c.claim_key == "nvda.gross_profit.fy2024")
            .unwrap();
        assert_eq!(gp.status, ClaimStatus::Unverified);
    }

    #[test]
    fn fts_query_drops_stopwords_and_punctuation() {
        // Only content words survive; filler + short tokens are dropped.
        let q = fts_query("What are the Tesla 2025 revenue figures?").unwrap();
        assert!(q.contains("\"tesla\""), "got: {q}");
        assert!(q.contains("\"revenue\""), "got: {q}");
        assert!(!q.contains("\"the\""), "stopword leaked: {q}");
        assert!(!q.contains("\"are\""), "stopword leaked: {q}");
        // Pure filler / punctuation yields no query (skip recall).
        assert_eq!(fts_query("what are the?"), None);
    }
    use fm_agent::Policy;
    use serde_json::json;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct TempDir(PathBuf);
    impl TempDir {
        fn new(tag: &str) -> Self {
            static N: AtomicU64 = AtomicU64::new(0);
            let n = N.fetch_add(1, Ordering::Relaxed);
            let p = std::env::temp_dir().join(format!("fmdrv-{tag}-{}-{}", std::process::id(), n));
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
        driver.seed_pending("ex", "build_model", json!({"ticker":"MSFT"}), Risk::Export);
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
        let apr = kinds
            .iter()
            .position(|k| *k == EventKind::ApprovalRequested)
            .unwrap();
        let starts: Vec<(usize, String)> = sink
            .events
            .lock()
            .iter()
            .enumerate()
            .filter(|(_, e)| e.event.kind == EventKind::ToolStarted)
            .filter_map(|(i, e)| {
                Some((
                    i,
                    e.event.payload.get("tool_call_id")?.as_str()?.to_string(),
                ))
            })
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
        let out = model_out_from_stream(&reg, &acc, None);
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
        let out = model_out_from_stream(&reg, &acc, None);
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
        let out = model_out_from_stream(&reg, &acc, None);
        assert_eq!(out.calls.len(), 1);
        assert_eq!(out.calls[0].risk, Risk::LocalCreate);
        assert!(!out.calls[0].needs_approval); // new immutable version auto-runs
        assert!(out.calls[0].args_valid);
    }

    #[test]
    fn stream_build_model_overwrite_refines_to_approval() {
        use std::io::Write;
        let reg = ToolRegistry::builtin();
        let acc = crate::agent::provider::accumulate(&[
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"b","function":{"name":"build_model","arguments":"{\"ticker\":\"MSFT\"}"}}]}}]}"#,
            "[DONE]",
        ]);
        let dir =
            std::env::temp_dir().join(format!("fmwr-{}-{}", std::process::id(), fastrand::u64(..)));
        std::fs::create_dir_all(&dir).unwrap();
        // Fresh output dir: a new model is LocalCreate → auto-runs (Task 4.3).
        let out = model_out_from_stream(&reg, &acc, Some(&dir));
        assert_eq!(out.calls[0].risk, Risk::LocalCreate);
        assert!(!out.calls[0].needs_approval);
        // Target already exists → overwrite → LocalOverwrite → MUST gate on approval.
        let stem = fm_build::ticker_to_stem("MSFT");
        let target = dir.join(format!("{stem}_model.xlsx"));
        std::fs::File::create(&target)
            .unwrap()
            .write_all(b"x")
            .unwrap();
        let out2 = model_out_from_stream(&reg, &acc, Some(&dir));
        assert_eq!(out2.calls[0].risk, Risk::LocalOverwrite);
        assert!(
            out2.calls[0].needs_approval,
            "an overwrite must route through approval (Task 4.3)"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stream_invalid_args_and_unknown_tool_flag_args_invalid() {
        let reg = ToolRegistry::builtin();
        let acc = crate::agent::provider::accumulate(&[
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"x","function":{"name":"get_quote","arguments":"{\"ticker\":\"not a ticker!!\"}"}}]}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":1,"id":"y","function":{"name":"frobnicate","arguments":"{}"}}]}}]}"#,
            "[DONE]",
        ]);
        let out = model_out_from_stream(&reg, &acc, None);
        assert_eq!(out.calls.len(), 2);
        assert!(
            !out.calls[0].args_valid,
            "bad ticker fails semantic validation"
        );
        assert!(!out.calls[1].args_valid, "unknown tool has no spec");
        assert_eq!(out.calls[1].risk, Risk::ReadOnly); // unknown defaults to read-only
        assert!(
            out.calls[1].needs_approval,
            "unknown tool fails closed (never auto-run)"
        );
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
        assert!(
            plan.needs_verification,
            "earnings is a numeric-finance turn"
        );
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
        driver.seed_pending(
            "lf",
            "list_filings",
            json!({"ticker":"NVDA"}),
            Risk::ReadOnly,
        );
        driver.seed_pending(
            "rf",
            "read_filing",
            json!({"ticker":"NVDA","item":"7"}),
            Risk::ReadOnly,
        );
        driver.seed_pending(
            "nw",
            "get_news",
            json!({"query":"NVDA guidance"}),
            Risk::ReadOnly,
        );
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
            ModelOut {
                calls: vec![],
                final_answer: true,
                tokens: 30,
            },
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
        driver.seed_pending(
            "bp",
            "benchmark_peers",
            json!({"tickers":["NVDA","AMD"]}),
            Risk::ReadOnly,
        );
        driver.seed_pending("qt", "get_quote", json!({"ticker":"NVDA"}), Risk::ReadOnly);
        driver.seed_pending(
            "lf",
            "list_filings",
            json!({"ticker":"NVDA"}),
            Risk::ReadOnly,
        );
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
            ModelOut {
                calls: vec![],
                final_answer: true,
                tokens: 30,
            },
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

    /// Durable-event honesty: a failed executor MUST emit ToolFailed for its
    /// id — never ToolSucceeded — while sibling successes still emit
    /// ToolSucceeded (the replayed UI must not render failures as successes).
    #[tokio::test]
    async fn failed_tool_emits_tool_failed_not_succeeded() {
        let (_td, store, sink, run) = setup();
        let backend = FakeBackend::new()
            .seed_ok("get_quote", "NVDA 788.17 USD", quote_card("NVDA", 788.17))
            .seed_err("get_news", "provider exploded");

        let ctx = SessionContext::test_ctx("c1", "quote + news NVDA");
        let mut driver = ScriptedDriver::new(backend, ctx);
        driver.seed_pending("qt", "get_quote", json!({"ticker":"NVDA"}), Risk::ReadOnly);
        driver.seed_pending("nw", "get_news", json!({"query":"NVDA"}), Risk::ReadOnly);
        driver.model_outs = vec![
            ModelOut {
                calls: vec![ro_call("qt", "get_quote"), ro_call("nw", "get_news")],
                final_answer: false,
                tokens: 40,
            },
            ModelOut {
                calls: vec![],
                final_answer: true,
                tokens: 20,
            },
        ];
        driver.verify_ok = true;

        let m = AgentMachine::new(Policy::INTERACTIVE);
        let out = run_turn(&store, &sink, "c1", &run, m, driver).await;
        assert_eq!(out.event, EventKind::RunCompleted);

        let evs = sink.events.lock();
        let kind_for = |id: &str| -> Vec<EventKind> {
            evs.iter()
                .filter(|e| {
                    e.event.payload.get("tool_call_id").and_then(|v| v.as_str()) == Some(id)
                        && matches!(
                            e.event.kind,
                            EventKind::ToolSucceeded | EventKind::ToolFailed
                        )
                })
                .map(|e| e.event.kind)
                .collect()
        };
        assert_eq!(kind_for("qt"), vec![EventKind::ToolSucceeded]);
        assert_eq!(kind_for("nw"), vec![EventKind::ToolFailed]);
    }
}
