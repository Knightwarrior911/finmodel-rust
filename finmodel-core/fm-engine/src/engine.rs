use std::collections::HashMap;

use fm_types::{CompanyConfig, ProjectedStatements, ReconciledData, StatementData};

// ---------------------------------------------------------------------------
// R.3 — Projection engine
// ---------------------------------------------------------------------------

/// Core projection engine that derives assumptions and projects forward.
pub struct ModelEngine {
    pub data: ReconciledData,
    pub config: CompanyConfig,
}

impl ModelEngine {
    pub fn new(data: ReconciledData, config: CompanyConfig) -> Self {
        Self { data, config }
    }

    /// Derive growth / margin assumptions from historical averages.
    ///
    /// Returns a map of assumption names → single scalar value.
    /// Typical keys:
    /// - `revenue_growth` — CAGR of revenue over historical periods
    /// - `gross_margin` — average gross margin
    /// - `opex_sales_ratio` — avg operating expense as % of revenue
    /// - `depreciation_pct` — avg D&A as % of revenue
    /// - `tax_rate` — average effective tax rate
    /// - `capex_revenue_pct` — avg capex as % of revenue
    pub fn derive_assumptions(&self) -> HashMap<String, f64> {
        let mut assumptions = HashMap::new();

        let is = &self.data.income_statement;
        let cf = &self.data.cash_flow_statement;
        let n = self.data.num_periods();

        if n < 2 {
            return assumptions;
        }

        // --- Revenue growth (CAGR over available periods) ---
        if let Some(revenues) = is.get("revenue").or_else(|| is.get("total_revenue")) {
            let valid: Vec<f64> = revenues.iter().filter_map(|v| *v).collect();
            if valid.len() >= 2 {
                let first = valid[0];
                let last = *valid.last().unwrap();
                if first.abs() > 1e-9 {
                    let cagr = (last / first).powf(1.0 / (valid.len() as f64 - 1.0)) - 1.0;
                    if let Some(cap) = self.config.growth_cap {
                        assumptions.insert("revenue_growth".into(), cagr.clamp(-cap, cap));
                    } else {
                        assumptions.insert("revenue_growth".into(), cagr);
                    }
                }
            }
        }

        // --- Gross margin ---
        if let (Some(rev), Some(cogs)) = (
            is.get("revenue").or_else(|| is.get("total_revenue")),
            is.get("cogs").or_else(|| is.get("cost_of_revenue")),
        ) {
            let margins: Vec<f64> = rev
                .iter()
                .zip(cogs.iter())
                .filter_map(|(r, c)| match (r, c) {
                    (Some(rv), Some(cv)) if *rv > 1e-9 => Some((rv - cv) / rv),
                    _ => None,
                })
                .collect();
            if !margins.is_empty() {
                let avg = margins.iter().sum::<f64>() / margins.len() as f64;
                assumptions.insert("gross_margin".into(), avg);
            }
        }

        // --- Tax rate ---
        if let (Some(pt), Some(tax)) = (is.get("pre_tax_income"), is.get("income_tax")) {
            let rates: Vec<f64> = pt
                .iter()
                .zip(tax.iter())
                .filter_map(|(p, t)| match (p, t) {
                    (Some(pv), Some(tv)) if *pv > 1e-9 => Some(tv / pv),
                    _ => None,
                })
                .collect();
            if !rates.is_empty() {
                let avg = rates.iter().sum::<f64>() / rates.len() as f64;
                assumptions.insert("tax_rate".into(), avg);
            }
        }

        // --- Depreciation as % of revenue ---
        if let Some(da) = cf
            .get("depreciation")
            .or_else(|| cf.get("depreciation_and_amortisation"))
        {
            if let Some(rev) = is.get("revenue").or_else(|| is.get("total_revenue")) {
                let pcts: Vec<f64> = rev
                    .iter()
                    .zip(da.iter())
                    .filter_map(|(r, d)| match (r, d) {
                        (Some(rv), Some(dv)) if *rv > 1e-9 => Some(dv / rv),
                        _ => None,
                    })
                    .collect();
                if !pcts.is_empty() {
                    let avg = pcts.iter().sum::<f64>() / pcts.len() as f64;
                    assumptions.insert("depreciation_pct".into(), avg);
                }
            }
        }

        assumptions
    }

