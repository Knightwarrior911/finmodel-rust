//! Embedded, typed finance `WorkflowSpec` definitions (no arbitrary scripts).
//!
//! These are pure data + validation, following the Phase A workflow-contract
//! validation. The Tauri driver (`src-tauri/src/agent/workflows.rs`) executes a
//! spec against the tool registry and scheduler; nothing here performs I/O.

use serde::{Deserialize, Serialize};

use crate::budget::Policy;
use crate::types::{Confidentiality, PartKind};

/// Approval posture for a workflow's writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalPolicy {
    /// Read-only workflow; nothing to approve.
    None,
    /// New immutable versions auto-run; in-place overwrite/export needs approval.
    NewVersionAuto,
}

/// A finance workflow contract.
/// Constructed in code; never deserialized from external input.
pub struct WorkflowSpec {
    pub id: &'static str,
    pub label: &'static str,
    /// Tools that MUST be available for this workflow.
    pub required_tools: &'static [&'static str],
    /// The full set of tools the workflow may use (superset of required).
    pub allowed_tools: &'static [&'static str],
    /// Default confidentiality when a workspace is created for this workflow
    /// kind (deal/company default Confidential; sector/personal Standard).
    pub default_confidentiality: Confidentiality,
    pub required_input: &'static [&'static str],
    pub optional_input: &'static [&'static str],
    pub expected_parts: &'static [PartKind],
    pub policy: Policy,
    pub max_children: u32,
    /// Numeric/current/artifact turns must pass verification.
    pub needs_verification: bool,
    pub approval_policy: ApprovalPolicy,
    pub plan_template: &'static str,
    pub disclaimer: &'static str,
    /// A golden fixture validated earliest (earnings review, trading comps).
    pub golden: bool,
}

impl WorkflowSpec {
    /// Validate a user-input object: every required field present and non-null.
    pub fn validate_input(&self, args: &serde_json::Value) -> Result<(), String> {
        for req in self.required_input {
            let present = args.get(*req).map(|v| !v.is_null()).unwrap_or(false);
            if !present {
                return Err(format!(
                    "workflow `{}` missing required input `{}`",
                    self.id, req
                ));
            }
        }
        Ok(())
    }

    /// Whether `tool` is permitted in this workflow.
    pub fn allows_tool(&self, tool: &str) -> bool {
        self.allowed_tools.contains(&tool)
    }

    /// Whether `required_tools` is a subset of `allowed_tools` (a spec invariant).
    pub fn is_consistent(&self) -> bool {
        self.required_tools
            .iter()
            .all(|t| self.allowed_tools.contains(t))
    }

    /// The initial visible plan: the `plan_template` arrow-chain split into
    /// stable, ordered steps (`s1..sN`). The orchestrator may refine labels,
    /// objective, and assumptions, but must not delete or invent these steps —
    /// authority-changing steps stay fixed (Task 3.2).
    pub fn initial_plan(&self, objective: &str) -> crate::types::Plan {
        let steps: Vec<crate::types::PlanStep> = self
            .plan_template
            .split('→')
            .map(|s| s.trim().trim_end_matches('.').trim())
            .filter(|s| !s.is_empty())
            .enumerate()
            .map(|(i, label)| crate::types::PlanStep {
                id: format!("s{}", i + 1),
                label: label.to_string(),
                status: crate::types::PlanStepStatus::Pending,
            })
            .collect();
        let objective: String = if objective.trim().is_empty() {
            self.label.to_string()
        } else {
            objective.trim().chars().take(140).collect()
        };
        crate::types::Plan {
            objective,
            assumptions: Vec::new(),
            steps,
            version: 1,
        }
    }

    /// The expected output parts still missing from `produced` (Task 9.1). A
    /// workflow may not reach success while any expected part is missing — the
    /// completion gate. Duplicates in `produced` are fine.
    pub fn missing_parts(
        &self,
        produced: &[crate::types::PartKind],
    ) -> Vec<crate::types::PartKind> {
        self.expected_parts
            .iter()
            .copied()
            .filter(|want| !produced.contains(want))
            .collect()
    }

    /// Whether every expected part has been produced.
    pub fn is_complete(&self, produced: &[crate::types::PartKind]) -> bool {
        self.missing_parts(produced).is_empty()
    }
}

