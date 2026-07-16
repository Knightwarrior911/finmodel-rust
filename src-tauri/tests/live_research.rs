//! Live model/browser matrix — OPT-IN, never PR CI (Phase 7 verification §4).
//!
//! A named integration target (so a zero-match run can't false-pass). Ignored by
//! default; run explicitly with a key + network:
//!
//! ```text
//! cargo test --manifest-path src-tauri/Cargo.toml --test live_research \
//!     -- --ignored --exact --nocapture
//! ```
//!
//! `OPENROUTER_API_KEY` (required) and `FINMODEL_MODEL` (optional; defaults to a
//! low-cost text model) come from the environment — never a key argument. The
//! test proves the LIVE weak model produces a schema-valid, quote-grounded
//! synthesis that passes `validate_synthesis`, or fails honestly.

use finmodel_app_lib::commands::research::{
    run_research, HttpBackend, OpenRouterSynthesizer, ResearchSynthesizer,
};
use fm_research::research::{
    ResearchDepth, ResearchMode, ResearchRequest, SourceBackend, SourceKind, SourceRecord,
    SourceStatus,
};

fn source(id: &str, kind: SourceKind, excerpt: &str) -> SourceRecord {
    SourceRecord {
        id: id.into(),
        requested_url: format!("https://example.com/{id}"),
        final_url: Some(format!("https://example.com/{id}")),
        canonical_url: format!("https://example.com/{id}"),
        title: format!("Source {id}"),
        domain: "example.com".into(),
        retrieved_at: "2026-01-01T00:00:00Z".into(),
        status: SourceStatus::Read,
        kind,
        backend: SourceBackend::BasicHttp,
        snippet: None,
        excerpt: Some(excerpt.into()),
        error_code: None,
    }
}

#[ignore = "requires OPENROUTER_API_KEY and network"]
#[tokio::test]
async fn live_synthesis_matrix() {
    let api_key = match std::env::var("OPENROUTER_API_KEY") {
        Ok(k) if !k.trim().is_empty() => k,
        _ => panic!("set OPENROUTER_API_KEY to run the live synthesis matrix"),
    };
    let model =
        std::env::var("FINMODEL_MODEL").unwrap_or_else(|_| "deepseek/deepseek-chat".to_string());

    let request = ResearchRequest {
        question: "What is the investment case for Nvidia? Cover catalysts and risks.".into(),
        mode: ResearchMode::Web,
        tickers: vec!["NVDA".into()],
        periods: vec![],
        filing_forms: vec![],
        target: None,
        acquirer: None,
        depth: ResearchDepth::Standard,
    };

    // Two readable sources with verbatim, quotable facts (one primary, one news).
    let read = vec![
        source(
            "S1",
            SourceKind::Regulatory,
            "Nvidia reported data center revenue of $30.8 billion, up 154% year over year, driven by demand for AI accelerators.",
        ),
        source(
            "S2",
            SourceKind::Newswire,
            "Analysts warn that competition from custom AI chips and export restrictions to China are the main risks to Nvidia's growth.",
        ),
    ];

    // deepseek-v4-flash advertises structured_outputs → use strict json_schema.
    let synth = OpenRouterSynthesizer {
        api_key: api_key.clone(),
        model: model.clone(),
        strict_json: true,
        request: request.clone(),
    };

    let answer = synth.synthesize(1, &read).await;
    match answer {
        Ok(a) => {
            println!("MODEL: {model}");
            println!("CONFIDENCE: {:?}", a.confidence);
            println!("SUMMARY: {}", a.summary.text);
            for c in &a.summary.citations {
                println!("  cite {} -> {:?}", c.source_id, c.quote);
            }
            for sec in &a.sections {
                println!("SECTION {}:", sec.heading);
                for p in &sec.paragraphs {
                    println!("  {}", p.text);
                    for c in &p.citations {
                        println!("    cite {} -> {:?}", c.source_id, c.quote);
                    }
                }
            }
            // Contract assertions: only S1/S2 cited; every factual paragraph cited.
            assert_eq!(
                a.question, request.question,
                "app owns the question, not the model"
            );
            assert!(!a.summary.citations.is_empty(), "summary is cited");
            for c in a.summary.citations.iter().chain(
                a.sections
                    .iter()
                    .flat_map(|s| s.paragraphs.iter().flat_map(|p| p.citations.iter())),
            ) {
                assert!(
                    matches!(c.source_id.as_str(), "S1" | "S2"),
                    "cited only known read ids, got {}",
                    c.source_id
                );
            }
            assert_eq!(a.sources.len(), 2);
        }
        Err(e) => panic!("live synthesis failed to validate: {e:?} (model {model})"),
    }
}