    /// Project forward for a given number of periods using provided assumptions.
    ///
    /// `assumptions` should contain per-year vectors (one element per projection year).
    /// Keys: `revenue_growth`, `gross_margin`, `tax_rate`, `depreciation_pct`, `capex_revenue_pct`.
    /// If a key is missing, `derive_assumptions().get(…)` is used as fallback.
    pub fn project(&self, assumptions: &HashMap<String, Vec<f64>>) -> ProjectedStatements {
        let scalar_assumptions = self.derive_assumptions();
        let proj_years = self.config.proj_periods;

        // Build period labels
        let last_hist = self.data.periods.last().cloned().unwrap_or_default();
        let base_year: i32 = last_hist.parse().unwrap_or(2024);
        let periods: Vec<String> = (1..=proj_years)
            .map(|i| format!("{}", base_year + i as i32))
            .collect();

        // Get last-historical revenue as base
        let last_revenue = self
            .data
            .income_statement
            .get("revenue")
            .or_else(|| self.data.income_statement.get("total_revenue"))
            .and_then(|v| v.iter().filter_map(|x| *x).last())
            .unwrap_or(0.0);

        // Pre-extract assumption vectors (or scalar-expand)
        let rev_growth: Vec<f64> = assumptions
            .get("revenue_growth")
            .cloned()
            .unwrap_or_else(|| {
                let s = scalar_assumptions
                    .get("revenue_growth")
                    .copied()
                    .unwrap_or(0.0);
                vec![s; proj_years]
            });

        let gross_margin: Vec<f64> = assumptions
            .get("gross_margin")
            .cloned()
            .unwrap_or_else(|| {
                let s = scalar_assumptions
                    .get("gross_margin")
                    .copied()
                    .unwrap_or(0.0);
                vec![s; proj_years]
            });

        let tax_rate: Vec<f64> = assumptions.get("tax_rate").cloned().unwrap_or_else(|| {
            let s = scalar_assumptions
                .get("tax_rate")
                .copied()
                .unwrap_or(0.0);
            vec![s; proj_years]
        });

        let depr_pct: Vec<f64> =
            assumptions
                .get("depreciation_pct")
                .cloned()
                .unwrap_or_else(|| {
                    let s = scalar_assumptions
                        .get("depreciation_pct")
                        .copied()
                        .unwrap_or(0.0);
                    vec![s; proj_years]
                });

        // --- Project Income Statement ---
        let mut projected_revenue = Vec::with_capacity(proj_years);
        let mut projected_cogs = Vec::with_capacity(proj_years);
        let mut projected_gross_profit = Vec::with_capacity(proj_years);
        let mut projected_da = Vec::with_capacity(proj_years);
        let mut projected_ebit = Vec::with_capacity(proj_years);
        let mut projected_tax = Vec::with_capacity(proj_years);
        let mut projected_net_income = Vec::with_capacity(proj_years);

        let mut rev = last_revenue;
        for i in 0..proj_years {
            let growth = *rev_growth.get(i).unwrap_or(&0.0);
            rev *= 1.0 + growth;

            let gm = *gross_margin.get(i).unwrap_or(&0.0);
            let cogs = rev * (1.0 - gm);
            let gp = rev - cogs;

            let depr = rev * depr_pct.get(i).unwrap_or(&0.0);
            // Simplified: sga = 10% of revenue placeholder
            let sga = rev * 0.10;
            let ebit = gp - depr - sga;

            let tr = *tax_rate.get(i).unwrap_or(&0.0);
            let tax = ebit.max(0.0) * tr;
            let ni = ebit - tax;

            projected_revenue.push(Some(rev));
            projected_cogs.push(Some(cogs));
            projected_gross_profit.push(Some(gp));
            projected_da.push(Some(depr));
            projected_ebit.push(Some(ebit));
            projected_tax.push(Some(tax));
            projected_net_income.push(Some(ni));
        }

        // --- Project Balance Sheet (simplified) ---
        let mut bs: StatementData = HashMap::new();
        // Carry forward last historical balance sheet items if available
        let last_cash = self
            .data
            .balance_sheet
            .get("cash")
            .and_then(|v| v.iter().filter_map(|x| *x).last())
            .unwrap_or(0.0);
        let last_assets = self
            .data
            .balance_sheet
            .get("total_assets")
            .and_then(|v| v.iter().filter_map(|x| *x).last())
            .unwrap_or(0.0);
        let last_equity = self
            .data
            .balance_sheet
            .get("total_equity")
            .and_then(|v| v.iter().filter_map(|x| *x).last())
            .unwrap_or(0.0);

        let pp_and_e = last_assets - last_cash; // rough fixed assets

        let mut cash_vec = vec![0.0_f64; proj_years];
        let mut ppe_vec = vec![0.0_f64; proj_years];
        let mut ta_vec = vec![0.0_f64; proj_years];
        let mut tl_vec = vec![0.0_f64; proj_years];
        let mut te_vec = vec![0.0_f64; proj_years];

        let mut cash_val = last_cash;
        let mut ppe_val = pp_and_e;
        for i in 0..proj_years {
            let ni = projected_net_income[i].unwrap_or(0.0);
            let depr = projected_da[i].unwrap_or(0.0);
            cash_val += ni + depr;
            ppe_val = (ppe_val - depr).max(0.0);
            let ta = cash_val + ppe_val;
            let equity = last_equity + ni;
            let tl_val = ta - equity;

            cash_vec[i] = cash_val;
            ppe_vec[i] = ppe_val;
            ta_vec[i] = ta;
            tl_vec[i] = tl_val;
            te_vec[i] = equity;
        }

        bs.insert("cash".into(), cash_vec.into_iter().map(Some).collect());
        bs.insert("pp_and_e".into(), ppe_vec.into_iter().map(Some).collect());
        bs.insert("total_assets".into(), ta_vec.into_iter().map(Some).collect());
        bs.insert("total_liabilities".into(), tl_vec.into_iter().map(Some).collect());
        bs.insert("total_equity".into(), te_vec.into_iter().map(Some).collect());

        // --- Project Cash Flow ---
        let mut cf: StatementData = HashMap::new();
        let mut ocf_vec = Vec::with_capacity(proj_years);
        let mut capex_vec = Vec::with_capacity(proj_years);
        let mut fcf_vec = Vec::with_capacity(proj_years);

        for i in 0..proj_years {
            let ni = projected_net_income[i].unwrap_or(0.0);
            let depr = projected_da[i].unwrap_or(0.0);
            let ocf = ni + depr;
            let capex = depr;
            let fcf = ocf - capex;

            ocf_vec.push(Some(ocf));
            capex_vec.push(Some(capex));
            fcf_vec.push(Some(fcf));
        }

        cf.insert("net_income".into(), projected_net_income);
        cf.insert("depreciation".into(), projected_da);
        cf.insert("operating_cash_flow".into(), ocf_vec);
        cf.insert("capex".into(), capex_vec);
        cf.insert("free_cash_flow".into(), fcf_vec);

        // --- Build IS HashMap (after BS/CF consumed the projected values) ---
        let mut is: StatementData = HashMap::new();
        is.insert("revenue".into(), projected_revenue);
        is.insert("cogs".into(), projected_cogs);
        is.insert("gross_profit".into(), projected_gross_profit);
        is.insert("ebit".into(), projected_ebit);
        is.insert("income_tax".into(), projected_tax);

        ProjectedStatements {
            periods,
            income_statement: is,
            balance_sheet: bs,
            cash_flow: cf,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fm_types::{CompanyConfig, ReconciledData};

    fn sample_data() -> ReconciledData {
        let mut is = StatementData::new();
        is.insert(
            "revenue".into(),
            vec![Some(1000.0), Some(1100.0), Some(1210.0)],
        );
        is.insert("cogs".into(), vec![Some(600.0), Some(660.0), Some(726.0)]);
        is.insert(
            "pre_tax_income".into(),
            vec![Some(200.0), Some(220.0), Some(250.0)],
        );
        is.insert(
            "income_tax".into(),
            vec![Some(40.0), Some(44.0), Some(50.0)],
        );

        let mut bs = StatementData::new();
        bs.insert("cash".into(), vec![Some(100.0), Some(150.0), Some(200.0)]);
        bs.insert(
            "total_assets".into(),
            vec![Some(500.0), Some(550.0), Some(600.0)],
        );
        bs.insert(
            "total_liabilities".into(),
            vec![Some(250.0), Some(270.0), Some(300.0)],
        );
        bs.insert(
            "total_equity".into(),
            vec![Some(250.0), Some(280.0), Some(300.0)],
        );

        let mut cf = StatementData::new();
        cf.insert(
            "depreciation".into(),
            vec![Some(50.0), Some(55.0), Some(60.0)],
        );
        cf.insert(
            "net_income".into(),
            vec![Some(160.0), Some(176.0), Some(200.0)],
        );

        ReconciledData {
            income_statement: is,
            balance_sheet: bs,
            cash_flow_statement: cf,
            periods: vec!["2023".into(), "2024".into(), "2025".into()],
            currency: "USD".into(),
        }
    }

    #[test]
    fn derive_assumptions_returns_non_empty() {
        let data = sample_data();
        let config = CompanyConfig {
            name: "TestCo".into(),
            hist_periods: 3,
            proj_periods: 3,
            ..Default::default()
        };
        let engine = ModelEngine::new(data, config);
        let assumptions = engine.derive_assumptions();
        assert!(!assumptions.is_empty(), "assumptions should not be empty");
        assert!(assumptions.contains_key("revenue_growth"));
        assert!(assumptions.contains_key("gross_margin"));
        assert!(assumptions.contains_key("tax_rate"));
    }

    #[test]
    fn project_returns_correct_number_of_periods() {
        let data = sample_data();
        let config = CompanyConfig {
            name: "TestCo".into(),
            hist_periods: 3,
            proj_periods: 3,
            ..Default::default()
        };
        let engine = ModelEngine::new(data, config);
        let scalar = engine.derive_assumptions();
        let mut assumptions = HashMap::new();
        // Expand scalars to vectors
        assumptions.insert(
            "revenue_growth".into(),
            vec![scalar.get("revenue_growth").copied().unwrap_or(0.0); 3],
        );
        assumptions.insert(
            "gross_margin".into(),
            vec![scalar.get("gross_margin").copied().unwrap_or(0.0); 3],
        );
        let projected = engine.project(&assumptions);
        assert_eq!(projected.periods.len(), 3);
        assert_eq!(projected.periods, vec!["2026", "2027", "2028"]);
        assert!(projected.income_statement.contains_key("revenue"));
        assert!(projected.balance_sheet.contains_key("total_assets"));
        assert!(projected.cash_flow.contains_key("free_cash_flow"));
    }
}
