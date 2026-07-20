//! Registry executors: validate → dispatch → [`ToolResultEnvelope`].
//!
//! The typed [`ToolRegistry`] owns name/risk/trust/validation. This module owns
//! the execution seam the real driver calls. Blocking finmodel cores stay in
//! `commands::chat` for now and are reached through a [`ToolBackend`] so unit
//! tests can inject a scripted backend without an `AppHandle`.

use std::collections::HashMap;
use std::sync::Arc;

use fm_agent::types::{ArtifactRef, Claim, Confidentiality, SourceRef, ToolResultEnvelope, Trust};
use parking_lot::Mutex;
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use crate::agent::tools::{ToolError, ToolRegistry, TrustPolicy};

/// Server-resolved session context passed to every executor.
#[derive(Clone, Debug)]
pub struct SessionContext {
    pub workspace_id: String,
    pub conversation_id: String,
    pub run_id: String,
    pub user_msg: String,
    /// Vision inputs for THIS turn's seed message (data URLs). Empty for
    /// text-only turns; never persisted to the conversation store.
    pub images: Vec<String>,
    pub confidentiality: Confidentiality,
    pub cancel: CancellationToken,
    /// Resumable pause signal, distinct from `cancel` (terminal stop).
    pub interrupt: CancellationToken,
}

impl SessionContext {
    pub fn test_ctx(conversation_id: &str, user_msg: &str) -> Self {
        SessionContext {
            workspace_id: "w".into(),
            conversation_id: conversation_id.into(),
            run_id: "r".into(),
            user_msg: user_msg.into(),
            images: Vec::new(),
            confidentiality: Confidentiality::Standard,
            cancel: CancellationToken::new(),
            interrupt: CancellationToken::new(),
        }
    }
}

/// Why an executor rejected or failed a call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExecuteError {
    Validation(ToolError),
    Runtime(String),
    Cancelled,
}

impl std::fmt::Display for ExecuteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExecuteError::Validation(e) => write!(f, "validation: {e}"),
            ExecuteError::Runtime(e) => write!(f, "runtime: {e}"),
            ExecuteError::Cancelled => write!(f, "cancelled"),
        }
    }
}

/// Blocking core invoked after registry validation. Production wires this to
/// chat tool cores; tests inject [`FakeBackend`].
pub trait ToolBackend: Send + Sync {
    /// Run the named tool. Returns `(summary_text, display_card)`.
    fn invoke(
        &self,
        name: &str,
        args: &Value,
        ctx: &SessionContext,
    ) -> Result<(String, Value), String>;
}

/// Scripted backend: returns a pre-seeded `(summary, card)` per tool name.
#[derive(Default)]
pub struct FakeBackend {
    responses: Mutex<HashMap<String, Result<(String, Value), String>>>,
    /// Record of `(name, args)` invocations for assertions.
    pub calls: Mutex<Vec<(String, Value)>>,
}

impl FakeBackend {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn seed_ok(self, name: &str, summary: &str, card: Value) -> Self {
        self.responses
            .lock()
            .insert(name.into(), Ok((summary.into(), card)));
        self
    }

    pub fn seed_err(self, name: &str, err: &str) -> Self {
        self.responses.lock().insert(name.into(), Err(err.into()));
        self
    }
}

impl ToolBackend for FakeBackend {
    fn invoke(
        &self,
        name: &str,
        args: &Value,
        _ctx: &SessionContext,
    ) -> Result<(String, Value), String> {
        self.calls.lock().push((name.into(), args.clone()));
        self.responses
            .lock()
            .get(name)
            .cloned()
            .unwrap_or_else(|| Err(format!("fake backend has no seed for `{name}`")))
    }
}

impl ToolBackend for Arc<FakeBackend> {
    fn invoke(
        &self,
        name: &str,
        args: &Value,
        ctx: &SessionContext,
    ) -> Result<(String, Value), String> {
        (**self).invoke(name, args, ctx)
    }
}

fn trust_from_policy(p: TrustPolicy) -> Trust {
    match p {
        TrustPolicy::Trusted => Trust::Trusted,
        TrustPolicy::Untrusted => Trust::Untrusted,
    }
}

