//! Answer-quality grading + an offline model×prompt **sweep** for the research
//! eval corpus (deterministic, no network).
//!
//! Distinct from [`crate::scoring`] (production URL/text *ranking*): this module
//! GRADES a built [`ResearchAnswer`] against a per-case gold spec, returning a
//! `0.0..=1.0` quality score, and RANKS competing answer variants.
//!
//! Two executable entry points, one scorer:
//!   * [`grade`] — score a single answer against its gold spec.
//!   * [`run_sweep`] — grade a bag of [`AnswerArtifact`]s keyed by
//!     `{model, prompt_variant, case_id}` and rank the variants best-first.
//!     Every variant is scored over the **whole** gold set: a case a variant
//!     did not answer counts as `0`, so a variant that nails one easy case can
//!     never outrank one that answers the full corpus. Coverage is reported and
//!     duplicate `{model, prompt_variant, case_id}` artifacts are rejected.
//!
//! Producer boundary: GENERATING the artifacts from real models (one answer per
//! model×prompt×case) is a separate app-layer step that needs the provider — it
//! writes JSON artifacts this harness then scores. The offline regression gate
//! (`research_eval.rs::engine_gate::quality_gate`) produces artifacts from fixed
//! scripted drafts and asserts the mean stays above a committed baseline.
//!
//! Applicability: only answer-producing cases carry a [`GoldAnswer`]. An
//! artifact whose `case_id` has no gold spec is ignored — `direct`/`digest`/
//! `error`/`cancelled`/`tool_contract` terminals are contract outcomes graded
//! elsewhere and never enter a quality denominator here.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::research::{ResearchAnswer, SourceStatus};

/// One atomic fact a good answer must state, expressed as accepted phrasings.
/// The fact is surfaced once if ANY phrasing is a whitespace-normalized,
/// case-insensitive substring of the answer prose. Aliases MUST preserve exact
/// numeric and entity semantics — "grew 200%" (a 3× level) is NOT "doubled"
/// (2×); list only same-value phrasings ("increased 200%", "rose 200%"), each
/// carrying the numeric/entity anchor so a wrong value matches none.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ExpectedFact {
    pub any_of: Vec<String>,
}

/// A malformed gold spec — a fact no answer could ever satisfy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GoldError {
    /// A fact lists no accepted phrasings.
    EmptyFact { case_id: String, fact_index: usize },
    /// A fact phrasing is blank / whitespace-only.
    BlankAlias { case_id: String, fact_index: usize },
    /// Two gold entries share a `case_id` (the sweep would silently drop one).
    DuplicateCase { case_id: String },
}

impl std::fmt::Display for GoldError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GoldError::EmptyFact {
                case_id,
                fact_index,
            } => {
                write!(f, "gold case {case_id} fact #{fact_index} has no phrasings")
            }
            GoldError::BlankAlias {
                case_id,
                fact_index,
            } => {
                write!(
                    f,
                    "gold case {case_id} fact #{fact_index} has a blank phrasing"
                )
            }
            GoldError::DuplicateCase { case_id } => {
                write!(f, "duplicate gold case_id {case_id}")
            }
        }
    }
}

impl std::error::Error for GoldError {}

/// Validate gold specs before scoring: every fact needs ≥1 non-blank phrasing,
/// else it is impossible to hit and would read as a permanent model regression.
/// Shared by the offline gate and any artifact-scoring CLI.
pub fn validate_gold(gold: &[GoldAnswer]) -> Result<(), GoldError> {
    let mut seen = std::collections::BTreeSet::new();
    for g in gold {
        if !seen.insert(g.case_id.as_str()) {
            return Err(GoldError::DuplicateCase {
                case_id: g.case_id.clone(),
            });
        }
        for (i, fact) in g.expected_facts.iter().enumerate() {
            if fact.any_of.is_empty() {
                return Err(GoldError::EmptyFact {
                    case_id: g.case_id.clone(),
                    fact_index: i,
                });
            }
            if fact.any_of.iter().any(|a| a.trim().is_empty()) {
                return Err(GoldError::BlankAlias {
                    case_id: g.case_id.clone(),
                    fact_index: i,
                });
            }
        }
    }
    Ok(())
}

/// Per-case gold expectations for an answer-producing research case.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GoldAnswer {
    /// Corpus case id (matches `research_cases.json`) this gold applies to.
    pub case_id: String,
    /// Key facts a good answer MUST state, each as a set of accepted phrasings
    /// ([`ExpectedFact::any_of`]). A fact counts once if ANY phrasing is a
    /// whitespace-normalized, case-insensitive substring of the answer's PROSE
    /// (paragraph text) — never citation quotes, so quoting a source that stated
    /// the fact earns no completeness credit. Aliases let a legitimate
    /// paraphrase match; put the numeric/entity anchor in every alias so a wrong
    /// value ("grew 100%") matches none.
    pub expected_facts: Vec<ExpectedFact>,
    /// Section headings a good answer MUST include (case-insensitive exact).
    pub expected_sections: Vec<String>,
    /// Minimum distinct sources the answer must carry.
    pub min_sources: usize,
}

