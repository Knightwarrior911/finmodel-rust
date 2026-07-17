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
                return Err(format!("workflow `{}` missing required input `{}`", self.id, req));
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
        self.required_tools.iter().all(|t| self.allowed_tools.contains(t))
    }
}

/// The six embedded workflows.
pub fn builtin_workflows() -> Vec<WorkflowSpec> {
    vec![
        WorkflowSpec {
            id: "company_brief",
            label: "Company/sector brief",
            required_tools: &["research", "read_page"],
            allowed_tools: &["research", "read_page", "web_search", "get_news", "list_filings", "read_filing", "get_quote"],
            default_confidentiality: Confidentiality::Confidential,
            required_input: &["entity"],
            optional_input: &["as_of", "focus_areas"],
            expected_parts: &[PartKind::Text, PartKind::Sources, PartKind::Artifact],
            policy: Policy::WORKFLOW,
            max_children: 12,
            needs_verification: true,
            approval_policy: ApprovalPolicy::NewVersionAuto,
            plan_template: "Gather primary sources → synthesize brief → cite every figure.",
            disclaimer: "Source-grounded summary; not investment advice.",
            golden: false,
        },
        WorkflowSpec {
            id: "earnings_review",
            label: "Earnings review",
            required_tools: &["list_filings", "read_filing", "get_news", "get_quote"],
            allowed_tools: &["list_filings", "read_filing", "get_news", "get_quote", "research", "web_search", "benchmark_peers"],
            default_confidentiality: Confidentiality::Confidential,
            required_input: &["ticker"],
            optional_input: &["period", "peer_prior"],
            expected_parts: &[PartKind::Result, PartKind::Sources],
            policy: Policy::WORKFLOW,
            max_children: 12,
            needs_verification: true,
            approval_policy: ApprovalPolicy::NewVersionAuto,
            plan_template: "Latest filing → period metrics → guidance/news → prior comparison → cited variance table.",
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
            allowed_tools: &["research_deal", "get_news", "web_search", "read_page"],
            default_confidentiality: Confidentiality::Confidential,
            required_input: &["theme"],
            optional_input: &["min_size", "since", "status"],
            expected_parts: &[PartKind::Result, PartKind::Sources],
            policy: Policy::WORKFLOW,
            max_children: 12,
            needs_verification: true,
            approval_policy: ApprovalPolicy::None,
            plan_template: "Find precedents → announcement date/status per row → cited screen table.",
            disclaimer: "Each precedent carries announcement date and status.",
            golden: false,
        },
        WorkflowSpec {
            id: "pitch_prep",
            label: "Pitch / meeting prep",
            required_tools: &["research", "build_model"],
            allowed_tools: &["research", "research_deal", "web_search", "get_news", "list_filings", "read_filing", "benchmark_peers", "build_model"],
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn six_workflows_present() {
        let ws = builtin_workflows();
        assert_eq!(ws.len(), 6);
        for id in ["company_brief", "earnings_review", "trading_comps", "dcf_model", "ma_screen", "pitch_prep"] {
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
        assert!(comps.validate_input(&serde_json::json!({"tickers":["NVDA","AMD"]})).is_ok());
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
        assert_eq!(workflow("earnings_review").unwrap().default_confidentiality, Confidentiality::Confidential);
    }
}