/// Map a legacy `(summary, card)` pair into the uniform envelope.
pub fn envelope_from_card(
    summary: String,
    card: Value,
    trust: Trust,
    workspace_id: &str,
) -> ToolResultEnvelope {
    let sources = extract_sources(&card, workspace_id);
    let artifacts = extract_artifacts(&card);
    let claims = extract_claims(&card, workspace_id);
    // Persist only bounded model text to provider context (Task 1.2 step 3); the
    // full structured result rides `display`/`data` for the UI/store.
    const SUMMARY_MAX: usize = 6000;
    let summary = if summary.chars().count() > SUMMARY_MAX {
        let head: String = summary.chars().take(SUMMARY_MAX).collect();
        format!("{head}…")
    } else {
        summary
    };
    ToolResultEnvelope {
        data: card.clone(),
        display: card,
        summary,
        sources,
        artifacts,
        warnings: Vec::new(),
        claims,
        progress: None,
        terminate: false,
        trust,
    }
}
fn extract_sources(card: &Value, workspace_id: &str) -> Vec<SourceRef> {
    let mut out = Vec::new();
    if let Some(url) = card.get("url").and_then(|v| v.as_str()) {
        if !url.is_empty() {
            out.push(SourceRef {
                // Workspace-scoped so the same URI never cross-links a source row
                // across a confidentiality boundary (Task 4.1).
                id: format!("src-{}", short_hash(&format!("{workspace_id}\u{1}{url}"))),
                kind: card
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("web")
                    .into(),
                canonical_uri: url.into(),
                title: card
                    .get("title")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                publisher: None,
                published_at: card
                    .get("filing_date")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                accessed_at: None,
            });
        }
    }
    out
}

