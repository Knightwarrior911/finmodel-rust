//! Enterprise-Value bridge — port of `kb/ev_bridge.py` (BIWS rules R-001..R-018).
//!
//! EV = Equity Value + non-equity investor claims (debt, leases, underfunded
//! pension, minority interest, preferred) − non-operating assets (cash, ST
//! investments, equity/financial investments, held-for-sale, discontinued, NOL).
//! Goodwill and ordinary intangibles are **never** subtracted (R-014). The bridge
//! is a checklist: only present, material, disclosed items are included.

/// One line in the bridge, with a provenance label.
#[derive(Clone, Debug, PartialEq)]
pub struct EvLineItem {
    pub item: String,
    pub amount: f64,
    pub source: String,
}

/// Result of an EV-bridge build.
#[derive(Clone, Debug, Default)]
pub struct EvBridge {
    pub market_cap: Option<f64>,
    pub additions: Vec<EvLineItem>,
    pub subtractions: Vec<EvLineItem>,
    pub total_ev: f64,
}

/// EV-bridge inputs (checklist — only non-zero items are applied).
#[derive(Clone, Debug, Default)]
pub struct EvBridgeInput {
    pub company: String,
    pub period: String,
    pub currency: String,

    pub share_price: Option<f64>,
    pub shares_outstanding: Option<f64>,
    pub market_cap: Option<f64>,

    // ADD (other investor groups / debt-like)
    pub total_debt: Option<f64>,
    pub finance_leases: Option<f64>,
    pub operating_leases: Option<f64>,
    pub underfunded_pension: Option<f64>,
    pub minority_interest: Option<f64>,
    pub preferred_stock: Option<f64>,

    // SUBTRACT (non-operating assets)
    pub cash: Option<f64>,
    pub short_term_investments: Option<f64>,
    pub equity_investments: Option<f64>,
    pub financial_investments: Option<f64>,
    pub assets_held_for_sale: Option<f64>,
    pub discontinued_ops_assets: Option<f64>,
    pub nol_dta: Option<f64>,

    // Never subtracted (R-014) — carried for reporting only.
    pub goodwill: Option<f64>,

    // For multiples.
    pub ltm_revenue: Option<f64>,
    pub ltm_ebitda: Option<f64>,
    pub ltm_ebit: Option<f64>,
}

impl EvBridgeInput {
    /// Market cap: explicit value, else price × shares (F-002), else None.
    pub fn computed_market_cap(&self) -> Option<f64> {
        if let Some(mc) = self.market_cap {
            if mc != 0.0 {
                return Some(mc);
            }
        }
        match (self.share_price, self.shares_outstanding) {
            (Some(p), Some(s)) if p != 0.0 && s != 0.0 => Some(p * s),
            _ => None,
        }
    }
}

fn present(v: Option<f64>) -> Option<f64> {
    v.filter(|&x| x != 0.0)
}

/// Build the EV bridge, including only present/material items.
pub fn build_ev_bridge(inp: &EvBridgeInput) -> EvBridge {
    let mc = inp.computed_market_cap();
    let mut ev = mc.unwrap_or(0.0);
    let mut additions = Vec::new();
    let mut subtractions = Vec::new();

    // (value, label, source) — order mirrors the Python checklist.
    let add_items: [(Option<f64>, &str, &str); 6] = [
        (inp.total_debt, "Total Debt", "Balance sheet / debt note"),
        (inp.finance_leases, "Finance Leases", "ASC 842 / IFRS 16 note"),
        (inp.operating_leases, "Operating Leases", "ASC 842 / IFRS 16 note (R-016)"),
        (inp.underfunded_pension, "Underfunded Pension", "Pension footnote (R-015)"),
        (inp.minority_interest, "Minority Interest", "Balance sheet"),
        (inp.preferred_stock, "Preferred Stock", "Balance sheet"),
    ];
    for (v, label, source) in add_items {
        if let Some(amount) = present(v) {
            ev += amount;
            additions.push(EvLineItem { item: label.into(), amount, source: source.into() });
        }
    }

    let sub_items: [(Option<f64>, &str, &str); 7] = [
        (inp.cash, "Cash & Equivalents", "Balance sheet"),
        (inp.short_term_investments, "Short-Term Investments", "Balance sheet"),
        (inp.equity_investments, "Equity-Method Investments", "Balance sheet (R-014)"),
        (inp.financial_investments, "Financial Investments", "Balance sheet"),
        (inp.assets_held_for_sale, "Assets Held for Sale", "Balance sheet"),
        (inp.discontinued_ops_assets, "Discontinued Ops Assets", "Balance sheet"),
        (inp.nol_dta, "NOL / DTA", "Tax footnote"),
    ];
    for (v, label, source) in sub_items {
        if let Some(amount) = present(v) {
            ev -= amount;
            subtractions.push(EvLineItem { item: label.into(), amount, source: source.into() });
        }
    }

    EvBridge { market_cap: mc, additions, subtractions, total_ev: ev }
}

/// Underfunded pension for the EV bridge (R-015 + F-018/F-019). Returns 0 when
/// overfunded or inputs missing; optionally tax-adjusts.
pub fn compute_unfunded_pension(
    pbo: Option<f64>,
    plan_assets: Option<f64>,
    tax_rate: f64,
    tax_adjusted: bool,
) -> f64 {
    match (pbo, plan_assets) {
        (Some(pbo), Some(pa)) => {
            let net = (pbo - pa).max(0.0);
            if tax_adjusted {
                net * (1.0 - tax_rate)
            } else {
                net
            }
        }
        _ => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const EPS: f64 = 1e-9;
    fn approx(a: f64, b: f64) {
        assert!((a - b).abs() < EPS, "expected {b}, got {a}");
    }

    // Oracle values from kb/ev_bridge.py (Python).
    #[test]
    fn ev_from_price_and_shares_ignores_goodwill() {
        let inp = EvBridgeInput {
            share_price: Some(100.0),
            shares_outstanding: Some(500.0),
            total_debt: Some(8000.0),
            operating_leases: Some(1200.0),
            underfunded_pension: Some(300.0),
            minority_interest: Some(150.0),
            cash: Some(2000.0),
            short_term_investments: Some(500.0),
            nol_dta: Some(100.0),
            goodwill: Some(9999.0), // must NOT affect EV
            ..Default::default()
        };
        let b = build_ev_bridge(&inp);
        approx(b.market_cap.unwrap(), 50000.0);
        approx(b.total_ev, 57050.0);
        assert_eq!(b.additions.len(), 4);
        assert_eq!(b.subtractions.len(), 3);
    }

    #[test]
    fn ev_from_explicit_market_cap() {
        let inp = EvBridgeInput {
            market_cap: Some(45000.0),
            total_debt: Some(12000.0),
            finance_leases: Some(800.0),
            preferred_stock: Some(600.0),
            cash: Some(3000.0),
            equity_investments: Some(1500.0),
            ..Default::default()
        };
        let b = build_ev_bridge(&inp);
        approx(b.total_ev, 53900.0);
    }

    #[test]
    fn unfunded_pension_cases() {
        approx(compute_unfunded_pension(Some(5000.0), Some(4200.0), 0.0, false), 800.0);
        approx(compute_unfunded_pension(Some(3000.0), Some(3500.0), 0.0, false), 0.0);
        approx(compute_unfunded_pension(Some(5000.0), Some(4200.0), 0.25, true), 600.0);
        approx(compute_unfunded_pension(None, Some(1.0), 0.0, false), 0.0);
    }
}
