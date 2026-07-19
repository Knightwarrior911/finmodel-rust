//! The typed tool registry: one entry per tool replacing the old
//! `ToolName`/`tool_schemas`/`run_tool` split.
//!
//! Each [`ToolSpec`] carries a stable name, human label, description, base risk,
//! capability/workflow tags, required arguments, interruptibility/idempotency,
//! and a semantic validator. Executors (which call the existing finmodel
//! blocking cores) are registered separately when the real driver is wired, so
//! this registry — name lookup, strict argument validation, risk, and trust
//! policy — is deterministic and unit-testable on its own.

use std::collections::HashMap;
use std::sync::LazyLock;

use fm_agent::types::Risk;
use serde_json::{json, Value};

use crate::agent::security;

/// Why a tool call was rejected before execution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolError {
    UnknownTool(String),
    MissingArg { tool: String, arg: String },
    Invalid { tool: String, reason: String },
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ToolError::UnknownTool(t) => write!(f, "unknown tool `{t}`"),
            ToolError::MissingArg { tool, arg } => write!(f, "tool `{tool}` missing arg `{arg}`"),
            ToolError::Invalid { tool, reason } => write!(f, "tool `{tool}`: {reason}"),
        }
    }
}

/// Trust policy for a tool's produced text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrustPolicy {
    /// Output is produced by a local deterministic engine we control.
    Trusted,
    /// Output contains external text; treat as untrusted quoted data.
    Untrusted,
}

/// A single registry entry.
#[derive(Clone)]
pub struct ToolSpec {
    pub name: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    /// Base risk. Artifact writers refine to `LocalCreate`/`LocalOverwrite`/
    /// `Export` at schedule time via [`security::classify_write_risk`].
    pub risk: Risk,
    pub capabilities: &'static [&'static str],
    pub required_args: &'static [&'static str],
    pub interruptible: bool,
    pub idempotent: bool,
    pub trust: TrustPolicy,
    /// Semantic validation beyond required-field presence.
    pub validate: fn(&Value) -> Result<(), String>,
    /// Provider-facing JSON parameter schema (OpenAI function `parameters`). The
    /// single authority for the model-facing tool definition (Task 1.1).
    pub params_schema: fn() -> Value,
    /// Whether this tool is offered to the model in the provider tool array.
    /// (`research_deal` is dispatchable but withheld from the model, matching the
    /// pre-collapse `agent_tool_schemas` set.)
    pub model_visible: bool,
}

/// No-op validator, reserved for tools that need no extra semantic checks.
#[allow(dead_code)]
fn ok(_: &Value) -> Result<(), String> {
    Ok(())
}

/// Non-empty string field validator factory.
fn require_nonempty(args: &Value, key: &str) -> Result<(), String> {
    match args.get(key).and_then(|v| v.as_str()) {
        Some(s) if !s.trim().is_empty() => Ok(()),
        _ => Err(format!("`{key}` must be a non-empty string")),
    }
}

fn validate_ticker(args: &Value) -> Result<(), String> {
    require_nonempty(args, "ticker")?;
    let t = args.get("ticker").and_then(|v| v.as_str()).unwrap_or("");
    if t.len() > 20
        || !t
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
    {
        return Err("ticker has invalid characters".into());
    }
    Ok(())
}

fn validate_read_page(args: &Value) -> Result<(), String> {
    require_nonempty(args, "url")?;
    let url = args.get("url").and_then(|v| v.as_str()).unwrap_or("");
    // Egress policy is enforced here so a private/loopback URL never executes.
    security::validate_url_for_egress(url).map_err(|e| e.to_string())?;
    Ok(())
}

fn validate_query(args: &Value) -> Result<(), String> {
    require_nonempty(args, "query")
}

fn validate_peers(args: &Value) -> Result<(), String> {
    match args.get("tickers").and_then(|v| v.as_array()) {
        Some(a) if !a.is_empty() => Ok(()),
        _ => Err("`tickers` must be a non-empty array".into()),
    }
}

