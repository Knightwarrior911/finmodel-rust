//! Reporting-period basis selection for benchmark metrics.
//!
//! The analyst picks which reporting period a peer row reflects: annual fiscal
//! year, LTM (trailing-twelve-months), the latest discrete quarter, or the
//! latest semi-annual half. Flows are period-scoped; balance-sheet items always
//! use the latest reported instant. Underivable components stay `None` — never
//! annualized fabrication.
//!
//! Shares the low-level fact plumbing with [`crate::ltm`] (one implementation).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ltm::{self, days_to_iso, facts_for, latest_instant, Fact, LtmData};

/// Which reporting period the metrics reflect.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PeriodBasis {
    /// Latest annual fiscal year (default; the parity-gated model builder basis).
    #[default]
    #[serde(rename = "annual")]
    AnnualFy,
    /// Trailing twelve months (FY + latest interim − prior-year interim).
    #[serde(rename = "ltm")]
    Ltm,
    /// Latest discrete quarter (native 3-month fact, else YTD − prior YTD).
    #[serde(rename = "quarter")]
    Quarter,
    /// Latest semi-annual half (native 6-month fact, else two discrete quarters).
    #[serde(rename = "semi")]
    SemiAnnual,
}

/// One reporting period's figures on the chosen basis + a display label.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PeriodData {
    /// Same field set as [`LtmData`] (flows period-scoped, BS latest instant).
    pub data: LtmData,
    /// Display label, e.g. `FY2025`, `LTM Sep-25`, `Q3 FY25`, `H1 FY25`.
    pub label: String,
}

/// Extract one period's figures on `basis` from companyfacts JSON.
pub fn extract_period(facts: &Value, currency: &str, basis: PeriodBasis) -> Option<PeriodData> {
    match basis {
        PeriodBasis::Ltm => {
            let d = ltm::extract_ltm(facts, currency)?;
            let label = format!("LTM {}", month_year(&d.as_of));
            Some(PeriodData { data: d, label })
        }
        PeriodBasis::AnnualFy => extract_discrete(facts, currency, DiscreteKind::Annual),
        PeriodBasis::Quarter => extract_discrete(facts, currency, DiscreteKind::Quarter),
        PeriodBasis::SemiAnnual => extract_discrete(facts, currency, DiscreteKind::Semi),
    }
}

#[derive(Clone, Copy, PartialEq)]
enum DiscreteKind {
    Annual,
    Quarter,
    Semi,
}

/// `Mon-YY` from a `YYYY-MM-DD` string (e.g. `Sep-25`).
fn month_year(iso: &str) -> String {
    const MON: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let mut it = iso.splitn(3, '-');
    let y = it.next().unwrap_or("");
    let m: usize = it.next().and_then(|s| s.parse().ok()).unwrap_or(1);
    let mon = MON.get(m.saturating_sub(1)).copied().unwrap_or("");
    let yy = y.get(y.len().saturating_sub(2)..).unwrap_or(y);
    format!("{mon}-{yy}")
}

/// Year + 2-digit year of an end-days value.
fn year_parts(end: i64) -> (i32, String) {
    let iso = days_to_iso(end);
    let y = iso.get(..4).and_then(|s| s.parse().ok()).unwrap_or(0);
    let yy = iso.get(2..4).unwrap_or("").to_string();
    (y, yy)
}

