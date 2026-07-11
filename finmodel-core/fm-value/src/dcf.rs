//! DCF valuation engine — port of `src/dcf.py`.

use std::collections::HashMap;

use crate::types::{DCFOutput, WACCOutput};

/// Lightweight DCF input used by the invariants suite (and unit tests).
#[derive(Debug, Clone)]
pub struct DCFInput {
    pub fcf: Vec<f64>,
    pub terminal_growth: f64,
    pub wacc: f64,
    pub projected_periods: usize,
}

/// Scenario drivers needed by the DCF (subset of AssumptionsBlock).
#[derive(Clone, Debug)]
pub struct DCFScenario {
    pub terminal_growth_rate: f64,
    pub exit_ebitda_multiple: f64,
}

/// Market / shared inputs needed by the DCF.
#[derive(Clone, Debug)]
pub struct DCFAssumptions {
    pub mid_year_convention: bool,
    pub current_share_price: f64,
    pub shares_diluted: f64,
    pub active: DCFScenario,
}

type Stmt = HashMap<String, Vec<Option<f64>>>;

fn get_padded(section: &Stmt, key: &str, n_all: usize) -> Vec<f64> {
    let v = section.get(key).cloned().unwrap_or_default();
    let mut out: Vec<f64> = v.into_iter().map(|x| x.unwrap_or(0.0)).collect();
    out.resize(n_all, 0.0);
    out
}

fn last_or(section: &Stmt, key: &str, default: f64) -> f64 {
    section
        .get(key)
        .and_then(|v| v.last().copied().flatten())
        .unwrap_or(default)
}

fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}
fn round4(x: f64) -> f64 {
    (x * 10_000.0).round() / 10_000.0
}
fn round6(x: f64) -> f64 {
    (x * 1_000_000.0).round() / 1_000_000.0
}

/// Terminal value via Gordon Growth. NaN when g ≥ wacc.
pub fn terminal_value(last_fcf: f64, growth: f64, wacc: f64) -> f64 {
    if wacc <= growth {
        f64::NAN
    } else {
        last_fcf * (1.0 + growth) / (wacc - growth)
    }
}

/// Simple EV from FCF vector (unit-test helper; not the full writer path).
pub fn enterprise_value_simple(fcf: &[f64], terminal_growth: f64, wacc: f64) -> f64 {
    if fcf.is_empty() {
        return 0.0;
    }
    let mut pv = 0.0;
    for (i, &f) in fcf.iter().enumerate() {
        pv += f / (1.0 + wacc).powi((i + 1) as i32);
    }
    if let Some(&last) = fcf.last() {
        let tv = terminal_value(last, terminal_growth, wacc);
        if tv.is_finite() {
            pv += tv / (1.0 + wacc).powi(fcf.len() as i32);
        }
    }
    pv
}

pub fn equity_value(ev: f64, net_debt: f64, preferred: f64, minorities: f64) -> f64 {
    ev - net_debt - preferred - minorities
}