fn validate_skill_name(args: &Value) -> Result<(), String> {
    require_nonempty(args, "name")
}

fn validate_memo_kind(args: &Value) -> Result<(), String> {
    let kind = args["kind"].as_str().unwrap_or("").trim();
    if crate::agent::memo::KINDS.contains(&kind) {
        Ok(())
    } else {
        Err(format!(
            "kind must be one of: {}",
            crate::agent::memo::KINDS.join(", ")
        ))
    }
}

// --- Provider-facing parameter schemas (single authority; see `agent_schemas`).

fn schema_ticker_only() -> Value {
    json!({ "type": "object", "properties": { "ticker": { "type": "string" } }, "required": ["ticker"] })
}
fn schema_get_quote() -> Value {
    schema_ticker_only()
}
fn schema_get_financials() -> Value {
    json!({
        "type": "object",
        "properties": {
            "ticker": { "type": "string", "description": "US-listed ticker, e.g. TSLA" },
            "year": { "type": "integer", "description": "Anchor fiscal year, e.g. 2025 (default: latest reported); the spread covers this year and earlier" },
            "years": { "type": "integer", "description": "How many fiscal years to return (default 3, max 6; annual basis only)" },
            "basis": { "type": "string", "enum": ["annual", "quarterly", "ltm"], "description": "annual (default): multi-year FY spread; quarterly: last 8 fiscal quarters (Q4 derived); ltm: trailing twelve months — use for comps and current-run-rate questions" }
        },
        "required": ["ticker"]
    })
}
fn schema_draft_memo() -> Value {
    json!({
        "type": "object",
        "properties": {
            "kind": { "type": "string", "enum": ["earnings_note", "company_profile", "deal_summary"], "description": "The memo type to draft." },
            "company": { "type": "string", "description": "Company display name for the title (optional; inferred from evidence when absent)." }
        },
        "required": ["kind"]
    })
}
fn schema_use_skill() -> Value {
    json!({
        "type": "object",
        "properties": { "name": { "type": "string", "description": "The skill name from the catalog." } },
        "required": ["name"]
    })
}
fn schema_get_news() -> Value {
    json!({
        "type": "object",
        "properties": { "query": { "type": "string" }, "limit": { "type": "integer" } },
        "required": ["query"]
    })
}
fn schema_list_filings() -> Value {
    json!({
        "type": "object",
        "properties": { "ticker": { "type": "string" }, "form": { "type": "string", "description": "e.g. 10-K, 10-Q, 8-K" } },
        "required": ["ticker"]
    })
}
fn schema_read_filing() -> Value {
    json!({
        "type": "object",
        "properties": {
            "ticker": { "type": "string" },
            "form": { "type": "string", "description": "e.g. 10-K, 10-Q (default 10-K)" },
            "item": { "type": "string", "description": "Item id, e.g. 1A (risk factors), 7 (MD&A)" }
        },
        "required": ["ticker"]
    })
}
fn schema_query_only() -> Value {
    json!({ "type": "object", "properties": { "query": { "type": "string" } }, "required": ["query"] })
}
fn schema_research() -> Value {
    json!({
        "type": "object",
        "properties": { "query": { "type": "string", "description": "The research question, in full." } },
        "required": ["query"]
    })
}
fn schema_read_page() -> Value {
    json!({ "type": "object", "properties": { "url": { "type": "string" } }, "required": ["url"] })
}
fn schema_analyze_pdf() -> Value {
    json!({
        "type": "object", "additionalProperties": false,
        "properties": {
            "artifact_id": { "type": "string", "description": "Opaque handle from pick_pdf_artifact" },
            "label": { "type": "string", "description": "Company/ticker label for the workbook" }
        },
        "required": ["artifact_id"]
    })
}
fn schema_benchmark_peers() -> Value {
    json!({
        "type": "object",
        "properties": {
            "tickers": { "type": "array", "items": { "type": "string" }, "description": "Peer tickers" },
            "period": { "type": "string", "enum": ["annual", "quarter", "semi", "ltm"] },
            "multiples": { "type": "boolean" },
            "usd": { "type": "boolean" }
        },
        "required": ["tickers"]
    })
}
fn schema_build_model() -> Value {
    json!({
        "type": "object",
        "properties": {
            "ticker": { "type": "string", "description": "Ticker, e.g. AAPL or SAND.ST" },
            "period": { "type": "string", "enum": ["annual", "quarter", "semi", "ltm"] },
            "years": { "type": "integer", "description": "Projection years (1-10)" },
            "skip_review": { "type": "boolean", "description": "Build immediately, skipping the assumptions grid" },
            "peers": { "type": "array", "items": { "type": "string" }, "description": "Optional peer tickers for a trading-comps tab" },
            "case": { "type": "string", "enum": ["base", "upside", "downside"], "description": "Scenario case (default base)" }
        },
        "required": ["ticker"]
    })
}

