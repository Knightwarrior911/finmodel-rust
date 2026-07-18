//! Valuation I/O types — mirrors `schemas.financial_data` (WACC/DCF/Peers/Comps).

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Peer {
    pub ticker: String,
    pub name: String,
    pub market_cap: f64,
    pub enterprise_value: f64,
    pub levered_beta: f64,
    pub de_ratio: f64,
    pub tax_rate: f64,
    pub rationale: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PeerSet {
    pub target_ticker: String,
    pub target_market_cap: f64,
    pub target_de_ratio: f64,
    pub peers: Vec<Peer>,
    pub excluded: Vec<(String, String)>,
    /// `"llm"` | `"fallback"`
    pub source: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct WACCOutput {
    pub peers: Vec<Peer>,
    pub median_unlevered_beta: f64,
    pub target_levered_beta: f64,
    pub target_de_ratio: f64,
    pub risk_free_rate: f64,
    pub equity_risk_premium: f64,
    pub cost_of_equity: f64,
    pub cost_of_debt_pretax: f64,
    pub tax_rate: f64,
    pub after_tax_cost_of_debt: f64,
    pub target_market_cap: f64,
    pub target_debt: f64,
    pub target_total_capital: f64,
    pub equity_weight: f64,
    pub debt_weight: f64,
    pub wacc: f64,
    /// Non-fatal diagnostics (e.g. WACC clamp bound). Empty by default, so
    /// snapshot parity is unaffected. `#[serde(default)]` keeps older JSON loadable.
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DCFOutput {
    pub ticker: String,
    pub mid_year_convention: bool,
    pub beta: f64,
    pub risk_free_rate: f64,
    pub equity_risk_premium: f64,
    pub cost_of_equity: f64,
    pub cost_of_debt_pretax: f64,
    pub tax_rate: f64,
    pub after_tax_cost_of_debt: f64,
    pub equity_weight: f64,
    pub debt_weight: f64,
    pub wacc: f64,
    pub proj_periods: Vec<String>,
    pub fcff_proj: Vec<f64>,
    pub dwc_proj: Vec<f64>,
    pub discount_factors: Vec<f64>,
    pub pv_fcfs_per_period: Vec<f64>,
    pub pv_fcfs: f64,
    pub terminal_ebitda: f64,
    pub tv_ebitda_multiple: f64,
    pub tv_ebitda: f64,
    pub tv_ebitda_pv: f64,
    pub tv_growth_rate: f64,
    pub tv_gordon: f64,
    pub tv_gordon_pv: f64,
    pub tv_method: i32,
    pub tv_selected: f64,
    pub pv_tv: f64,
    pub enterprise_value: f64,
    pub total_debt: f64,
    pub preferred_stock: f64,
    pub noncontrolling_interest: f64,
    pub cash: f64,
    pub investments: f64,
    pub net_debt: f64,
    pub equity_value: f64,
    pub shares_diluted: f64,
    pub implied_price: f64,
    pub current_share_price: f64,
    pub upside_downside_pct: f64,
    pub tv_pct_of_ev: f64,
    pub wacc_minus_g: f64,
    pub implied_exit_mult_from_gordon: f64,
    pub implied_g_from_exit_mult: f64,
    pub wacc_range: Vec<f64>,
    pub ebitda_multiple_range: Vec<f64>,
    pub gordon_growth_range: Vec<f64>,
    pub sensitivity_ebitda: Vec<Vec<f64>>,
    pub sensitivity_gordon: Vec<Vec<f64>>,
    /// Non-fatal diagnostics (e.g. Gordon TV undefined when g ≥ WACC). Empty by
    /// default; `#[serde(default)]` keeps older snapshot JSON loadable.
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PublicCompPeer {
    pub ticker: String,
    pub name: String,
    pub country: String,
    pub currency: String,
    pub tier: i32,
    pub share_price: f64,
    pub shares_diluted: f64,
    pub market_cap: f64,
    pub total_debt: f64,
    pub cash: f64,
    pub enterprise_value: f64,
    pub week52_high: f64,
    pub week52_low: f64,
    pub ltm_revenue: f64,
    pub ltm_ebitda: f64,
    pub ltm_ebit: f64,
    pub ltm_net_income: f64,
    pub ltm_eps_diluted: f64,
    pub ntm_revenue: f64,
    pub ntm_ebitda: f64,
    pub fy1_revenue: f64,
    pub fy1_ebitda: f64,
    pub fy2_revenue: f64,
    pub fy2_ebitda: f64,
    pub ntm_eps: f64,
    pub fy1_eps: f64,
    pub ev_rev_ltm: Option<f64>,
    pub ev_ebitda_ltm: Option<f64>,
    pub ev_ebit_ltm: Option<f64>,
    pub pe_ltm: Option<f64>,
    pub ev_rev_ntm: Option<f64>,
    pub ev_ebitda_ntm: Option<f64>,
    pub ev_rev_fy1: Option<f64>,
    pub ev_ebitda_fy1: Option<f64>,
    pub ev_rev_fy2: Option<f64>,
    pub ev_ebitda_fy2: Option<f64>,
    pub pe_ntm: Option<f64>,
    pub pe_fy1: Option<f64>,
    pub rationale: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CompMultipleStats {
    pub multiple_name: String,
    pub values: Vec<f64>,
    pub min: f64,
    pub p25: f64,
    pub median: f64,
    pub mean: f64,
    pub p75: f64,
    pub max: f64,
    pub count: i32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PublicCompsOutput {
    pub target_ticker: String,
    pub target_company_name: String,
    pub as_of_date: String,
    pub target_revenue: f64,
    pub target_ebitda: f64,
    pub target_ebit: f64,
    pub target_net_income: f64,
    pub target_total_debt: f64,
    pub target_cash: f64,
    pub target_shares_diluted: f64,
    pub peers: Vec<PublicCompPeer>,
    pub excluded: Vec<(String, String)>,
    pub stats: std::collections::HashMap<String, CompMultipleStats>,
    pub implied_price_low: f64,
    pub implied_price_median: f64,
    pub implied_price_high: f64,
    pub source: String,
}
