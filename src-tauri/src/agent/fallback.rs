//! The isolated `FallbackDispatcher`: deterministic no-key natural-language
//! intents plus typed Quick Actions. It uses strict deterministic parsers and
//! the same [`ToolRegistry`](crate::agent::tools) validators, and it NEVER calls
//! the keyed agent path — keyed ordinary chat never enters here.

use serde_json::json;

use crate::agent::tools::{ToolError, ToolRegistry};

/// What the dispatcher resolved a message to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FallbackDecision {
    /// Execute this validated tool with these args.
    Tool {
        name: String,
        args: serde_json::Value,
    },
    /// No tool matched — answer directly (bare definitional question).
    Direct,
}

/// A word shaped like a filing-form designation (`10-K`, `8-K`, `20-F`, `S-1`):
/// a hyphen with digits on one side and a letter on the other. Such words never
/// contribute a ticker.
fn is_filing_form(word: &str) -> bool {
    let w = word.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '-');
    let Some((l, r)) = w.split_once('-') else {
        return false;
    };
    if l.is_empty() || r.is_empty() {
        return false;
    }
    let l_digit = l.chars().all(|c| c.is_ascii_digit());
    let r_digit = r.chars().all(|c| c.is_ascii_digit());
    let l_alpha = l.chars().all(|c| c.is_ascii_alphabetic());
    let r_alpha = r.chars().all(|c| c.is_ascii_alphabetic());
    (l_digit && r_alpha) || (l_alpha && r_digit)
}

/// Whether `sym[.ex]` is a ticker shape: 1–5 uppercase letters, optional
/// 1–4 uppercase exchange suffix.
fn is_ticker_shape(raw: &str) -> bool {
    const STOP: &[&str] = &[
        "A", "I", "THE", "DCF", "PDF", "SEC", "CEO", "CFO", "USD", "EUR", "AND", "OR", "US", "EV",
        "IPO", "GAAP", "IFRS", "Q1", "Q2", "Q3", "Q4", "FY",
    ];
    let (sym, ex) = match raw.split_once('.') {
        Some((s, e)) => (s, Some(e)),
        None => (raw, None),
    };
    let looks = (1..=5).contains(&sym.len())
        && sym.chars().all(|c| c.is_ascii_uppercase())
        && ex
            .map(|e| (1..=4).contains(&e.len()) && e.chars().all(|c| c.is_ascii_uppercase()))
            .unwrap_or(true);
    looks && !STOP.contains(&sym)
}

/// Extract the first ticker-shaped token, skipping filing-form designations and
/// common all-caps stopwords. Single-letter tickers (F, T, C, V) are valid.
pub fn first_ticker(msg: &str) -> Option<String> {
    for word in msg.split_whitespace() {
        // Path-like tokens (Windows paths, URLs) must not contribute tickers —
        // otherwise `C:/tmp/x.pdf` yields ticker `C`.
        if word.contains('/') || word.contains('\\') {
            continue;
        }
        if is_filing_form(word) {
            continue;
        }
        for raw in word.split(|c: char| !(c.is_ascii_alphanumeric() || c == '.')) {
            if !raw.is_empty() && is_ticker_shape(raw) {
                return Some(raw.to_string());
            }
        }
    }
    None
}

/// All ticker-shaped tokens (for peer sets), in order, de-duplicated.
pub fn all_tickers(msg: &str) -> Vec<String> {
    let mut out = Vec::new();
    for word in msg.split_whitespace() {
        if word.contains('/') || word.contains('\\') {
            continue;
        }
        if is_filing_form(word) {
            continue;
        }
        for raw in word.split(|c: char| !(c.is_ascii_alphanumeric() || c == '.')) {
            if !raw.is_empty() && is_ticker_shape(raw) && !out.contains(&raw.to_string()) {
                out.push(raw.to_string());
            }
        }
    }
    out
}

fn has_any(m: &str, words: &[&str]) -> bool {
    words.iter().any(|w| m.contains(w))
}

