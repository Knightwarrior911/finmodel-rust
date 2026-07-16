//! Research → modeling bridge (Phase 5.6) — pure, offline-testable.
//!
//! A [`SuggestedAssumption`] is a per-driver, per-year override the research
//! layer proposes to the analyst grid. It is NEVER auto-applied: the UI shows one
//! row per suggestion and requires individual acceptance, and only a row that
//! passes [`validate_suggested_assumption`] may become an `AssumptionOverride`
//! (with `origin = Research` provenance). Validation proves the driver is known,
//! the year/value grid is well-formed and horizon-bounded, the values are finite
//! and within the driver's grid bounds, the declared unit matches the driver, and
//! every citation resolves to a `Read` source — so a malformed model suggestion
//! can never silently perturb a financial model.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::research::{CitationRef, ResearchConfidence};

/// A projection driver the analyst grid can override. Each variant's snake_case
/// serialization is exactly its `ScenarioInputs` field name, so an unknown driver
/// fails to deserialize (there is no "unknown key" state to validate later).
///
/// This set MUST match `fm_build`'s `scenario_field_mut` key list; the
/// [`tests::keys_match_scenario_fields`] test pins the field strings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssumptionKey {
    RevenueGrowthPct,
    GrossMarginPct,
    SgaPctRev,
    RdPctRev,
    DaPctRev,
    CapexPctRev,
    TaxRatePct,
    InterestRatePct,
    DsoDays,
    DioDays,
    DpoDays,
    DividendPerShare,
}

/// The unit a driver's values are expressed in — used to reject a suggestion
/// whose declared unit contradicts the driver (e.g. `days` for a margin).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssumptionUnit {
    /// Percentage points (growth, margins, rates).
    Percent,
    /// A day count (working-capital drivers).
    Days,
    /// A per-share currency amount (dividend per share).
    Currency,
}

impl AssumptionKey {
    /// The `ScenarioInputs` field name this driver overrides.
    pub fn field(self) -> &'static str {
        match self {
            AssumptionKey::RevenueGrowthPct => "revenue_growth_pct",
            AssumptionKey::GrossMarginPct => "gross_margin_pct",
            AssumptionKey::SgaPctRev => "sga_pct_rev",
            AssumptionKey::RdPctRev => "rd_pct_rev",
            AssumptionKey::DaPctRev => "da_pct_rev",
            AssumptionKey::CapexPctRev => "capex_pct_rev",
            AssumptionKey::TaxRatePct => "tax_rate_pct",
            AssumptionKey::InterestRatePct => "interest_rate_pct",
            AssumptionKey::DsoDays => "dso_days",
            AssumptionKey::DioDays => "dio_days",
            AssumptionKey::DpoDays => "dpo_days",
            AssumptionKey::DividendPerShare => "dividend_per_share",
        }
    }

    /// The unit this driver's values must be expressed in.
    pub fn unit(self) -> AssumptionUnit {
        match self {
            AssumptionKey::DsoDays | AssumptionKey::DioDays | AssumptionKey::DpoDays => {
                AssumptionUnit::Days
            }
            AssumptionKey::DividendPerShare => AssumptionUnit::Currency,
            _ => AssumptionUnit::Percent,
        }
    }

    /// Inclusive `(min, max)` grid bounds a value must fall within. Wide but
    /// finite — a defense against absurd model output, not a tight sanity band.
    pub fn bounds(self) -> (f64, f64) {
        match self.unit() {
            // Growth can be sharply negative; margins/rates are non-negative but a
            // single band across all percent drivers is enough as an outer guard.
            AssumptionUnit::Percent => (-100.0, 500.0),
            AssumptionUnit::Days => (0.0, 1000.0),
            AssumptionUnit::Currency => (0.0, 1_000_000.0),
        }
    }
}

/// A per-driver, per-year override the research layer proposes for analyst review.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SuggestedAssumption {
    pub key: AssumptionKey,
    /// Projection years this suggestion sets (parallel to `values`).
    pub years: Vec<i32>,
    /// One value per entry in `years`.
    pub values: Vec<f64>,
    pub unit: AssumptionUnit,
    pub rationale: String,
    pub citations: Vec<CitationRef>,
    pub confidence: ResearchConfidence,
}

