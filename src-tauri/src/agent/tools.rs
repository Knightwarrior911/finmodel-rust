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

use fm_agent::types::Risk;
use serde_json::Value;

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
    if t.len() > 20 || !t.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-') {
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
                description: "Latest market quote for a ticker.",
                risk: Risk::ReadOnly,
                capabilities: &["market", "quote"],
                required_args: &["ticker"],
                interruptible: true,
                idempotent: true,
                trust: TrustPolicy::Untrusted,
                validate: validate_ticker,
            },
            ToolSpec {
                name: "get_financials",
                label: "Get financials",
                description: "Exact reported annual financials from SEC EDGAR XBRL.",
                risk: Risk::ReadOnly,
                capabilities: &["market", "filings", "financials"],
                required_args: &["ticker"],
                interruptible: true,
                idempotent: true,
                trust: TrustPolicy::Untrusted,
                validate: validate_ticker,
            },
            ToolSpec {
                name: "get_news",
                label: "Get news",
                description: "Recent news headlines for a topic or ticker.",
                risk: Risk::ReadOnly,
                capabilities: &["news"],
                required_args: &["query"],
                interruptible: true,
                idempotent: true,
                trust: TrustPolicy::Untrusted,
                validate: validate_query,
            },
            ToolSpec {
                name: "list_filings",
                label: "List filings",
                description: "Recent SEC filings for a ticker.",
                risk: Risk::ReadOnly,
                capabilities: &["filings", "edgar"],
                required_args: &["ticker"],
                interruptible: true,
                idempotent: true,
                trust: TrustPolicy::Untrusted,
                validate: validate_ticker,
            },
            ToolSpec {
                name: "read_filing",
                label: "Read filing",
                description: "Fetch and section a filing (item 1A/7).",
                risk: Risk::ReadOnly,
                capabilities: &["filings", "edgar"],
                required_args: &["ticker"],
                interruptible: true,
                idempotent: true,
                trust: TrustPolicy::Untrusted,
                validate: validate_ticker,
            },
            ToolSpec {
                name: "web_search",
                label: "Web search",
                description: "Search the web for current information.",
                risk: Risk::ReadOnly,
                capabilities: &["web", "search"],
                required_args: &["query"],
                interruptible: true,
                idempotent: true,
                trust: TrustPolicy::Untrusted,
                validate: validate_query,
            },
            ToolSpec {
                name: "read_page",
                label: "Read page",
                description: "Fetch and read a web page (SSRF-guarded).",
                risk: Risk::ReadOnly,
                capabilities: &["web"],
                required_args: &["url"],
                interruptible: true,
                idempotent: true,
                trust: TrustPolicy::Untrusted,
                validate: validate_read_page,
            },
            ToolSpec {
                name: "analyze_pdf",
                label: "Analyze PDF",
                description: "Extract financials from a picker-minted PDF artifact (never a raw path).",
                risk: Risk::ReadOnly,
                capabilities: &["pdf", "extract"],
                required_args: &["artifact_id"],
                interruptible: true,
                idempotent: true,
                trust: TrustPolicy::Untrusted,
                validate: |a| require_nonempty(a, "artifact_id"),
            },
            ToolSpec {
                name: "research",
                label: "Research",
                description: "Bounded search→read→synthesize research cascade.",
                risk: Risk::ReadOnly,
                capabilities: &["research", "web"],
                required_args: &["query"],
                interruptible: true,
                idempotent: false,
                trust: TrustPolicy::Untrusted,
                validate: validate_query,
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
            },
            ToolSpec {
                name: "benchmark_peers",
                label: "Benchmark peers",
                description: "Trading comps over a typed peer set.",
                risk: Risk::ReadOnly,
                capabilities: &["comps", "benchmark"],
                required_args: &["tickers"],
                interruptible: true,
                idempotent: true,
                trust: TrustPolicy::Untrusted,
                validate: validate_peers,
            },
            ToolSpec {
                name: "build_model",
                label: "Build model",
                description: "3-statement + DCF model; writes an immutable workbook.",
                risk: Risk::LocalCreate,
                capabilities: &["model", "excel", "artifact"],
                required_args: &["ticker"],
                interruptible: false,
                idempotent: false,
                trust: TrustPolicy::Trusted,
                validate: validate_ticker,
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
            "get_quote", "get_news", "list_filings", "read_filing", "web_search", "read_page",
            "analyze_pdf", "research", "research_deal", "benchmark_peers", "build_model",
            "get_financials",
        ] {
            assert!(r.get(name).is_some(), "missing {name}");
        }
        assert_eq!(r.names().len(), 12);
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
            Err(ToolError::MissingArg { tool: "get_quote".into(), arg: "ticker".into() })
        );
    }

    #[test]
    fn valid_calls_pass() {
        let r = ToolRegistry::builtin();
        assert!(r.validate_call("get_quote", &serde_json::json!({"ticker":"NVDA"})).is_ok());
        assert!(r.validate_call("web_search", &serde_json::json!({"query":"nvidia earnings"})).is_ok());
        assert!(r
            .validate_call("benchmark_peers", &serde_json::json!({"tickers":["NVDA","AMD"]}))
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
            r.validate_call("read_page", &serde_json::json!({"url":"http://127.0.0.1/admin"})),
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
        assert_eq!(cat.lines().count(), 11);
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
}