/// Deterministically resolve a free-text no-key message to a tool or Direct.
/// Precedence matters: explicit web search → news → quote → filings →
/// benchmark (before build) → build → research → direct.
pub fn dispatch(registry: &ToolRegistry, message: &str) -> FallbackDecision {
    let m = message.to_lowercase();
    let ticker = first_ticker(message);
    let tickers = all_tickers(message);

    // 1. Explicit web search. Keep this ahead of news: "use web search for
    // latest news" means the user chose the search backend, not the news feed.
    if has_any(
        &m,
        &[
            "web_search",
            "web search",
            "search the web",
            "google search",
        ],
    ) {
        return tool(
            registry,
            "web_search",
            json!({ "query": strip_lead(message) }),
        );
    }

    // 2. News.
    if has_any(&m, &["news", "headline", "headlines", "latest on"]) {
        let topic = ticker.clone().unwrap_or_else(|| strip_lead(message));
        return tool(registry, "get_news", json!({ "query": topic }));
    }
    // 2. Quote / price.
    if has_any(
        &m,
        &["price", "quote", "trading at", "share price", "stock price"],
    ) {
        if let Some(t) = &ticker {
            return tool(registry, "get_quote", json!({ "ticker": t }));
        }
    }
    // 3. Filings.
    if has_any(
        &m,
        &[
            "10-k",
            "10-q",
            "10k",
            "10q",
            "filing",
            "filings",
            "risk factor",
            "md&a",
            "mda",
        ],
    ) {
        if let Some(t) = &ticker {
            let name = if has_any(&m, &["risk factor", "md&a", "mda", "read"]) {
                "read_filing"
            } else {
                "list_filings"
            };
            return tool(registry, name, json!({ "ticker": t }));
        }
    }
    // 4. Benchmark / comps (BEFORE build).
    if has_any(&m, &["benchmark", "comps", "compare", "peers", "peer set"]) && tickers.len() >= 2 {
        return tool(registry, "benchmark_peers", json!({ "tickers": tickers }));
    }
    // 5. Build / model / valuation.
    if has_any(
        &m,
        &[
            "build",
            "model",
            "dcf",
            "valuation",
            "3-statement",
            "three statement",
        ],
    ) {
        if let Some(t) = &ticker {
            return tool(registry, "build_model", json!({ "ticker": t }));
        }
    }
    // 6. Research / deal.
    if has_any(&m, &["m&a", "acquisition", "merger", "deal"]) {
        return tool(
            registry,
            "research_deal",
            json!({ "query": strip_lead(message) }),
        );
    }
    if has_any(&m, &["research", "look into", "investigate", "find out"]) {
        return tool(
            registry,
            "research",
            json!({ "query": strip_lead(message) }),
        );
    }
    // 7. Direct answer (bare definitional question).
    FallbackDecision::Direct
}

/// Map a typed Quick Action to a validated tool call. Unknown actions or invalid
/// args return a typed error.
pub fn quick_action(
    registry: &ToolRegistry,
    action: &str,
    args: serde_json::Value,
) -> Result<FallbackDecision, ToolError> {
    let tool_name = match action {
        "quote" => "get_quote",
        "news" => "get_news",
        "filings" => "list_filings",
        "read_filing" => "read_filing",
        "search" => "web_search",
        "comps" => "benchmark_peers",
        "build" => "build_model",
        "research" => "research",
        other => return Err(ToolError::UnknownTool(other.to_string())),
    };
    registry.validate_call(tool_name, &args)?;
    Ok(FallbackDecision::Tool {
        name: tool_name.to_string(),
        args,
    })
}

/// A validated tool decision, or Direct if validation fails (never fabricate a
/// tool call the registry rejects).
fn tool(registry: &ToolRegistry, name: &str, args: serde_json::Value) -> FallbackDecision {
    match registry.validate_call(name, &args) {
        Ok(()) => FallbackDecision::Tool {
            name: name.to_string(),
            args,
        },
        Err(_) => FallbackDecision::Direct,
    }
}

