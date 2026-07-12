//! LTM (last-twelve-months) extraction from SEC XBRL companyfacts.
//!
//! Real investment-banking comps are LTM-based, not last-fiscal-year: LTM stitches
//! the latest annual FY with the most recent interim year-to-date, less the
//! prior-year same interim —
//!   `LTM(flow) = FY + latest_YTD − prior_year_YTD`.
//! Balance-sheet (instant) items use the **latest** reported point-in-time value
//! (a fresh 10-Q beats the older 10-K). When no usable interim exists, each flow
//! gracefully falls back to the annual FY value (`used_interim=false`).
//!
//! Isolated from the gated annual `parse_xbrl_to_raw` path — additive, opt-in.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::xbrl::xbrl_tag_map;

/// LTM figures for one company. Flows are trailing-twelve-months (or annual
/// fallback); balance-sheet items are the latest reported instant.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LtmData {
    pub currency: String,
    /// Latest period end used (YYYY-MM-DD), the "as of" for the LTM window.
    pub as_of: String,
    /// True if any flow used interim stitching (a genuine LTM, not annual fallback).
    pub is_ltm: bool,

    // ── Flows (LTM or annual fallback) ───────────────────────────────────
    pub revenue: Option<f64>,
    pub gross_profit: Option<f64>,
    pub ebit: Option<f64>,
    pub da: Option<f64>,
    pub net_income: Option<f64>,
    pub interest_expense: Option<f64>,
    pub cfo: Option<f64>,
    pub capex: Option<f64>,
    pub dividends_paid: Option<f64>,
    pub buybacks: Option<f64>,

    // ── Balance sheet (latest instant) ───────────────────────────────────
    pub cash: Option<f64>,
    pub long_term_debt: Option<f64>,
    pub short_term_debt: Option<f64>,
    pub total_equity: Option<f64>,
    pub total_assets: Option<f64>,
    pub total_current_assets: Option<f64>,
    pub total_current_liabilities: Option<f64>,
}

/// Days since a fixed epoch for `YYYY-MM-DD` (Howard Hinnant days_from_civil).
fn parse_days(date: &str) -> Option<i64> {
    let mut it = date.splitn(3, '-');
    let y: i64 = it.next()?.parse().ok()?;
    let m: i64 = it.next()?.parse().ok()?;
    let d: i64 = it.next()?.parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

/// The values array for the unit matching `currency` (else the first unit).
fn unit_array<'a>(entry: &'a Value, currency: &str) -> Option<&'a Vec<Value>> {
    let units = entry.get("units")?.as_object()?;
    for (name, vals) in units {
        if name.contains(currency) {
            return vals.as_array();
        }
    }
    units.values().next().and_then(Value::as_array)
}

#[derive(Clone, Copy)]
struct Fact {
    start: Option<i64>,
    end: i64,
    val: f64,
    is_annual: bool, // 10-K / 20-F
    is_quarterly: bool, // 10-Q
}

fn facts_for(
    gaap: &serde_json::Map<String, Value>,
    tags: &[&str],
    currency: &str,
) -> Option<Vec<Fact>> {
    // Among candidate tags, pick the one with the MOST RECENT data — a
    // discontinued tag (e.g. AAPL's old InterestExpense) must not shadow a
    // currently-tagged alternative later in the priority list.
    let mut best: Option<(i64, Vec<Fact>)> = None;
    for tag in tags {
        let entry = match gaap.get(*tag) {
            Some(e) => e,
            None => continue,
        };
        let arr = match unit_array(entry, currency) {
            Some(a) => a,
            None => continue,
        };
        let mut facts = Vec::new();
        for v in arr {
            let end = match v.get("end").and_then(Value::as_str).and_then(parse_days) {
                Some(e) => e,
                None => continue,
            };
            let val = match v.get("val").and_then(Value::as_f64) {
                Some(x) => x,
                None => continue,
            };
            let start = v.get("start").and_then(Value::as_str).and_then(parse_days);
            let form = v.get("form").and_then(Value::as_str).unwrap_or("");
            facts.push(Fact {
                start,
                end,
                val,
                is_annual: form == "10-K" || form == "20-F",
                is_quarterly: form == "10-Q",
            });
        }
        if let Some(max_end) = facts.iter().map(|f| f.end).max() {
            if best.as_ref().map(|(e, _)| max_end > *e).unwrap_or(true) {
                best = Some((max_end, facts));
            }
        }
    }
    best.map(|(_, facts)| facts)
}

