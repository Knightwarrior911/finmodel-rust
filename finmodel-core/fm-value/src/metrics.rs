//! Deterministic finance metric operations (Task 4.4).
//!
//! Typed, pure functions — never a free-form expression evaluator, and the LLM
//! never performs material arithmetic. These are the authority the verifier
//! (Task 4.2) recomputes ratios/growth from, and the tolerance policy for
//! comparing a claimed figure against a recomputed one.

/// Period-over-period growth as a fraction (0.15 = +15%). `None` when the base
/// is zero (undefined growth).
pub fn growth_rate(current: f64, prior: f64) -> Option<f64> {
    if prior == 0.0 {
        None
    } else {
        Some((current - prior) / prior)
    }
}

/// A margin (part / whole) as a fraction. `None` when the whole is zero.
pub fn margin(part: f64, whole: f64) -> Option<f64> {
    if whole == 0.0 {
        None
    } else {
        Some(part / whole)
    }
}

/// A generic numerator/denominator ratio. `None` when the denominator is zero.
pub fn ratio(numerator: f64, denominator: f64) -> Option<f64> {
    if denominator == 0.0 {
        None
    } else {
        Some(numerator / denominator)
    }
}

/// Compound annual growth rate over `years` periods. `None` for a non-positive
/// base/years or a negative ending value.
pub fn cagr(begin: f64, end: f64, years: f64) -> Option<f64> {
    if begin <= 0.0 || end < 0.0 || years <= 0.0 {
        None
    } else {
        Some((end / begin).powf(1.0 / years) - 1.0)
    }
}

/// Fraction → basis points (0.0125 → 125.0).
pub fn to_bps(fraction: f64) -> f64 {
    fraction * 10_000.0
}

/// Fraction → percentage (0.15 → 15.0).
pub fn to_pct(fraction: f64) -> f64 {
    fraction * 100.0
}

/// Normalize a reported value by a base-10 scale exponent: `value * 10^scale_exp`
/// (a figure in millions with `scale_exp = 6` → absolute units). Deterministic.
pub fn apply_scale(value: f64, scale_exp: i32) -> f64 {
    value * 10f64.powi(scale_exp)
}

/// An additive bridge: `start + Σ deltas`. Returns the computed end value so a
/// verifier can check `start → end` reconciles (EV/IFRS bridges build on this).
pub fn bridge(start: f64, deltas: &[f64]) -> f64 {
    start + deltas.iter().sum::<f64>()
}

/// The metric class that drives verification tolerance (mirrors the verifier's
/// policy; kept here so tolerance lives with the deterministic engine).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetricClass {
    /// Source-recorded quantity / share count: exact after normalization.
    ExactQuantity,
    /// Rounded currency / per-share value: half the displayed unit.
    RoundedCurrency,
    /// Computed ratio / percentage / basis points: half the displayed precision.
    ComputedRatio,
}

/// Comparison tolerance for `class` at `displayed_decimals` precision. Never a
/// single blanket tolerance (plan 4.2 step 5).
pub fn tolerance(class: MetricClass, displayed_decimals: u32) -> f64 {
    match class {
        MetricClass::ExactQuantity => 0.0,
        MetricClass::RoundedCurrency | MetricClass::ComputedRatio => {
            0.5 * 10f64.powi(-(displayed_decimals as i32))
        }
    }
}

/// Whether `claimed` matches `recomputed` within the class tolerance.
pub fn agrees(claimed: f64, recomputed: f64, class: MetricClass, displayed_decimals: u32) -> bool {
    (claimed - recomputed).abs() <= tolerance(class, displayed_decimals) + f64::EPSILON
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn growth_rate_basic_and_zero_base() {
        assert!((growth_rate(115.0, 100.0).unwrap() - 0.15).abs() < 1e-12);
        assert_eq!(growth_rate(1.0, 0.0), None);
        assert!((growth_rate(80.0, 100.0).unwrap() - (-0.2)).abs() < 1e-12);
    }

    #[test]
    fn margin_and_ratio_guard_zero() {
        assert!((margin(30.0, 120.0).unwrap() - 0.25).abs() < 1e-12);
        assert_eq!(margin(1.0, 0.0), None);
        assert!((ratio(15.0, 3.0).unwrap() - 5.0).abs() < 1e-12);
        assert_eq!(ratio(1.0, 0.0), None);
    }

    #[test]
    fn cagr_and_edge_cases() {
        // 100 → 133.1 over 3y ≈ 10%.
        assert!((cagr(100.0, 133.1, 3.0).unwrap() - 0.1).abs() < 1e-6);
        assert_eq!(cagr(0.0, 100.0, 3.0), None);
        assert_eq!(cagr(100.0, 100.0, 0.0), None);
    }

    #[test]
    fn conversions_and_scale() {
        assert!((to_bps(0.0125) - 125.0).abs() < 1e-9);
        assert!((to_pct(0.15) - 15.0).abs() < 1e-9);
        assert!((apply_scale(60_922.0, 6) - 60_922_000_000.0).abs() < 1.0);
    }

    #[test]
    fn bridge_reconciles() {
        // EV bridge shape: equity + debt - cash = EV.
        assert!((bridge(1000.0, &[200.0, -50.0]) - 1150.0).abs() < 1e-9);
    }

    #[test]
    fn tolerance_is_metric_specific_and_agrees() {
        assert_eq!(tolerance(MetricClass::ExactQuantity, 0), 0.0);
        assert!((tolerance(MetricClass::ComputedRatio, 1) - 0.05).abs() < 1e-12);
        // A recomputed 15% growth claim agrees with 0.1499 at 1-decimal pct? No —
        // compare in percentage space at the displayed precision.
        assert!(agrees(15.0, 15.02, MetricClass::ComputedRatio, 1));
        assert!(!agrees(15.0, 12.0, MetricClass::ComputedRatio, 1));
    }
}
