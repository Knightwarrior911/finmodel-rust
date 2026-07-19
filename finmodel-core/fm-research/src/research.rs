//! Research domain contracts (Phase 2.1).
//!
//! Explicitly serialized types shared by the application-controlled research
//! workflow, the pure [`crate::machine::ResearchMachine`] reducer, and the
//! desktop UI. Enums are `snake_case`; structs `deny_unknown_fields` so a
//! malformed model or IPC payload is rejected, never silently coerced.
//!
//! The application owns the original question, source metadata/URLs/statuses,
//! limitations, confidence, timestamp, and model id. The ONLY thing the model
//! produces is a [`SynthesisDraft`]; everything else is app-derived and trusted.

use serde::{Deserialize, Serialize};

/// The research workflow's mode — selects which sources/collector shape apply.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResearchMode {
    Web,
    Company,
    Earnings,
    Filing,
    Deal,
    Comparison,
}

/// Effort level, mapped to concrete budgets by [`ResearchDepth::budgets`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResearchDepth {
    Quick,
    Standard,
    Deep,
}

/// Concrete per-depth budgets: max queries, max sources, and the overall
/// deadline. Fixed by the roadmap so a fixture executes an exact budget.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DepthBudgets {
    pub max_queries: u32,
    pub max_sources: u32,
    pub deadline_secs: u64,
}

impl ResearchDepth {
    /// Quick = 1 query / 3 sources / 30 s (fast check); Standard = 4 / 10 /
    /// 180 s (the default — sized so the primary-source query set runs in
    /// full); Deep = 8 / 16 / 420 s (exhaustive dig).
    pub fn budgets(self) -> DepthBudgets {
        match self {
            ResearchDepth::Quick => DepthBudgets {
                max_queries: 1,
                max_sources: 3,
                deadline_secs: 30,
            },
            ResearchDepth::Standard => DepthBudgets {
                max_queries: 4,
                max_sources: 10,
                deadline_secs: 180,
            },
            ResearchDepth::Deep => DepthBudgets {
                max_queries: 8,
                max_sources: 16,
                deadline_secs: 420,
            },
        }
    }

    /// Quick searches the user's question unchanged (no model planning round).
    pub fn plans(self) -> bool {
        !matches!(self, ResearchDepth::Quick)
    }
}

/// A validated research request. The application normalizes raw user text plus
/// the compact tool args into this before any stage runs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResearchRequest {
    pub question: String,
    pub mode: ResearchMode,
    #[serde(default)]
    pub tickers: Vec<String>,
    #[serde(default)]
    pub periods: Vec<String>,
    #[serde(default)]
    pub filing_forms: Vec<String>,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub acquirer: Option<String>,
    pub depth: ResearchDepth,
}

/// A per-mode validation failure (never a stage error).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestError(pub String);

impl std::fmt::Display for RequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for RequestError {}

impl ResearchRequest {
    /// Validate mode-specific requirements. A blank question always fails; a
    /// company/earnings/filing request needs at least one ticker; a deal needs a
    /// target; a comparison needs at least two tickers.
    pub fn validate(&self) -> Result<(), RequestError> {
        if self.question.trim().is_empty() {
            return Err(RequestError("question is empty".into()));
        }
        match self.mode {
            ResearchMode::Company | ResearchMode::Earnings | ResearchMode::Filing => {
                if self.tickers.is_empty() {
                    return Err(RequestError(format!(
                        "{:?} mode needs at least one ticker",
                        self.mode
                    )));
                }
            }
            ResearchMode::Deal => {
                if self
                    .target
                    .as_deref()
                    .map(str::trim)
                    .unwrap_or("")
                    .is_empty()
                {
                    return Err(RequestError("deal mode needs a target".into()));
                }
            }
            ResearchMode::Comparison => {
                if self.tickers.len() < 2 {
                    return Err(RequestError(
                        "comparison mode needs at least two tickers".into(),
                    ));
                }
            }
            ResearchMode::Web => {}
        }
        Ok(())
    }
}

