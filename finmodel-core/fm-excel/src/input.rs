//! Writer input contract — everything the workbook builder consumes.
//!
//! Mirrors the Python `ExcelWriter.__init__` arguments: a `ModelOutput`
//! (historical + projected statement arrays), a derived `AssumptionsBlock`
//! (toggle + Base/Upside/Downside scenarios + shared valuation inputs), and
//! company metadata + the verification report.

use std::collections::HashMap;

/// A statement: line-item key → per-period values (some periods may be null).
pub type Statement = HashMap<String, Vec<Option<f64>>>;

/// The projected model output. Only the historical columns are written as
/// hardcoded numbers; projected columns are emitted as Excel formulas, so the
/// projected values here are used only where writer.py bakes them (none in the
/// snapshot layout beyond historicals).
#[derive(Clone, Debug, Default)]
pub struct ModelOutput {
    pub periods: Vec<String>,
    pub income_statement: Statement,
    pub balance_sheet: Statement,
    pub cash_flow_statement: Statement,
    pub plug_used: bool,
}

impl ModelOutput {
    /// Number of historical (`A`-suffixed) periods.
    pub fn n_hist(&self) -> usize {
        self.periods.iter().filter(|p| p.ends_with('A')).count()
    }
    /// Number of projected (`E`-suffixed) periods.
    pub fn n_proj(&self) -> usize {
        self.periods.iter().filter(|p| p.ends_with('E')).count()
    }
}

/// One scenario's projection drivers. Per-period lists carry one value per
/// projected period; the two valuation drivers are scenario scalars.
#[derive(Clone, Debug, Default)]
pub struct ScenarioInputs {
    pub name: String,
    pub revenue_growth_pct: Vec<f64>,
    pub gross_margin_pct: Vec<f64>,
    pub sga_pct_rev: Vec<f64>,
    pub rd_pct_rev: Vec<f64>,
    pub da_pct_rev: Vec<f64>,
    pub capex_pct_rev: Vec<f64>,
    pub tax_rate_pct: Vec<f64>,
    pub interest_rate_pct: Vec<f64>,
    pub dso_days: Vec<f64>,
    pub dio_days: Vec<f64>,
    pub dpo_days: Vec<f64>,
    pub dividend_per_share: Vec<f64>,
    pub terminal_growth_rate: f64,
    pub exit_ebitda_multiple: f64,
}

/// Toggle + three scenarios + shared (non-scenario) valuation inputs.
#[derive(Clone, Debug, Default)]
pub struct AssumptionsBlock {
    pub proj_periods: Vec<String>,
    pub active_case: i64,
    pub base: ScenarioInputs,
    pub upside: ScenarioInputs,
    pub downside: ScenarioInputs,
    pub risk_free_rate: f64,
    pub equity_risk_premium: f64,
    pub target_de_ratio: f64,
    pub cost_of_debt_pretax: f64,
    pub current_share_price: f64,
    pub shares_diluted: f64,
    /// Mid-year discounting convention for DCF (Python default True).
    pub mid_year_convention: bool,
}

/// Verification report — drives the Sources tab status + failures/warnings.
#[derive(Clone, Debug, Default)]
pub struct Verification {
    pub passed: bool,
    pub critical_failures: Vec<String>,
    pub warnings: Vec<String>,
    pub notes: Vec<String>,
}

/// Company / run metadata.
#[derive(Clone, Debug, Default)]
pub struct Meta {
    pub company: String,
    pub ticker: String,
    pub currency: String,
    pub fiscal_year_end: String,
    pub sector: String,
    /// ISO date string for the Cover "As of …" line (frozen in parity tests).
    pub as_of: String,
}

/// Everything the workbook builder needs.
#[derive(Clone, Debug)]
pub struct WorkbookInput {
    pub meta: Meta,
    pub model: ModelOutput,
    pub assumptions: AssumptionsBlock,
    pub verification: Verification,
    /// Dynamic IS structure. Empty → header-only IS (matches committed snapshots);
    /// populated → full IS body + BS/CF reference the dynamic IS row-map.
    pub is_structure: Vec<crate::is_structure::ISRow>,
    /// Optional valuation tabs. `None` keeps the 6-sheet snapshot layout.
    pub wacc: Option<fm_value::WACCOutput>,
    pub peer_source: String,
    pub dcf: Option<fm_value::DCFOutput>,
    pub public_comps: Option<fm_value::PublicCompsOutput>,
}

impl WorkbookInput {
    /// Resolve the 0-based IS row for a data key. Uses the dynamic row-map when
    /// an IS body is built, else the empty-IS fallback (so BS/CF stay correct
    /// for both variants).
    pub fn is_row(&self, key: &str) -> u32 {
        if !self.is_structure.is_empty() {
            if let Some(r) = crate::is_structure::compute_is_row_map(&self.is_structure).get(key) {
                return *r;
            }
        }
        crate::is_structure::fallback_is_row(key)
    }
}