/// Component scores for one graded case, each in `0.0..=1.0`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct CaseScore {
    /// Fraction of `expected_facts` stated in the answer's prose.
    pub completeness: f64,
    /// Fraction of `expected_sections` present as headings.
    pub section_coverage: f64,
    /// Fraction of factual paragraphs (summary + sections) carrying ≥1 citation.
    pub citation_coverage: f64,
    /// Fraction of citations whose quote resolves to a `Read` source excerpt
    /// (exact whitespace-normalized substring) — the quote-integrity guard.
    /// Proves each cited quote is verbatim from a `Read` source; it does NOT
    /// verify the quote supports the paragraph's specific claim.
    pub quote_integrity: f64,
    /// `1.0` when the answer carries at least `min_sources`, else `0.0`.
    pub source_sufficiency: f64,
    /// Weighted overall in `0.0..=1.0`.
    pub overall: f64,
}

impl CaseScore {
    /// The score for a gold case a variant never answered: zero on every axis.
    pub const MISSING: CaseScore = CaseScore {
        completeness: 0.0,
        section_coverage: 0.0,
        citation_coverage: 0.0,
        quote_integrity: 0.0,
        source_sufficiency: 0.0,
        overall: 0.0,
    };
}

// Metric weights (sum = 1.0). Completeness and quote integrity dominate: a good
// answer says the right things and never cites what a source doesn't support.
const W_COMPLETENESS: f64 = 0.35;
const W_QUOTE_INTEGRITY: f64 = 0.30;
const W_CITATION: f64 = 0.15;
const W_SECTION: f64 = 0.15;
const W_SOURCES: f64 = 0.05;

/// Version tag for the metric SET (names + weights). Bump when a metric is
/// renamed or a weight changes, so the gate's committed baseline must be
/// refreshed deliberately. v2 renamed `faithfulness` → `quote_integrity`.
pub const WEIGHTS_VERSION: &str = "v2";

/// Metric weights in report order: completeness, quote integrity, citation
/// coverage, section coverage, source sufficiency. Sums to 1.0.
pub fn metric_weights() -> [f64; 5] {
    [
        W_COMPLETENESS,
        W_QUOTE_INTEGRITY,
        W_CITATION,
        W_SECTION,
        W_SOURCES,
    ]
}