fn extract_artifacts(card: &Value) -> Vec<ArtifactRef> {
    let mut out = Vec::new();
    let id = card
        .get("artifact_id")
        .or_else(|| card.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if id.starts_with("art-") {
        out.push(ArtifactRef {
            id: id.into(),
            kind: card
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("workbook")
                .into(),
            label: card
                .get("label")
                .or_else(|| card.get("ticker"))
                .and_then(|v| v.as_str())
                .unwrap_or(id)
                .into(),
            mime: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet".into(),
            version: 1,
            sha256: String::new(),
        });
    }
    out
}

/// Extract material numeric claims from a tool result card (Task 4.2). Today the
/// exact-reported `financials` card (SEC EDGAR XBRL) is the claim source: each
/// row becomes a `Claim` keyed `{ticker}.{metric}.{period}`, backed by the same
/// workspace-scoped source id the citation uses. These are what `verify_run`
/// checks against their source value before a run is badged verified.
fn extract_claims(card: &Value, workspace_id: &str) -> Vec<Claim> {
    let mut out = Vec::new();
    if card.get("type").and_then(|v| v.as_str()) != Some("financials") {
        return out;
    }
    let entity = card
        .get("entity")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let ticker = card
        .get("ticker")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_lowercase();
    let period = card
        .get("fiscal_year")
        .and_then(|v| v.as_str())
        .map(|f| format!("FY{f}"))
        .unwrap_or_default();
    let period_end = card
        .get("period_end")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let currency = card
        .get("currency")
        .and_then(|v| v.as_str())
        .unwrap_or("USD");
    let source_url = card.get("source").and_then(|v| v.as_str()).unwrap_or("");
    let source_id = format!(
        "src-{}",
        short_hash(&format!("{workspace_id}\u{1}{source_url}"))
    );
    let rows = match card.get("rows").and_then(|v| v.as_array()) {
        Some(r) => r,
        None => return out,
    };
    for r in rows {
        let label = r.get("label").and_then(|v| v.as_str()).unwrap_or("");
        let value = match r.get("value") {
            Some(v) if v.is_number() => v.to_string(),
            _ => continue,
        };
        if label.is_empty() {
            continue;
        }
        let metric = label.to_lowercase().replace(' ', "_");
        let key = format!("{ticker}.{metric}.{}", period.to_lowercase());
        let unit = if label.to_lowercase().contains("eps") {
            format!("{currency}/shares")
        } else {
            currency.to_string()
        };
        out.push(Claim {
            claim_key: key,
            entity: entity.clone(),
            normalized_value: value.clone(),
            unit,
            currency: Some(currency.to_string()),
            scale: "1".into(),
            period: period.clone(),
            locator: format!("10-K XBRL (period ended {period_end})"),
            source_id: source_id.clone(),
            quote_hash: short_hash(&format!("{metric}\u{1}{value}")),
        });
    }
    out
}

fn short_hash(s: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    format!("{:x}", h.finish() & 0xffff_ffff)
}

/// Validate against the registry, then invoke the backend and wrap the result.
pub fn execute<B: ToolBackend>(
    registry: &ToolRegistry,
    backend: &B,
    name: &str,
    args: &Value,
    ctx: &SessionContext,
) -> Result<ToolResultEnvelope, ExecuteError> {
    if ctx.cancel.is_cancelled() {
        return Err(ExecuteError::Cancelled);
    }
    registry
        .validate_call(name, args)
        .map_err(ExecuteError::Validation)?;
    let spec = registry
        .get(name)
        .ok_or_else(|| ExecuteError::Validation(ToolError::UnknownTool(name.into())))?;
    let trust = trust_from_policy(spec.trust);
    let (summary, card) = backend
        .invoke(name, args, ctx)
        .map_err(ExecuteError::Runtime)?;
    Ok(envelope_from_card(summary, card, trust, &ctx.workspace_id))
}

/// What the MODEL reads when a tool call fails — built for self-repair, not
/// for logs. Validation failures echo the tool's exact parameter schema so
/// the next round can fix the arguments instead of flailing or giving up;
/// unknown tools list the real catalog; runtime failures stay terse (the
/// schema is noise when the args were fine).
pub fn tool_error_content(
    registry: &crate::agent::tools::ToolRegistry,
    e: &ExecuteError,
) -> String {
    use crate::agent::tools::ToolError;
    const SCHEMA_CAP: usize = 900;
    let schema_line = |tool: &str| -> String {
        registry
            .get(tool)
            .map(|spec| {
                let mut s = serde_json::to_string(&(spec.params_schema)()).unwrap_or_default();
                if s.len() > SCHEMA_CAP {
                    s.truncate(SCHEMA_CAP);
                    s.push_str("...");
                }
                format!(
                    "\nParameters schema for {tool}: {s}\nRequired: [{}]. Fix the arguments and retry the call.",
                    spec.required_args.join(", ")
                )
            })
            .unwrap_or_default()
    };
    match e {
        ExecuteError::Validation(ToolError::UnknownTool(t)) => {
            let mut names: Vec<&str> = registry.names();
            names.sort_unstable();
            format!(
                "Tool error: no tool named `{t}`. Available tools: {}. Pick one of these exact names and retry.",
                names.join(", ")
            )
        }
        ExecuteError::Validation(ToolError::MissingArg { tool, arg }) => {
            format!(
                "Tool error: `{tool}` is missing the required argument `{arg}`.{}",
                schema_line(tool)
            )
        }
        ExecuteError::Validation(ToolError::Invalid { tool, reason }) => {
            format!(
                "Tool error: invalid arguments for `{tool}`: {reason}.{}",
                schema_line(tool)
            )
        }
        ExecuteError::Cancelled => "Tool error: cancelled".to_string(),
        ExecuteError::Runtime(msg) => format!("Tool error: {msg}"),
    }
}

/// Execute a batch of independent calls concurrently. Returns a conservative
/// token charge. Independent read-only tools (e.g. a peer set's per-ticker
/// fetches) run in parallel — capped at [`PER_RUN_SLOTS`] in-flight — instead
/// of serializing. Result ordering is preserved by the caller (it walks calls
/// in input order and looks each up in `results`), so parallelism is invisible
/// to rendering.
pub fn execute_batch<B: ToolBackend + Sync>(
    registry: &ToolRegistry,
    backend: &B,
    calls: &[(String, String, Value)],
    ctx: &SessionContext,
    results: &mut HashMap<String, Result<ToolResultEnvelope, ExecuteError>>,
) -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    let tokens = AtomicU64::new(0);
    let collected: std::sync::Mutex<Vec<(String, Result<ToolResultEnvelope, ExecuteError>)>> =
        std::sync::Mutex::new(Vec::new());
    // Run in waves of at most PER_RUN_SLOTS so a large batch never oversubscribes.
    for chunk in calls.chunks(crate::agent::registry::PER_RUN_SLOTS) {
        std::thread::scope(|s| {
            for (id, name, args) in chunk {
                let tokens = &tokens;
                let collected = &collected;
                s.spawn(move || {
                    let env = if ctx.cancel.is_cancelled() {
                        Err(ExecuteError::Cancelled)
                    } else {
                        execute(registry, backend, name, args, ctx)
                    };
                    tokens.fetch_add(25, Ordering::Relaxed);
                    collected.lock().unwrap().push((id.clone(), env));
                });
            }
        });
    }
    for (id, env) in collected.into_inner().unwrap() {
        results.insert(id, env);
    }
    tokens.into_inner()
}