/// The compact schema the LLM sees for the single `research` tool (exists only
/// for an allowed multi-action plan; normal research is application-invoked).
/// The application NEVER trusts the model's rewritten question — it normalizes
/// these hints against the ORIGINAL user text into a full [`ResearchRequest`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResearchToolArgs {
    #[serde(default)]
    pub question: String,
    #[serde(default = "default_mode")]
    pub mode: ResearchMode,
    #[serde(default)]
    pub tickers: Vec<String>,
    #[serde(default = "default_depth")]
    pub depth: ResearchDepth,
}

fn default_mode() -> ResearchMode {
    ResearchMode::Web
}

fn default_depth() -> ResearchDepth {
    ResearchDepth::Standard
}

impl ResearchToolArgs {
    /// Normalize into a full [`ResearchRequest`]. The model's `question` arg is
    /// the question (the model never rewrites it) and the tool's mode/tickers/
    /// depth as hints. Mode-specific fields (target/acquirer/periods/forms) are
    /// left empty for the application to fill from the routed intent / original
    /// user text (never model-supplied parties).
    /// the question of record: tool-calling exists so the model resolves the
    /// conversation into a self-contained ask ("yes" after "want me to check
    /// the 10-Q?" arrives here as a real question, not the literal word "yes" —
    /// which once sent research off to Yes Bank and the prog-rock band). The
    /// original user text is the fallback when the model passed nothing.
    /// Deal parties are still parsed from the user text by the app (never
    /// trusted from the model).
    pub fn into_request(self, original_user_text: &str) -> ResearchRequest {
        let model_q = self.question.trim();
        let question = if model_q.is_empty() {
            original_user_text.trim().to_string()
        } else {
            model_q.to_string()
        };
        ResearchRequest {
            question,
            mode: self.mode,
            tickers: self.tickers,
            periods: Vec::new(),
            filing_forms: Vec::new(),
            target: None,
            acquirer: None,
            depth: self.depth,
        }
    }
}

/// The bounded plan the model may return for Standard/Deep before synthesis.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResearchPlan {
    pub queries: Vec<String>,
    pub required_source_types: Vec<SourceKind>,
}

/// The read status of a consulted source. Only `Read` sources may back a
/// citation; blocked/thin/failed count as consulted but never support synthesis.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceStatus {
    Read,
    Blocked,
    Thin,
    Failed,
}

/// The evidentiary tier of a source, driving prioritization.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Regulatory,
    Company,
    Primary,
    Newswire,
    Secondary,
}

impl SourceKind {
    /// Lower rank = higher priority (regulatory/issuer/primary before newswire,
    /// newswire before generic secondary).
    pub fn rank(self) -> u8 {
        match self {
            SourceKind::Regulatory => 0,
            SourceKind::Company => 1,
            SourceKind::Primary => 2,
            SourceKind::Newswire => 3,
            SourceKind::Secondary => 4,
        }
    }
}

/// Which backend fetched a source.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceBackend {
    BasicHttp,
    RoamMcp,
}

/// A consulted source with full provenance. Stable `S1…` id is assigned after
/// canonical dedupe + initial ranking and NEVER changes once progress begins.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceRecord {
    pub id: String,
    pub requested_url: String,
    pub final_url: Option<String>,
    pub canonical_url: String,
    pub title: String,
    pub domain: String,
    pub retrieved_at: String,
    pub status: SourceStatus,
    pub kind: SourceKind,
    pub backend: SourceBackend,
    pub snippet: Option<String>,
    pub excerpt: Option<String>,
    pub error_code: Option<String>,
}

/// A citation: a source id plus the exact quote it backs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CitationRef {
    pub source_id: String,
    pub quote: String,
}

/// A paragraph plus the citations that back its factual claims.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CitedParagraph {
    pub text: String,
    pub citations: Vec<CitationRef>,
}

/// A titled group of cited paragraphs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnswerSection {
    pub heading: String,
    pub paragraphs: Vec<CitedParagraph>,
}

/// Derived answer confidence (app-derived, never model-set).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResearchConfidence {
    High,
    Medium,
    Low,
}

/// The complete, validated, citation-grounded answer. Everything except
/// `summary`/`sections` is app-owned and trusted.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResearchAnswer {
    pub question: String,
    pub summary: CitedParagraph,
    pub sections: Vec<AnswerSection>,
    pub sources: Vec<SourceRecord>,
    pub limitations: Vec<String>,
    pub confidence: ResearchConfidence,
    pub generated_at: String,
    pub model: String,
}