/// Collapse whitespace runs to one space and trim — mirrors `synth::normalize_ws`
/// so quote-integrity scoring uses the same quote↔excerpt rule as validation.
fn normalize_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `num/den`, or `empty` when `den == 0` (a spec with no expectations is
/// vacuously satisfied rather than dividing by zero).
fn ratio(num: usize, den: usize, empty: f64) -> f64 {
    if den == 0 {
        empty
    } else {
        num as f64 / den as f64
    }
}

/// The answer's PROSE — every paragraph's text (summary + sections),
/// whitespace-normalized and lowercased. Completeness matches against THIS
/// only; citation quotes feed quote integrity, never completeness.
fn answer_prose(answer: &ResearchAnswer) -> String {
    let mut parts: Vec<String> = vec![normalize_ws(&answer.summary.text).to_lowercase()];
    for sec in &answer.sections {
        for p in &sec.paragraphs {
            parts.push(normalize_ws(&p.text).to_lowercase());
        }
    }
    parts.join(" ")
}

/// Grade one built answer against its gold spec. Faithfulness is scored against
/// the answer's own `Read` sources (which `build_answer` populates with their
/// excerpts), so an [`AnswerArtifact`] is self-contained.
pub fn grade(answer: &ResearchAnswer, gold: &GoldAnswer) -> CaseScore {
    let prose = answer_prose(answer);

    // Completeness: expected facts STATED in the answer's prose.
    let facts_hit = gold
        .expected_facts
        .iter()
        .filter(|f| {
            f.any_of.iter().any(|alias| {
                let n = normalize_ws(alias).to_lowercase();
                !n.is_empty() && prose.contains(&n)
            })
        })
        .count();
    let completeness = ratio(facts_hit, gold.expected_facts.len(), 1.0);

    // Section coverage: expected headings present (case-insensitive exact).
    let headings: Vec<String> = answer
        .sections
        .iter()
        .map(|s| s.heading.trim().to_lowercase())
        .collect();
    let sections_hit = gold
        .expected_sections
        .iter()
        .filter(|h| {
            let n = h.trim().to_lowercase();
            headings.iter().any(|x| *x == n)
        })
        .count();
    let section_coverage = ratio(sections_hit, gold.expected_sections.len(), 1.0);

    // Citation coverage + quote integrity over summary + every section paragraph.
    let mut paragraphs = vec![&answer.summary];
    for sec in &answer.sections {
        for p in &sec.paragraphs {
            paragraphs.push(p);
        }
    }
    let cited = paragraphs
        .iter()
        .filter(|p| !p.citations.is_empty())
        .count();
    let citation_coverage = ratio(cited, paragraphs.len(), 1.0);

    // Faithfulness: each citation's quote is an exact normalized substring of a
    // `Read` source's excerpt (same rule as `synth::validate_synthesis`, graded).
    let mut total_cites = 0usize;
    let mut faithful = 0usize;
    for p in &paragraphs {
        for c in &p.citations {
            total_cites += 1;
            // Case-SENSITIVE after whitespace normalization — identical to
            // `synth::validate_synthesis`, so the scorer never grants grounding
            // that production would reject (external artifacts skip validation).
            let q = normalize_ws(&c.quote);
            let ok = !q.is_empty()
                && answer.sources.iter().any(|r| {
                    r.status == SourceStatus::Read
                        && r.id == c.source_id
                        && r.excerpt
                            .as_deref()
                            .is_some_and(|e| normalize_ws(e).contains(&q))
                });
            if ok {
                faithful += 1;
            }
        }
    }
    // No citations → no evidence → zero quote integrity (absence is not
    // credit; `citation_coverage` separately records that nothing was cited).
    let quote_integrity = ratio(faithful, total_cites, 0.0);

    // Source sufficiency: distinct `Read` sources ACTUALLY cited by the answer
    // — never the raw ledger length, so padding `answer.sources` with failed or
    // uncited records earns nothing.
    let read_ids: std::collections::BTreeSet<&str> = answer
        .sources
        .iter()
        .filter(|r| r.status == SourceStatus::Read)
        .map(|r| r.id.as_str())
        .collect();
    let cited_read: std::collections::BTreeSet<&str> = paragraphs
        .iter()
        .flat_map(|p| p.citations.iter())
        .map(|c| c.source_id.as_str())
        .filter(|id| read_ids.contains(id))
        .collect();
    let source_sufficiency = if cited_read.len() >= gold.min_sources {
        1.0
    } else {
        0.0
    };

    let overall = W_COMPLETENESS * completeness
        + W_QUOTE_INTEGRITY * quote_integrity
        + W_CITATION * citation_coverage
        + W_SECTION * section_coverage
        + W_SOURCES * source_sufficiency;

    CaseScore {
        completeness,
        section_coverage,
        citation_coverage,
        quote_integrity,
        source_sufficiency,
        overall,
    }
}

/// One produced answer for a `{model, prompt_variant, case_id}` cell — the unit
/// a sweep grades. Serializable so a live producer can emit these as JSON and
/// this harness can score them offline.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AnswerArtifact {
    pub model: String,
    pub prompt_variant: String,
    pub case_id: String,
    pub answer: ResearchAnswer,
}

/// Aggregate score for one `{model, prompt_variant}` variant over the whole gold
/// set. `per_case` lists every gold case in id order; a case the variant did not
/// answer is `None` and contributes `0` to the mean.
#[derive(Clone, Debug, Serialize)]
pub struct VariantScore {
    pub model: String,
    pub prompt_variant: String,
    pub per_case: Vec<(String, Option<CaseScore>)>,
    /// Gold cases this variant actually answered.
    pub covered: usize,
    /// Total gold cases in the sweep (the mean's denominator).
    pub total: usize,
    /// Mean overall over ALL `total` cases (missing scored as `0`).
    pub mean_overall: f64,
}

/// A full sweep result: every variant that answered ≥1 gold case, ranked best
/// mean first (ties broken by `model`, then `prompt_variant`).
#[derive(Clone, Debug, Serialize)]
pub struct SweepReport {
    pub variants: Vec<VariantScore>,
}

impl SweepReport {
    /// The winning variant, if any.
    pub fn best(&self) -> Option<&VariantScore> {
        self.variants.first()
    }
}

/// A malformed sweep input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SweepError {
    /// Two artifacts share a `{model, prompt_variant, case_id}` cell.
    DuplicateArtifact {
        model: String,
        prompt_variant: String,
        case_id: String,
    },
    /// The gold specs supplied are malformed (see [`validate_gold`]).
    InvalidGold(GoldError),
}

impl fmt::Display for SweepError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SweepError::DuplicateArtifact {
                model,
                prompt_variant,
                case_id,
            } => write!(
                f,
                "duplicate artifact for {model}::{prompt_variant} case {case_id}"
            ),
            SweepError::InvalidGold(e) => write!(f, "invalid gold: {e}"),
        }
    }
}