/// LTM value for a flow concept: `FY + latest_YTD − prior_year_YTD`, else the
/// latest annual FY value. Returns `(value, used_interim, latest_end_days)`.
fn ltm_flow(
    gaap: &serde_json::Map<String, Value>,
    tags: &[&str],
    currency: &str,
) -> Option<(f64, bool, i64)> {
    let facts = facts_for(gaap, tags, currency)?;

    // Annual FY: duration ~1 year, from a 10-K/20-F. Latest by end.
    let annual = facts
        .iter()
        .filter(|f| {
            f.is_annual
                && f.start
                    .map(|s| {
                        let dur = f.end - s;
                        (330..=400).contains(&dur)
                    })
                    .unwrap_or(false)
        })
        .max_by_key(|f| f.end)?;

    // Latest interim YTD (10-Q): among the latest end date, the longest duration.
    let latest_q_end = facts.iter().filter(|f| f.is_quarterly && f.start.is_some()).map(|f| f.end).max();
    let latest_q_end = match latest_q_end {
        Some(e) if e > annual.end => e,
        _ => return Some((annual.val, false, annual.end)), // no newer interim → annual
    };
    let cur = facts
        .iter()
        .filter(|f| f.is_quarterly && f.end == latest_q_end && f.start.is_some())
        .max_by_key(|f| f.end - f.start.unwrap())?;
    let cur_dur = cur.end - cur.start.unwrap();

    // Prior-year same-length YTD: end ≈ latest − 365d, duration ≈ cur_dur.
    let prior = facts.iter().find(|f| {
        f.start.is_some()
            && (f.end - latest_q_end + 365).abs() <= 20
            && ((f.end - f.start.unwrap()) - cur_dur).abs() <= 20
    });
    match prior {
        Some(p) => Some((annual.val + cur.val - p.val, true, latest_q_end)),
        None => Some((annual.val, false, annual.end)), // can't stitch → annual
    }
}

/// Latest reported instant (point-in-time) value for a balance-sheet concept.
fn latest_instant(
    gaap: &serde_json::Map<String, Value>,
    tags: &[&str],
    currency: &str,
) -> Option<(f64, i64)> {
    let facts = facts_for(gaap, tags, currency)?;
    facts
        .iter()
        .filter(|f| f.start.is_none()) // instants have no start
        .max_by_key(|f| f.end)
        .map(|f| (f.val, f.end))
}

fn sum_opt(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a + b),
        (Some(a), None) | (None, Some(a)) => Some(a),
        (None, None) => None,
    }
}