/// Full live pipeline: DuckDuckGo search → safe page read → OpenRouter
/// synthesis, driven by the reducer. A public/current question either yields a
/// cited `Answer` (sources read) or an honest `Digest` (all blocked/thin) — both
/// terminal and valid. Proves the whole engine wires together against live I/O.
#[ignore = "requires OPENROUTER_API_KEY and network"]
#[tokio::test]
async fn live_full_pipeline_matrix() {
    use fm_research::machine::{Action, ResearchBudgets, ResearchMachine};
    use fm_research::research::{ResearchDepth, ResearchMode, ResearchOutput, ResearchRequest};

    let api_key = match std::env::var("OPENROUTER_API_KEY") {
        Ok(k) if !k.trim().is_empty() => k,
        _ => panic!("set OPENROUTER_API_KEY to run the live pipeline matrix"),
    };
    let model =
        std::env::var("FINMODEL_MODEL").unwrap_or_else(|_| "deepseek/deepseek-chat".to_string());

    let request = ResearchRequest {
        question: "What is the current investment case for Nvidia? Cover catalysts and risks."
            .into(),
        mode: ResearchMode::Web,
        tickers: vec!["NVDA".into()],
        periods: vec![],
        filing_forms: vec![],
        target: None,
        acquirer: None,
        depth: ResearchDepth::Standard,
    };
    let budgets = ResearchBudgets::from_depth(request.depth);
    let machine = ResearchMachine::new(request.clone(), budgets, stamp());
    let backend = HttpBackend {
        max_sources: budgets.max_sources,
        per_query_results: 6,
        mode: request.mode,
        tickers: request.tickers.clone(),
        filing_forms: request.filing_forms.clone(),
        question: request.question.clone(),
        target: String::new(),
        acquirer: String::new(),
    };
    let synth = OpenRouterSynthesizer {
        api_key,
        model: model.clone(),
        strict_json: true,
        request: request.clone(),
    };

    let cancel = tokio_util::sync::CancellationToken::new();
    let terminal = run_research(machine, request, &backend, &synth, &cancel, &|_| {}).await;
    match terminal {
        Action::Done(ResearchOutput::Answer(a)) => {
            println!("=== ANSWER ({model}) confidence {:?} ===", a.confidence);
            println!("{}", a.summary.text);
            println!("consulted {} sources:", a.sources.len());
            for s in &a.sources {
                println!("  {} [{:?}/{:?}] {}", s.id, s.status, s.kind, s.domain);
            }
            assert!(!a.summary.citations.is_empty(), "cited summary");
        }
        Action::Done(ResearchOutput::Digest(d)) => {
            println!("=== HONEST DIGEST ({model}) ===");
            println!("limitations: {:?}", d.limitations);
            for it in &d.items {
                println!("  {} [{:?}] {}", it.source_id, it.status, it.url);
            }
            // A digest is a valid honest terminal (all sources blocked/thin, or no synthesis).
        }
        other => panic!("pipeline did not reach a Done terminal: {other:?}"),
    }
}