/// Why a suggested assumption was rejected. A rejected row NEVER reaches a model.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SuggestionReject {
    /// `years` or `values` is empty.
    Empty,
    /// `years` and `values` differ in length.
    LengthMismatch,
    /// A year appears more than once.
    DuplicateYear,
    /// A year lies outside the model's projection horizon.
    YearOutOfHorizon,
    /// A value is NaN or infinite.
    NonFiniteValue,
    /// A value lies outside the driver's grid bounds.
    ValueOutOfBounds,
    /// The declared unit does not match the driver's unit.
    UnitMismatch,
    /// The suggestion carries no citation.
    NoCitation,
    /// A citation references a source that was not `Read`.
    CitationNotRead,
}

/// Validate a suggested assumption against the projection horizon and the set of
/// `Read` source ids. `Ok(())` means the row is safe to convert into an
/// `AssumptionOverride`; any `Err` means the UI must reject the row.
pub fn validate_suggested_assumption(
    s: &SuggestedAssumption,
    horizon_years: &[i32],
    read_source_ids: &HashSet<&str>,
) -> Result<(), SuggestionReject> {
    if s.years.is_empty() || s.values.is_empty() {
        return Err(SuggestionReject::Empty);
    }
    if s.years.len() != s.values.len() {
        return Err(SuggestionReject::LengthMismatch);
    }
    let mut seen: HashSet<i32> = HashSet::with_capacity(s.years.len());
    for y in &s.years {
        if !seen.insert(*y) {
            return Err(SuggestionReject::DuplicateYear);
        }
    }
    let horizon: HashSet<i32> = horizon_years.iter().copied().collect();
    if s.years.iter().any(|y| !horizon.contains(y)) {
        return Err(SuggestionReject::YearOutOfHorizon);
    }
    if s.unit != s.key.unit() {
        return Err(SuggestionReject::UnitMismatch);
    }
    let (lo, hi) = s.key.bounds();
    for v in &s.values {
        if !v.is_finite() {
            return Err(SuggestionReject::NonFiniteValue);
        }
        if *v < lo || *v > hi {
            return Err(SuggestionReject::ValueOutOfBounds);
        }
    }
    if s.citations.is_empty() {
        return Err(SuggestionReject::NoCitation);
    }
    for c in &s.citations {
        if !read_source_ids.contains(c.source_id.as_str()) {
            return Err(SuggestionReject::CitationNotRead);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cite(id: &str) -> CitationRef {
        CitationRef {
            source_id: id.into(),
            quote: "q".into(),
        }
    }

    fn good() -> SuggestedAssumption {
        SuggestedAssumption {
            key: AssumptionKey::RevenueGrowthPct,
            years: vec![2026, 2027],
            values: vec![12.0, 10.0],
            unit: AssumptionUnit::Percent,
            rationale: "Guidance implies deceleration.".into(),
            citations: vec![cite("S1")],
            confidence: ResearchConfidence::Medium,
        }
    }

    fn read_ids() -> HashSet<&'static str> {
        HashSet::from(["S1", "S2"])
    }

    #[test]
    fn keys_match_scenario_fields() {
        // Pins the field strings to `fm_build`'s `scenario_field_mut` list.
        let all = [
            (AssumptionKey::RevenueGrowthPct, "revenue_growth_pct"),
            (AssumptionKey::GrossMarginPct, "gross_margin_pct"),
            (AssumptionKey::SgaPctRev, "sga_pct_rev"),
            (AssumptionKey::RdPctRev, "rd_pct_rev"),
            (AssumptionKey::DaPctRev, "da_pct_rev"),
            (AssumptionKey::CapexPctRev, "capex_pct_rev"),
            (AssumptionKey::TaxRatePct, "tax_rate_pct"),
            (AssumptionKey::InterestRatePct, "interest_rate_pct"),
            (AssumptionKey::DsoDays, "dso_days"),
            (AssumptionKey::DioDays, "dio_days"),
            (AssumptionKey::DpoDays, "dpo_days"),
            (AssumptionKey::DividendPerShare, "dividend_per_share"),
        ];
        for (k, field) in all {
            assert_eq!(k.field(), field);
            // The snake_case serialization also equals the field name.
            let ser = serde_json::to_string(&k).unwrap();
            assert_eq!(ser, format!("\"{field}\""));
        }
        // Pin to fm-excel's canonical list: the enum's fields must equal it
        // exactly, so a new ScenarioInputs driver forces an AssumptionKey update.
        let enum_fields: std::collections::HashSet<&str> =
            all.iter().map(|(k, _)| k.field()).collect();
        let canonical: std::collections::HashSet<&str> = fm_excel::input::SCENARIO_DRIVER_KEYS
            .iter()
            .copied()
            .collect();
        assert_eq!(
            enum_fields, canonical,
            "AssumptionKey drifted from ScenarioInputs drivers"
        );
    }

    #[test]
    fn unknown_driver_key_fails_to_deserialize() {
        assert!(serde_json::from_str::<AssumptionKey>("\"ebit_margin_pct\"").is_err());
    }

    #[test]
    fn valid_suggestion_passes() {
        assert!(validate_suggested_assumption(&good(), &[2026, 2027, 2028], &read_ids()).is_ok());
    }

    #[test]
    fn rejects_length_mismatch_and_empty() {
        let mut s = good();
        s.values = vec![12.0];
        assert_eq!(
            validate_suggested_assumption(&s, &[2026, 2027], &read_ids()),
            Err(SuggestionReject::LengthMismatch)
        );
        let mut e = good();
        e.years.clear();
        e.values.clear();
        assert_eq!(
            validate_suggested_assumption(&e, &[2026, 2027], &read_ids()),
            Err(SuggestionReject::Empty)
        );
    }

    #[test]
    fn rejects_duplicate_and_out_of_horizon_years() {
        let mut dup = good();
        dup.years = vec![2026, 2026];
        dup.values = vec![12.0, 11.0];
        assert_eq!(
            validate_suggested_assumption(&dup, &[2026, 2027], &read_ids()),
            Err(SuggestionReject::DuplicateYear)
        );
        let mut oob = good();
        oob.years = vec![2099];
        oob.values = vec![12.0];
        assert_eq!(
            validate_suggested_assumption(&oob, &[2026, 2027], &read_ids()),
            Err(SuggestionReject::YearOutOfHorizon)
        );
    }

    #[test]
    fn rejects_unit_mismatch_nonfinite_and_out_of_bounds() {
        let mut u = good();
        u.unit = AssumptionUnit::Days;
        assert_eq!(
            validate_suggested_assumption(&u, &[2026, 2027], &read_ids()),
            Err(SuggestionReject::UnitMismatch)
        );
        let mut nf = good();
        nf.values = vec![f64::NAN, 10.0];
        assert_eq!(
            validate_suggested_assumption(&nf, &[2026, 2027], &read_ids()),
            Err(SuggestionReject::NonFiniteValue)
        );
        let mut oob = good();
        oob.values = vec![9000.0, 10.0];
        assert_eq!(
            validate_suggested_assumption(&oob, &[2026, 2027], &read_ids()),
            Err(SuggestionReject::ValueOutOfBounds)
        );
    }

    #[test]
    fn rejects_missing_and_non_read_citations() {
        let mut nc = good();
        nc.citations.clear();
        assert_eq!(
            validate_suggested_assumption(&nc, &[2026, 2027], &read_ids()),
            Err(SuggestionReject::NoCitation)
        );
        let mut bad = good();
        bad.citations = vec![cite("S9")];
        assert_eq!(
            validate_suggested_assumption(&bad, &[2026, 2027], &read_ids()),
            Err(SuggestionReject::CitationNotRead)
        );
    }

    #[test]
    fn days_and_currency_drivers_accept_their_units() {
        let dso = SuggestedAssumption {
            key: AssumptionKey::DsoDays,
            years: vec![2026],
            values: vec![45.0],
            unit: AssumptionUnit::Days,
            rationale: "r".into(),
            citations: vec![cite("S2")],
            confidence: ResearchConfidence::Low,
        };
        assert!(validate_suggested_assumption(&dso, &[2026], &read_ids()).is_ok());
        let dps = SuggestedAssumption {
            key: AssumptionKey::DividendPerShare,
            years: vec![2026],
            values: vec![1.25],
            unit: AssumptionUnit::Currency,
            rationale: "r".into(),
            citations: vec![cite("S1")],
            confidence: ResearchConfidence::Low,
        };
        assert!(validate_suggested_assumption(&dps, &[2026], &read_ids()).is_ok());
    }
}