/// The tool registry.
pub struct ToolRegistry {
    specs: HashMap<&'static str, ToolSpec>,
}

impl ToolRegistry {
    /// Build the registry with all current capabilities registered.
    pub fn builtin() -> Self {
        let list = [
            ToolSpec {
                name: "get_quote",
                label: "Get quote",
                description: "Fetch the latest share price quote for a ticker.",
                risk: Risk::ReadOnly,
                capabilities: &["market", "quote"],
                required_args: &["ticker"],
                interruptible: true,
                idempotent: true,
                trust: TrustPolicy::Untrusted,
                validate: validate_ticker,
                params_schema: schema_get_quote,
                model_visible: true,
            },
            ToolSpec {
                name: "get_financials",
                label: "Get financials",
                description: "Get a company's EXACT reported financials from SEC EDGAR XBRL on three bases: annual (default; multi-year FY spread with income statement, balance sheet incl. debt, cash flow, interest, D&A, share counts), quarterly (last 8 fiscal quarters), or ltm (trailing twelve months — the comps basis). Growth, margins, EBITDA, FCF, leverage, interest coverage, and net cash are PRE-COMPUTED deterministically (use those numbers as-is; never recompute). Straight from SEC EDGAR XBRL — the right tool for a specific reported figure like 'what were Tesla's 2025 sales'. Returns precise, citable numbers from the 10-K. US filers only; for foreign filers use build_model.",
                risk: Risk::ReadOnly,
                capabilities: &["market", "filings", "financials"],
                required_args: &["ticker"],
                interruptible: true,
                idempotent: true,
                trust: TrustPolicy::Untrusted,
                validate: validate_ticker,
                params_schema: schema_get_financials,
                model_visible: true,
            },
            ToolSpec {
                name: "draft_memo",
                label: "Draft memo",
                description: "Write a professional memo (earnings_note, company_profile, or deal_summary) FROM THE EVIDENCE ALREADY GATHERED in this conversation - run get_financials/research/read_filing first, then call this to draft the write-up. Produces a Markdown artifact with cited prose, key-figure tables, and a sources list. Never invents numbers.",
                risk: Risk::ReadOnly,
                capabilities: &["drafting"],
                required_args: &["kind"],
                interruptible: true,
                idempotent: false,
                trust: TrustPolicy::Untrusted,
                validate: validate_memo_kind,
                params_schema: schema_draft_memo,
                model_visible: true,
            },
            ToolSpec {
                name: "use_skill",
                label: "Use skill",
                description: "Load a named skill's full step-by-step instructions from the user's skill library (see the Skills catalog in the system prompt), then follow them. Call this when the request matches a listed skill.",
                risk: Risk::ReadOnly,
                capabilities: &["skills"],
                required_args: &["name"],
                interruptible: true,
                idempotent: true,
                trust: TrustPolicy::Untrusted,
                validate: validate_skill_name,
                params_schema: schema_use_skill,
                model_visible: true,
            },
            ToolSpec {
                name: "get_news",
                label: "Get news",
                description: "Fetch recent news headlines for a ticker or query.",
                risk: Risk::ReadOnly,
                capabilities: &["news"],
                required_args: &["query"],
                interruptible: true,
                idempotent: true,
                trust: TrustPolicy::Untrusted,
                validate: validate_query,
                params_schema: schema_get_news,
                model_visible: true,
            },
            ToolSpec {
                name: "list_filings",
                label: "List filings",
                description: "List recent SEC EDGAR filings for a US ticker.",
                risk: Risk::ReadOnly,
                capabilities: &["filings", "edgar"],
                required_args: &["ticker"],
                interruptible: true,
                idempotent: true,
                trust: TrustPolicy::Untrusted,
                validate: validate_ticker,
                params_schema: schema_list_filings,
                model_visible: true,
            },
            ToolSpec {
                name: "read_filing",
                label: "Read filing",
                description: "Read the narrative text of a company's latest SEC filing (10-K/10-Q): risk factors (item 1A), MD&A (item 7), business description (item 1), financial statements & notes (item 8 — includes the SEGMENT reporting note: revenue/profit by business segment and geography, not available via XBRL company facts). For a SPECIFIC reported figure — revenue/sales, net income, EPS, margins — prefer `research` (cited) or `build_model`; do not scrape numbers out of narrative items. Never use web_search for filing content.",
                risk: Risk::ReadOnly,
                capabilities: &["filings", "edgar"],
                required_args: &["ticker"],
                interruptible: true,
                idempotent: true,
                trust: TrustPolicy::Untrusted,
                validate: validate_ticker,
                params_schema: schema_read_filing,
                model_visible: true,
            },
            ToolSpec {
                name: "web_search",
                label: "Web search",
                description: "Search the web and return ranked results with canonical URL, source, and date. Prefer `research` for a full cited answer; use this for a quick link lookup.",
                risk: Risk::ReadOnly,
                capabilities: &["web", "search"],
                required_args: &["query"],
                interruptible: true,
                idempotent: true,
                trust: TrustPolicy::Untrusted,
                validate: validate_query,
                params_schema: schema_query_only,
                model_visible: true,
            },
            ToolSpec {
                name: "read_page",
                label: "Read page",
                description: "Fetch and read the readable text of a public web page by URL (HTTP(S) only; SSRF-guarded).",
                risk: Risk::ReadOnly,
                capabilities: &["web"],
                required_args: &["url"],
                interruptible: true,
                idempotent: true,
                trust: TrustPolicy::Untrusted,
                validate: validate_read_page,
                params_schema: schema_read_page,
                model_visible: true,
            },
            ToolSpec {
                name: "analyze_pdf",
                label: "Analyze PDF",
                description: "Analyze a local annual-report PDF (registered via the file picker) into a 3-statement + DCF model. Requires an OpenRouter API key and a picker-minted artifact_id — never a raw filesystem path.",
                risk: Risk::ReadOnly,
                capabilities: &["pdf", "extract"],
                required_args: &["artifact_id"],
                interruptible: true,
                idempotent: true,
                trust: TrustPolicy::Untrusted,
                validate: |a| require_nonempty(a, "artifact_id"),
                params_schema: schema_analyze_pdf,
                model_visible: true,
            },
            ToolSpec {
                name: "research",
                label: "Research",
                description: "Research a question with web search + page/filing reading + cited synthesis. Use for current, factual, entity, or numeric questions that need up-to-date primary-source evidence (e.g. revenue growth, market trends, guidance). Returns a cited answer.",
                risk: Risk::ReadOnly,
                capabilities: &["research", "web"],
                required_args: &["query"],
                interruptible: true,
                idempotent: false,
                trust: TrustPolicy::Untrusted,
                validate: validate_query,
                params_schema: schema_research,
                model_visible: true,
            },
            ToolSpec {
                name: "research_deal",
                label: "Research deal",
                description: "M&A / deal research and synthesis.",
                risk: Risk::ReadOnly,
                capabilities: &["research", "deal"],
                required_args: &["query"],
                interruptible: true,
                idempotent: false,
                trust: TrustPolicy::Untrusted,
                validate: validate_query,
                params_schema: schema_query_only,
                model_visible: false,
            },
            ToolSpec {
                name: "benchmark_peers",
                label: "Benchmark peers",
                description: "Benchmark a set of peer tickers (revenue, margins, ROE, leverage) into a comparison workbook.",
                risk: Risk::ReadOnly,
                capabilities: &["comps", "benchmark"],
                required_args: &["tickers"],
                interruptible: true,
                idempotent: true,
                trust: TrustPolicy::Untrusted,
                validate: validate_peers,
                params_schema: schema_benchmark_peers,
                model_visible: true,
            },
            ToolSpec {
                name: "build_model",
                label: "Build model",
                description: "Build a 3-statement + DCF Excel model for a ticker from SEC EDGAR. By default it presents an editable assumptions grid to the user; the user finalizes it manually. Set skip_review=true to build immediately without review.",
                risk: Risk::LocalCreate,
                capabilities: &["model", "excel", "artifact"],
                required_args: &["ticker"],
                interruptible: false,
                idempotent: false,
                trust: TrustPolicy::Trusted,
                validate: validate_ticker,
                params_schema: schema_build_model,
                model_visible: true,
            },
        ];
        let mut specs = HashMap::new();
        for s in list {
            specs.insert(s.name, s);
        }
        ToolRegistry { specs }
    }

