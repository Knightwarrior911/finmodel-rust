//! Offline research-pipeline bench (Phase 3 gate).
//!
//! Drives the REAL async research driver (`run_research`) against a deterministic
//! in-memory backend + synthesizer — no network, no model, no generation. This
//! isolates the app's orchestration overhead (the pipeline cost the roadmap caps
//! at ≤500 ms app overhead / ≤5 s cached standard pipeline p95, excluding the
//! provider round-trips this fixture removes).
//!
//! Emits one JSON line per run to stdout: `{"run":N,"phase":"warm|cold","ms":F}`.
//! `scripts/bench-research.ps1` computes nearest-rank p50/p95 and writes the
//! final report. Args: `--warmups N --measured M`.

use std::time::Instant;

use finmodel_app_lib::commands::research::{run_research, ResearchBackend, ResearchSynthesizer};
use fm_research::machine::{ResearchBudgets, ResearchMachine};
use fm_research::research::{
    AnswerSection, CitationRef, CitedParagraph, ResearchAnswer, ResearchConfidence, ResearchDepth,
    ResearchMode, ResearchPlan, ResearchRequest, ResearchToolArgs, SourceBackend, SourceKind,
    SourceRecord, SourceStatus,
};
use fm_research::synth::SynthReject;

/// Fixture source: a fully-read record with an excerpt the synth can cite.
fn fixture_source(i: usize) -> SourceRecord {
    let id = format!("S{i}");
    SourceRecord {
        id: id.clone(),
        requested_url: format!("https://example.com/{i}"),
        final_url: Some(format!("https://example.com/{i}")),
        canonical_url: format!("https://example.com/{i}"),
        title: format!("Fixture {i}"),
        domain: "example.com".into(),
        retrieved_at: "2026-01-01T00:00:00Z".into(),
        status: SourceStatus::Read,
        kind: if i % 2 == 0 {
            SourceKind::Primary
        } else {
            SourceKind::Newswire
        },
        backend: SourceBackend::BasicHttp,
        snippet: Some(format!("snippet {i}")),
        excerpt: Some(format!(
            "The company reported revenue growth of {i} percent this fiscal year."
        )),
        error_code: None,
    }
}

struct FixtureBackend {
    sources: Vec<SourceRecord>,
}

impl ResearchBackend for FixtureBackend {
    async fn plan(&self, _request: &ResearchRequest) -> Option<ResearchPlan> {
        Some(ResearchPlan {
            queries: vec!["fixture query".into()],
            required_source_types: Vec::new(),
        })
    }
    async fn search(&self, _queries: &[String]) -> Vec<SourceRecord> {
        self.sources.clone()
    }
    async fn read(&self, ledger: Vec<SourceRecord>) -> Vec<SourceRecord> {
        ledger // already Read
    }
}

struct FixtureSynth;

impl ResearchSynthesizer for FixtureSynth {
    async fn synthesize(
        &self,
        _attempt: u32,
        read: &[SourceRecord],
    ) -> Result<ResearchAnswer, SynthReject> {
        // Cite the first source's exact excerpt (validates cleanly).
        let first = read.first().ok_or(SynthReject::Empty)?;
        let quote = first.excerpt.clone().unwrap_or_default();
        let para = CitedParagraph {
            text: "Revenue grew this year.".into(),
            citations: vec![CitationRef {
                source_id: first.id.clone(),
                quote,
            }],
        };
        Ok(ResearchAnswer {
            question: "q".into(),
            summary: para.clone(),
            sections: vec![AnswerSection {
                heading: "Detail".into(),
                paragraphs: vec![para],
            }],
            sources: read.to_vec(),
            limitations: Vec::new(),
            confidence: ResearchConfidence::Medium,
            generated_at: "2026-01-01T00:00:00Z".into(),
            model: "fixture".into(),
        })
    }
}

fn one_run() -> f64 {
    let args = ResearchToolArgs {
        question: "What drove revenue growth?".into(),
        mode: ResearchMode::Web,
        tickers: Vec::new(),
        depth: ResearchDepth::Standard,
    };
    let request = args.into_request("What drove revenue growth?");
    let budgets = ResearchBudgets::from_depth(ResearchDepth::Standard);
    let machine =
        ResearchMachine::new(request.clone(), budgets, "2026-01-01T00:00:00Z".to_string());
    let backend = FixtureBackend {
        sources: (1..=6).map(fixture_source).collect(),
    };
    let synth = FixtureSynth;
    let cancel = tokio_util::sync::CancellationToken::new();
    let start = Instant::now();
    let _ = tauri::async_runtime::block_on(run_research(
        machine,
        request,
        &backend,
        &synth,
        &cancel,
        &|_| {},
    ));
    start.elapsed().as_secs_f64() * 1000.0
}

fn main() {
    let mut warmups = 3usize;
    let mut measured = 30usize;
    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--warmups" => {
                i += 1;
                if let Some(v) = args.get(i) {
                    warmups = v.parse().unwrap_or(warmups);
                }
            }
            "--measured" => {
                i += 1;
                if let Some(v) = args.get(i) {
                    measured = v.parse().unwrap_or(measured);
                }
            }
            _ => {}
        }
        i += 1;
    }
    // Cold runs: the first `warmups` are discarded (JIT of caches/allocators).
    for w in 0..warmups {
        let ms = one_run();
        println!("{{\"run\":{w},\"phase\":\"warm\",\"ms\":{ms:.4}}}");
    }
    for m in 0..measured {
        let ms = one_run();
        println!("{{\"run\":{m},\"phase\":\"cold\",\"ms\":{ms:.4}}}");
    }
}