/// The six embedded workflows.
pub fn builtin_workflows() -> Vec<WorkflowSpec> {
    vec![
        WorkflowSpec {
            id: "company_brief",
            label: "Company/sector brief",
            required_tools: &["research", "read_page"],
            allowed_tools: &["research", "read_page", "web_search", "get_news", "list_filings", "read_filing", "get_quote", "draft_memo"],
            default_confidentiality: Confidentiality::Confidential,
            required_input: &["entity"],
            optional_input: &["as_of", "focus_areas"],
            expected_parts: &[PartKind::Text, PartKind::Sources, PartKind::Artifact],
            policy: Policy::WORKFLOW,
            max_children: 12,
            needs_verification: true,
            approval_policy: ApprovalPolicy::NewVersionAuto,
            plan_template: "Gather primary sources → synthesize brief → cite every figure → draft the company profile.",
            disclaimer: "Source-grounded summary; not investment advice.",
            golden: false,
        },
        WorkflowSpec {
            id: "earnings_review",
            label: "Earnings review",
            required_tools: &["list_filings", "read_filing", "get_news", "get_quote"],
            allowed_tools: &["list_filings", "read_filing", "get_news", "get_quote", "research", "web_search", "benchmark_peers", "draft_memo"],
            default_confidentiality: Confidentiality::Confidential,
            required_input: &["ticker"],
            optional_input: &["period", "peer_prior"],
            expected_parts: &[PartKind::Result, PartKind::Sources],
            policy: Policy::WORKFLOW,
            max_children: 12,
            needs_verification: true,
            approval_policy: ApprovalPolicy::NewVersionAuto,
            plan_template: "Latest filing → period metrics → guidance/news → prior comparison → cited variance table → draft the earnings note.",
            disclaimer: "Figures normalized to the issuer fiscal calendar.",
            golden: true,
        },
        WorkflowSpec {
            id: "trading_comps",
            label: "Trading comps",
            required_tools: &["benchmark_peers", "get_quote", "list_filings"],
            allowed_tools: &["benchmark_peers", "get_quote", "list_filings", "read_filing", "build_model"],
            default_confidentiality: Confidentiality::Confidential,
            required_input: &["tickers"],
            optional_input: &["multiples", "as_of", "to_usd"],
            expected_parts: &[PartKind::Result, PartKind::Artifact],
            policy: Policy::WORKFLOW,
            max_children: 12,
            needs_verification: true,
            approval_policy: ApprovalPolicy::NewVersionAuto,
            plan_template: "Assemble peer set → per-peer metrics → normalized comps table (as-of dated).",
            disclaimer: "Multiples as of the stated date; units/currency normalized.",
            golden: true,
        },
        WorkflowSpec {
            id: "dcf_model",
            label: "DCF / 3-statement",
            required_tools: &["build_model"],
            allowed_tools: &["build_model", "read_filing", "list_filings", "get_quote"],
            default_confidentiality: Confidentiality::Confidential,
            required_input: &["ticker"],
            optional_input: &["case", "overrides"],
            expected_parts: &[PartKind::Result, PartKind::Artifact],
            policy: Policy::WORKFLOW,
            max_children: 4,
            needs_verification: true,
            approval_policy: ApprovalPolicy::NewVersionAuto,
            plan_template: "Extract → build 3-statement + DCF → verify balance/WACC → immutable workbook.",
            disclaimer: "Engine-computed; assumptions separated; not investment advice.",
            golden: false,
        },
        WorkflowSpec {
            id: "ma_screen",
            label: "M&A / deal screen",
            required_tools: &["research_deal", "get_news", "web_search"],
            allowed_tools: &["research_deal", "get_news", "web_search", "read_page", "draft_memo"],
            default_confidentiality: Confidentiality::Confidential,
            required_input: &["theme"],
            optional_input: &["min_size", "since", "status"],
            expected_parts: &[PartKind::Result, PartKind::Sources],
            policy: Policy::WORKFLOW,
            max_children: 12,
            needs_verification: true,
            approval_policy: ApprovalPolicy::None,
            plan_template: "Find precedents → announcement date/status per row → cited screen table → draft the deal summary.",
            disclaimer: "Each precedent carries announcement date and status.",
            golden: false,
        },
        WorkflowSpec {
            id: "pitch_prep",
            label: "Pitch / meeting prep",
            required_tools: &["research", "build_model"],
            allowed_tools: &["research", "research_deal", "web_search", "get_news", "list_filings", "read_filing", "benchmark_peers", "build_model", "draft_memo"],
            default_confidentiality: Confidentiality::Confidential,
            required_input: &["deal"],
            optional_input: &["sections", "audience"],
            expected_parts: &[PartKind::Artifact, PartKind::Sources],
            policy: Policy::WORKFLOW,
            max_children: 12,
            needs_verification: true,
            approval_policy: ApprovalPolicy::NewVersionAuto,
            plan_template: "Research → assemble deck via one write-capable parent → cite every figure.",
            disclaimer: "Deck figures inherit source verification.",
            golden: false,
        },
    ]
}

