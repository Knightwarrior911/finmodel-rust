//! Registry executors: validate → dispatch → [`ToolResultEnvelope`].
//!
//! The typed [`ToolRegistry`] owns name/risk/trust/validation. This module owns
//! the execution seam the real driver calls. Blocking finmodel cores stay in
//! `commands::chat` for now and are reached through a [`ToolBackend`] so unit
//! tests can inject a scripted backend without an `AppHandle`.

use std::collections::HashMap;
use std::sync::Arc;

use fm_agent::types::{ArtifactRef, Confidentiality, SourceRef, ToolResultEnvelope, Trust};
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
    pub confidentiality: Confidentiality,
    pub cancel: CancellationToken,
}

impl SessionContext {
    pub fn test_ctx(conversation_id: &str, user_msg: &str) -> Self {
        SessionContext {
            workspace_id: "w".into(),
            conversation_id: conversation_id.into(),
            run_id: "r".into(),
            user_msg: user_msg.into(),
            confidentiality: Confidentiality::Standard,
            cancel: CancellationToken::new(),
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
pub fn envelope_from_card(summary: String, card: Value, trust: Trust) -> ToolResultEnvelope {
    let sources = extract_sources(&card);
    let artifacts = extract_artifacts(&card);
    ToolResultEnvelope {
        data: card.clone(),
        display: card,
        summary,
        sources,
        artifacts,
        warnings: Vec::new(),
        trust,
    }
}

fn extract_sources(card: &Value) -> Vec<SourceRef> {
    let mut out = Vec::new();
    if let Some(url) = card.get("url").and_then(|v| v.as_str()) {
        if !url.is_empty() {
            out.push(SourceRef {
                id: format!("src-{}", short_hash(url)),
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
    Ok(envelope_from_card(summary, card, trust))
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
    fn rejects_unknown_and_invalid_before_backend() {
        let reg = ToolRegistry::builtin();
        let backend = FakeBackend::new().seed_ok(
            "get_quote",
            "should not run",
            quote_card("AAPL", 1.0),
        );
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
        let backend = FakeBackend::new().seed_ok(
            "get_quote",
            "AAPL 190.00 USD",
            quote_card("AAPL", 190.0),
        );
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
        let err = execute(
            &reg,
            &backend,
            "get_news",
            &json!({"query": "NVDA"}),
            &ctx,
        )
        .unwrap_err();
        assert_eq!(err, ExecuteError::Cancelled);
        assert!(backend.calls.lock().is_empty());
    }

    #[test]
    fn research_is_a_normal_registry_tool() {
        let reg = ToolRegistry::builtin();
        assert!(reg.get("research").is_some());
        let backend = FakeBackend::new().seed_ok(
            "research",
            "summary",
            json!({"type":"research_answer"}),
        );
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