fn days_to_iso(days: i64) -> String {
    // Inverse civil_from_days.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

/// Extract LTM figures from companyfacts JSON.
pub fn extract_ltm(facts: &Value, currency: &str) -> Option<LtmData> {
    let gaap = facts.pointer("/facts/us-gaap").and_then(Value::as_object)?;
    let tm = xbrl_tag_map();
    let flow = |key: &str| -> Option<(f64, bool, i64)> {
        tm.get(key).and_then(|tags| ltm_flow(gaap, tags, currency))
    };
    let inst = |key: &str| -> Option<(f64, i64)> {
        tm.get(key).and_then(|tags| latest_instant(gaap, tags, currency))
    };

    let mut d = LtmData { currency: currency.to_string(), ..Default::default() };

    // Revenue anchors the company's latest reported period. Every other flow /
    // instant must be within ~1 year of it, else it's a discontinued / stale tag
    // (e.g. AAPL stopped tagging InterestExpense) and is dropped — NEVER blended
    // in as an out-of-date figure masquerading as LTM.
    let rev = flow("revenue")?;
    let anchor = rev.2;
    const STALE_DAYS: i64 = 400;
    let mut latest_end = rev.2;
    let mut any_interim = rev.1;
    d.revenue = Some(rev.0);

    let mut set_flow = |slot: &mut Option<f64>, v: Option<(f64, bool, i64)>| {
        if let Some((val, used, end)) = v {
            if end >= anchor - STALE_DAYS {
                *slot = Some(val);
                any_interim |= used;
                if end > latest_end {
                    latest_end = end;
                }
            }
        }
    };
    set_flow(&mut d.gross_profit, flow("gross_profit"));
    set_flow(&mut d.ebit, flow("ebit"));
    set_flow(&mut d.da, flow("da"));
    set_flow(&mut d.net_income, flow("net_income"));
    set_flow(&mut d.interest_expense, flow("interest_expense"));
    set_flow(&mut d.cfo, flow("cfo"));
    set_flow(&mut d.capex, flow("capex"));
    set_flow(&mut d.dividends_paid, flow("dividends_paid"));
    set_flow(&mut d.buybacks, flow("buybacks"));

    // Balance-sheet instants: latest point-in-time, same staleness guard.
    let inst_guard = |v: Option<(f64, i64)>| -> Option<f64> {
        v.filter(|(_, end)| *end >= anchor - STALE_DAYS).map(|(val, _)| val)
    };
    d.cash = inst_guard(inst("cash"));
    d.long_term_debt = inst_guard(inst("long_term_debt"));
    d.short_term_debt = inst_guard(inst("short_term_debt"));
    d.total_equity = inst_guard(inst("total_equity"));
    d.total_assets = inst_guard(inst("total_assets"));
    d.total_current_assets = inst_guard(inst("total_current_assets"));
    d.total_current_liabilities = inst_guard(inst("total_current_liabilities"));

    // Gross profit fallback = revenue − COGS (COGS must be current too).
    if d.gross_profit.is_none() {
        if let Some((cogs, _, cend)) = flow("cogs") {
            if cend >= anchor - STALE_DAYS {
                d.gross_profit = Some(rev.0 - cogs);
            }
        }
    }

    d.is_ltm = any_interim;
    d.as_of = days_to_iso(latest_end);
    Some(d)
}

/// Total debt (long-term + current portion).
impl LtmData {
    pub fn total_debt(&self) -> Option<f64> {
        sum_opt(self.long_term_debt, self.short_term_debt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dur(start: &str, end: &str, val: f64, form: &str) -> Value {
        serde_json::json!({"start": start, "end": end, "val": val, "form": form})
    }
    fn inst_v(end: &str, val: f64, form: &str) -> Value {
        serde_json::json!({"end": end, "val": val, "form": form})
    }

    #[test]
    fn parse_days_roundtrip() {
        for date in ["2024-12-31", "2025-09-30", "2020-02-29", "2000-01-01"] {
            let d = parse_days(date).unwrap();
            assert_eq!(days_to_iso(d), date, "roundtrip {date}");
        }
        // One year apart = 365 or 366 days.
        let a = parse_days("2024-09-30").unwrap();
        let b = parse_days("2025-09-30").unwrap();
        assert_eq!(b - a, 365);
    }

    #[test]
    fn ltm_stitches_fy_plus_interim_minus_prior() {
        // FY2024 revenue 1000; 9mo-2025 YTD = 800; 9mo-2024 YTD = 700.
        // LTM = 1000 + 800 − 700 = 1100.
        let facts = serde_json::json!({
            "facts": { "us-gaap": {
                "Revenues": { "units": { "USD": [
                    dur("2024-01-01", "2024-12-31", 1000.0, "10-K"),
                    dur("2024-01-01", "2024-09-30", 700.0, "10-Q"),
                    dur("2025-01-01", "2025-09-30", 800.0, "10-Q"),
                    dur("2025-07-01", "2025-09-30", 300.0, "10-Q")  // Q3 quarterly (shorter) — not YTD
                ]}},
                "StockholdersEquity": { "units": { "USD": [
                    inst_v("2024-12-31", 5000.0, "10-K"),
                    inst_v("2025-09-30", 5500.0, "10-Q")  // fresher instant wins
                ]}}
            }}
        });
        let d = extract_ltm(&facts, "USD").expect("ltm");
        assert!((d.revenue.unwrap() - 1100.0).abs() < 1e-6, "LTM revenue {:?}", d.revenue);
        assert!(d.is_ltm);
        assert_eq!(d.as_of, "2025-09-30");
        assert_eq!(d.total_equity, Some(5500.0)); // latest instant, not the 10-K
    }

    #[test]
    fn falls_back_to_annual_when_no_interim() {
        let facts = serde_json::json!({
            "facts": { "us-gaap": {
                "Revenues": { "units": { "USD": [
                    dur("2023-01-01", "2023-12-31", 900.0, "10-K"),
                    dur("2024-01-01", "2024-12-31", 1000.0, "10-K")
                ]}}
            }}
        });
        let d = extract_ltm(&facts, "USD").expect("ltm");
        assert_eq!(d.revenue, Some(1000.0)); // latest FY
        assert!(!d.is_ltm);
        assert_eq!(d.as_of, "2024-12-31");
    }

    #[test]
    fn interim_older_than_fy_uses_annual() {
        // A stale 10-Q that predates the latest 10-K must not override it.
        let facts = serde_json::json!({
            "facts": { "us-gaap": {
                "Revenues": { "units": { "USD": [
                    dur("2024-01-01", "2024-12-31", 1000.0, "10-K"),
                    dur("2024-01-01", "2024-06-30", 480.0, "10-Q")
                ]}}
            }}
        });
        let d = extract_ltm(&facts, "USD").expect("ltm");
        assert_eq!(d.revenue, Some(1000.0));
        assert!(!d.is_ltm);
    }

    #[test]
    fn drops_stale_discontinued_tag() {
        // Revenue is current (FY2024 + 2025 interim); InterestExpense was only
        // ever tagged in FY2019 — it must be DROPPED, not surfaced as LTM.
        let facts = serde_json::json!({
            "facts": { "us-gaap": {
                "Revenues": { "units": { "USD": [
                    dur("2024-01-01", "2024-12-31", 1000.0, "10-K"),
                    dur("2024-01-01", "2024-09-30", 700.0, "10-Q"),
                    dur("2025-01-01", "2025-09-30", 800.0, "10-Q")
                ]}},
                "InterestExpense": { "units": { "USD": [
                    dur("2019-01-01", "2019-12-31", 50.0, "10-K")
                ]}}
            }}
        });
        let d = extract_ltm(&facts, "USD").expect("ltm");
        assert!(d.revenue.is_some());
        assert_eq!(d.interest_expense, None, "stale FY2019 tag must be dropped");
        assert_eq!(d.as_of, "2025-09-30");
    }

    #[test]
    fn no_revenue_yields_none() {
        let facts = serde_json::json!({ "facts": { "us-gaap": {} } });
        assert!(extract_ltm(&facts, "USD").is_none());
    }
}