/// The ONLY thing the model produces during synthesis.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SynthesisDraft {
    pub summary: CitedParagraph,
    pub sections: Vec<AnswerSection>,
}

/// One item of a no-synthesis source digest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DigestItem {
    pub source_id: String,
    pub title: String,
    pub url: String,
    pub snippet: Option<String>,
    pub status: SourceStatus,
}

/// A source digest — the honest fallback when synthesis is impossible (no key,
/// twice-invalid synthesis, all-blocked). Never carries free-form prose.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResearchDigest {
    pub question: String,
    pub items: Vec<DigestItem>,
    pub limitations: Vec<String>,
    pub generated_at: String,
}

/// The terminal research payload: a validated answer OR an honest digest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "data")]
pub enum ResearchOutput {
    Answer(ResearchAnswer),
    Digest(ResearchDigest),
}

/// The staged lifecycle phase of a research run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResearchPhase {
    Planning,
    Searching,
    Reading,
    Synthesizing,
    Done,
    Cancelled,
    Error,
}

/// The phases a retry may resume from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetryPhase {
    Searching,
    Reading,
    Synthesizing,
}

/// A progress event. Within a phase, `total` is fixed on the first event and
/// `completed` is monotonic; source fields are non-null only during source
/// transitions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResearchProgress {
    pub run_id: String,
    pub parent_run_id: Option<String>,
    pub attempt: u32,
    pub conversation_id: String,
    pub phase: ResearchPhase,
    pub completed: u32,
    pub total: u32,
    pub source_id: Option<String>,
    pub title: Option<String>,
    pub url: Option<String>,
    pub source_status: Option<SourceStatus>,
    pub detail_code: Option<String>,
}