/// Convenience: a one-shot quote card used by scripted tests.
pub fn quote_card(ticker: &str, price: f64) -> Value {
    json!({
        "type": "quote",
        "ticker": ticker,
        "price": price,
        "currency": "USD",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use fm_agent::types::Trust;

    #[test]
    fn source_ids_are_workspace_scoped() {
        let card = json!({ "type": "web", "url": "https://a.com", "title": "A" });
        let a = envelope_from_card("s".into(), card.clone(), Trust::Untrusted, "ws-a");
        let b = envelope_from_card("s".into(), card, Trust::Untrusted, "ws-b");
        assert_eq!(a.sources.len(), 1);
        assert_eq!(b.sources.len(), 1);
        // Same URL under two workspaces must mint DIFFERENT source ids — no
        // cross-confidentiality-boundary citation linkage (Task 4.1).
        assert_ne!(a.sources[0].id, b.sources[0].id);
        // Same workspace + same URL → stable id (dedup within a workspace).
        let a2 = envelope_from_card(
            "s".into(),
            json!({ "type": "web", "url": "https://a.com" }),
            Trust::Untrusted,
            "ws-a",
        );
        assert_eq!(a.sources[0].id, a2.sources[0].id);
    }
    #[test]
    fn tool_errors_teach_the_model_to_self_repair() {
        use crate::agent::tools::{ToolError, ToolRegistry};
        let reg = ToolRegistry::shared();

        // Unknown tool → the real catalog, so the next round picks a valid name.
        let s = tool_error_content(
            reg,
            &ExecuteError::Validation(ToolError::UnknownTool("get_finances".into())),
        );
        assert!(s.contains("`get_finances`"), "names the bad call: {s}");
        assert!(s.contains("get_financials"), "offers the real catalog: {s}");

        // Missing arg → the exact parameter schema + required list.
        let s = tool_error_content(
            reg,
            &ExecuteError::Validation(ToolError::MissingArg {
                tool: "get_financials".into(),
                arg: "ticker".into(),
            }),
        );
        assert!(s.contains("required argument `ticker`"), "{s}");
        assert!(s.contains("Parameters schema for get_financials"), "{s}");
        assert!(s.contains("\"ticker\""), "schema JSON present: {s}");
        assert!(s.contains("Required: [ticker]"), "{s}");

        // Invalid args → reason + schema.
        let s = tool_error_content(
            reg,
            &ExecuteError::Validation(ToolError::Invalid {
                tool: "get_quote".into(),
                reason: "ticker has invalid characters".into(),
            }),
        );
        assert!(s.contains("invalid arguments for `get_quote`"), "{s}");
        assert!(s.contains("Parameters schema for get_quote"), "{s}");

        // Runtime failures stay terse — the schema is noise when args were fine.
        let s = tool_error_content(reg, &ExecuteError::Runtime("EDGAR timed out".into()));
        assert_eq!(s, "Tool error: EDGAR timed out");
        assert!(!s.contains("Parameters schema"));
    }

    #[test]
    fn financials_card_extracts_material_claims() {
        // The exact-reported `financials` card (SEC EDGAR XBRL) is the claim
        // source (Task 4.2): each numeric row becomes a Claim with a workspace-
        // scoped source id, EPS carrying a per-share unit.
        let card = json!({
            "type": "financials",
            "ticker": "NVDA",
            "entity": "NVIDIA Corp",
            "fiscal_year": "2024",
            "period_end": "2024-01-28",
            "currency": "USD",
            "source": "https://www.sec.gov/cgi-bin/browse-edgar?CIK=NVDA&type=10-K",
            "rows": [
                { "label": "Revenue", "value": 60922000000i64, "display": "60,922.0" },
                { "label": "Diluted EPS", "value": 11.93, "display": "11.93" },
                { "label": "Segment", "value": "Data Center", "display": "Data Center" }
            ]
        });
        let env = envelope_from_card("s".into(), card, Trust::Trusted, "ws-a");
        // Only the two numeric rows become claims; the string row is skipped.
        assert_eq!(env.claims.len(), 2);
        let rev = env
            .claims
            .iter()
            .find(|c| c.claim_key == "nvda.revenue.fy2024")
            .unwrap();
        assert_eq!(rev.normalized_value, "60922000000");
        assert_eq!(rev.unit, "USD");
        assert_eq!(rev.entity, "NVIDIA Corp");
        assert!(rev.source_id.starts_with("src-"));
        let eps = env
            .claims
            .iter()
            .find(|c| c.claim_key == "nvda.diluted_eps.fy2024")
            .unwrap();
        assert_eq!(eps.unit, "USD/shares");
    }

    #[test]
    fn rejects_unknown_and_invalid_before_backend() {
        let reg = ToolRegistry::builtin();
        let backend =
            FakeBackend::new().seed_ok("get_quote", "should not run", quote_card("AAPL", 1.0));
        let ctx = SessionContext::test_ctx("c1", "quote AAPL");
        let err = execute(&reg, &backend, "nope", &json!({}), &ctx).unwrap_err();
        assert!(matches!(
            err,
            ExecuteError::Validation(ToolError::UnknownTool(_))
        ));
        assert!(backend.calls.lock().is_empty());

        let err = execute(&reg, &backend, "get_quote", &json!({}), &ctx).unwrap_err();
        assert!(matches!(
            err,
            ExecuteError::Validation(ToolError::MissingArg { .. })
        ));
        assert!(backend.calls.lock().is_empty());
    }

    #[test]
    fn maps_card_to_envelope_with_registry_trust() {
        let reg = ToolRegistry::builtin();
        let backend =
            FakeBackend::new().seed_ok("get_quote", "AAPL 190.00 USD", quote_card("AAPL", 190.0));
        let ctx = SessionContext::test_ctx("c1", "quote AAPL");
        let env = execute(
            &reg,
            &backend,
            "get_quote",
            &json!({"ticker": "AAPL"}),
            &ctx,
        )
        .unwrap();
        assert_eq!(env.summary, "AAPL 190.00 USD");
        assert_eq!(env.trust, Trust::Untrusted);
        assert_eq!(env.display["ticker"], "AAPL");
        assert_eq!(backend.calls.lock().len(), 1);
    }

    #[test]
    fn build_model_envelope_is_trusted_and_carries_artifact() {
        let reg = ToolRegistry::builtin();
        let card = json!({
            "type": "model",
            "ticker": "NVDA",
            "artifact_id": "art-0123456789abcdef0123456789abcdef",
            "label": "NVDA model",
        });
        let backend = FakeBackend::new().seed_ok("build_model", "built NVDA", card);
        let ctx = SessionContext::test_ctx("c1", "build NVDA");
        let env = execute(
            &reg,
            &backend,
            "build_model",
            &json!({"ticker": "NVDA"}),
            &ctx,
        )
        .unwrap();
        assert_eq!(env.trust, Trust::Trusted);
        assert_eq!(env.artifacts.len(), 1);
        assert_eq!(env.artifacts[0].id, "art-0123456789abcdef0123456789abcdef");
    }

    #[test]
    fn filing_card_promotes_source_url() {
        let reg = ToolRegistry::builtin();
        let card = json!({
            "type": "filing_doc",
            "ticker": "MSFT",
            "url": "https://www.sec.gov/Archives/edgar/data/789019/000156459021000000/msft.htm",
            "filing_date": "2024-08-01",
        });
        let backend = FakeBackend::new().seed_ok("read_filing", "MSFT 10-K", card);
        let ctx = SessionContext::test_ctx("c1", "read MSFT 10-K");
        let env = execute(
            &reg,
            &backend,
            "read_filing",
            &json!({"ticker": "MSFT"}),
            &ctx,
        )
        .unwrap();
        assert_eq!(env.sources.len(), 1);
        assert!(env.sources[0].canonical_uri.contains("sec.gov"));
    }

    #[test]
    fn ssrf_blocked_url_never_hits_backend() {
        let reg = ToolRegistry::builtin();
        let backend = FakeBackend::new().seed_ok(
            "read_page",
            "leak",
            json!({"type":"page","url":"http://127.0.0.1/"}),
        );
        let ctx = SessionContext::test_ctx("c1", "read localhost");
        let err = execute(
            &reg,
            &backend,
            "read_page",
            &json!({"url": "http://127.0.0.1/secret"}),
            &ctx,
        )
        .unwrap_err();
        assert!(matches!(err, ExecuteError::Validation(_)));
        assert!(backend.calls.lock().is_empty());
    }

    #[test]
    fn cancelled_context_short_circuits() {
        let reg = ToolRegistry::builtin();
        let backend = FakeBackend::new().seed_ok("get_news", "n", json!({"type":"news"}));
        let ctx = SessionContext::test_ctx("c1", "news");
        ctx.cancel.cancel();
        let err = execute(&reg, &backend, "get_news", &json!({"query": "NVDA"}), &ctx).unwrap_err();
        assert_eq!(err, ExecuteError::Cancelled);
        assert!(backend.calls.lock().is_empty());
    }

    #[test]
    fn research_is_a_normal_registry_tool() {
        let reg = ToolRegistry::builtin();
        assert!(reg.get("research").is_some());
        let backend =
            FakeBackend::new().seed_ok("research", "summary", json!({"type":"research_answer"}));
        let ctx = SessionContext::test_ctx("c1", "what changed in NVDA guidance?");
        let env = execute(
            &reg,
            &backend,
            "research",
            &json!({"query": "NVDA guidance"}),
            &ctx,
        )
        .unwrap();
        assert_eq!(env.summary, "summary");
        assert_eq!(env.trust, Trust::Untrusted);
    }

    #[test]
    fn batch_executes_two_reads_and_charges_tokens() {
        let reg = ToolRegistry::builtin();
        let backend = Arc::new(
            FakeBackend::new()
                .seed_ok("get_quote", "AAPL 190", quote_card("AAPL", 190.0))
                .seed_ok("get_news", "news", json!({"type":"news","query":"AAPL"})),
        );
        let ctx = SessionContext::test_ctx("c1", "AAPL quote and news");
        let calls = vec![
            ("t1".into(), "get_quote".into(), json!({"ticker":"AAPL"})),
            ("t2".into(), "get_news".into(), json!({"query":"AAPL"})),
        ];
        let mut results = HashMap::new();
        let tokens = execute_batch(&reg, &backend, &calls, &ctx, &mut results);
        assert_eq!(tokens, 50);
        assert!(results.get("t1").unwrap().is_ok());
        assert!(results.get("t2").unwrap().is_ok());
        assert_eq!(backend.calls.lock().len(), 2);
    }
    #[test]
    fn wave_calls_genuinely_overlap_in_time() {
        // The child-agent fan-out story rests on this: independent calls in
        // one wave must RUN concurrently, not merely both complete. A probe
        // backend records the peak number of in-flight invocations; two
        // sleeping calls must overlap (peak 2), or delegation/fan-out is a
        // serial loop wearing a parallel costume.
        use std::sync::atomic::{AtomicUsize, Ordering};
        struct Probe {
            in_flight: AtomicUsize,
            peak: AtomicUsize,
        }
        impl ToolBackend for Probe {
            fn invoke(
                &self,
                _name: &str,
                _args: &Value,
                _ctx: &SessionContext,
            ) -> Result<(String, Value), String> {
                let now = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                self.peak.fetch_max(now, Ordering::SeqCst);
                std::thread::sleep(std::time::Duration::from_millis(60));
                self.in_flight.fetch_sub(1, Ordering::SeqCst);
                Ok(("ok".into(), json!({ "type": "quote", "ticker": "X" })))
            }
        }
        let reg = ToolRegistry::builtin();
        let probe = Probe {
            in_flight: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
        };
        let ctx = SessionContext::test_ctx("c1", "overlap");
        let calls = vec![
            ("t1".into(), "get_quote".into(), json!({"ticker":"AAPL"})),
            ("t2".into(), "get_quote".into(), json!({"ticker":"MSFT"})),
        ];
        let mut results = HashMap::new();
        execute_batch(&reg, &probe, &calls, &ctx, &mut results);
        assert!(results.get("t1").unwrap().is_ok());
        assert!(results.get("t2").unwrap().is_ok());
        assert_eq!(
            probe.peak.load(Ordering::SeqCst),
            2,
            "two independent calls must be in flight simultaneously"
        );
    }

    #[test]
    fn analyze_pdf_requires_artifact_id_not_path() {
        let reg = ToolRegistry::builtin();
        let backend = FakeBackend::new();
        let ctx = SessionContext::test_ctx("c1", "pdf");
        let err = execute(
            &reg,
            &backend,
            "analyze_pdf",
            &json!({"path": "C:/secret.pdf"}),
            &ctx,
        )
        .unwrap_err();
        assert!(matches!(err, ExecuteError::Validation(_)));
        let err = execute(
            &reg,
            &backend,
            "analyze_pdf",
            &json!({"artifact_id": "art-0123456789abcdef0123456789abcdef"}),
            &ctx,
        )
        .unwrap_err();
        assert!(matches!(err, ExecuteError::Runtime(_)));
        assert_eq!(backend.calls.lock().len(), 1);
    }
}
