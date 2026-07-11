//! IFRS 16 ↔ US GAAP lease-accounting conversion.
//!
//! Faithful port of `kb/ifrs.py` (EBIT/EBITDA/EBITA conversion) plus
//! `src/research/us_gaap_leases.ASC842LeaseData.compute_ifrs_adjustments`
//! (estimating ROU depreciation + lease interest from a 10-K ASC 842 note).
//!
//! Only **ROU depreciation** and **lease interest** are adjustment items.
//! Short-term rent is already OPEX in both frameworks and is never adjusted.

use serde::{Deserialize, Serialize};

/// Conversion direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AdjustmentDirection {
    /// Strip IFRS 16 lease capitalization (IFRS → US GAAP / "pre-IFRS").
    IfrsToUsGaap,
    /// Add IFRS 16 lease capitalization (US GAAP → IFRS / "post-IFRS").
    UsGaapToIfrs,
}

impl AdjustmentDirection {
    pub fn as_str(self) -> &'static str {
        match self {
            AdjustmentDirection::IfrsToUsGaap => "ifrs_to_us_gaap",
            AdjustmentDirection::UsGaapToIfrs => "us_gaap_to_ifrs",
        }
    }
}

/// Inputs extracted from the lease note (Phase 3 of the IFRS workflow).
#[derive(Clone, Debug, Default)]
pub struct IfrsAdjustmentInput {
    pub rou_depreciation: f64,
    pub lease_interest: f64,
    /// Short-term lease exemption — already OPEX in both frameworks; excluded.
    pub short_term_rent: f64,
    pub reported_ebit: f64,
    pub reported_ebitda: f64,
    pub reported_ebita: f64,
    pub standard_depreciation: f64,
    pub standard_amortization: f64,
    /// `"IFRS"` or `"US GAAP"` (case-insensitive; anything not IFRS = US GAAP).
    pub accounting_standard: String,
    pub weighted_discount_rate: Option<f64>,
    pub weighted_lease_term: Option<f64>,
}

impl IfrsAdjustmentInput {
    /// Total cash rental = ROU depreciation + lease interest.
    pub fn total_cash_rental_expense(&self) -> f64 {
        self.rou_depreciation + self.lease_interest
    }
}

/// Adjusted EBIT/EBITDA/EBITA + margins, deltas, and provenance.
#[derive(Clone, Debug)]
pub struct IfrsAdjustmentOutput {
    pub direction: AdjustmentDirection,
    pub adjusted_ebit: f64,
    pub adjusted_ebitda: f64,
    pub adjusted_ebita: f64,
    pub reported_ebit_margin: f64,
    pub adjusted_ebit_margin: f64,
    pub reported_ebitda_margin: f64,
    pub adjusted_ebitda_margin: f64,
    pub reported_ebita_margin: f64,
    pub adjusted_ebita_margin: f64,
    pub ebit_delta: f64,
    pub ebitda_delta: f64,
    pub ebita_delta: f64,
    pub adjustment_items_used: Vec<String>,
    pub items_excluded: Vec<String>,
}

fn items_used() -> Vec<String> {
    vec!["ROU Depreciation".into(), "Lease Interest".into()]
}
fn items_excluded() -> Vec<String> {
    vec!["Short-term rent (already OPEX in both frameworks)".into()]
}

fn with_margins(mut out: IfrsAdjustmentOutput, inp: &IfrsAdjustmentInput, revenue: f64) -> IfrsAdjustmentOutput {
    if revenue > 0.0 {
        out.reported_ebit_margin = inp.reported_ebit / revenue * 100.0;
        out.adjusted_ebit_margin = out.adjusted_ebit / revenue * 100.0;
        out.reported_ebitda_margin = inp.reported_ebitda / revenue * 100.0;
        out.adjusted_ebitda_margin = out.adjusted_ebitda / revenue * 100.0;
        out.reported_ebita_margin = inp.reported_ebita / revenue * 100.0;
        out.adjusted_ebita_margin = out.adjusted_ebita / revenue * 100.0;
    }
    out
}

/// IFRS 16 → US GAAP: strip lease capitalization.
///   EBIT   −= lease_interest
///   EBITDA −= lease_interest + rou_depreciation
///   EBITA  −= lease_interest
pub fn convert_ifrs_to_us_gaap(inp: &IfrsAdjustmentInput, revenue: f64) -> IfrsAdjustmentOutput {
    let adj_ebit = inp.reported_ebit - inp.lease_interest;
    let adj_ebitda = inp.reported_ebitda - inp.lease_interest - inp.rou_depreciation;
    let adj_ebita = inp.reported_ebita - inp.lease_interest;
    let out = IfrsAdjustmentOutput {
        direction: AdjustmentDirection::IfrsToUsGaap,
        adjusted_ebit: adj_ebit,
        adjusted_ebitda: adj_ebitda,
        adjusted_ebita: adj_ebita,
        reported_ebit_margin: 0.0,
        adjusted_ebit_margin: 0.0,
        reported_ebitda_margin: 0.0,
        adjusted_ebitda_margin: 0.0,
        reported_ebita_margin: 0.0,
        adjusted_ebita_margin: 0.0,
        ebit_delta: adj_ebit - inp.reported_ebit,
        ebitda_delta: adj_ebitda - inp.reported_ebitda,
        ebita_delta: adj_ebita - inp.reported_ebita,
        adjustment_items_used: items_used(),
        items_excluded: items_excluded(),
    };
    with_margins(out, inp, revenue)
}