/// Look up a workflow by id.
pub fn workflow(id: &str) -> Option<WorkflowSpec> {
    builtin_workflows().into_iter().find(|w| w.id == id)
}

/// Deterministically select a workflow id from a user message using
/// high-confidence keyword intents, most specific first. Under-specified or
/// ambiguous asks return `None` — the run stays on the interactive policy and
/// proposes a reversible scope rather than committing to a workflow. A typed
/// low-temperature classifier for the ambiguous middle is a planned refinement.
pub fn select_workflow(user_msg: &str) -> Option<&'static str> {
    let m = user_msg.to_lowercase();
    let has = |ns: &[&str]| ns.iter().any(|n| m.contains(n));
    // Targeted lookups stay interactive: "did they say anything about X?" wants
    // a direct sourced answer, not a five-deliverable workflow. Escalating a
    // lookup burns the turn on scope nobody asked for (a v0.9.10 tariff
    // question became a full earnings review this way).
    if has(&[
        "say anything",
        "said anything",
        "mention",
        "talk about",
        "talked about",
        "discuss",
        "comment on",
        "comments on",
        "did they say",
        "did it say",
    ]) {
        return None;
    }
    if has(&[
        "pitch",
        "deck",
        "board-ready",
        "board ready",
        "pitch book",
        "meeting prep",
        "board deck",
    ]) {
        return Some("pitch_prep");
    }
    if has(&[
        "m&a",
        "precedent transaction",
        "precedents",
        "deal screen",
        "screen deals",
        "announced deals",
        "acquisitions since",
        "deals since",
        "deal activity",
    ]) {
        return Some("ma_screen");
    }
    if has(&[
        "ev/ebitda",
        "ev / ebitda",
        "p/e",
        "trading comps",
        "comps",
        "multiples",
        "comparable companies",
    ]) {
        return Some("trading_comps");
    }
    if has(&[
        "dcf",
        "3-statement",
        "three-statement",
        "3 statement",
        "discounted cash flow",
        "base/bull/bear",
        "bull and bear",
        "intrinsic value",
    ]) {
        return Some("dcf_model");
    }
    if has(&[
        "earnings",
        "just reported",
        "beat/miss",
        "beat or miss",
        "beat vs miss",
        "guidance",
        "quarterly results",
        "q1 results",
        "q2 results",
        "q3 results",
        "q4 results",
        "latest quarter",
    ]) {
        return Some("earnings_review");
    }
    if has(&[
        "brief",
        "overview",
        "company profile",
        "tell me about",
        "summary of",
        "write-up",
        "primer",
        "deep dive",
    ]) {
        return Some("company_brief");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn six_workflows_present() {
        let ws = builtin_workflows();
        assert_eq!(ws.len(), 6);
        for id in [
            "company_brief",
            "earnings_review",
            "trading_comps",
            "dcf_model",
            "ma_screen",
            "pitch_prep",
        ] {
            assert!(workflow(id).is_some(), "missing {id}");
        }
    }

    #[test]
    fn required_tools_subset_of_allowed() {
        for w in builtin_workflows() {
            assert!(w.is_consistent(), "{} required tools not all allowed", w.id);
        }
    }

    #[test]
    fn golden_fixtures_are_earnings_and_comps() {
        let golden: Vec<&str> = builtin_workflows()
            .iter()
            .filter(|w| w.golden)
            .map(|w| w.id)
            .collect();
        assert_eq!(golden, vec!["earnings_review", "trading_comps"]);
    }

    #[test]
    fn input_validation() {
        let comps = workflow("trading_comps").unwrap();
        assert!(comps
            .validate_input(&serde_json::json!({"tickers":["NVDA","AMD"]}))
            .is_ok());
        assert!(comps.validate_input(&serde_json::json!({})).is_err());
    }

    #[test]
    fn numeric_workflows_require_verification() {
        for id in ["earnings_review", "trading_comps", "dcf_model"] {
            assert!(workflow(id).unwrap().needs_verification, "{id} must verify");
        }
    }

    #[test]
    fn workflow_budgets_are_workflow_policy() {
        for w in builtin_workflows() {
            assert_eq!(w.policy, Policy::WORKFLOW);
        }
        // DCF caps children at 4 (single-model build), comps allows up to 12.
        assert_eq!(workflow("dcf_model").unwrap().max_children, 4);
        assert_eq!(workflow("trading_comps").unwrap().max_children, 12);
    }

    #[test]
    fn allows_tool_checks_membership() {
        let comps = workflow("trading_comps").unwrap();
        assert!(comps.allows_tool("benchmark_peers"));
        assert!(!comps.allows_tool("analyze_pdf"));
    }

    #[test]
    fn deal_workflows_default_confidential() {
        assert_eq!(
            workflow("earnings_review").unwrap().default_confidentiality,
            Confidentiality::Confidential
        );
    }

    #[test]
    fn golden_fixtures_select_expected_workflow() {
        assert_eq!(
            select_workflow(
                "NVDA just reported — beat/miss versus the prior period and explain guidance changes."
            ),
            Some("earnings_review")
        );
        assert_eq!(
            select_workflow(
                "Compare NVDA, AMD, and INTC on current EV/EBITDA and P/E and give me an Excel workbook."
            ),
            Some("trading_comps")
        );
    }

    #[test]
    fn all_six_workflows_accept_a_representative_request() {
        let cases = [
            ("Give me a brief on Apple's cloud segment", "company_brief"),
            ("Review NVDA earnings and guidance", "earnings_review"),
            (
                "Trading comps for the megacap chipmakers on EV/EBITDA",
                "trading_comps",
            ),
            ("Build a base/bull/bear DCF for MSFT", "dcf_model"),
            (
                "Screen announced European payments deals since 2024",
                "ma_screen",
            ),
            (
                "Prepare a board-ready acquisition pitch for Deal X",
                "pitch_prep",
            ),
        ];
        for (msg, id) in cases {
            assert_eq!(select_workflow(msg), Some(id), "request: {msg}");
            // Every selected workflow must actually exist in the registry.
            assert!(workflow(id).is_some());
        }
    }

    #[test]
    fn underspecified_ask_stays_interactive() {
        assert_eq!(select_workflow("Check on NVDA"), None);
        assert_eq!(select_workflow("what's up with tesla"), None);
        assert_eq!(select_workflow(""), None);
    }

    #[test]
    fn targeted_lookups_stay_interactive() {
        // A question about what a filing/call SAID is a lookup, not a mission —
        // even when it names an earnings release or guidance.
        for msg in [
            "in the first quarter earnings release did tesla say anything about tariff impact or competition from china?",
            "did NVDA mention export controls in the latest quarter?",
            "any mention of buybacks in the Q2 results?",
            "did management discuss margins on the earnings call?",
            "what did the 10-K say — did they comment on China competition?",
        ] {
            assert_eq!(select_workflow(msg), None, "request: {msg}");
        }
        // Full-review asks still route to the workflow.
        assert_eq!(
            select_workflow("Review NVDA earnings and guidance"),
            Some("earnings_review")
        );
    }

    #[test]
    fn initial_plan_splits_template_into_stable_steps() {
        let er = workflow("earnings_review").unwrap();
        let plan = er.initial_plan("NVDA earnings");
        assert_eq!(plan.objective, "NVDA earnings");
        assert_eq!(plan.version, 1);
        // 6-arrow template → 6 stable steps s1..s6 (the draft-the-note step
        // completes the mission with a written deliverable).
        assert_eq!(plan.steps.len(), 6);
        assert_eq!(plan.steps[0].id, "s1");
        assert_eq!(plan.steps[0].label, "Latest filing");
        assert_eq!(plan.steps[5].id, "s6");
        assert_eq!(plan.steps[5].label, "draft the earnings note");
        assert!(plan
            .steps
            .iter()
            .all(|s| s.status == crate::types::PlanStepStatus::Pending));
        assert!(!plan.is_empty());
    }

    #[test]
    fn every_workflow_has_a_non_empty_initial_plan() {
        for w in builtin_workflows() {
            let plan = w.initial_plan("");
            assert!(!plan.is_empty(), "empty plan for {}", w.id);
            assert!(!plan.steps.is_empty(), "no steps for {}", w.id);
        }
    }

    #[test]
    fn completion_gate_requires_all_expected_parts() {
        use crate::types::PartKind;
        let er = workflow("earnings_review").unwrap();
        // earnings_review expects [Result, Sources].
        assert!(!er.is_complete(&[PartKind::Result]));
        assert_eq!(
            er.missing_parts(&[PartKind::Result]),
            vec![PartKind::Sources]
        );
        assert!(er.is_complete(&[PartKind::Result, PartKind::Sources]));
    }

    #[test]
    fn every_workflow_completes_only_with_its_expected_parts() {
        for w in builtin_workflows() {
            let full: Vec<_> = w.expected_parts.to_vec();
            assert!(
                w.is_complete(&full),
                "{} should complete with all parts",
                w.id
            );
            if w.expected_parts.len() > 1 {
                let missing_one: Vec<_> = w.expected_parts.iter().skip(1).copied().collect();
                assert!(
                    !w.is_complete(&missing_one),
                    "{} must not reach success missing an expected part",
                    w.id
                );
            }
        }
    }
}