impl std::error::Error for SweepError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SweepError::InvalidGold(e) => Some(e),
            _ => None,
        }
    }
}

/// Grade answer artifacts against gold and rank the variants.
///
/// Each `{model, prompt_variant}` variant is scored over the FULL `gold` set;
/// a gold case the variant produced no artifact for scores `0`. Variants that
/// answered no gold case at all are omitted (no quality signal). Duplicate
/// `{model, prompt_variant, case_id}` artifacts are a hard error.
pub fn run_sweep(
    artifacts: &[AnswerArtifact],
    gold: &[GoldAnswer],
) -> Result<SweepReport, SweepError> {
    // Enforce the gold invariant at the boundary — a caller cannot skip it.
    validate_gold(gold).map_err(SweepError::InvalidGold)?;

    let gold_by_case: BTreeMap<&str, &GoldAnswer> =
        gold.iter().map(|g| (g.case_id.as_str(), g)).collect();

    // Group by (model, prompt_variant) → case_id → artifact, rejecting dups.
    let mut groups: BTreeMap<(&str, &str), BTreeMap<&str, &AnswerArtifact>> = BTreeMap::new();
    for a in artifacts {
        let by_case = groups
            .entry((a.model.as_str(), a.prompt_variant.as_str()))
            .or_default();
        if by_case.insert(a.case_id.as_str(), a).is_some() {
            return Err(SweepError::DuplicateArtifact {
                model: a.model.clone(),
                prompt_variant: a.prompt_variant.clone(),
                case_id: a.case_id.clone(),
            });
        }
    }

    let total = gold.len();
    let mut variants: Vec<VariantScore> = Vec::new();
    for ((model, prompt_variant), by_case) in groups {
        // Score over the whole gold set in id order; missing case → 0.
        let mut per_case: Vec<(String, Option<CaseScore>)> = Vec::new();
        let mut covered = 0usize;
        let mut sum = 0.0f64;
        for g in &gold_by_case {
            let case_id = *g.0;
            match by_case.get(case_id) {
                Some(a) => {
                    let s = grade(&a.answer, g.1);
                    sum += s.overall;
                    covered += 1;
                    per_case.push((case_id.to_string(), Some(s)));
                }
                None => {
                    // Not answered → contributes CaseScore::MISSING (overall 0).
                    per_case.push((case_id.to_string(), None));
                }
            }
        }
        if covered == 0 {
            continue; // variant answered no gold case → no quality signal
        }
        let mean_overall = if total == 0 { 0.0 } else { sum / total as f64 };
        variants.push(VariantScore {
            model: model.to_string(),
            prompt_variant: prompt_variant.to_string(),
            per_case,
            covered,
            total,
            mean_overall,
        });
    }

    variants.sort_by(|a, b| {
        b.mean_overall
            .partial_cmp(&a.mean_overall)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.model.cmp(&b.model))
            .then_with(|| a.prompt_variant.cmp(&b.prompt_variant))
    });

    Ok(SweepReport { variants })
}

/// Render a compact, deterministic ranked report (one block per variant).
pub fn report_sweep(report: &SweepReport) -> String {
    let mut out = String::from("research answer-quality sweep (ranked best-first)\n");
    for (rank, v) in report.variants.iter().enumerate() {
        out.push_str(&format!(
            "  #{} {}::{}  mean_overall={:.3}  coverage={}/{}\n",
            rank + 1,
            v.model,
            v.prompt_variant,
            v.mean_overall,
            v.covered,
            v.total
        ));
        for (id, s) in &v.per_case {
            match s {
                Some(s) => out.push_str(&format!(
                    "       {id:<24} overall={:.3}  complete={:.2} integrity={:.2} cite={:.2} section={:.2} src={:.0}\n",
                    s.overall, s.completeness, s.quote_integrity, s.citation_coverage, s.section_coverage, s.source_sufficiency
                )),
                None => out.push_str(&format!("       {id:<24} — not answered (0)\n")),
            }
        }
    }
    out
}

/// A failure ingesting a JSON sweep.
#[derive(Debug)]
pub enum IngestError {
    /// The artifacts or gold JSON did not parse.
    Json(serde_json::Error),
    /// The parsed inputs were rejected by [`run_sweep`].
    Sweep(SweepError),
}

impl fmt::Display for IngestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IngestError::Json(e) => write!(f, "invalid sweep JSON: {e}"),
            IngestError::Sweep(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for IngestError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            IngestError::Json(e) => Some(e),
            IngestError::Sweep(e) => Some(e),
        }
    }
}

impl From<serde_json::Error> for IngestError {
    fn from(e: serde_json::Error) -> Self {
        IngestError::Json(e)
    }
}

impl From<SweepError> for IngestError {
    fn from(e: SweepError) -> Self {
        IngestError::Sweep(e)
    }
}