/// US GAAP → IFRS 16: add lease capitalization.
///   EBIT   += lease_interest
///   EBITDA += lease_interest + rou_depreciation
///   EBITA  += lease_interest
pub fn convert_us_gaap_to_ifrs(inp: &IfrsAdjustmentInput, revenue: f64) -> IfrsAdjustmentOutput {
    let adj_ebit = inp.reported_ebit + inp.lease_interest;
    let adj_ebitda = inp.reported_ebitda + inp.lease_interest + inp.rou_depreciation;
    let adj_ebita = inp.reported_ebita + inp.lease_interest;
    let out = IfrsAdjustmentOutput {
        direction: AdjustmentDirection::UsGaapToIfrs,
        adjusted_ebit: adj_ebit,
        adjusted_ebitda: adj_ebitda,
        adjusted_ebita: adj_ebita,
        reported_ebit_margin: 0.0,
        adjusted_ebit_margin: 0.0,
        reported_ebitda_margin: 0.0,
        adjusted_ebitda_margin: 0.0,
        reported_ebita_margin: 0.0,
        adjusted_ebita_margin: 0.0,
        ebit_delta: adj_ebit - inp.reported_ebit,
        ebitda_delta: adj_ebitda - inp.reported_ebitda,
        ebita_delta: adj_ebita - inp.reported_ebita,
        adjustment_items_used: items_used(),
        items_excluded: items_excluded(),
    };
    with_margins(out, inp, revenue)
}

/// Auto-detect direction from `accounting_standard` (IFRS → strip; else add).
pub fn auto_convert(inp: &IfrsAdjustmentInput, revenue: f64) -> IfrsAdjustmentOutput {
    if inp.accounting_standard.trim().eq_ignore_ascii_case("IFRS") {
        convert_ifrs_to_us_gaap(inp, revenue)
    } else {
        convert_us_gaap_to_ifrs(inp, revenue)
    }
}

/// ASC 842 lease-note data extracted from a 10-K (US GAAP filers).
#[derive(Clone, Debug, Default)]
pub struct Asc842LeaseData {
    pub operating_lease_cost: Option<f64>,
    pub finance_lease_cost: Option<f64>,
    pub variable_lease_cost: Option<f64>,
    pub short_term_lease_cost: Option<f64>,
    pub operating_rou_assets: Option<f64>,
    pub finance_rou_assets: Option<f64>,
    pub operating_lease_liability: Option<f64>,
    pub finance_lease_liability: Option<f64>,
    /// Weighted-average discount rate as a percentage (e.g. 3.4 → 3.4%).
    pub weighted_avg_discount_rate: Option<f64>,
    /// Weighted-average remaining lease term in years.
    pub weighted_avg_lease_term: Option<f64>,
    // Computed:
    pub estimated_rou_depreciation: Option<f64>,
    pub estimated_lease_interest: Option<f64>,
}

/// Python-truthy: `None` and `0.0` are both falsy.
fn truthy(o: Option<f64>) -> Option<f64> {
    o.filter(|&x| x != 0.0)
}