/// Strip common leading filler so the search/research topic is clean.
fn strip_lead(message: &str) -> String {
    let leads = [
        "search the web for ",
        "search for ",
        "research ",
        "look into ",
        "find out about ",
        "tell me about ",
        "what's the latest on ",
        "latest on ",
        "news on ",
        "news about ",
    ];
    let lower = message.trim().to_lowercase();
    for l in leads {
        if lower.starts_with(l) {
            return message.trim()[l.len()..].trim().to_string();
        }
    }
    message.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reg() -> ToolRegistry {
        ToolRegistry::builtin()
    }

    #[test]
    fn path_like_tokens_do_not_yield_drive_letter_tickers() {
        // TESTCO is 6 letters (not a ticker shape); path token is skipped → None.
        assert_eq!(
            first_ticker(r#"Analyze the filing PDF at "C:/tmp/annual.pdf" for TESTCO"#),
            None
        );
        assert_eq!(
            first_ticker(r#"open C:/data/AAPL.pdf about AAPL"#),
            Some("AAPL".into())
        );
        assert!(first_ticker(r#"see C:\Users\x\a.pdf"#).is_none());
    }

    #[test]
    fn ticker_extraction() {
        assert_eq!(
            first_ticker("build a model for NVDA please"),
            Some("NVDA".to_string())
        );
        assert_eq!(first_ticker("check TSM.TW"), Some("TSM.TW".to_string()));
        // Stopwords are not tickers.
        assert_eq!(first_ticker("what is a DCF"), None);
        assert_eq!(first_ticker("the quick brown fox"), None);
        // Filing-form designations are not tickers, but a real ticker in the
        // same message still resolves.
        assert_eq!(
            first_ticker("show me the latest 10-K for MSFT"),
            Some("MSFT".to_string())
        );
        assert_eq!(first_ticker("pull the S-1"), None);
        // Single-letter tickers are valid (Ford, AT&T).
        assert_eq!(first_ticker("quote for F"), Some("F".to_string()));
    }

    #[test]
    fn news_intent() {
        let d = dispatch(&reg(), "latest news on NVDA");
        assert_eq!(
            d,
            FallbackDecision::Tool {
                name: "get_news".into(),
                args: json!({"query":"NVDA"})
            }
        );
    }

    #[test]
    fn quote_intent() {
        let d = dispatch(&reg(), "what's the price of AAPL");
        assert_eq!(
            d,
            FallbackDecision::Tool {
                name: "get_quote".into(),
                args: json!({"ticker":"AAPL"})
            }
        );
    }

    #[test]
    fn filing_intent() {
        let d = dispatch(&reg(), "show me the latest 10-K for MSFT");
        assert_eq!(
            d,
            FallbackDecision::Tool {
                name: "list_filings".into(),
                args: json!({"ticker":"MSFT"})
            }
        );
        let d2 = dispatch(&reg(), "read the risk factors in AMD's filing");
        assert_eq!(
            d2,
            FallbackDecision::Tool {
                name: "read_filing".into(),
                args: json!({"ticker":"AMD"})
            }
        );
    }

    #[test]
    fn benchmark_precedes_build() {
        // "build" keyword present, but comps + 2 tickers wins.
        let d = dispatch(&reg(), "build a comps benchmark for NVDA and AMD");
        assert_eq!(
            d,
            FallbackDecision::Tool {
                name: "benchmark_peers".into(),
                args: json!({"tickers":["NVDA","AMD"]})
            }
        );
    }

    #[test]
    fn build_intent_single_ticker() {
        let d = dispatch(&reg(), "build a DCF model for NVDA");
        assert_eq!(
            d,
            FallbackDecision::Tool {
                name: "build_model".into(),
                args: json!({"ticker":"NVDA"})
            }
        );
    }

    #[test]
    fn deal_and_research_intents() {
        assert_eq!(
            dispatch(&reg(), "any M&A activity in medtech"),
            FallbackDecision::Tool {
                name: "research_deal".into(),
                args: json!({"query":"any M&A activity in medtech"})
            }
        );
        match dispatch(&reg(), "research the semiconductor supply chain") {
            FallbackDecision::Tool { name, .. } => assert_eq!(name, "research"),
            _ => panic!("expected research tool"),
        }
    }

    #[test]
    fn bare_question_is_direct() {
        assert_eq!(
            dispatch(&reg(), "what is a discounted cash flow"),
            FallbackDecision::Direct
        );
    }

    #[test]
    fn quick_action_maps_and_validates() {
        let ok = quick_action(&reg(), "quote", json!({"ticker":"NVDA"})).unwrap();
        assert_eq!(
            ok,
            FallbackDecision::Tool {
                name: "get_quote".into(),
                args: json!({"ticker":"NVDA"})
            }
        );
        // Invalid args are rejected, not fabricated.
        assert!(quick_action(&reg(), "quote", json!({})).is_err());
        assert!(quick_action(&reg(), "nonexistent", json!({})).is_err());
    }

    #[test]
    fn invalid_extracted_call_falls_back_to_direct() {
        // A build request whose only "ticker" is a stopword -> no ticker -> not build.
        assert_eq!(dispatch(&reg(), "build a model"), FallbackDecision::Direct);
    }

    /// Legacy no-key free-text fixture corpus: each message resolves through the
    /// FallbackDispatcher, validates against the registry, and (when a tool is
    /// chosen) executes via the FakeBackend envelope seam — without calling
    /// `route_intent` or a live LLM.
    #[test]
    fn no_key_corpus_traverses_registry_and_executors() {
        use crate::agent::executors::{execute, quote_card, FakeBackend, SessionContext};
        use serde_json::json;

        let reg = reg();
        let backend = FakeBackend::new()
            .seed_ok("get_quote", "AAPL 190", quote_card("AAPL", 190.0))
            .seed_ok("get_news", "news", json!({"type":"news"}))
            .seed_ok("list_filings", "filings", json!({"type":"filings","ticker":"MSFT"}))
            .seed_ok("read_filing", "risks", json!({"type":"filing_doc","ticker":"AMD","url":"https://www.sec.gov/x"}))
            .seed_ok("benchmark_peers", "comps", json!({"type":"benchmark"}))
            .seed_ok("build_model", "model", json!({"type":"model","ticker":"NVDA","artifact_id":"art-0123456789abcdef0123456789abcdef"}))
            .seed_ok("research_deal", "deal", json!({"type":"deal"}))
            .seed_ok("research", "research", json!({"type":"research_answer"}))
            .seed_ok("web_search", "hits", json!({"type":"search"}));

        let corpus: &[(&str, Option<&str>)] = &[
            ("quote AAPL", Some("get_quote")),
            ("news NVDA", Some("get_news")),
            ("show filings for MSFT", Some("list_filings")),
            ("read the risk factors in AMD's filing", Some("read_filing")),
            ("benchmark AAPL, MSFT", Some("benchmark_peers")),
            ("build a dcf model for NVDA", Some("build_model")),
            ("the figma adobe merger", Some("research_deal")),
            ("research the semiconductor supply chain", Some("research")),
            ("search the web for margins", Some("web_search")),
            ("what is a discounted cash flow", None),
            ("tell me a joke about accounting", None),
            (
                "Analyze the filing PDF at \"C:/tmp/annual.pdf\" for TESTCO",
                None,
            ),
        ];

        let ctx = SessionContext::test_ctx("c1", "corpus");
        for (msg, expected) in corpus {
            let d = dispatch(&reg, msg);
            match (expected, d) {
                (None, FallbackDecision::Direct) => {}
                (Some(name), FallbackDecision::Tool { name: n, args }) => {
                    assert_eq!(&n, name, "msg={msg}");
                    // Must validate + execute through the new seam.
                    let env = execute(&reg, &backend, &n, &args, &ctx)
                        .unwrap_or_else(|e| panic!("execute {name} for '{msg}': {e}"));
                    assert!(!env.summary.is_empty());
                }
                (exp, got) => panic!("msg={msg}: expected {exp:?}, got {got:?}"),
            }
        }
    }
}