    pub fn get(&self, name: &str) -> Option<&ToolSpec> {
        self.specs.get(name)
    }

    pub fn names(&self) -> Vec<&'static str> {
        let mut v: Vec<&'static str> = self.specs.keys().copied().collect();
        v.sort_unstable();
        v
    }

    /// Validate a tool call: name known, required args present & non-null, and
    /// the semantic validator passes. Never executes.
    pub fn validate_call(&self, name: &str, args: &Value) -> Result<(), ToolError> {
        let spec = self
            .specs
            .get(name)
            .ok_or_else(|| ToolError::UnknownTool(name.to_string()))?;
        for req in spec.required_args {
            let present = args.get(*req).map(|v| !v.is_null()).unwrap_or(false);
            if !present {
                return Err(ToolError::MissingArg {
                    tool: name.to_string(),
                    arg: (*req).to_string(),
                });
            }
        }
        (spec.validate)(args).map_err(|reason| ToolError::Invalid {
            tool: name.to_string(),
            reason,
        })
    }

    /// The stable tool catalog text for the context builder / provider.
    pub fn catalog(&self) -> String {
        let mut lines: Vec<String> = self
            .specs
            .values()
            .map(|s| format!("- {} ({}): {}", s.name, s.label, s.description))
            .collect();
        lines.sort();
        lines.join("\n")
    }

    /// Provider tool array (OpenAI function-tool shape) for every model-visible
    /// tool. The single authority the driver hands the provider (Task 1.1);
    /// replaces the removed `commands::chat::tool_schemas`/`agent_tool_schemas`.
    pub fn agent_schemas(&self) -> Vec<Value> {
        let mut specs: Vec<&ToolSpec> = self.specs.values().filter(|s| s.model_visible).collect();
        specs.sort_by_key(|s| s.name);
        specs
            .into_iter()
            .map(|s| {
                json!({
                    "type": "function",
                    "function": {
                        "name": s.name,
                        "description": s.description,
                        "parameters": (s.params_schema)(),
                    }
                })
            })
            .collect()
    }

    /// A process-wide shared registry, so hot paths never rebuild `builtin()`
    /// per tool wave (Task 1.1).
    pub fn shared() -> &'static ToolRegistry {
        static REG: LazyLock<ToolRegistry> = LazyLock::new(ToolRegistry::builtin);
        &REG
    }

    /// Validate that every workflow's required/allowed tools exist in the live
    /// registry. Returns the first offending `(workflow, tool)` pair. Called at
    /// startup so a stale workflow reference fails deterministically.
    pub fn validate_workflows(&self) -> Result<(), (String, String)> {
        for wf in fm_agent::workflows::builtin_workflows() {
            for t in wf.required_tools.iter().chain(wf.allowed_tools.iter()) {
                if !self.specs.contains_key(*t) {
                    return Err((wf.id.to_string(), (*t).to_string()));
                }
            }
        }
        Ok(())
    }

    /// Estimated token cost of the full model-visible schema set (~4 chars/token).
    pub fn schema_token_estimate(&self) -> usize {
        self.agent_schemas()
            .iter()
            .map(|s| s.to_string().len())
            .sum::<usize>()
            / 4
    }

    /// Progressive tool disclosure fires only when the deferred schema cost would
    /// exceed 10% of the model context window (or 20,000 tokens when the window is
    /// unknown / `0`), per the Hermes threshold (Task 6.2).
    pub fn should_defer_tools(&self, context_window: usize) -> bool {
        let threshold = if context_window == 0 {
            20_000
        } else {
            context_window / 10
        };
        self.schema_token_estimate() > threshold
    }

    /// Deterministic relevance ranking of every tool for a query: term overlap
    /// over name + description + capabilities, stable tie-break by name. Used to
    /// rank the deferred catalog; no hidden model call, fully reproducible.
    pub fn rank_for_query(&self, query: &str) -> Vec<&'static str> {
        let terms: Vec<String> = query
            .to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|t| t.len() >= 3)
            .map(|s| s.to_string())
            .collect();
        let mut scored: Vec<(&'static str, usize)> = self
            .specs
            .values()
            .map(|s| {
                let hay = format!("{} {} {}", s.name, s.description, s.capabilities.join(" "))
                    .to_lowercase();
                let score = terms.iter().filter(|t| hay.contains(t.as_str())).count();
                (s.name, score)
            })
            .collect();
        scored.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
        scored.into_iter().map(|(n, _)| n).collect()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::builtin()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_capabilities_registered() {
        let r = ToolRegistry::builtin();
        for name in [
            "get_quote",
            "get_news",
            "list_filings",
            "read_filing",
            "web_search",
            "read_page",
            "analyze_pdf",
            "research",
            "research_deal",
            "benchmark_peers",
            "build_model",
            "get_financials",
            "use_skill",
            "draft_memo",
        ] {
            assert!(r.get(name).is_some(), "missing {name}");
        }
        assert_eq!(r.names().len(), 14);
    }

    #[test]
    fn all_workflow_tools_registered() {
        assert!(ToolRegistry::builtin().validate_workflows().is_ok());
    }

    #[test]
    fn research_deal_dispatchable_but_not_model_visible() {
        let r = ToolRegistry::builtin();
        assert!(r.get("research_deal").is_some(), "still dispatchable");
        let names: Vec<String> = r
            .agent_schemas()
            .iter()
            .map(|t| t["function"]["name"].as_str().unwrap().to_string())
            .collect();
        assert!(!names.contains(&"research_deal".to_string()));
    }

    #[test]
    fn unknown_tool_rejected() {
        let r = ToolRegistry::builtin();
        assert_eq!(
            r.validate_call("frobnicate", &serde_json::json!({})),
            Err(ToolError::UnknownTool("frobnicate".into()))
        );
    }

    #[test]
    fn missing_required_arg_rejected() {
        let r = ToolRegistry::builtin();
        assert_eq!(
            r.validate_call("get_quote", &serde_json::json!({})),
            Err(ToolError::MissingArg {
                tool: "get_quote".into(),
                arg: "ticker".into()
            })
        );
    }

    #[test]
    fn valid_calls_pass() {
        let r = ToolRegistry::builtin();
        assert!(r
            .validate_call("get_quote", &serde_json::json!({"ticker":"NVDA"}))
            .is_ok());
        assert!(r
            .validate_call(
                "web_search",
                &serde_json::json!({"query":"nvidia earnings"})
            )
            .is_ok());
        assert!(r
            .validate_call(
                "benchmark_peers",
                &serde_json::json!({"tickers":["NVDA","AMD"]})
            )
            .is_ok());
    }

    #[test]
    fn semantic_validation_rejects_bad_ticker() {
        let r = ToolRegistry::builtin();
        assert!(matches!(
            r.validate_call("get_quote", &serde_json::json!({"ticker":"not a ticker!!"})),
            Err(ToolError::Invalid { .. })
        ));
    }

    #[test]
    fn read_page_enforces_ssrf_policy() {
        let r = ToolRegistry::builtin();
        // Private/loopback URL is rejected at validation, before any fetch.
        assert!(matches!(
            r.validate_call(
                "read_page",
                &serde_json::json!({"url":"http://127.0.0.1/admin"})
            ),
            Err(ToolError::Invalid { .. })
        ));
        // A public URL passes.
        assert!(r
            .validate_call("read_page", &serde_json::json!({"url":"https://8.8.8.8/"}))
            .is_ok());
    }

    #[test]
    fn empty_peer_set_rejected() {
        let r = ToolRegistry::builtin();
        assert!(matches!(
            r.validate_call("benchmark_peers", &serde_json::json!({"tickers":[]})),
            Err(ToolError::Invalid { .. })
        ));
    }

    #[test]
    fn risk_and_trust_metadata() {
        let r = ToolRegistry::builtin();
        assert_eq!(r.get("get_quote").unwrap().risk, Risk::ReadOnly);
        assert_eq!(r.get("build_model").unwrap().risk, Risk::LocalCreate);
        assert_eq!(r.get("build_model").unwrap().trust, TrustPolicy::Trusted);
        assert_eq!(r.get("read_page").unwrap().trust, TrustPolicy::Untrusted);
        assert!(!r.get("build_model").unwrap().interruptible);
        assert!(r.get("get_quote").unwrap().interruptible);
    }

    #[test]
    fn catalog_lists_all_tools_sorted() {
        let r = ToolRegistry::builtin();
        let cat = r.catalog();
        assert_eq!(cat.lines().count(), 14);
        assert!(cat.contains("build_model"));
    }

    #[test]
    fn analyze_pdf_requires_artifact_id_not_path() {
        let r = ToolRegistry::builtin();
        assert!(matches!(
            r.validate_call("analyze_pdf", &serde_json::json!({"path":"C:/x.pdf"})),
            Err(ToolError::MissingArg { .. })
        ));
        assert!(r
            .validate_call(
                "analyze_pdf",
                &serde_json::json!({"artifact_id":"art-0123456789abcdef0123456789abcdef"}),
            )
            .is_ok());
    }

    // keep `ok` referenced (reserved for tools with no extra validation)
    #[test]
    fn ok_validator_passes() {
        assert!(ok(&serde_json::json!({})).is_ok());
    }

    #[test]
    fn progressive_disclosure_threshold() {
        let r = ToolRegistry::builtin();
        assert!(r.schema_token_estimate() > 0);
        // Tiny window → small 10% threshold → defer.
        assert!(r.should_defer_tools(100));
        // Huge window → never defer.
        assert!(!r.should_defer_tools(100_000_000));
    }

    #[test]
    fn rank_for_query_orders_by_relevance_deterministically() {
        let r = ToolRegistry::builtin();
        let ranked = r.rank_for_query("build a dcf excel model");
        assert_eq!(ranked[0], "build_model");
        // Deterministic: identical query → identical order.
        assert_eq!(ranked, r.rank_for_query("build a dcf excel model"));
        assert_eq!(ranked.len(), 14);
    }
}