impl Asc842LeaseData {
    /// Estimate ROU depreciation + lease interest for IFRS 16 conversion.
    /// Faithful to the sequential fallback ordering in `compute_ifrs_adjustments`.
    pub fn compute_ifrs_adjustments(&mut self) {
        // Lease Interest = Lease Liability × Discount Rate.
        if let (Some(liab), Some(rate)) = (
            truthy(self.operating_lease_liability),
            truthy(self.weighted_avg_discount_rate),
        ) {
            self.estimated_lease_interest = Some(liab * rate / 100.0);
        }
        // ROU Depreciation = Operating Lease Cost − Lease Interest (≥ 0).
        if let (Some(cost), Some(interest)) = (
            truthy(self.operating_lease_cost),
            truthy(self.estimated_lease_interest),
        ) {
            let rou = (cost - interest).max(0.0);
            self.estimated_rou_depreciation = Some(rou);
        }
        // Alternative: ROU assets ÷ lease term.
        if truthy(self.estimated_rou_depreciation).is_none() {
            if let (Some(rou_a), Some(term)) = (
                truthy(self.operating_rou_assets),
                truthy(self.weighted_avg_lease_term),
            ) {
                self.estimated_rou_depreciation = Some(rou_a / term);
            }
        }
        // Fallback interest: 3.5% of lease liability.
        if truthy(self.estimated_lease_interest).is_none() {
            if let Some(liab) = truthy(self.operating_lease_liability) {
                self.estimated_lease_interest = Some(liab * 0.035);
            }
        }
        // Fallback ROU depreciation: 75% of operating lease cost.
        if truthy(self.estimated_rou_depreciation).is_none() {
            if let Some(cost) = truthy(self.operating_lease_cost) {
                self.estimated_rou_depreciation = Some(cost * 0.75);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f64 = 1e-9;

    fn approx(a: f64, b: f64) {
        assert!((a - b).abs() < EPS, "expected {b}, got {a}");
    }

    // Oracle values captured from kb/ifrs.py + us_gaap_leases.py (Python).
    #[test]
    fn ifrs_to_us_gaap_with_revenue() {
        let inp = IfrsAdjustmentInput {
            rou_depreciation: 800.0,
            lease_interest: 120.0,
            short_term_rent: 50.0,
            reported_ebit: 5000.0,
            reported_ebitda: 7000.0,
            reported_ebita: 5200.0,
            standard_depreciation: 1800.0,
            accounting_standard: "IFRS".into(),
            ..Default::default()
        };
        let o = convert_ifrs_to_us_gaap(&inp, 20000.0);
        approx(o.adjusted_ebit, 4880.0);
        approx(o.adjusted_ebitda, 6080.0);
        approx(o.adjusted_ebita, 5080.0);
        approx(o.ebit_delta, -120.0);
        approx(o.ebitda_delta, -920.0);
        approx(o.adjusted_ebit_margin, 24.4);
        approx(o.adjusted_ebitda_margin, 30.4);
        approx(o.adjusted_ebita_margin, 25.4);
        assert_eq!(o.direction, AdjustmentDirection::IfrsToUsGaap);
    }

    #[test]
    fn us_gaap_to_ifrs_with_revenue() {
        let inp = IfrsAdjustmentInput {
            rou_depreciation: 640.0,
            lease_interest: 95.0,
            reported_ebit: 3000.0,
            reported_ebitda: 4200.0,
            reported_ebita: 3100.0,
            accounting_standard: "US GAAP".into(),
            ..Default::default()
        };
        let o = convert_us_gaap_to_ifrs(&inp, 15000.0);
        approx(o.adjusted_ebit, 3095.0);
        approx(o.adjusted_ebitda, 4935.0);
        approx(o.adjusted_ebita, 3195.0);
        approx(o.ebitda_delta, 735.0);
        approx(o.adjusted_ebit_margin, 20.633333333333333);
        approx(o.adjusted_ebitda_margin, 32.9);
        assert_eq!(o.direction, AdjustmentDirection::UsGaapToIfrs);
    }

    #[test]
    fn auto_convert_ifrs_no_revenue() {
        let inp = IfrsAdjustmentInput {
            rou_depreciation: 100.0,
            lease_interest: 20.0,
            reported_ebit: 900.0,
            reported_ebitda: 1200.0,
            reported_ebita: 950.0,
            accounting_standard: "IFRS".into(),
            ..Default::default()
        };
        let o = auto_convert(&inp, 0.0);
        approx(o.adjusted_ebit, 880.0);
        approx(o.adjusted_ebitda, 1080.0);
        approx(o.adjusted_ebita, 930.0);
        approx(o.adjusted_ebit_margin, 0.0); // no revenue → margins stay 0
    }

    #[test]
    fn asc842_primary_estimation() {
        let mut d = Asc842LeaseData {
            operating_lease_cost: Some(1000.0),
            operating_lease_liability: Some(5000.0),
            weighted_avg_discount_rate: Some(3.4),
            ..Default::default()
        };
        d.compute_ifrs_adjustments();
        approx(d.estimated_lease_interest.unwrap(), 170.0);
        approx(d.estimated_rou_depreciation.unwrap(), 830.0);
    }

    #[test]
    fn asc842_rou_assets_over_term() {
        let mut d = Asc842LeaseData {
            operating_rou_assets: Some(9800.0),
            weighted_avg_lease_term: Some(9.8),
            ..Default::default()
        };
        d.compute_ifrs_adjustments();
        assert!(d.estimated_lease_interest.is_none());
        approx(d.estimated_rou_depreciation.unwrap(), 9800.0 / 9.8);
    }

    #[test]
    fn asc842_fallbacks_when_no_discount_rate() {
        // No discount rate → interest falls to 3.5% of liability; ROU dep to 75%
        // of cost (step 2 skipped because interest was still unset at that point).
        let mut d = Asc842LeaseData {
            operating_lease_cost: Some(800.0),
            operating_lease_liability: Some(6000.0),
            ..Default::default()
        };
        d.compute_ifrs_adjustments();
        approx(d.estimated_lease_interest.unwrap(), 6000.0 * 0.035);
        approx(d.estimated_rou_depreciation.unwrap(), 600.0);
    }
}
