//! Pure research reducer (Phase 2.2).
//!
//! [`ResearchMachine`] is a runtime-agnostic state machine: it emits typed
//! [`Action`]s (Plan / Search / Read / Synthesize / terminal) and accepts typed
//! [`Input`]s, with no async, I/O, or clock of its own. The async driver
//! (`src-tauri/src/commands/research.rs::run_research`) owns the real
//! `ResearchClock`/`ResearchControl`, executes each emitted action against a
//! `ResearchBackend`/`ResearchSynthesizer`, and feeds results back. Cancellation
//! and deadline arrive as ordinary inputs, so the entire policy — stage order,
//! budgets, one-repair synthesis, honest digest fallback, targeted termination —
//! is deterministic and unit-testable with fakes.

use crate::research::{
    DigestItem, ResearchAnswer, ResearchDepth, ResearchDigest, ResearchOutput, ResearchPlan,
    ResearchRequest, SourceRecord, SourceStatus,
};

/// Concrete run budgets. Depth fixes queries/sources/deadline; the collector
/// caps (per-domain, concurrency, bytes) are fixed by the roadmap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResearchBudgets {
    pub max_queries: u32,
    pub max_sources: u32,
    pub max_pages_per_domain: u32,
    pub max_concurrent_reads: u32,
    pub max_bytes_per_page: usize,
    pub deadline_secs: u64,
}

impl ResearchBudgets {
    /// Derive from a depth: per-depth query/source/deadline plus the fixed
    /// collector caps (2 pages/domain, 3 concurrent reads, 2 MiB/page).
    pub fn from_depth(depth: ResearchDepth) -> Self {
        let b = depth.budgets();
        Self {
            max_queries: b.max_queries,
            max_sources: b.max_sources,
            max_pages_per_domain: 2,
            max_concurrent_reads: 3,
            max_bytes_per_page: 2 * 1024 * 1024,
            deadline_secs: b.deadline_secs,
        }
    }
}

/// An action the reducer asks the driver to perform.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Ask the model for a bounded [`ResearchPlan`] (Standard/Deep only).
    Plan,
    /// Run these queries through the backend (already clamped to the budget).
    Search { queries: Vec<String> },
    /// Read these ranked source ids (collector enforces domain/concurrency caps).
    Read { source_ids: Vec<String> },
    /// Synthesize from the read ledger; `attempt` is 1 (first) or 2 (one repair).
    Synthesize { attempt: u32 },
    /// Terminal: a validated answer or an honest digest.
    Done(ResearchOutput),
    /// Terminal: the run was cancelled.
    Cancelled,
    /// Terminal: an unrecoverable error (opaque code).
    Error { code: String },
}

/// Why a synthesis attempt was rejected (opaque to the reducer).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SynthesisReject {
    pub code: String,
}

/// A typed result fed back into the reducer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Input {
    /// Begin the run.
    Start,
    /// A plan was produced (`Some`) or planning failed/repaired to nothing (`None`,
    /// meaning: fall back to the unchanged question).
    Planned(Option<ResearchPlan>),
    /// The collector returned ranked, deduped, budget-capped candidate records
    /// with stable `S#` ids assigned.
    Searched(Vec<SourceRecord>),
    /// Reads finished; records carry their final statuses.
    ReadDone(Vec<SourceRecord>),
    /// Synthesis produced a validated, app-built answer or was rejected.
    Synthesized(Result<ResearchAnswer, SynthesisReject>),
    /// A cancellation was requested.
    Cancel,
    /// The overall deadline elapsed.
    Deadline,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Idle,
    Planning,
    Searching,
    Reading,
    Synthesizing,
    Terminal,
}

/// The pure research reducer.
#[derive(Clone, Debug)]
pub struct ResearchMachine {
    request: ResearchRequest,
    budgets: ResearchBudgets,
    generated_at: String,
    phase: Phase,
    ledger: Vec<SourceRecord>,
    synth_attempts: u32,
}

impl ResearchMachine {
    /// `generated_at` is the run's ISO timestamp (used only when the reducer must
    /// build a digest); the answer path carries its own timestamp from the app.
    pub fn new(
        request: ResearchRequest,
        budgets: ResearchBudgets,
        generated_at: impl Into<String>,
    ) -> Self {
        Self {
            request,
            budgets,
            generated_at: generated_at.into(),
            phase: Phase::Idle,
            ledger: Vec::new(),
            synth_attempts: 0,
        }
    }