/// Ingest a JSON array of [`AnswerArtifact`]s and a JSON array of [`GoldAnswer`]s
/// (as a live model×prompt producer emits them) and run the offline sweep. This
/// is the executable ingestion path the CLI (`examples/quality_sweep.rs`) wraps;
/// the grading is identical to the in-process [`run_sweep`].
pub fn run_sweep_from_json(
    artifacts_json: &str,
    gold_json: &str,
) -> Result<SweepReport, IngestError> {
    let artifacts: Vec<AnswerArtifact> = serde_json::from_str(artifacts_json)?;
    // Gold is the versioned wrapper committed as `gold_answers.json`
    // (`{ "schema_version": .., "gold": [ … ] }`), not a bare array — so the CLI
    // ingests the repository fixture directly.
    #[derive(Deserialize)]
    struct GoldFile {
        gold: Vec<GoldAnswer>,
    }
    let gold: Vec<GoldAnswer> = serde_json::from_str::<GoldFile>(gold_json)?.gold;
    Ok(run_sweep(&artifacts, &gold)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::research::{
        AnswerSection, CitationRef, CitedParagraph, ResearchConfidence, SourceBackend, SourceKind,
        SourceRecord,
    };

    fn src(id: &str, excerpt: &str) -> SourceRecord {
        SourceRecord {
            id: id.into(),
            requested_url: format!("https://ex.com/{id}"),
            final_url: Some(format!("https://ex.com/{id}")),
            canonical_url: format!("https://ex.com/{id}"),
            title: format!("Source {id}"),
            domain: "ex.com".into(),
            retrieved_at: "2026-01-01T00:00:00Z".into(),
            status: SourceStatus::Read,
            kind: SourceKind::Newswire,
            backend: SourceBackend::BasicHttp,
            snippet: Some("snippet".into()),
            excerpt: Some(excerpt.into()),
            error_code: None,
        }
    }

    fn para(text: &str, cites: &[(&str, &str)]) -> CitedParagraph {
        CitedParagraph {
            text: text.into(),
            citations: cites
                .iter()
                .map(|(s, q)| CitationRef {
                    source_id: (*s).into(),
                    quote: (*q).into(),
                })
                .collect(),
        }
    }

    fn answer(
        summary: CitedParagraph,
        sections: Vec<AnswerSection>,
        sources: Vec<SourceRecord>,
    ) -> ResearchAnswer {
        ResearchAnswer {
            question: "q".into(),
            summary,
            sections,
            sources,
            limitations: vec![],
            confidence: ResearchConfidence::Medium,
            generated_at: "2026-01-01T00:00:00Z".into(),
            model: "test/model".into(),
        }
    }

    fn ledger() -> Vec<SourceRecord> {
        vec![
            src(
                "S1",
                "Data-center revenue grew 200% as AI demand accelerated.",
            ),
            src(
                "S2",
                "Competition and export controls are key risks to the thesis.",
            ),
        ]
    }

    fn gold() -> GoldAnswer {
        GoldAnswer {
            case_id: "web_current_factual".into(),
            expected_facts: vec![
                ExpectedFact {
                    any_of: vec![
                        "revenue grew 200%".into(),
                        "revenue increased 200%".into(),
                        "revenue rose 200%".into(),
                    ],
                },
                ExpectedFact {
                    any_of: vec!["export controls".into()],
                },
            ],
            expected_sections: vec!["Catalysts".into(), "Risks".into()],
            min_sources: 2,
        }
    }

    // Prose STATES both facts; citations back them with matching quotes.
    fn strong_answer() -> ResearchAnswer {
        answer(
            para(
                "Data-center revenue grew 200% as AI demand accelerated the bull case.",
                &[("S1", "revenue grew 200%")],
            ),
            vec![
                AnswerSection {
                    heading: "Catalysts".into(),
                    paragraphs: vec![para(
                        "AI demand is the primary catalyst.",
                        &[("S1", "AI demand accelerated")],
                    )],
                },
                AnswerSection {
                    heading: "Risks".into(),
                    paragraphs: vec![para(
                        "Competition and export controls could invalidate the thesis.",
                        &[("S2", "export controls are key risks")],
                    )],
                },
            ],
            ledger(),
        )
    }

    fn weak_answer() -> ResearchAnswer {
        // Missing both facts + a section, uncited summary, one hallucinated
        // quote (not in any excerpt), and too few sources.
        answer(
            para("Some generic take.", &[]),
            vec![AnswerSection {
                heading: "Catalysts".into(),
                paragraphs: vec![para(
                    "AI stuff.",
                    &[("S1", "a quote that is not in the excerpt")],
                )],
            }],
            vec![src(
                "S1",
                "Data-center revenue grew 200% as AI demand accelerated.",
            )],
        )
    }

    #[test]
    fn strong_answer_scores_near_perfect() {
        let s = grade(&strong_answer(), &gold());
        assert_eq!(s.completeness, 1.0, "both facts stated in prose");
        assert_eq!(s.section_coverage, 1.0, "both sections present");
        assert_eq!(s.citation_coverage, 1.0, "every paragraph cited");
        assert_eq!(
            s.quote_integrity, 1.0,
            "every quote resolves to a Read excerpt"
        );
        assert_eq!(s.source_sufficiency, 1.0);
        assert!((s.overall - 1.0).abs() < 1e-9, "overall {}", s.overall);
    }

    #[test]
    fn weak_answer_scores_low_on_every_axis() {
        let s = grade(&weak_answer(), &gold());
        assert_eq!(s.completeness, 0.0, "neither fact stated");
        assert_eq!(s.section_coverage, 0.5, "only Catalysts present");
        assert_eq!(s.citation_coverage, 0.5, "summary uncited");
        assert_eq!(s.quote_integrity, 0.0, "the one quote is unfaithful");
        assert_eq!(s.source_sufficiency, 0.0, "only one source");
        assert!(
            s.overall < 0.3,
            "weak overall must be low, got {}",
            s.overall
        );
    }

    #[test]
    fn completeness_ignores_citation_quotes() {
        // Prose omits "revenue grew 200%" but the citation quote contains it.
        // Only "export controls" is actually stated → completeness 0.5, proving
        // a quote never grants completeness credit.
        let a = answer(
            para("Nvidia looks strong.", &[("S1", "revenue grew 200%")]),
            vec![AnswerSection {
                heading: "Risks".into(),
                paragraphs: vec![para(
                    "Export controls are a risk.",
                    &[("S2", "export controls are key risks")],
                )],
            }],
            ledger(),
        );
        let s = grade(&a, &gold());
        assert_eq!(
            s.completeness, 0.5,
            "quoted-but-unstated fact earns no credit"
        );
        assert_eq!(s.quote_integrity, 1.0, "both quotes are still faithful");
    }

    #[test]
    fn empty_expectations_do_not_divide_by_zero() {
        let g = GoldAnswer {
            case_id: "x".into(),
            expected_facts: vec![],
            expected_sections: vec![],
            min_sources: 0,
        };
        let a = answer(
            para("hi", &[("S1", "AI demand accelerated")]),
            vec![],
            ledger(),
        );
        let s = grade(&a, &g);
        assert_eq!(s.completeness, 1.0);
        assert_eq!(s.section_coverage, 1.0);
        assert_eq!(s.source_sufficiency, 1.0);
    }

    #[test]
    fn sweep_ranks_a_good_variant_above_a_flawed_one() {
        let artifacts = vec![
            AnswerArtifact {
                model: "m-bad".into(),
                prompt_variant: "v1".into(),
                case_id: "web_current_factual".into(),
                answer: weak_answer(),
            },
            AnswerArtifact {
                model: "m-good".into(),
                prompt_variant: "v1".into(),
                case_id: "web_current_factual".into(),
                answer: strong_answer(),
            },
            // No gold for this case_id → ignored, not a regression.
            AnswerArtifact {
                model: "m-good".into(),
                prompt_variant: "v1".into(),
                case_id: "digest_only_case".into(),
                answer: weak_answer(),
            },
        ];
        let rep = run_sweep(&artifacts, &[gold()]).expect("no duplicates");
        assert_eq!(rep.variants.len(), 2, "two variants graded");
        let best = rep.best().unwrap();
        assert_eq!(best.model, "m-good", "good variant ranks first");
        assert_eq!(best.covered, 1, "the no-gold case was ignored");
        assert_eq!(best.total, 1);
        assert!(
            best.mean_overall > rep.variants[1].mean_overall,
            "good beats bad"
        );
        assert!(report_sweep(&rep).contains("#1 m-good::v1"));
    }

    #[test]
    fn full_corpus_coverage_beats_one_perfect_case() {
        // "second" case gold, distinct facts/section.
        let gold_b = GoldAnswer {
            case_id: "second".into(),
            expected_facts: vec![ExpectedFact {
                any_of: vec!["margins expanded".into()],
            }],
            expected_sections: vec!["Margins".into()],
            min_sources: 1,
        };
        let src_b = src("S3", "Operating margins expanded materially this year.");
        let answer_b = answer(
            para(
                "Margins expanded on operating leverage.",
                &[("S3", "margins expanded")],
            ),
            vec![AnswerSection {
                heading: "Margins".into(),
                paragraphs: vec![para(
                    "Operating leverage lifted margins.",
                    &[("S3", "margins expanded")],
                )],
            }],
            vec![src_b],
        );

        let artifacts = vec![
            // "wide" answers BOTH gold cases well.
            AnswerArtifact {
                model: "wide".into(),
                prompt_variant: "v".into(),
                case_id: "web_current_factual".into(),
                answer: strong_answer(),
            },
            AnswerArtifact {
                model: "wide".into(),
                prompt_variant: "v".into(),
                case_id: "second".into(),
                answer: answer_b,
            },
            // "narrow" answers only ONE case, perfectly.
            AnswerArtifact {
                model: "narrow".into(),
                prompt_variant: "v".into(),
                case_id: "web_current_factual".into(),
                answer: strong_answer(),
            },
        ];
        let rep = run_sweep(&artifacts, &[gold(), gold_b]).expect("no dups");
        let best = rep.best().unwrap();
        assert_eq!(
            best.model, "wide",
            "full coverage wins over one perfect case"
        );
        assert_eq!(best.covered, 2);
        assert_eq!(best.total, 2);
        let narrow = rep.variants.iter().find(|v| v.model == "narrow").unwrap();
        assert_eq!(narrow.covered, 1);
        assert_eq!(narrow.total, 2);
        // narrow's one perfect case is halved by the unanswered second case.
        assert!(
            (narrow.mean_overall - 0.5).abs() < 1e-9,
            "narrow {}",
            narrow.mean_overall
        );
        assert!(best.mean_overall > narrow.mean_overall);
    }

    #[test]
    fn duplicate_artifact_is_rejected() {
        let dup = AnswerArtifact {
            model: "m".into(),
            prompt_variant: "v".into(),
            case_id: "web_current_factual".into(),
            answer: strong_answer(),
        };
        let err = run_sweep(&[dup.clone(), dup], &[gold()]).unwrap_err();
        assert!(matches!(err, SweepError::DuplicateArtifact { .. }));
    }

    #[test]
    fn padding_sources_does_not_earn_sufficiency() {
        // min_sources=2, but the answer cites only one Read source; the ledger
        // is padded with a second FAILED, uncited record. Sufficiency stays 0.
        let failed = SourceRecord {
            status: SourceStatus::Failed,
            excerpt: None,
            error_code: Some("fetch_failed".into()),
            ..src("S2", "unused")
        };
        let a = answer(
            para(
                "Data-center revenue grew 200% and export controls are a risk.",
                &[("S1", "revenue grew 200%")],
            ),
            vec![
                AnswerSection {
                    heading: "Catalysts".into(),
                    paragraphs: vec![para("AI demand.", &[("S1", "AI demand accelerated")])],
                },
                AnswerSection {
                    heading: "Risks".into(),
                    paragraphs: vec![para("Export risk.", &[("S1", "AI demand accelerated")])],
                },
            ],
            vec![
                src(
                    "S1",
                    "Data-center revenue grew 200% as AI demand accelerated.",
                ),
                failed,
            ],
        );
        let s = grade(&a, &gold());
        assert_eq!(
            s.source_sufficiency, 0.0,
            "one cited Read source < min_sources 2"
        );
        // Sanity: the other axes are unaffected by the padding.
        assert_eq!(s.completeness, 1.0);
        assert_eq!(s.quote_integrity, 1.0);
    }

    #[test]
    fn uncited_but_complete_answer_scores_low() {
        // Perfect prose + sections, but ZERO citations: no evidence at all.
        // Must not bank faithfulness's 0.30 — caps at completeness+section.
        let a = answer(
            para(
                "Data-center revenue grew 200% as AI demand accelerated.",
                &[],
            ),
            vec![
                AnswerSection {
                    heading: "Catalysts".into(),
                    paragraphs: vec![para("AI demand is the catalyst.", &[])],
                },
                AnswerSection {
                    heading: "Risks".into(),
                    paragraphs: vec![para("Export controls are a risk.", &[])],
                },
            ],
            ledger(),
        );
        let s = grade(&a, &gold());
        assert_eq!(s.completeness, 1.0);
        assert_eq!(s.section_coverage, 1.0);
        assert_eq!(s.citation_coverage, 0.0, "nothing cited");
        assert_eq!(
            s.quote_integrity, 0.0,
            "no citations → no quote-integrity credit"
        );
        assert_eq!(s.source_sufficiency, 0.0, "no cited Read sources");
        assert!(
            (s.overall - 0.50).abs() < 1e-9,
            "capped at 0.50, got {}",
            s.overall
        );
    }

    #[test]
    fn paraphrase_alias_is_accepted() {
        // Prose uses an accepted alias ("increased 200%"), not the primary
        // phrasing; the revenue fact still counts once.
        let a = answer(
            para(
                "Data-center revenue increased 200% year over year.",
                &[("S1", "revenue grew 200%")],
            ),
            vec![AnswerSection {
                heading: "Risks".into(),
                paragraphs: vec![para(
                    "Export controls are a risk.",
                    &[("S2", "export controls are key risks")],
                )],
            }],
            ledger(),
        );
        assert_eq!(
            grade(&a, &gold()).completeness,
            1.0,
            "alias phrasing surfaces the fact"
        );
    }

    #[test]
    fn wrong_value_or_magnitude_earns_no_credit() {
        // "doubled" (2×) and "grew 100%" are NOT "grew 200%" (3×): neither is an
        // accepted alias, so the revenue fact misses. Only export controls hits.
        let a = answer(
            para(
                "Revenue doubled and later grew 100%.",
                &[("S1", "revenue grew 200%")],
            ),
            vec![AnswerSection {
                heading: "Risks".into(),
                paragraphs: vec![para(
                    "Export controls are a risk.",
                    &[("S2", "export controls are key risks")],
                )],
            }],
            ledger(),
        );
        assert_eq!(
            grade(&a, &gold()).completeness,
            0.5,
            "wrong magnitude is not the fact"
        );
    }

    #[test]
    fn case_mismatched_quote_is_unfaithful() {
        // Artifact built DIRECTLY (bypassing build_answer/validate_synthesis).
        // The quote differs only in case; production validation is case-sensitive,
        // so the scorer must score it unfaithful too.
        let a = answer(
            para("Revenue grew.", &[("S1", "REVENUE GREW 200%")]),
            vec![],
            vec![src(
                "S1",
                "Data-center revenue grew 200% as AI demand accelerated.",
            )],
        );
        assert_eq!(
            grade(&a, &gold()).quote_integrity,
            0.0,
            "case-mismatched quote grounds nothing"
        );
    }

    #[test]
    fn blank_quote_is_unfaithful() {
        let a = answer(
            para("Revenue grew.", &[("S1", "   ")]),
            vec![],
            vec![src(
                "S1",
                "Data-center revenue grew 200% as AI demand accelerated.",
            )],
        );
        assert_eq!(
            grade(&a, &gold()).quote_integrity,
            0.0,
            "blank quote grounds nothing"
        );
    }

    #[test]
    fn validate_gold_rejects_malformed_specs() {
        let empty = GoldAnswer {
            case_id: "a".into(),
            expected_facts: vec![ExpectedFact { any_of: vec![] }],
            expected_sections: vec![],
            min_sources: 0,
        };
        assert!(matches!(
            validate_gold(std::slice::from_ref(&empty)),
            Err(GoldError::EmptyFact { .. })
        ));
        let blank = GoldAnswer {
            case_id: "a".into(),
            expected_facts: vec![ExpectedFact {
                any_of: vec!["  ".into()],
            }],
            expected_sections: vec![],
            min_sources: 0,
        };
        assert!(matches!(
            validate_gold(std::slice::from_ref(&blank)),
            Err(GoldError::BlankAlias { .. })
        ));
        assert!(matches!(
            validate_gold(&[gold(), gold()]),
            Err(GoldError::DuplicateCase { .. })
        ));
        assert!(validate_gold(&[gold()]).is_ok());
    }

    #[test]
    fn run_sweep_rejects_invalid_gold() {
        let art = AnswerArtifact {
            model: "m".into(),
            prompt_variant: "v".into(),
            case_id: "web_current_factual".into(),
            answer: strong_answer(),
        };
        assert!(matches!(
            run_sweep(&[art], &[gold(), gold()]),
            Err(SweepError::InvalidGold(_))
        ));
    }

    #[test]
    fn ingests_json_artifacts_and_ranks() {
        // Serialize artifacts + gold exactly as a producer would, then ingest.
        let arts = vec![
            AnswerArtifact {
                model: "m-bad".into(),
                prompt_variant: "v".into(),
                case_id: "web_current_factual".into(),
                answer: weak_answer(),
            },
            AnswerArtifact {
                model: "m-good".into(),
                prompt_variant: "v".into(),
                case_id: "web_current_factual".into(),
                answer: strong_answer(),
            },
        ];
        let aj = serde_json::to_string(&arts).expect("serialize artifacts");
        let gj = serde_json::json!({ "schema_version": 1, "gold": [gold()] }).to_string();
        let rep = run_sweep_from_json(&aj, &gj).expect("ingest");
        assert_eq!(
            rep.best().unwrap().model,
            "m-good",
            "JSON round-trip preserves ranking"
        );
    }

    #[test]
    fn ingest_rejects_malformed_json_and_bad_gold() {
        assert!(matches!(
            run_sweep_from_json("not json", "[]"),
            Err(IngestError::Json(_))
        ));
        // Wrapped gold with a duplicate case_id.
        let gj = serde_json::json!({ "schema_version": 1, "gold": [gold(), gold()] }).to_string();
        assert!(matches!(
            run_sweep_from_json("[]", &gj),
            Err(IngestError::Sweep(SweepError::InvalidGold(_)))
        ));
    }
}
