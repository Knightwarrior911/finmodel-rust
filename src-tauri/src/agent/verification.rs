//! Claim + artifact verification (Task 4.2).
//!
//! Verification begins from structured tool outputs, never by asking a model
//! "is this correct?". A material [`Claim`] carries entity/metric/value/unit/
//! currency/scale/period/source/locator; the verifier compares its normalized
//! value against the source-recorded (or, in Task 4.4, deterministically
//! recomputed) value under a **metric-specific tolerance** — never one blanket
//! tolerance. A run finishes `verified`, `verified_with_warnings`, or
//! `partial_unverified`; no generic success badge may mask the last state.

use fm_agent::types::Claim;
use fm_value::metrics;
// `fm-value::metrics` is the single authority for metric class + tolerance
// (Task 4.4); re-export so callers use one type.
pub use fm_value::metrics::MetricClass;

/// Verification status for a claim or the whole run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClaimStatus {
    Verified,
    VerifiedWithWarnings,
    Unverified,
}

impl ClaimStatus {
    /// The user-facing badge string; `partial_unverified` never renders as a
    /// clean success.
    pub fn badge(self) -> &'static str {
        match self {
            ClaimStatus::Verified => "verified",
            ClaimStatus::VerifiedWithWarnings => "verified_with_warnings",
            ClaimStatus::Unverified => "partial_unverified",
        }
    }
}

/// A per-claim verification record persisted for the UI badge + inspection.
#[derive(Clone, Debug, PartialEq)]
pub struct ClaimVerification {
    pub claim_key: String,
    pub status: ClaimStatus,
    pub method: &'static str,
    pub expected: Option<f64>,
    pub observed: Option<f64>,
    pub tolerance: f64,
}

/// Verify a numeric claim against a source-recorded value (direct-source
/// verification; ratio/growth recompute lands with `fm-value::metrics` in Task
/// 4.4). A claim with no source value or an unparseable value is `Unverified` —
/// missing evidence never certifies.
pub fn verify_direct(
    claim: &Claim,
    source_value: Option<f64>,
    class: MetricClass,
    displayed_decimals: u32,
) -> ClaimVerification {
    let claimed = claim.normalized_value.parse::<f64>().ok();
    let tol = metrics::tolerance(class, displayed_decimals);
    let status = match (claimed, source_value) {
        (Some(c), Some(_s)) if metrics::agrees(c, _s, class, displayed_decimals) => {
            ClaimStatus::Verified
        }
        _ => ClaimStatus::Unverified,
    };
    ClaimVerification {
        claim_key: claim.claim_key.clone(),
        status,
        method: "direct_source",
        expected: claimed,
        observed: source_value,
        tolerance: tol,
    }
}

/// A qualitative claim is `verified_with_warnings` when it has a cited source
/// excerpt/locator but semantic support could not be established deterministically
/// (`llm_assisted_untrusted`), and `Unverified` when it has no citation at all.
pub fn verify_qualitative(has_citation: bool) -> ClaimStatus {
    if has_citation {
        ClaimStatus::VerifiedWithWarnings
    } else {
        ClaimStatus::Unverified
    }
}

/// The run's overall status is the weakest of its claim statuses (any
/// `Unverified` → the run is `partial_unverified`).
pub fn rollup<'a>(statuses: impl IntoIterator<Item = &'a ClaimStatus>) -> ClaimStatus {
    let mut worst = ClaimStatus::Verified;
    for s in statuses {
        match s {
            ClaimStatus::Unverified => return ClaimStatus::Unverified,
            ClaimStatus::VerifiedWithWarnings => worst = ClaimStatus::VerifiedWithWarnings,
            ClaimStatus::Verified => {}
        }
    }
    worst
}

/// A run-level verification report: per-claim records + the rolled-up status.
#[derive(Clone, Debug, PartialEq)]
pub struct RunVerification {
    pub claims: Vec<ClaimVerification>,
    pub status: ClaimStatus,
}

impl RunVerification {
    /// A run needs a deterministic repair pass when it rolled up to
    /// `partial_unverified` (any material claim unverified).
    pub fn needs_repair(&self) -> bool {
        self.status == ClaimStatus::Unverified
    }
}

