//! Assumption-block derivation — port of Python `src/assumptions.py`
//! `build_assumptions_block`.
//!
//! Base drivers come from the parity-verified `fm_engine::ModelEngine::
//! derive_assumptions` (fed historical-only columns, exactly as the Python
//! snapshot generator does). Upside/Downside apply fixed scenario deltas; the
//! two genuinely-external market inputs (risk-free rate, current share price)
//! are injected by the caller.

use std::collections::HashMap;

use fm_engine::ModelEngine;
use fm_types::{CompanyConfig, ReconciledData};

use crate::input::{AssumptionsBlock, ModelOutput, ScenarioInputs};

/// Round to 6 decimals (matches Python `round(x, 6)` for non-tie values).
fn round6(x: f64) -> f64 {
    (x * 1e6).round() / 1e6
}

/// First value under any of `keys`, else `default` (mirrors Python `a.get`).
fn pick(a: &HashMap<String, f64>, keys: &[&str], default: f64) -> f64 {
    for k in keys {
        if let Some(v) = a.get(*k) {
            return *v;
        }
    }
    default
}

/// Sector-appropriate exit-EBITDA multiples: (base, upside, downside).
fn sector_multiples(sector: &str) -> (f64, f64, f64) {
    match sector {
        "utility" => (14.0, 16.0, 12.0),
        "bank" | "insurance" => (12.0, 14.0, 10.0),
        "reit" => (16.0, 18.0, 14.0),
        _ => (16.0, 20.0, 12.0),
    }
}

fn build_scenario(
    name: &str,
    a: &HashMap<String, f64>,
    n_proj: usize,
    rev_g_delta: f64,
    gm_delta: f64,
    capex_delta: f64,
    terminal_g: f64,
    exit_mult: f64,
) -> ScenarioInputs {
    let flat = |v: f64| vec![round6(v); n_proj];
    let rev_g = pick(a, &["revenue_growth", "revenue_growth_pct"], 0.05);
    let gm = pick(a, &["gross_margin", "gross_margin_pct"], 0.30);
    let capex = pick(a, &["capex_pct_rev"], 0.05);
    ScenarioInputs {
        name: name.to_string(),
        revenue_growth_pct: flat(rev_g + rev_g_delta),
        gross_margin_pct: flat(gm + gm_delta),
        sga_pct_rev: flat(pick(a, &["sga_pct_rev"], 0.10)),
        rd_pct_rev: flat(pick(a, &["rd_pct_rev"], 0.05)),
        da_pct_rev: flat(pick(a, &["da_pct_rev"], 0.04)),
        capex_pct_rev: flat(capex + capex_delta),
        tax_rate_pct: flat(pick(a, &["tax_rate", "tax_rate_pct"], 0.21)),
        interest_rate_pct: flat(pick(a, &["interest_rate_pct"], 0.035)),
        dso_days: flat(pick(a, &["dso_days"], 45.0)),
        dio_days: flat(pick(a, &["dio_days"], 60.0)),
        dpo_days: flat(pick(a, &["dpo_days"], 50.0)),
        dividend_per_share: flat(pick(a, &["dividend_per_share"], 0.0)),
        terminal_growth_rate: terminal_g,
        exit_ebitda_multiple: exit_mult,
    }
}

/// Slice the first `n_hist` columns of every line item — the historical data the
/// engine derives ratios from.
fn hist_slice(stmt: &crate::input::Statement, n_hist: usize) -> fm_types::StatementData {
    stmt.iter()
        .map(|(k, v)| (k.clone(), v.iter().take(n_hist).cloned().collect()))
        .collect()
}

/// Upside scenario deltas vs Base for the drivers that diverge across scenarios.
/// Downside applies the negation. Shared so the analyst grid overlay (fm-build)
/// mirrors exactly what the derived scenarios use — one source of truth.
pub const UPSIDE_REVENUE_GROWTH_DELTA: f64 = 0.02;
pub const UPSIDE_GROSS_MARGIN_DELTA: f64 = 0.01;
pub const UPSIDE_CAPEX_DELTA: f64 = -0.01;
/// ±0.5pp terminal-growth spread around the base case.
pub const TERMINAL_GROWTH_DELTA: f64 = 0.005;

