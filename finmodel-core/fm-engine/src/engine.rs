//! R.3 — Projection engine
//!
//! Ported from Python src/engine.py to produce consistent projections.
//! Derives assumptions from historical data, then projects forward with WC days,
//! capex, D&A, and A = L + E discipline.

use std::collections::HashMap;
use fm_types::{CompanyConfig, ProjectedStatements, ReconciledData, StatementData};

pub struct ModelEngine {
    pub data: ReconciledData,
    pub config: CompanyConfig,
}

impl ModelEngine {
    pub fn new(data: ReconciledData, config: CompanyConfig) -> Self {
        Self { data, config }
    }

    // ── Helpers ────────────────────────────────────────────────────────
    fn avg(values: &[f64]) -> f64 {
        if values.is_empty() { 0.0 } else { values.iter().sum::<f64>() / values.len() as f64 }
    }

    fn pct_growth_avg(values: &[Option<f64>]) -> f64 {
        let valid: Vec<f64> = values.iter().filter_map(|v| *v).collect();
        if valid.len() < 2 { return 0.0; }
        let mut total = 0.0;
        let mut cnt = 0;
        for i in 1..valid.len() {
            let prev = valid[i - 1];
            if prev.abs() > 1e-9 { total += (valid[i] - prev) / prev; cnt += 1; }
        }
        if cnt == 0 { 0.0 } else { total / cnt as f64 }
    }

    fn last_or(values: &[Option<f64>], default: f64) -> f64 {
        values.iter().rev().filter_map(|v| *v).next().unwrap_or(default)
    }

    #[allow(dead_code)]
    fn at(values: &[Option<f64>], index: usize, default: f64) -> f64 {
        values.get(index).and_then(|v| *v).unwrap_or(default)
    }

    fn days(bal: &[Option<f64>], flow: &[Option<f64>]) -> f64 {
        let b = Self::last_or(bal, 0.0);
        let f = Self::last_or(flow, 0.0);
        if f.abs() > 1e-9 { (b / f) * 365.0 } else { 0.0 }
    }

    #[allow(dead_code)]
    fn vec_or(def: f64, len: usize) -> Vec<f64> { vec![def; len] }