/// Extract flows on a discrete basis (annual / quarter / semi) + latest-instant
/// BS. Flow concepts are anchored to revenue's latest reported end; concepts
/// stale by >400d are dropped (never blended). `None` per field it can't derive.
fn extract_discrete(facts: &Value, currency: &str, kind: DiscreteKind) -> Option<PeriodData> {
    let (gaap, tm, _tax) = crate::xbrl::select_taxonomy(facts)?;
    let flow = |key: &str| -> Option<(f64, i64)> {
        let fv = tm.get(key).and_then(|tags| facts_for(gaap, tags, currency))?;
        match kind {
            DiscreteKind::Annual => annual_flow(&fv),
            DiscreteKind::Quarter => discrete_flow(&fv, 1),
            DiscreteKind::Semi => discrete_flow(&fv, 2),
        }
    };
    let inst = |key: &str| -> Option<(f64, i64)> {
        tm.get(key).and_then(|tags| latest_instant(gaap, tags, currency))
    };

    let rev = flow("revenue")?;
    let anchor = rev.1;
    const STALE: i64 = 400;
    let mut d = LtmData {
        currency: currency.to_string(),
        ..Default::default()
    };
    let mut latest_end = rev.1;
    d.revenue = Some(rev.0);

    let mut set = |slot: &mut Option<f64>, v: Option<(f64, i64)>| {
        if let Some((val, end)) = v {
            if end >= anchor - STALE {
                *slot = Some(val);
                if end > latest_end {
                    latest_end = end;
                }
            }
        }
    };
    set(&mut d.gross_profit, flow("gross_profit"));
    set(&mut d.ebit, flow("ebit"));
    set(&mut d.da, flow("da"));
    set(&mut d.net_income, flow("net_income"));
    set(&mut d.interest_expense, flow("interest_expense"));
    set(&mut d.cfo, flow("cfo"));
    set(&mut d.capex, flow("capex"));
    set(&mut d.dividends_paid, flow("dividends_paid"));
    set(&mut d.buybacks, flow("buybacks"));

    let inst_guard = |v: Option<(f64, i64)>| -> Option<f64> {
        v.filter(|(_, end)| *end >= anchor - STALE).map(|(val, _)| val)
    };
    d.cash = inst_guard(inst("cash"));
    d.long_term_debt = inst_guard(inst("long_term_debt"));
    d.short_term_debt = inst_guard(inst("short_term_debt"));
    d.total_equity = inst_guard(inst("total_equity"));
    d.total_assets = inst_guard(inst("total_assets"));
    d.total_current_assets = inst_guard(inst("total_current_assets"));
    d.total_current_liabilities = inst_guard(inst("total_current_liabilities"));

    if d.gross_profit.is_none() {
        if let Some((cogs, cend)) = flow("cogs") {
            if cend >= anchor - STALE {
                d.gross_profit = Some(rev.0 - cogs);
            }
        }
    }

    d.is_ltm = false;
    d.as_of = days_to_iso(latest_end);

    // Label from the anchor (revenue) end + the period kind. Quarter/half derive
    // from the anchor's end month (robust across native + derived facts).
    let (fy, yy) = year_parts(anchor);
    let qn = quarter_from_end(&d.as_of);
    let label = match kind {
        DiscreteKind::Annual => format!("FY{fy}"),
        DiscreteKind::Quarter => format!("Q{qn} FY{yy}"),
        DiscreteKind::Semi => {
            let half = if qn <= 2 { 1 } else { 2 };
            format!("H{half} FY{yy}")
        }
    };
    Some(PeriodData { data: d, label })
}

/// Calendar quarter of a `YYYY-MM-DD` end date (ceil(month/3)).
fn quarter_from_end(iso: &str) -> u32 {
    let m: u32 = iso.splitn(3, '-').nth(1).and_then(|s| s.parse().ok()).unwrap_or(12);
    ((m + 2) / 3).clamp(1, 4)
}

/// Latest annual FY value (duration ~1yr, from a 10-K/20-F).
fn annual_flow(fv: &[Fact]) -> Option<(f64, i64)> {
    fv.iter()
        .filter(|f| {
            f.is_annual
                && f.start
                    .map(|s| (330..=400).contains(&(f.end - s)))
                    .unwrap_or(false)
        })
        .max_by_key(|f| f.end)
        .map(|f| (f.val, f.end))
}