/// Live filing-mode pipeline through the shipped `HttpBackend`: ticker → EDGAR
/// CIK → recent 10-K filings → item-selected filing-body read → synthesis. Proves
/// Filing mode routes to EDGAR: every consulted source is a Regulatory sec.gov
/// filing (answer or honest digest, both valid terminals).
#[ignore = "requires OPENROUTER_API_KEY and network"]
#[tokio::test]
async fn live_filing_pipeline_matrix() {
    use fm_research::machine::{Action, ResearchBudgets, ResearchMachine};
    use fm_research::research::ResearchOutput;

    let api_key = match std::env::var("OPENROUTER_API_KEY") {
        Ok(k) if !k.trim().is_empty() => k,
        _ => panic!("set OPENROUTER_API_KEY to run the live filing matrix"),
    };
    let model =
        std::env::var("FINMODEL_MODEL").unwrap_or_else(|_| "deepseek/deepseek-chat".to_string());

    let request = ResearchRequest {
        question: "What are the main risk factors NVIDIA discloses?".into(),
        mode: ResearchMode::Filing,
        tickers: vec!["NVDA".into()],
        periods: vec![],
        filing_forms: vec!["10-K".into()],
        target: None,
        acquirer: None,
        depth: ResearchDepth::Quick,
    };
    let budgets = ResearchBudgets::from_depth(request.depth);
    let machine = ResearchMachine::new(request.clone(), budgets, stamp());
    let backend = HttpBackend {
        max_sources: budgets.max_sources,
        per_query_results: 6,
        mode: request.mode,
        tickers: request.tickers.clone(),
        filing_forms: request.filing_forms.clone(),
        question: request.question.clone(),
        target: String::new(),
        acquirer: String::new(),
    };
    let synth = OpenRouterSynthesizer {
        api_key,
        model: model.clone(),
        strict_json: true,
        request: request.clone(),
    };
    let cancel = tokio_util::sync::CancellationToken::new();
    match run_research(machine, request, &backend, &synth, &cancel, &|_| {}).await {
        Action::Done(ResearchOutput::Answer(a)) => {
            println!(
                "=== FILING ANSWER ({model}) sources {} ===",
                a.sources.len()
            );
            assert!(!a.sources.is_empty(), "consulted at least one filing");
            for s in &a.sources {
                println!("  {} [{:?}/{:?}] {}", s.id, s.status, s.kind, s.domain);
                assert_eq!(
                    s.kind,
                    SourceKind::Regulatory,
                    "filing sources are Regulatory"
                );
                assert!(
                    s.domain.contains("sec.gov"),
                    "filing sources are on sec.gov"
                );
            }
            assert!(!a.summary.citations.is_empty(), "cited summary");
        }
        Action::Done(ResearchOutput::Digest(d)) => {
            println!("=== FILING DIGEST ({model}) ===");
            assert!(!d.items.is_empty(), "consulted filings listed");
            for it in &d.items {
                println!("  {} [{:?}] {}", it.source_id, it.status, it.url);
                assert!(it.url.contains("sec.gov"), "filing sources are on sec.gov");
            }
        }
        other => panic!("filing pipeline did not reach a Done terminal: {other:?}"),
    }
}

/// Live company-brief pipeline through the shipped `HttpBackend`: fuse recent
/// 10-K/10-Q filings + web (IR + independent) + a market-quote synthetic source
/// into one ledger, read per-URL, then synthesize. Proves the multi-source fusion
/// end-to-end: the consulted ledger spans EDGAR filings AND a market quote.
#[ignore = "requires OPENROUTER_API_KEY and network"]
#[tokio::test]
async fn live_company_pipeline_matrix() {
    use fm_research::machine::{Action, ResearchBudgets, ResearchMachine};
    use fm_research::research::ResearchOutput;

    let api_key = match std::env::var("OPENROUTER_API_KEY") {
        Ok(k) if !k.trim().is_empty() => k,
        _ => panic!("set OPENROUTER_API_KEY to run the live company matrix"),
    };
    let model =
        std::env::var("FINMODEL_MODEL").unwrap_or_else(|_| "deepseek/deepseek-chat".to_string());

    let request = ResearchRequest {
        question: "Give a company brief on NVIDIA.".into(),
        mode: ResearchMode::Company,
        tickers: vec!["NVDA".into()],
        periods: vec![],
        filing_forms: vec![],
        target: None,
        acquirer: None,
        depth: ResearchDepth::Standard,
    };
    let budgets = ResearchBudgets::from_depth(request.depth);
    let machine = ResearchMachine::new(request.clone(), budgets, stamp());
    let backend = HttpBackend {
        max_sources: budgets.max_sources,
        per_query_results: 6,
        mode: request.mode,
        tickers: request.tickers.clone(),
        filing_forms: request.filing_forms.clone(),
        question: request.question.clone(),
        target: String::new(),
        acquirer: String::new(),
    };
    let synth = OpenRouterSynthesizer {
        api_key,
        model: model.clone(),
        strict_json: true,
        request: request.clone(),
    };
    let cancel = tokio_util::sync::CancellationToken::new();
    // Collect the consulted ledger's domains/URLs from whichever terminal fired.
    let (label, refs): (&str, Vec<String>) =
        match run_research(machine, request, &backend, &synth, &cancel, &|_| {}).await {
            Action::Done(ResearchOutput::Answer(a)) => (
                "ANSWER",
                a.sources.iter().map(|s| s.domain.clone()).collect(),
            ),
            Action::Done(ResearchOutput::Digest(d)) => {
                ("DIGEST", d.items.iter().map(|it| it.url.clone()).collect())
            }
            other => panic!("company pipeline did not reach a Done terminal: {other:?}"),
        };
    println!("=== COMPANY {label} ({model}) refs: {refs:?} ===");
    assert!(
        refs.iter().any(|d| d.contains("sec.gov")),
        "company brief must consult an EDGAR filing"
    );
    assert!(
        refs.iter().any(|d| d.contains("finance.yahoo.com")),
        "company brief must include the market-quote source"
    );
}

fn stamp() -> String {
    fm_research::today_iso()
}