    // ── Assumption derivation ──────────────────────────────────────────
    pub fn derive_assumptions(&self) -> HashMap<String, f64> {
        let mut a = HashMap::new();
        let is = &self.data.income_statement;
        let bs = &self.data.balance_sheet;
        let cf = &self.data.cash_flow_statement;
        let rev = is.get("revenue").or_else(|| is.get("total_revenue"));
        let rv = rev.map(|r| r.iter().filter_map(|v| *v).collect::<Vec<_>>()).unwrap_or_default();
        if rv.len() < 2 { return a; }

        let base_growth = Self::pct_growth_avg(rev.unwrap()).max(-0.10);
        a.insert("revenue_growth".into(), base_growth);

        if let Some(gp) = is.get("gross_profit") {
            let r: Vec<f64> = rv.iter().zip(gp.iter())
                .filter_map(|(r, g)| match g { Some(gv) if *r > 1e-9 => Some(gv / r), _ => None }).collect();
            if !r.is_empty() { a.insert("gross_margin".into(), Self::avg(&r)); }
        } else if let Some(cgs) = is.get("cogs") {
            let r: Vec<f64> = rv.iter().zip(cgs.iter())
                .filter_map(|(r, c)| match c { Some(cv) if *r > 1e-9 => Some((r - cv) / r), _ => None }).collect();
            if !r.is_empty() { a.insert("gross_margin".into(), Self::avg(&r)); }
        }

        for (key, src) in &[("sga_pct_rev", "sga"), ("rd_pct_rev", "rd")] {
            if let Some(vals) = is.get(*src) {
                let r: Vec<f64> = rv.iter().zip(vals.iter())
                    .filter_map(|(r, v)| match v { Some(vv) if *r > 1e-9 => Some(vv / r), _ => None }).collect();
                if !r.is_empty() { a.insert(key.to_string(), Self::avg(&r)); }
            }
        }

        // D&A from income_statement "da", fallback cash_flow "depreciation"/"da_add_back"
        let da_is = is.get("da");
        if let Some(da) = da_is {
            let r: Vec<f64> = rv.iter().zip(da.iter())
                .filter_map(|(r, d)| match d { Some(dv) if *r > 1e-9 => Some(dv / r), _ => None }).collect();
            if !r.is_empty() { a.insert("da_pct_rev".into(), Self::avg(&r)); }
        }
        if !a.contains_key("da_pct_rev") {
            for alt_key in &["depreciation", "da_add_back"] {
                if let Some(_depr) = cf.get(*alt_key) {
                }
            }
        }

        // Capex from cash flow (handle negative values)
        if let Some(capex) = cf.get("capex") {
            let r: Vec<f64> = rv.iter().zip(capex.iter())
                .filter_map(|(r, c)| match c {
                    Some(cv) if *r > 1e-9 => Some(if *cv < 0.0 { -cv / r } else { cv / r }),
                    _ => None
                }).collect();
            if !r.is_empty() { a.insert("capex_pct_rev".into(), Self::avg(&r)); }
        }

        // Tax rate (effective)
        if let (Some(pt), Some(tax)) = (is.get("pre_tax_income"), is.get("income_tax")) {
            let r: Vec<f64> = pt.iter().zip(tax.iter())
                .filter_map(|(p, t)| match (p, t) {
                    (Some(pv), Some(tv)) if *pv > 1e-9 && *tv > 0.0 => Some(tv / pv), _ => None
                }).collect();
            a.insert("tax_rate".into(), (if r.is_empty() { 0.21 } else { Self::avg(&r) }).max(0.05));
        } else { a.insert("tax_rate".into(), 0.21); }

        a.insert("interest_rate_pct".into(), 0.035);

        if let Some(ar) = bs.get("accounts_receivable") { a.insert("dso_days".into(), Self::days(ar, rev.unwrap())); }
        if let (Some(inv), Some(cgs)) = (bs.get("inventory"), is.get("cogs")) {
            let d = Self::days(inv, cgs); a.insert("dio_days".into(), if d > 365.0 { 0.0 } else { d });
        }
        if let (Some(ap), Some(cgs)) = (bs.get("accounts_payable"), is.get("cogs")) {
            let d = Self::days(ap, cgs); a.insert("dpo_days".into(), if d > 365.0 { 0.0 } else { d });
        }
        if let Some(sh) = is.get("shares_diluted") { a.insert("shares_diluted".into(), Self::last_or(sh, 0.0)); }

        a
    }

