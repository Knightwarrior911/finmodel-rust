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

/// Build the toggle + Base/Upside/Downside scenarios + shared valuation inputs.
///
/// `risk_free_rate` and `current_share_price` are external market inputs the
/// writer does not compute (live yfinance in Python); the caller supplies them.
pub fn build_assumptions_block(
    model: &ModelOutput,
    sector: &str,
    risk_free_rate: f64,
    current_share_price: f64,
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

    let (mult_base, mult_up, mult_down) = sector_multiples(sector);

    let base = build_scenario("Base", &a, n_proj, 0.0, 0.0, 0.0, 0.025, mult_base);
    let upside = build_scenario("Upside", &a, n_proj, 0.02, 0.01, -0.01, 0.030, mult_up);
    let downside = build_scenario("Downside", &a, n_proj, -0.02, -0.01, 0.01, 0.020, mult_down);

    AssumptionsBlock {
        proj_periods,
        active_case: 1,
        base,
        upside,
        downside,
        risk_free_rate,
        equity_risk_premium: 0.055,
        target_de_ratio: 0.30,
        cost_of_debt_pretax: pick(&a, &["interest_rate_pct"], 0.035),
        current_share_price,
        shares_diluted: pick(&a, &["shares_diluted"], 0.0),
        mid_year_convention: true,
    }
}