/// A terminal failure, paired with exactly one `research_error` event.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResearchFailure {
    pub run_id: String,
    pub conversation_id: String,
    pub phase: ResearchPhase,
    pub code: String,
    pub retryable: bool,
    pub retry_from: Option<RetryPhase>,
    pub backend_options: Vec<SourceBackend>,
    pub diagnostic_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn depth_budgets_match_roadmap() {
        assert_eq!(
            ResearchDepth::Quick.budgets(),
            DepthBudgets {
                max_queries: 1,
                max_sources: 3,
                deadline_secs: 30
            }
        );
        assert_eq!(
            ResearchDepth::Standard.budgets(),
            DepthBudgets {
                max_queries: 4,
                max_sources: 10,
                deadline_secs: 180
            }
        );
        assert_eq!(
            ResearchDepth::Deep.budgets(),
            DepthBudgets {
                max_queries: 8,
                max_sources: 16,
                deadline_secs: 420
            }
        );
        assert!(!ResearchDepth::Quick.plans());
        assert!(ResearchDepth::Standard.plans());
        assert!(ResearchDepth::Deep.plans());
    }

    #[test]
    fn enums_serialize_snake_case() {
        assert_eq!(
            serde_json::to_string(&ResearchMode::Comparison).unwrap(),
            "\"comparison\""
        );
        assert_eq!(
            serde_json::to_string(&SourceStatus::Read).unwrap(),
            "\"read\""
        );
        assert_eq!(
            serde_json::to_string(&SourceBackend::RoamMcp).unwrap(),
            "\"roam_mcp\""
        );
        assert_eq!(
            serde_json::to_string(&ResearchConfidence::Medium).unwrap(),
            "\"medium\""
        );
    }

    #[test]
    fn source_kind_priority_order() {
        assert!(SourceKind::Regulatory.rank() < SourceKind::Newswire.rank());
        assert!(SourceKind::Company.rank() < SourceKind::Secondary.rank());
        assert!(SourceKind::Primary.rank() < SourceKind::Newswire.rank());
    }

    #[test]
    fn request_validation_enforces_mode_requirements() {
        let base = ResearchRequest {
            question: "q".into(),
            mode: ResearchMode::Web,
            tickers: vec![],
            periods: vec![],
            filing_forms: vec![],
            target: None,
            acquirer: None,
            depth: ResearchDepth::Standard,
        };
        assert!(base.validate().is_ok());

        let mut blank = base.clone();
        blank.question = "   ".into();
        assert!(blank.validate().is_err());

        let mut company = base.clone();
        company.mode = ResearchMode::Company;
        assert!(company.validate().is_err(), "company needs a ticker");
        company.tickers = vec!["AAPL".into()];
        assert!(company.validate().is_ok());

        let mut deal = base.clone();
        deal.mode = ResearchMode::Deal;
        assert!(deal.validate().is_err(), "deal needs a target");
        deal.target = Some("Activision".into());
        assert!(deal.validate().is_ok());

        let mut cmp = base.clone();
        cmp.mode = ResearchMode::Comparison;
        cmp.tickers = vec!["KO".into()];
        assert!(cmp.validate().is_err(), "comparison needs two tickers");
        cmp.tickers.push("PEP".into());
        assert!(cmp.validate().is_ok());
    }

    #[test]
    fn request_rejects_unknown_fields() {
        let json = r#"{"question":"q","mode":"web","depth":"quick","bogus":1}"#;
        assert!(serde_json::from_str::<ResearchRequest>(json).is_err());
    }

    #[test]
    fn output_tagged_roundtrip() {
        let digest = ResearchOutput::Digest(ResearchDigest {
            question: "q".into(),
            items: vec![],
            limitations: vec!["The selected model could not produce a validated synthesis".into()],
            generated_at: "2026-01-01T00:00:00Z".into(),
        });
        let s = serde_json::to_string(&digest).unwrap();
        assert!(s.contains("\"kind\":\"digest\""));
        let back: ResearchOutput = serde_json::from_str(&s).unwrap();
        assert_eq!(back, digest);
    }

    #[test]
    fn tool_args_normalize_prefers_model_question() {
        let args: ResearchToolArgs = serde_json::from_str(
            r#"{"question":"NVDA data-center revenue growth drivers","mode":"company","tickers":["NVDA"],"depth":"deep"}"#,
        )
        .unwrap();
        // A follow-up turn's raw text ("yes") carries no searchable content;
        // the model's context-resolved question is the ask of record.
        let req = args.into_request("yes");
        assert_eq!(req.question, "NVDA data-center revenue growth drivers");
        assert_eq!(req.mode, ResearchMode::Company);
        assert_eq!(req.tickers, vec!["NVDA".to_string()]);
        assert_eq!(req.depth, ResearchDepth::Deep);
        assert!(req.validate().is_ok());
    }

    #[test]
    fn tool_args_deal_leaves_parties_for_app() {
        // Compact schema has no target/acquirer; the application fills them from
        // the original user text after into_request (never trusts model parties).
        let args: ResearchToolArgs = serde_json::from_str(
            r#"{"question":"rewrite","mode":"deal","tickers":[],"depth":"quick"}"#,
        )
        .unwrap();
        let mut req = args.into_request("Intel acquires Mobileye terms");
        assert_eq!(req.question, "rewrite");
        assert_eq!(req.mode, ResearchMode::Deal);
        assert!(req.target.is_none());
        assert!(req.acquirer.is_none());
        // Unfilled deal fails validation — the app must fill parties first.
        assert!(req.validate().is_err());
        req.target = Some("Mobileye".into());
        req.acquirer = Some("Intel".into());
        assert!(req.validate().is_ok());
    }

    #[test]
    fn tool_args_empty_question_falls_back_to_user_text() {
        let args: ResearchToolArgs =
            serde_json::from_str(r#"{"mode":"web","tickers":[],"depth":"quick"}"#).unwrap();
        let req = args.into_request("What did Tesla say about tariffs?");
        assert_eq!(req.question, "What did Tesla say about tariffs?");
    }

    #[test]
    fn tool_args_defaults_and_rejects_unknown() {
        let args: ResearchToolArgs = serde_json::from_str(r#"{"question":"q"}"#).unwrap();
        assert_eq!(args.mode, ResearchMode::Web);
        assert_eq!(args.depth, ResearchDepth::Standard);
        assert!(serde_json::from_str::<ResearchToolArgs>(r#"{"question":"q","bogus":1}"#).is_err());
    }
}