/// Shared valuation inputs for [`build_assumptions_block`]. `Default` reproduces
/// the engine's historical hardcoded values, so a default-constructed params set
/// yields byte-identical output to the pre-flexibility code (parity gate).
#[derive(Clone, Debug)]
pub struct ValuationParams {
    pub risk_free_rate: f64,
    pub equity_risk_premium: f64,
    pub target_de_ratio: f64,
    /// `None` → engine-derived interest rate.
    pub cost_of_debt_pretax: Option<f64>,
    pub share_price: f64,
    /// Base-case terminal growth; Upside/Downside are ±0.5pp around it.
    pub terminal_growth: f64,
    /// `None` → sector default exit-multiple table.
    pub exit_multiple: Option<f64>,
}

impl Default for ValuationParams {
    fn default() -> Self {
        Self {
            risk_free_rate: 0.045,
            equity_risk_premium: 0.055,
            target_de_ratio: 0.30,
            cost_of_debt_pretax: None,
            share_price: 0.0,
            terminal_growth: 0.025,
            exit_multiple: None,
        }
    }
}

/// Build the toggle + Base/Upside/Downside scenarios + shared valuation inputs.
///
/// External market inputs (risk-free rate, share price) and analyst overrides
/// (ERP, target D/E, cost of debt, terminal growth, exit multiple) come from
/// `params`; the per-scenario drivers still derive from the parity-verified
/// engine. A [`ValuationParams::default`] reproduces the legacy output exactly.
pub fn build_assumptions_block(
    model: &ModelOutput,
    sector: &str,
    params: &ValuationParams,
) -> AssumptionsBlock {
    let n_hist = model.n_hist();
    let n_proj = model.n_proj();
    let proj_periods: Vec<String> =
        model.periods.iter().filter(|p| p.ends_with('E')).cloned().collect();

    let data = ReconciledData {
        income_statement: hist_slice(&model.income_statement, n_hist),
        balance_sheet: hist_slice(&model.balance_sheet, n_hist),
        cash_flow_statement: hist_slice(&model.cash_flow_statement, n_hist),
        periods: model.periods.iter().filter(|p| p.ends_with('A')).cloned().collect(),
        currency: String::new(),
    };
    let config = CompanyConfig {
        hist_periods: n_hist,
        proj_periods: n_proj,
        ..Default::default()
    };
    let a = ModelEngine::new(data, config).derive_assumptions();

    // Exit multiples: explicit override → base ±2 (tighter than the legacy sector
    // spread); otherwise the sector default table.
    let (mult_base, mult_up, mult_down) = match params.exit_multiple {
        Some(m) => (m, m + 2.0, m - 2.0),
        None => sector_multiples(sector),
    };
    let tg = params.terminal_growth;

    let base = build_scenario("Base", &a, n_proj, 0.0, 0.0, 0.0, tg, mult_base);
    let upside = build_scenario(
        "Upside", &a, n_proj,
        UPSIDE_REVENUE_GROWTH_DELTA, UPSIDE_GROSS_MARGIN_DELTA, UPSIDE_CAPEX_DELTA,
        tg + TERMINAL_GROWTH_DELTA, mult_up,
    );
    let downside = build_scenario(
        "Downside", &a, n_proj,
        -UPSIDE_REVENUE_GROWTH_DELTA, -UPSIDE_GROSS_MARGIN_DELTA, -UPSIDE_CAPEX_DELTA,
        tg - TERMINAL_GROWTH_DELTA, mult_down,
    );

    AssumptionsBlock {
        proj_periods,
        active_case: 1,
        base,
        upside,
        downside,
        risk_free_rate: params.risk_free_rate,
        equity_risk_premium: params.equity_risk_premium,
        target_de_ratio: params.target_de_ratio,
        cost_of_debt_pretax: params
            .cost_of_debt_pretax
            .unwrap_or_else(|| pick(&a, &["interest_rate_pct"], 0.035)),
        current_share_price: params.share_price,
        shares_diluted: pick(&a, &["shares_diluted"], 0.0),
        mid_year_convention: true,
    }
}