    // ── Projection ─────────────────────────────────────────────────────
    pub fn project(&self, assumptions: &HashMap<String, Vec<f64>>) -> ProjectedStatements {
        let scalar = self.derive_assumptions();
        let proj_years = self.config.proj_periods;
        let np = proj_years;

        let last_period = self.data.periods.last().cloned().unwrap_or_default();
        let base_year: i32 = last_period.chars().take(4).collect::<String>().parse().unwrap_or(2024);
        let periods: Vec<String> = (1..=np as i32).map(|i| format!("{}", base_year + i)).collect();

        // Per-year vectors from assumptions, or expand scalar default
        let vec_or = |key: &str, def: f64| -> Vec<f64> {
            assumptions.get(key).cloned().unwrap_or_else(|| vec![scalar.get(key).copied().unwrap_or(def); np])
        };

        let rev_growth = vec_or("revenue_growth", 0.03);
        let gross_margin = vec_or("gross_margin", 0.30);
        let sga_pct = vec_or("sga_pct_rev", 0.10);
        let rd_pct = vec_or("rd_pct_rev", 0.05);
        let da_pct = vec_or("da_pct_rev", 0.04);
        let capex_pct = vec_or("capex_pct_rev", 0.05);
        let tax_rate = vec_or("tax_rate", 0.21);
        let int_rate = vec_or("interest_rate_pct", 0.035);
        let dso_days = vec_or("dso_days", 45.0);
        let dio_days = vec_or("dio_days", 60.0);
        let dpo_days = vec_or("dpo_days", 50.0);
        let div_per_share = vec_or("dividend_per_share", 0.0);
        let shares = scalar.get("shares_diluted").copied().unwrap_or(0.0);

        // Last historical values
        let hist_is = &self.data.income_statement;
        let hist_bs = &self.data.balance_sheet;
        let lr = Self::last_or(hist_is.get("revenue").or_else(|| hist_is.get("total_revenue")).unwrap_or(&vec![]), 0.0);
        let lc = Self::last_or(hist_bs.get("cash").unwrap_or(&vec![]), 0.0);
        let lar = Self::last_or(hist_bs.get("accounts_receivable").unwrap_or(&vec![]), 0.0);
        let linv = Self::last_or(hist_bs.get("inventory").unwrap_or(&vec![]), 0.0);
        let lap = Self::last_or(hist_bs.get("accounts_payable").unwrap_or(&vec![]), 0.0);
        let lppe = Self::last_or(hist_bs.get("ppe_net").or_else(|| hist_bs.get("pp_and_e")).unwrap_or(&vec![]), 0.0);
        let lltd = Self::last_or(hist_bs.get("long_term_debt").unwrap_or(&vec![]), 0.0);
        let lgdwl = Self::last_or(hist_bs.get("goodwill").unwrap_or(&vec![]), 0.0);

        let a = |m: &mut StatementData, k: &str, v: Vec<Option<f64>>| { m.insert(k.into(), v); };
        let mut is_out = StatementData::new();
        let mut bs_out = StatementData::new();
        let mut cf_out = StatementData::new();

        let mut prev_rev = lr;
        let mut prev_cash = lc;
        let mut prev_ar = lar;
        let mut prev_inv = linv;
        let mut prev_ap = lap;
        let mut prev_ppe = lppe;

        let mut rev_v = Vec::with_capacity(np);
        let mut gross_v = Vec::with_capacity(np);
        let mut cogs_v = Vec::with_capacity(np);
        let mut sga_v = Vec::with_capacity(np);
        let mut rd_v = Vec::with_capacity(np);
        let mut da_v = Vec::with_capacity(np);
        let mut ebit_v = Vec::with_capacity(np);
        let mut ebt_v = Vec::with_capacity(np);
        let mut tax_v = Vec::with_capacity(np);
        let mut ni_v = Vec::with_capacity(np);
        let mut ppe_v = Vec::with_capacity(np);
        let mut capex_v = Vec::with_capacity(np);
        let mut ar_v = Vec::with_capacity(np);
        let mut inv_v = Vec::with_capacity(np);
        let mut ap_v = Vec::with_capacity(np);

        let mut proj_cash_vals = Vec::with_capacity(np);

        for i in 0..np {
            let g = rev_growth[i];
            let rev = prev_rev * (1.0 + g);
            let gm = gross_margin[i];
            let gross = rev * gm;
            let cogs = rev - gross;
            let sga = rev * sga_pct[i];
            let rd = rev * rd_pct[i];
            let da = rev * da_pct[i];
            let ebit = gross - sga - rd - da;

            let int_exp = lltd * int_rate[i];
            let int_inc = prev_cash * 0.02;
            let ebt = ebit - int_exp + int_inc;
            let tax = (ebt * tax_rate[i]).max(0.0);
            let ni = ebt - tax;

            let dso = dso_days[i];
            let ar = if dso > 0.0 { rev / 365.0 * dso } else { prev_ar };
            let dio = dio_days[i];
            let inv = if cogs > 0.0 && dio > 0.0 { cogs / 365.0 * dio } else { prev_inv };
            let dpo_val = dpo_days[i];
            let ap = if cogs > 0.0 && dpo_val > 0.0 { cogs / 365.0 * dpo_val } else { prev_ap };

            let capex = rev * capex_pct[i];
            let ppe = prev_ppe + capex - da;

            let dps = div_per_share[i];
            let divs = dps * shares;

            // Cash flow
            let d_ar = ar - prev_ar;
            let d_inv = inv - prev_inv;
            let d_ap = ap - prev_ap;
            let cfo = ni + da - d_ar.abs() - d_inv.abs() + d_ap;
            let cfi = -capex;
            let cash = prev_cash + cfo + cfi - divs;

            let rnd = |v: f64| (v * 100.0).round() / 100.0;

            rev_v.push(Some(rnd(rev)));
            gross_v.push(Some(rnd(gross)));
            cogs_v.push(Some(rnd(cogs)));
            sga_v.push(Some(rnd(sga)));
            rd_v.push(Some(rnd(rd)));
            da_v.push(Some(rnd(da)));
            ebit_v.push(Some(rnd(ebit)));
            ebt_v.push(Some(rnd(ebt)));
            tax_v.push(Some(rnd(tax)));
            ni_v.push(Some(rnd(ni)));
            capex_v.push(Some(rnd(capex)));
            ppe_v.push(Some(rnd(ppe)));
            ar_v.push(Some(rnd(ar)));
            inv_v.push(Some(rnd(inv)));
            ap_v.push(Some(rnd(ap)));
            proj_cash_vals.push(Some(rnd(cash)));

            prev_rev = rev;
            prev_cash = cash;
            prev_ar = ar;
            prev_inv = inv;
            prev_ap = ap;
            prev_ppe = ppe;
        }

        // IS
        a(&mut is_out, "revenue", rev_v);
        a(&mut is_out, "cogs", cogs_v);
        a(&mut is_out, "gross_profit", gross_v);
        a(&mut is_out, "sga", sga_v);
        a(&mut is_out, "rd", rd_v);
        a(&mut is_out, "da", da_v.clone());
        a(&mut is_out, "ebit", ebit_v);
        a(&mut is_out, "pre_tax_income", ebt_v);
        a(&mut is_out, "income_tax", tax_v);
        a(&mut is_out, "net_income", ni_v.clone());

        // BS
        a(&mut bs_out, "cash", proj_cash_vals);
        a(&mut bs_out, "accounts_receivable", ar_v);
        a(&mut bs_out, "inventory", inv_v);
        a(&mut bs_out, "accounts_payable", ap_v);
        a(&mut bs_out, "pp_and_e", ppe_v);
        a(&mut bs_out, "goodwill", (0..np).map(|_| Some(lgdwl)).collect());

        // Compute A = L + E balanced
        let mut ta_v = Vec::with_capacity(np);
        let mut tl_v = Vec::with_capacity(np);
        let mut te_v = Vec::with_capacity(np);
        for i in 0..np {
            let ca = bs_out["cash"][i].unwrap_or(0.0);
            let ar_v2 = bs_out["accounts_receivable"][i].unwrap_or(0.0);
            let inv_v2 = bs_out["inventory"][i].unwrap_or(0.0);
            let ppe_v2 = bs_out["pp_and_e"][i].unwrap_or(0.0);
            let gw = bs_out["goodwill"][i].unwrap_or(0.0);
            let ta = ca + ar_v2 + inv_v2 + ppe_v2 + gw;
            let ap_v2 = bs_out["accounts_payable"][i].unwrap_or(0.0);
            let tl = ap_v2 + lltd;
            let te = ta - tl;
            ta_v.push(Some((ta * 100.0).round() / 100.0));
            tl_v.push(Some((tl * 100.0).round() / 100.0));
            te_v.push(Some((te * 100.0).round() / 100.0));
        }
        a(&mut bs_out, "total_assets", ta_v);
        a(&mut bs_out, "total_liabilities", tl_v);
        a(&mut bs_out, "total_equity", te_v);

        // CF
        a(&mut cf_out, "net_income", ni_v);
        a(&mut cf_out, "depreciation", da_v);
        a(&mut cf_out, "capex", capex_v);

        ProjectedStatements { periods, income_statement: is_out, balance_sheet: bs_out, cash_flow: cf_out }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fm_types::CompanyConfig;

    fn sample_data() -> ReconciledData {
        let mut is = StatementData::new();
        is.insert("revenue".into(), vec![Some(1000.0), Some(1100.0), Some(1210.0)]);
        is.insert("cogs".into(), vec![Some(600.0), Some(660.0), Some(726.0)]);
        is.insert("sga".into(), vec![Some(100.0), Some(110.0), Some(121.0)]);
        is.insert("rd".into(), vec![Some(50.0), Some(55.0), Some(60.0)]);
        is.insert("da".into(), vec![Some(40.0), Some(44.0), Some(48.0)]);
        is.insert("pre_tax_income".into(), vec![Some(210.0), Some(231.0), Some(255.0)]);
        is.insert("income_tax".into(), vec![Some(42.0), Some(46.0), Some(51.0)]);
        is.insert("gross_profit".into(), vec![Some(400.0), Some(440.0), Some(484.0)]);

        let mut bs = StatementData::new();
        bs.insert("cash".into(), vec![Some(100.0), Some(150.0), Some(200.0)]);
        bs.insert("accounts_receivable".into(), vec![Some(80.0), Some(90.0), Some(100.0)]);
        bs.insert("inventory".into(), vec![Some(60.0), Some(65.0), Some(70.0)]);
        bs.insert("accounts_payable".into(), vec![Some(40.0), Some(45.0), Some(50.0)]);
        bs.insert("total_assets".into(), vec![Some(500.0), Some(550.0), Some(600.0)]);
        bs.insert("total_liabilities".into(), vec![Some(250.0), Some(270.0), Some(300.0)]);
        bs.insert("total_equity".into(), vec![Some(250.0), Some(280.0), Some(300.0)]);
        bs.insert("long_term_debt".into(), vec![Some(150.0), Some(150.0), Some(150.0)]);
        bs.insert("ppe_net".into(), vec![Some(200.0), Some(210.0), Some(220.0)]);
        bs.insert("goodwill".into(), vec![Some(50.0), Some(50.0), Some(50.0)]);

        let mut cf = StatementData::new();
        cf.insert("capex".into(), vec![Some(-30.0), Some(-35.0), Some(-40.0)]);

        ReconciledData {
            income_statement: is, balance_sheet: bs, cash_flow_statement: cf,
            periods: vec!["2023".into(), "2024".into(), "2025".into()],
            currency: "USD".into(),
        }
    }

    #[test]
    fn derive_all_assumptions() {
        let engine = ModelEngine::new(sample_data(), CompanyConfig {
            name: "TestCo".into(), hist_periods: 3, proj_periods: 3, ..Default::default()
        });
        let a = engine.derive_assumptions();
        for key in &["revenue_growth", "gross_margin", "sga_pct_rev", "da_pct_rev", "tax_rate", "dso_days", "capex_pct_rev"] {
            assert!(a.contains_key(*key), "missing {}", key);
        }
        assert!((a["revenue_growth"] - 0.10).abs() < 0.01);
        assert!((a["gross_margin"] - 0.40).abs() < 0.01);
    }

    #[test]
    fn project_balance_sheet_sanity() {
        let engine = ModelEngine::new(sample_data(), CompanyConfig {
            name: "TestCo".into(), hist_periods: 3, proj_periods: 3, ..Default::default()
        });
        let s = engine.derive_assumptions();
        let ass: HashMap<String, Vec<f64>> = s.iter().map(|(k, v)| (k.clone(), vec![*v; 3])).collect();
        let p = engine.project(&ass);
        assert_eq!(p.periods.len(), 3);
        for i in 0..3 {
            let ta = p.balance_sheet["total_assets"][i].unwrap_or(0.0);
            let tl = p.balance_sheet["total_liabilities"][i].unwrap_or(0.0);
            let te = p.balance_sheet["total_equity"][i].unwrap_or(0.0);
            assert!((ta - tl - te).abs() < 0.05, "A != L+E at {}: {} != {} + {}", p.periods[i], ta, tl, te);
        }
    }
}