/// Latest discrete `n_quarters`-quarter flow: a native ~n*91d fact when present,
/// else derived from YTD deltas. `None` when underivable (never annualized).
fn discrete_flow(fv: &[Fact], n_quarters: i64) -> Option<(f64, i64)> {
    let target = n_quarters * 91;
    let tol = if n_quarters == 1 { 14 } else { 20 };
    // Interim (sub-annual durational) facts, any form (10-Q, 6-K half-years, …).
    let interim: Vec<&Fact> = fv
        .iter()
        .filter(|f| f.start.map(|s| (10..320).contains(&(f.end - s))).unwrap_or(false))
        .collect();
    let latest_end = interim.iter().map(|f| f.end).max()?;
    // A native discrete fact of the wanted length ENDING at the latest interim
    // date (so a Q1 YTD never masquerades as the current quarter).
    if let Some(n) = interim
        .iter()
        .filter(|f| f.end == latest_end && ((f.end - f.start.unwrap()) - target).abs() <= tol)
        .max_by_key(|f| f.end - f.start.unwrap())
    {
        return Some((n.val, n.end));
    }
    if n_quarters == 1 {
        // Latest YTD at the latest end, minus the immediately-prior YTD of the
        // SAME fiscal year (same start). Q1 (YTD ≤ ~1 quarter) passes through.
        let cur = interim
            .iter()
            .filter(|f| f.end == latest_end)
            .max_by_key(|f| f.end - f.start.unwrap())?;
        let span = cur.end - cur.start.unwrap();
        if span <= 104 {
            return Some((cur.val, cur.end));
        }
        let prior = interim
            .iter()
            .filter(|f| f.start == cur.start && f.end < cur.end)
            .max_by_key(|f| f.end)?;
        Some((cur.val - prior.val, cur.end))
    } else {
        // Semi = the latest two discrete quarters summed. Requires both.
        let (q_latest, end) = discrete_flow(fv, 1)?;
        let mut ends: Vec<i64> = interim.iter().map(|f| f.end).collect();
        ends.sort_unstable();
        ends.dedup();
        let prior_end = ends.iter().rev().nth(1).copied()?;
        let prior_fv: Vec<Fact> = fv
            .iter()
            .filter(|f| f.start.is_none() || f.end <= prior_end)
            .copied()
            .collect();
        let (q_prior, _) = discrete_flow(&prior_fv, 1)?;
        Some((q_latest + q_prior, end))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a companyfacts JSON with the given us-gaap concept → facts.
    /// Each fact: (start|"", end, val, form).
    fn facts_json(concepts: &[(&str, &[(&str, &str, f64, &str)])]) -> Value {
        let mut gaap = serde_json::Map::new();
        for (tag, rows) in concepts {
            let arr: Vec<Value> = rows
                .iter()
                .map(|(start, end, val, form)| {
                    let mut o = serde_json::Map::new();
                    if !start.is_empty() {
                        o.insert("start".into(), Value::String((*start).into()));
                    }
                    o.insert("end".into(), Value::String((*end).into()));
                    o.insert("val".into(), serde_json::json!(val));
                    o.insert("form".into(), Value::String((*form).into()));
                    o.insert("fy".into(), serde_json::json!("2025"));
                    o.insert("fp".into(), Value::String("Q3".into()));
                    Value::Object(o)
                })
                .collect();
            gaap.insert(
                (*tag).to_string(),
                serde_json::json!({ "label": tag, "units": { "USD": arr } }),
            );
        }
        serde_json::json!({ "facts": { "us-gaap": gaap } })
    }

    // Revenue tag used by the taxonomy map.
    const REV: &str = "RevenueFromContractWithCustomerExcludingAssessedTax";

    #[test]
    fn quarter_from_ytd_deltas() {
        // Q3 YTD (9mo) 300 − Q2 YTD (6mo) 190 = Q3 discrete 110. Same FY start.
        let facts = facts_json(&[(
            REV,
            &[
                ("2025-01-01", "2025-03-31", 90.0, "10-Q"),  // Q1 YTD (3mo)
                ("2025-01-01", "2025-06-30", 190.0, "10-Q"), // H1 YTD (6mo)
                ("2025-01-01", "2025-09-30", 300.0, "10-Q"), // 9mo YTD
            ],
        )]);
        let pd = extract_period(&facts, "USD", PeriodBasis::Quarter).expect("period");
        assert_eq!(pd.data.revenue, Some(110.0)); // 300 − 190
        assert!(pd.label.starts_with("Q3"), "label = {}", pd.label);
    }

    #[test]
    fn quarter_q1_passthrough() {
        let facts = facts_json(&[(REV, &[("2025-01-01", "2025-03-31", 90.0, "10-Q")])]);
        let pd = extract_period(&facts, "USD", PeriodBasis::Quarter).expect("period");
        assert_eq!(pd.data.revenue, Some(90.0)); // Q1 YTD == the quarter
        assert!(pd.label.starts_with("Q1"), "label = {}", pd.label);
    }

    #[test]
    fn semi_from_native_half_year_fact() {
        // Native 6-month fact (foreign 6-K half-year filer).
        let facts = facts_json(&[(REV, &[("2025-01-01", "2025-06-30", 500.0, "6-K")])]);
        let pd = extract_period(&facts, "USD", PeriodBasis::SemiAnnual).expect("period");
        assert_eq!(pd.data.revenue, Some(500.0));
        assert!(pd.label.starts_with("H1"), "label = {}", pd.label);
    }

    #[test]
    fn semi_from_two_quarters() {
        // No native half-year; two discrete quarters: Q2 (6mo YTD 190 − 90) + Q3 (300 − 190).
        let facts = facts_json(&[(
            REV,
            &[
                ("2025-01-01", "2025-03-31", 90.0, "10-Q"),
                ("2025-01-01", "2025-06-30", 190.0, "10-Q"),
                ("2025-01-01", "2025-09-30", 300.0, "10-Q"),
            ],
        )]);
        let pd = extract_period(&facts, "USD", PeriodBasis::SemiAnnual).expect("period");
        // Q3 (110) + Q2 (100) = 210.
        assert_eq!(pd.data.revenue, Some(210.0));
    }

    #[test]
    fn quarter_missing_prior_ytd_blanks_non_q1() {
        // 9-month YTD with no prior same-FY YTD → discrete quarter underivable → None.
        let facts = facts_json(&[(REV, &[("2025-01-01", "2025-09-30", 300.0, "10-Q")])]);
        // revenue is the anchor; without a derivable discrete quarter, extract fails.
        let pd = extract_period(&facts, "USD", PeriodBasis::Quarter);
        assert!(pd.is_none(), "no derivable discrete quarter → None");
    }

    #[test]
    fn annual_basis_uses_latest_fy() {
        let facts = facts_json(&[(
            REV,
            &[
                ("2024-01-01", "2024-12-31", 1000.0, "10-K"),
                ("2025-01-01", "2025-12-31", 1200.0, "10-K"),
            ],
        )]);
        let pd = extract_period(&facts, "USD", PeriodBasis::AnnualFy).expect("period");
        assert_eq!(pd.data.revenue, Some(1200.0));
        assert_eq!(pd.label, "FY2025");
    }

    #[test]
    fn period_basis_serde_lowercase() {
        assert_eq!(
            serde_json::to_string(&PeriodBasis::Quarter).unwrap(),
            "\"quarter\""
        );
        let b: PeriodBasis = serde_json::from_str("\"semi\"").unwrap();
        assert_eq!(b, PeriodBasis::SemiAnnual);
    }
}