    /// Advance the machine with one input, returning the next action. Any input
    /// after a terminal action is a no-op error guard.
    pub fn next(&mut self, input: Input) -> Action {
        // Cancellation and deadline are honored from any non-terminal phase.
        if self.phase != Phase::Terminal {
            match input {
                Input::Cancel => {
                    self.phase = Phase::Terminal;
                    return Action::Cancelled;
                }
                Input::Deadline => {
                    self.phase = Phase::Terminal;
                    // Honest digest from whatever was consulted, with a deadline note.
                    return self.finish_digest(
                        "I ran out of research time before I could pull a validated answer together — ask me to continue and I'll pick this up.",
                    );
                }
                _ => {}
            }
        }

        match (self.phase, input) {
            (Phase::Idle, Input::Start) => {
                if self.request.depth.plans() {
                    self.phase = Phase::Planning;
                    Action::Plan
                } else {
                    // Quick: search the unchanged question once.
                    self.phase = Phase::Searching;
                    Action::Search {
                        queries: vec![self.request.question.clone()],
                    }
                }
            }
            (Phase::Planning, Input::Planned(plan)) => {
                let queries = self.resolve_queries(plan);
                self.phase = Phase::Searching;
                Action::Search { queries }
            }
            (Phase::Searching, Input::Searched(records)) => {
                self.ledger = Self::cap_sources(records, self.budgets.max_sources);
                if self.ledger.is_empty() {
                    self.phase = Phase::Terminal;
                    return self.finish_digest("No sources were found for this question.");
                }
                self.phase = Phase::Reading;
                Action::Read {
                    source_ids: self.ledger.iter().map(|r| r.id.clone()).collect(),
                }
            }
            (Phase::Reading, Input::ReadDone(records)) => {
                self.ledger = records;
                let any_read = self.ledger.iter().any(|r| r.status == SourceStatus::Read);
                if !any_read {
                    self.phase = Phase::Terminal;
                    return self
                        .finish_digest("No source could be read (all blocked, thin, or failed).");
                }
                self.phase = Phase::Synthesizing;
                self.synth_attempts = 1;
                Action::Synthesize { attempt: 1 }
            }
            (Phase::Synthesizing, Input::Synthesized(Ok(answer))) => {
                self.phase = Phase::Terminal;
                Action::Done(ResearchOutput::Answer(answer))
            }
            (Phase::Synthesizing, Input::Synthesized(Err(_reject))) => {
                if self.synth_attempts < 2 {
                    // Exactly one repair.
                    self.synth_attempts += 1;
                    Action::Synthesize {
                        attempt: self.synth_attempts,
                    }
                } else {
                    self.phase = Phase::Terminal;
                    self.finish_digest("The selected model could not produce a validated synthesis")
                }
            }
            // Any unexpected input in a phase is a protocol error, terminated honestly.
            (phase, _) => {
                if phase == Phase::Terminal {
                    Action::Error {
                        code: "already_terminal".into(),
                    }
                } else {
                    self.phase = Phase::Terminal;
                    Action::Error {
                        code: "unexpected_input".into(),
                    }
                }
            }
        }
    }

    /// Clamp the plan's queries to the budget, or fall back to the unchanged
    /// question when planning failed or produced nothing.
    fn resolve_queries(&self, plan: Option<ResearchPlan>) -> Vec<String> {
        match plan {
            Some(p) if !p.queries.is_empty() => {
                let max = self.budgets.max_queries as usize;
                p.queries.into_iter().take(max).collect()
            }
            _ => vec![self.request.question.clone()],
        }
    }

    /// Cap candidate records to the source budget while preserving the
    /// collector's ranking/order. The reducer trusts the collector to have
    /// already deduped canonical URLs and applied the per-domain cap.
    fn cap_sources(mut records: Vec<SourceRecord>, max: u32) -> Vec<SourceRecord> {
        records.truncate(max as usize);
        records
    }