/// Full DCF matching `src/dcf.py::compute_dcf`.
pub fn compute_dcf(
    periods: &[String],
    income_statement: &Stmt,
    balance_sheet: &Stmt,
    cash_flow_statement: &Stmt,
    ticker: &str,
    wacc_output: &WACCOutput,
    assumptions: &DCFAssumptions,
    tv_method: i32,
) -> DCFOutput {
    let n_hist = periods.iter().filter(|p| p.ends_with('A')).count();
    let proj_periods: Vec<String> = periods.iter().filter(|p| p.ends_with('E')).cloned().collect();
    let n_proj = proj_periods.len();
    let n_all = periods.len();
    let mid_year = assumptions.mid_year_convention;

    let wacc = wacc_output.wacc;
    let tax_rate = wacc_output.tax_rate;
    let terminal_g = assumptions.active.terminal_growth_rate;
    let exit_mult = assumptions.active.exit_ebitda_multiple;

    let ebit_all = get_padded(income_statement, "ebit", n_all);
    let da_all = get_padded(income_statement, "da", n_all);
    let cap_all = get_padded(cash_flow_statement, "capex", n_all);
    let ar_all = get_padded(balance_sheet, "accounts_receivable", n_all);
    let inv_all = get_padded(balance_sheet, "inventory", n_all);
    let ap_all = get_padded(balance_sheet, "accounts_payable", n_all);

    let nwc = |idx: usize| ar_all[idx] + inv_all[idx] - ap_all[idx];

    let mut fcff_proj = Vec::with_capacity(n_proj);
    let mut dwc_proj = Vec::with_capacity(n_proj);
    for i in 0..n_proj {
        let t = n_hist + i;
        let nopat = ebit_all[t] * (1.0 - tax_rate);
        let da = da_all[t];
        let capex = cap_all[t];
        let dnwc = if t > 0 { nwc(t) - nwc(t - 1) } else { 0.0 };
        dwc_proj.push(round2(dnwc));
        fcff_proj.push(round2(nopat + da - capex - dnwc));
    }

    let mut discount_factors = Vec::with_capacity(n_proj);
    for i in 0..n_proj {
        let t = if mid_year {
            (i as f64 + 1.0) - 0.5
        } else {
            i as f64 + 1.0
        };
        discount_factors.push(round6(1.0 / (1.0 + wacc).powf(t)));
    }

    let pv_fcfs_per_period: Vec<f64> = fcff_proj
        .iter()
        .zip(discount_factors.iter())
        .map(|(f, df)| round2(f * df))
        .collect();
    let pv_fcfs = round2(pv_fcfs_per_period.iter().sum());

    let terminal_ebit = *ebit_all.last().unwrap_or(&0.0);
    let terminal_da = *da_all.last().unwrap_or(&0.0);
    let terminal_ebitda = terminal_ebit + terminal_da;
    let last_fcff = *fcff_proj.last().unwrap_or(&0.0);

    let tv_ebitda = terminal_ebitda * exit_mult;
    let tv_gordon = if wacc > terminal_g {
        (last_fcff * (1.0 + terminal_g)) / (wacc - terminal_g)
    } else {
        0.0
    };

    let n_terminal = if mid_year {
        n_proj as f64 - 0.5
    } else {
        n_proj as f64
    };
    let df_terminal = 1.0 / (1.0 + wacc).powf(n_terminal);
    let tv_ebitda_pv = round2(tv_ebitda * df_terminal);
    let tv_gordon_pv = round2(tv_gordon * df_terminal);

    let tv_selected = if tv_method == 1 { tv_ebitda } else { tv_gordon };
    let pv_tv = if tv_method == 1 {
        tv_ebitda_pv
    } else {
        tv_gordon_pv
    };

    let last_cash = last_or(balance_sheet, "cash", 0.0);
    let last_debt = last_or(balance_sheet, "long_term_debt", 0.0);
    let preferred = last_or(balance_sheet, "preferred_stock", 0.0);
    let nci_balance = last_or(balance_sheet, "redeemable_nci", 0.0);
    let investments = last_or(balance_sheet, "short_term_investments", 0.0);

    let enterprise_value = round2(pv_fcfs + pv_tv);
    // Match Python net_debt used in sensitivity: debt - cash + pref + nci - inv
    let net_debt = last_debt - last_cash + preferred + nci_balance - investments;
    let equity_value = round2(enterprise_value - net_debt);

    let shares_all = get_padded(income_statement, "shares_diluted", n_all);
    let mut shares = *shares_all.last().unwrap_or(&0.0);
    if shares == 0.0 {
        shares = assumptions.shares_diluted;
    }
    let implied_price = if shares != 0.0 {
        round2(equity_value / shares)
    } else {
        0.0
    };

    let current_px = assumptions.current_share_price;
    let upside_pct = if current_px != 0.0 {
        round4(implied_price / current_px - 1.0)
    } else {
        0.0
    };
    let tv_pct_ev = if enterprise_value != 0.0 {
        round4(pv_tv / enterprise_value)
    } else {
        0.0
    };
    let wacc_minus_g = round4(wacc - terminal_g);
    let implied_exit = if terminal_ebitda != 0.0 {
        round2(tv_gordon / terminal_ebitda)
    } else {
        0.0
    };
    let implied_g = if tv_ebitda > 0.0 && last_fcff != 0.0 {
        round4(wacc - (last_fcff * (1.0 + terminal_g)) / tv_ebitda)
    } else {
        0.0
    };

    let wacc_range: Vec<f64> = (0..5).map(|i| round4(wacc - 0.01 + i as f64 * 0.005)).collect();
    let ebitda_mult_range: Vec<f64> = [-4.0, -2.0, 0.0, 2.0, 4.0]
        .iter()
        .map(|d| exit_mult + d)
        .collect();
    let gordon_growth_range: Vec<f64> = (0..5)
        .map(|i| round4(terminal_g - 0.010 + i as f64 * 0.005))
        .collect();

    let implied_price_at = |w: f64, tv_val: f64| -> f64 {
        let n_t = if mid_year {
            n_proj as f64 - 0.5
        } else {
            n_proj as f64
        };
        let pv_f: f64 = fcff_proj
            .iter()
            .enumerate()
            .map(|(i, f)| {
                let t = if mid_year {
                    (i as f64 + 1.0) - 0.5
                } else {
                    i as f64 + 1.0
                };
                f / (1.0 + w).powf(t)
            })
            .sum();
        let pv_t = tv_val / (1.0 + w).powf(n_t);
        let eq = pv_f + pv_t - net_debt;
        if shares != 0.0 {
            round2(eq / shares)
        } else {
            0.0
        }
    };

    let sensitivity_ebitda: Vec<Vec<f64>> = wacc_range
        .iter()
        .map(|&w| {
            ebitda_mult_range
                .iter()
                .map(|&m| implied_price_at(w, terminal_ebitda * m))
                .collect()
        })
        .collect();
    let sensitivity_gordon: Vec<Vec<f64>> = wacc_range
        .iter()
        .map(|&w| {
            gordon_growth_range
                .iter()
                .map(|&g| {
                    let tv = if w > g {
                        (last_fcff * (1.0 + g)) / (w - g)
                    } else {
                        0.0
                    };
                    implied_price_at(w, tv)
                })
                .collect()
        })
        .collect();

    DCFOutput {
        ticker: ticker.to_string(),
        mid_year_convention: mid_year,
        beta: wacc_output.target_levered_beta,
        risk_free_rate: wacc_output.risk_free_rate,
        equity_risk_premium: wacc_output.equity_risk_premium,
        cost_of_equity: wacc_output.cost_of_equity,
        cost_of_debt_pretax: wacc_output.cost_of_debt_pretax,
        tax_rate,
        after_tax_cost_of_debt: wacc_output.after_tax_cost_of_debt,
        equity_weight: wacc_output.equity_weight,
        debt_weight: wacc_output.debt_weight,
        wacc,
        proj_periods,
        fcff_proj,
        dwc_proj,
        discount_factors,
        pv_fcfs_per_period,
        pv_fcfs,
        terminal_ebitda,
        tv_ebitda_multiple: exit_mult,
        tv_ebitda,
        tv_ebitda_pv,
        tv_growth_rate: terminal_g,
        tv_gordon,
        tv_gordon_pv,
        tv_method,
        tv_selected,
        pv_tv,
        enterprise_value,
        total_debt: last_debt,
        preferred_stock: preferred,
        noncontrolling_interest: nci_balance,
        cash: last_cash,
        investments,
        net_debt,
        equity_value,
        shares_diluted: shares,
        implied_price,
        current_share_price: current_px,
        upside_downside_pct: upside_pct,
        tv_pct_of_ev: tv_pct_ev,
        wacc_minus_g,
        implied_exit_mult_from_gordon: implied_exit,
        implied_g_from_exit_mult: implied_g,
        wacc_range,
        ebitda_multiple_range: ebitda_mult_range,
        gordon_growth_range,
        sensitivity_ebitda,
        sensitivity_gordon,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::WACCOutput;
    use crate::wacc::{compute_wacc, fallback_peer_set};

    fn pad(v: &[f64]) -> Vec<Option<f64>> {
        v.iter().copied().map(Some).collect()
    }

    #[test]
    fn gordon_tv_known() {
        let tv = terminal_value(100.0, 0.02, 0.10);
        assert!((tv - 100.0 * 1.02 / 0.08).abs() < 1e-9);
    }

    #[test]
    fn compute_dcf_smoke() {
        let periods = vec![
            "2023A".into(),
            "2024A".into(),
            "2025E".into(),
            "2026E".into(),
        ];
        let mut is = Stmt::new();
        is.insert("ebit".into(), pad(&[100.0, 110.0, 120.0, 130.0]));
        is.insert("da".into(), pad(&[20.0, 22.0, 24.0, 26.0]));
        is.insert("shares_diluted".into(), pad(&[50.0, 50.0, 50.0, 50.0]));
        let mut bs = Stmt::new();
        bs.insert("accounts_receivable".into(), pad(&[30.0, 32.0, 34.0, 36.0]));
        bs.insert("inventory".into(), pad(&[40.0, 42.0, 44.0, 46.0]));
        bs.insert("accounts_payable".into(), pad(&[20.0, 21.0, 22.0, 23.0]));
        bs.insert("cash".into(), pad(&[80.0, 90.0, 100.0, 110.0]));
        bs.insert("long_term_debt".into(), pad(&[200.0, 200.0, 200.0, 200.0]));
        let mut cf = Stmt::new();
        cf.insert("capex".into(), pad(&[25.0, 26.0, 27.0, 28.0]));

        let ps = fallback_peer_set("TEST", 2500.0, 0.3);
        let w = compute_wacc(&ps, 2500.0, 200.0, 0.04, 0.05, 0.04, 0.21, Some(0.3), 1.0);
        let a = DCFAssumptions {
            mid_year_convention: true,
            current_share_price: 40.0,
            shares_diluted: 50.0,
            active: DCFScenario {
                terminal_growth_rate: 0.02,
                exit_ebitda_multiple: 10.0,
            },
        };
        let dcf = compute_dcf(&periods, &is, &bs, &cf, "TEST", &w, &a, 1);
        assert_eq!(dcf.proj_periods.len(), 2);
        assert!(dcf.enterprise_value > 0.0);
        assert!(dcf.implied_price > 0.0);
        assert_eq!(dcf.wacc_range.len(), 5);
        assert_eq!(dcf.sensitivity_ebitda.len(), 5);
        assert_eq!(dcf.sensitivity_ebitda[0].len(), 5);
        let _ = WACCOutput::default();
    }
}