/// Verify every material claim in a run against its authoritative value, then
/// roll up to the run status (Tasks 4.2/4.4 — the driver's verify loop).
/// `authoritative(claim)` returns the source-recorded or deterministically
/// recomputed `(value, class, displayed_decimals)`, or `None` when no evidence
/// exists (→ `Unverified`; missing evidence never certifies). Pure over the
/// injected recompute, so the loop is testable without live tool results.
pub fn verify_run<F>(claims: &[Claim], mut authoritative: F) -> RunVerification
where
    F: FnMut(&Claim) -> Option<(f64, MetricClass, u32)>,
{
    let records: Vec<ClaimVerification> = claims
        .iter()
        .map(|c| match authoritative(c) {
            Some((v, class, dec)) => verify_direct(c, Some(v), class, dec),
            None => verify_direct(c, None, MetricClass::ExactQuantity, 0),
        })
        .collect();
    let status = rollup(records.iter().map(|r| &r.status));
    RunVerification {
        claims: records,
        status,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claim(key: &str, value: &str) -> Claim {
        Claim {
            claim_key: key.into(),
            entity: "NVDA".into(),
            normalized_value: value.into(),
            unit: "percent".into(),
            currency: None,
            scale: "1".into(),
            period: "FY2024".into(),
            locator: "10-K p.42".into(),
            source_id: "src-1".into(),
            quote_hash: "h".into(),
        }
    }
    #[test]
    fn injected_15_vs_12_mismatch_is_caught() {
        // The golden verification gate: a 15% claim against a 12% source value
        // must NOT verify — never a clean success badge.
        let c = claim("nvda.growth.fy2024", "15.0");
        let v = verify_direct(&c, Some(12.0), MetricClass::ComputedRatio, 1);
        assert_eq!(v.status, ClaimStatus::Unverified);
        assert_ne!(v.status.badge(), "verified");
    }

    #[test]
    fn matching_value_within_tolerance_verifies() {
        let c = claim("nvda.eps.fy2024", "2.53");
        // Rounded per-share, 2 decimals → tol 0.005; 2.53 vs 2.531 passes.
        let v = verify_direct(&c, Some(2.531), MetricClass::RoundedCurrency, 2);
        assert_eq!(v.status, ClaimStatus::Verified);
    }

    #[test]
    fn missing_source_value_never_certifies() {
        let c = claim("nvda.revenue.fy2024", "60922");
        let v = verify_direct(&c, None, MetricClass::ExactQuantity, 0);
        assert_eq!(v.status, ClaimStatus::Unverified);
    }

    #[test]
    fn exact_quantity_requires_exact_match() {
        let c = claim("nvda.shares", "2470000000");
        assert_eq!(
            verify_direct(&c, Some(2_470_000_000.0), MetricClass::ExactQuantity, 0).status,
            ClaimStatus::Verified
        );
        assert_eq!(
            verify_direct(&c, Some(2_470_000_001.0), MetricClass::ExactQuantity, 0).status,
            ClaimStatus::Unverified
        );
    }

    #[test]
    fn qualitative_needs_a_citation() {
        assert_eq!(verify_qualitative(true), ClaimStatus::VerifiedWithWarnings);
        assert_eq!(verify_qualitative(false), ClaimStatus::Unverified);
    }

    #[test]
    fn rollup_takes_the_weakest_status() {
        use ClaimStatus::*;
        assert_eq!(rollup([&Verified, &Verified]), Verified);
        assert_eq!(
            rollup([&Verified, &VerifiedWithWarnings]),
            VerifiedWithWarnings
        );
        assert_eq!(
            rollup([&Verified, &VerifiedWithWarnings, &Unverified]),
            Unverified
        );
        assert_eq!(rollup::<'_>(std::iter::empty()), Verified);
    }

    #[test]
    fn verify_run_rolls_up_and_flags_repair() {
        // Three claims: one matches recompute, one is a mismatch, one lacks
        // evidence — the run rolls up to the weakest status and demands repair.
        let claims = vec![
            claim("nvda.eps.fy2024", "2.53"),
            claim("nvda.growth.fy2024", "15.0"),
            claim("nvda.margin.fy2024", "40.0"),
        ];
        let report = verify_run(&claims, |c| match c.claim_key.as_str() {
            "nvda.eps.fy2024" => Some((2.531, MetricClass::RoundedCurrency, 2)),
            "nvda.growth.fy2024" => Some((12.0, MetricClass::ComputedRatio, 1)),
            _ => None, // no evidence for the margin claim
        });
        assert_eq!(report.status, ClaimStatus::Unverified);
        assert!(report.needs_repair());
        assert_eq!(report.claims.len(), 3);
        // A run whose every claim verifies needs no repair.
        let clean = verify_run(&[claim("nvda.eps.fy2024", "2.53")], |_| {
            Some((2.531, MetricClass::RoundedCurrency, 2))
        });
        assert_eq!(clean.status, ClaimStatus::Verified);
        assert!(!clean.needs_repair());
    }
}