    /// Build an honest digest from the current ledger with `reason` appended to
    /// its limitations. Every consulted source appears in deterministic id order.
    fn finish_digest(&self, reason: &str) -> Action {
        let mut items: Vec<DigestItem> = self
            .ledger
            .iter()
            .map(|r| DigestItem {
                source_id: r.id.clone(),
                title: r.title.clone(),
                url: r
                    .final_url
                    .clone()
                    .unwrap_or_else(|| r.requested_url.clone()),
                snippet: r.snippet.clone(),
                status: r.status,
            })
            .collect();
        items.sort_by(|a, b| a.source_id.cmp(&b.source_id));
        Action::Done(ResearchOutput::Digest(ResearchDigest {
            question: self.request.question.clone(),
            items,
            limitations: vec![reason.to_string()],
            generated_at: self.generated_at.clone(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::research::{
        AnswerSection, CitationRef, CitedParagraph, ResearchConfidence, ResearchMode,
        SourceBackend, SourceKind,
    };

    fn req(depth: ResearchDepth) -> ResearchRequest {
        ResearchRequest {
            question: "Research the current investment case for Nvidia.".into(),
            mode: ResearchMode::Web,
            tickers: vec!["NVDA".into()],
            periods: vec![],
            filing_forms: vec![],
            target: None,
            acquirer: None,
            depth,
        }
    }

    fn source(id: &str, status: SourceStatus) -> SourceRecord {
        SourceRecord {
            id: id.into(),
            requested_url: format!("https://example.com/{id}"),
            final_url: Some(format!("https://example.com/{id}")),
            canonical_url: format!("https://example.com/{id}"),
            title: format!("Source {id}"),
            domain: "example.com".into(),
            retrieved_at: "2026-01-01T00:00:00Z".into(),
            status,
            kind: SourceKind::Newswire,
            backend: SourceBackend::BasicHttp,
            snippet: Some("snippet".into()),
            excerpt: if status == SourceStatus::Read {
                Some("Nvidia data-center revenue grew.".into())
            } else {
                None
            },
            error_code: None,
        }
    }

    fn valid_answer() -> ResearchAnswer {
        ResearchAnswer {
            question: "Research the current investment case for Nvidia.".into(),
            summary: CitedParagraph {
                text: "Nvidia's data-center revenue grew.".into(),
                citations: vec![CitationRef {
                    source_id: "S1".into(),
                    quote: "Nvidia data-center revenue grew.".into(),
                }],
            },
            sections: vec![AnswerSection {
                heading: "Catalysts".into(),
                paragraphs: vec![CitedParagraph {
                    text: "AI demand is a catalyst.".into(),
                    citations: vec![CitationRef {
                        source_id: "S2".into(),
                        quote: "AI demand.".into(),
                    }],
                }],
            }],
            sources: vec![
                source("S1", SourceStatus::Read),
                source("S2", SourceStatus::Read),
            ],
            limitations: vec![],
            confidence: ResearchConfidence::Medium,
            generated_at: "2026-01-01T00:00:00Z".into(),
            model: "test/model".into(),
        }
    }

    fn budgets() -> ResearchBudgets {
        ResearchBudgets::from_depth(ResearchDepth::Standard)
    }

    #[test]
    fn standard_happy_path_plan_search_read_synthesize_answer() {
        let mut m = ResearchMachine::new(
            req(ResearchDepth::Standard),
            budgets(),
            "2026-01-01T00:00:00Z",
        );
        assert_eq!(m.next(Input::Start), Action::Plan);
        let plan = ResearchPlan {
            queries: vec!["nvidia catalysts".into(), "nvidia risks".into()],
            required_source_types: vec![],
        };
        match m.next(Input::Planned(Some(plan))) {
            Action::Search { queries } => assert_eq!(queries.len(), 2),
            a => panic!("expected Search, got {a:?}"),
        }
        let found = vec![
            source("S1", SourceStatus::Read),
            source("S2", SourceStatus::Read),
        ];
        match m.next(Input::Searched(found.clone())) {
            Action::Read { source_ids } => assert_eq!(source_ids, vec!["S1", "S2"]),
            a => panic!("expected Read, got {a:?}"),
        }
        assert_eq!(
            m.next(Input::ReadDone(found)),
            Action::Synthesize { attempt: 1 }
        );
        match m.next(Input::Synthesized(Ok(valid_answer()))) {
            Action::Done(ResearchOutput::Answer(a)) => assert_eq!(a.sources.len(), 2),
            a => panic!("expected Done(Answer), got {a:?}"),
        }
    }

    #[test]
    fn quick_skips_planning() {
        let mut m = ResearchMachine::new(
            req(ResearchDepth::Quick),
            ResearchBudgets::from_depth(ResearchDepth::Quick),
            "t",
        );
        match m.next(Input::Start) {
            Action::Search { queries } => assert_eq!(
                queries,
                vec!["Research the current investment case for Nvidia.".to_string()]
            ),
            a => panic!("expected Search on Quick start, got {a:?}"),
        }
    }

    #[test]
    fn plan_queries_clamped_to_budget() {
        let mut m = ResearchMachine::new(req(ResearchDepth::Standard), budgets(), "t");
        assert_eq!(m.next(Input::Start), Action::Plan);
        // 5 queries but Standard budget is 3.
        let plan = ResearchPlan {
            queries: (0..5).map(|i| format!("q{i}")).collect(),
            required_source_types: vec![],
        };
        match m.next(Input::Planned(Some(plan))) {
            Action::Search { queries } => assert_eq!(queries.len(), 3),
            a => panic!("expected clamped Search, got {a:?}"),
        }
    }

    #[test]
    fn plan_failure_falls_back_to_unchanged_question() {
        let mut m = ResearchMachine::new(req(ResearchDepth::Standard), budgets(), "t");
        m.next(Input::Start);
        match m.next(Input::Planned(None)) {
            Action::Search { queries } => {
                assert_eq!(queries, vec![req(ResearchDepth::Standard).question])
            }
            a => panic!("expected fallback Search, got {a:?}"),
        }
    }

    #[test]
    fn synthesis_permits_exactly_one_repair_then_digest() {
        let mut m = ResearchMachine::new(req(ResearchDepth::Standard), budgets(), "t");
        m.next(Input::Start);
        m.next(Input::Planned(Some(ResearchPlan {
            queries: vec!["q".into()],
            required_source_types: vec![],
        })));
        let recs = vec![source("S1", SourceStatus::Read)];
        m.next(Input::Searched(recs.clone()));
        assert_eq!(
            m.next(Input::ReadDone(recs)),
            Action::Synthesize { attempt: 1 }
        );
        // First rejection → one repair.
        assert_eq!(
            m.next(Input::Synthesized(Err(SynthesisReject {
                code: "uncited".into()
            }))),
            Action::Synthesize { attempt: 2 }
        );
        // Second rejection → honest digest with the exact limitation.
        match m.next(Input::Synthesized(Err(SynthesisReject {
            code: "uncited".into(),
        }))) {
            Action::Done(ResearchOutput::Digest(d)) => {
                assert_eq!(
                    d.limitations,
                    vec!["The selected model could not produce a validated synthesis".to_string()]
                );
            }
            a => panic!("expected Done(Digest), got {a:?}"),
        }
    }

    #[test]
    fn all_blocked_sources_yield_digest() {
        let mut m = ResearchMachine::new(
            req(ResearchDepth::Quick),
            ResearchBudgets::from_depth(ResearchDepth::Quick),
            "t",
        );
        m.next(Input::Start);
        let recs = vec![
            source("S1", SourceStatus::Blocked),
            source("S2", SourceStatus::Thin),
        ];
        m.next(Input::Searched(recs.clone()));
        match m.next(Input::ReadDone(recs)) {
            Action::Done(ResearchOutput::Digest(d)) => {
                assert_eq!(d.items.len(), 2);
                assert!(d.limitations[0].contains("all blocked"));
            }
            a => panic!("expected Done(Digest), got {a:?}"),
        }
    }

    #[test]
    fn cancel_terminates_immediately_from_any_phase() {
        let mut m = ResearchMachine::new(req(ResearchDepth::Standard), budgets(), "t");
        m.next(Input::Start);
        assert_eq!(m.next(Input::Cancel), Action::Cancelled);
        // Post-terminal input is an error guard, never another action.
        assert_eq!(
            m.next(Input::Start),
            Action::Error {
                code: "already_terminal".into()
            }
        );
    }

    #[test]
    fn deadline_yields_digest_from_partial_ledger() {
        let mut m = ResearchMachine::new(req(ResearchDepth::Standard), budgets(), "t");
        m.next(Input::Start);
        m.next(Input::Planned(Some(ResearchPlan {
            queries: vec!["q".into()],
            required_source_types: vec![],
        })));
        m.next(Input::Searched(vec![source("S1", SourceStatus::Read)]));
        match m.next(Input::Deadline) {
            Action::Done(ResearchOutput::Digest(d)) => {
                assert!(d.limitations[0].contains("ran out of research time"))
            }
            a => panic!("expected Done(Digest) on deadline, got {a:?}"),
        }
    }

    #[test]
    fn source_budget_caps_ledger() {
        let mut m = ResearchMachine::new(req(ResearchDepth::Standard), budgets(), "t");
        m.next(Input::Start);
        m.next(Input::Planned(Some(ResearchPlan {
            queries: vec!["q".into()],
            required_source_types: vec![],
        })));
        // 10 candidates, Standard source budget is 6.
        let many: Vec<SourceRecord> = (0..10)
            .map(|i| source(&format!("S{i}"), SourceStatus::Read))
            .collect();
        match m.next(Input::Searched(many)) {
            Action::Read { source_ids } => assert_eq!(source_ids.len(), 6),
            a => panic!("expected capped Read, got {a:?}"),
        }
    }
}
