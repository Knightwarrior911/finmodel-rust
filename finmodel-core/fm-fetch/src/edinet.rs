//! Japanese structured financials via the EDINET v2 API — the Japan leg of
//! the numbers pipeline.
//!
//! Output contract: EDGAR-companyfacts-shaped JSON (taxonomy key
//! `ifrs-full`, unit `JPY`, `form: "EDINET-CSV"`) so the downstream pipeline
//! (annual spread, LTM, comps, verification) consumes EDINET filings through
//! the code path proven on EDGAR and ESEF.
//!
//! Access model (observed live, 2026-07-20): every endpoint returns
//! `{"StatusCode": 401, "message": "Access denied due to invalid
//! subscription key…"}` without a key. Registration is free on the EDINET
//! portal; the key travels as the `Subscription-Key` query parameter. All
//! network functions therefore take `api_key`; the pure CSV conversion is
//! fully testable offline.
//!
//! HONESTY NOTE: the TSV column layout is matched BY HEADER NAME (要素ID /
//! コンテキストID / 値 …) rather than position, and the parser returns an
//! empty set — never garbage — when headers don't match. The fixture in the
//! tests is constructed from the documented format, not captured live (the
//! spec portal is session-gated); the `live_edinet_*` test exists for the
//! first run with a real key.

use std::io::Read;
use std::time::Duration;

use serde_json::{json, Map, Value};

const BASE: &str = "https://api.edinet-fsa.go.jp/api/v2";

/// One EDINET document hit from the daily index.
#[derive(Clone, Debug)]
pub struct EdinetDoc {
    pub doc_id: String,
    pub filer_name: String,
    pub sec_code: Option<String>,
    pub period_end: Option<String>,
    pub submitted: String,
}

fn client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(45))
        .user_agent("finmodel (research tool)")
        .build()
        .map_err(|e| format!("edinet client: {e}"))
}

/// List annual securities reports (有価証券報告書: ordinance 010, form 030000)
/// in one day's index whose filer matches `query` (name substring or
/// 4/5-digit securities code).
pub fn search_day(
    date: &str,
    query: &str,
    api_key: &str,
) -> Result<Vec<EdinetDoc>, String> {
    let ql = query.trim().to_lowercase();
    Ok(day_annual_reports(date, api_key)?
        .into_iter()
        .filter(|d| {
            d.filer_name.to_lowercase().contains(&ql)
                || (!ql.is_empty()
                    && d.sec_code.as_deref().map_or(false, |s| s.starts_with(&ql)))
        })
        .collect())
}

fn day_cache() -> &'static crate::cache::SyncCache<String, Vec<EdinetDoc>> {
    static CACHE: std::sync::LazyLock<crate::cache::SyncCache<String, Vec<EdinetDoc>>> =
        std::sync::LazyLock::new(|| {
            crate::cache::SyncCache::new(500, Duration::from_secs(6 * 3600))
        });
    &CACHE
}

/// One day's annual-report index (query-independent, cached 6h) — the
/// backwards scan across companies reuses it instead of re-spending quota.
fn day_annual_reports(date: &str, api_key: &str) -> Result<Vec<EdinetDoc>, String> {
    if let Some(v) = day_cache().get(&date.to_string()) {
        return Ok(v);
    }
    let url = format!(
        "{BASE}/documents.json?date={date}&type=2&Subscription-Key={api_key}"
    );
    let v: Value = client()?
        .get(&url)
        .send()
        .map_err(|e| format!("edinet transport: {e}"))?
        .json()
        .map_err(|e| format!("edinet body: {e}"))?;
    if v["StatusCode"].as_i64() == Some(401) {
        return Err(
            "EDINET rejected the key — check the EDINET API key in Settings → Connections."
                .into(),
        );
    }
    let docs: Vec<EdinetDoc> = v["results"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter(|d| {
                    d["ordinanceCode"].as_str() == Some("010")
                        && d["formCode"].as_str() == Some("030000")
                })
                .filter_map(|d| {
                    Some(EdinetDoc {
                        doc_id: d["docID"].as_str()?.to_string(),
                        filer_name: d["filerName"].as_str().unwrap_or("").to_string(),
                        sec_code: d["secCode"].as_str().map(String::from),
                        period_end: d["periodEnd"].as_str().map(String::from),
                        submitted: d["submitDateTime"].as_str().unwrap_or("").to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    day_cache().insert(date.to_string(), docs.clone());
    Ok(docs)
}

/// Download a document's CSV bundle (type=5: a zip of TSV fact tables) and
/// return every TSV's text content.
pub fn fetch_csv_bundle(doc_id: &str, api_key: &str) -> Result<Vec<String>, String> {
    let url = format!("{BASE}/documents/{doc_id}?type=5&Subscription-Key={api_key}");
    let bytes = client()?
        .get(&url)
        .send()
        .map_err(|e| format!("edinet transport: {e}"))?
        .error_for_status()
        .map_err(|e| format!("edinet http: {e}"))?
        .bytes()
        .map_err(|e| format!("edinet body: {e}"))?;
    let reader = std::io::Cursor::new(bytes.as_ref());
    let mut zip = zip::ZipArchive::new(reader).map_err(|e| format!("edinet zip: {e}"))?;
    let mut out = Vec::new();
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(|e| format!("edinet zip entry: {e}"))?;
        let name = f.name().to_lowercase();
        if !name.ends_with(".csv") && !name.ends_with(".tsv") {
            continue;
        }
        let mut raw = Vec::new();
        f.read_to_end(&mut raw).map_err(|e| format!("edinet read: {e}"))?;
        // EDINET CSVs are UTF-16LE with BOM; fall back to UTF-8.
        let text = decode_utf16_or_utf8(&raw);
        if !text.trim().is_empty() {
            out.push(text);
        }
    }
    Ok(out)
}

fn decode_utf16_or_utf8(raw: &[u8]) -> String {
    if raw.len() >= 2 && raw[0] == 0xFF && raw[1] == 0xFE {
        let units: Vec<u16> = raw[2..]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(raw).into_owned()
    }
}

// ── conversion (pure, offline-testable) ─────────────────────────────

/// Element-id candidates per canonical ifrs-full output tag. EDINET carries
/// both Japan-GAAP (`jppfs_cor:`) and IFRS (`jpigp_cor:`/designated IFRS)
/// element ids; candidates are matched against the local name,
/// case-insensitively, prefix-stripped.
const ELEMENT_MAP: &[(&str, &[&str])] = &[
    ("Revenue", &["NetSales", "Revenue", "RevenueIFRS", "OperatingRevenue1", "NetSalesSummaryOfBusinessResults"]),
    ("ProfitLossFromOperatingActivities", &["OperatingIncome", "OperatingProfitLossIFRS"]),
    ("ProfitLoss", &["ProfitLoss", "NetIncomeLoss", "ProfitLossAttributableToOwnersOfParent", "ProfitLossAttributableToOwnersOfParentIFRS"]),
    ("Assets", &["Assets", "TotalAssetsIFRS", "TotalAssetsSummaryOfBusinessResults"]),
    ("Equity", &["NetAssets", "EquityIFRS", "EquityAttributableToOwnersOfParentIFRS"]),
    ("CashAndCashEquivalents", &["CashAndDeposits", "CashAndCashEquivalentsIFRS", "CashAndCashEquivalents"]),
    ("Borrowings", &["BorrowingsIFRS", "LongTermLoansPayable", "ShortTermLoansPayable"]),
    ("CashFlowsFromUsedInOperatingActivities", &["NetCashProvidedByUsedInOperatingActivities", "CashFlowsFromUsedInOperatingActivitiesIFRS"]),
    // Per-share metrics: without these, the EPS verification identity never
    // fires on Japanese filers. (Candidates cover jppfs and designated-IFRS
    // spellings; unknown names simply never match.)
    ("DilutedEarningsLossPerShare", &["DilutedEarningsPerShare", "DilutedEarningsPerShareIFRS", "DilutedEarningsLossPerShare"]),
    ("BasicEarningsLossPerShare", &["BasicEarningsPerShare", "BasicEarningsPerShareIFRS", "BasicEarningsLossPerShare"]),
    ("AdjustmentsForDepreciationAndAmortisationExpense", &["DepreciationAndAmortizationOpeCF", "DepreciationAndAmortisationIFRS", "DepreciationAndAmortizationSGA"]),
];

/// Parse one EDINET fact TSV into companyfacts entries, merged into
/// `facts_out`. Columns are located BY HEADER NAME; a sheet whose header
/// carries none of the expected names contributes nothing.
///
/// Context discipline: only current-year consolidated totals survive —
/// contexts named `CurrentYearDuration` / `CurrentYearInstant` (optionally
/// with a `_NonConsolidatedMember` suffix REJECTED) — because prior-year
/// figures arrive in their own filings and member contexts are slices.
pub fn merge_edinet_tsv(
    tsv: &str,
    period_end: &str,
    filed: &str,
    facts_out: &mut Map<String, Value>,
) {
    let mut lines = tsv.lines();
    let Some(header) = lines.next() else { return };
    let cols: Vec<&str> = header.split('\t').map(|c| c.trim_matches('"').trim()).collect();
    let find = |names: &[&str]| -> Option<usize> {
        cols.iter().position(|c| names.iter().any(|n| c.contains(n)))
    };
    // Japanese header names per the documented CSV layout (element id,
    // context id, unit id, value).
    let (Some(i_elem), Some(i_ctx), Some(i_val)) = (
        find(&["要素ID"]),
        find(&["コンテキストID"]),
        find(&["値"]),
    ) else {
        return; // unknown sheet layout: contribute nothing, never garbage
    };
    let i_unit = find(&["ユニットID", "単位"]);
    for line in lines {
        let fields: Vec<&str> = line.split('\t').map(|c| c.trim_matches('"').trim()).collect();
        let (Some(elem), Some(ctx), Some(val_s)) = (
            fields.get(i_elem).copied(),
            fields.get(i_ctx).copied(),
            fields.get(i_val).copied(),
        ) else {
            continue;
        };
        // Current-year consolidated totals only.
        let is_duration = ctx == "CurrentYearDuration";
        let is_instant = ctx == "CurrentYearInstant";
        if !is_duration && !is_instant {
            continue;
        }
        if let Some(iu) = i_unit {
            let unit = fields.get(iu).copied().unwrap_or("");
            if !unit.is_empty() && !unit.to_uppercase().contains("JPY") {
                continue;
            }
        }
        let local = elem.rsplit(':').next().unwrap_or(elem);
        let Some((tag, _)) = ELEMENT_MAP.iter().find(|(_, cands)| {
            cands.iter().any(|c| local.eq_ignore_ascii_case(c))
        }) else {
            continue;
        };
        let Ok(val) = val_s.replace(',', "").parse::<f64>() else {
            continue;
        };
        let fy: i64 = period_end.get(..4).and_then(|y| y.parse().ok()).unwrap_or(0);
        let mut entry = json!({
            "val": val,
            "end": period_end,
            "fy": fy,
            "fp": "FY",
            "form": "EDINET-CSV",
            "filed": filed,
        });
        if is_duration {
            // Fiscal years in Japan usually run Apr..Mar; the exact start
            // isn't in the row — derive a nominal one year window.
            if let Some(y) = period_end.get(..4).and_then(|y| y.parse::<i64>().ok()) {
                entry["start"] = json!(format!("{}-{}", y - 1, &period_end[5..]));
            }
        }
        // Per-share tags carry the EDGAR-style compound unit key.
        let unit_key = if tag.contains("EarningsLossPerShare") {
            "JPY/shares"
        } else {
            "JPY"
        };
        let tag_obj = facts_out
            .entry(tag.to_string())
            .or_insert_with(|| json!({ "units": {} }));
        let units = tag_obj["units"].as_object_mut().expect("units created above");
        let arr = units
            .entry(unit_key.to_string())
            .or_insert_with(|| json!([]));
        let arr = arr.as_array_mut().expect("array created above");
        if let Some(existing) = arr.iter_mut().find(|e| e["end"] == entry["end"]) {
            let old = existing["filed"].as_str().unwrap_or("");
            if filed > old {
                *existing = entry;
            }
            continue;
        }
        arr.push(entry);
    }
}

/// Fetch a Japanese company's latest annual report as companyfacts-shaped
/// JSON. Scans backwards day-by-day (bounded to ~400 days, weekends
/// included) for the newest 有価証券報告書 matching `query`.
pub fn fetch_edinet_companyfacts(query: &str, api_key: &str) -> Result<Value, String> {
    if api_key.trim().is_empty() {
        return Err(
            "Japanese filings need a free EDINET API key — add one in Settings → Connections."
                .into(),
        );
    }
    // Annual reports cluster in late June (March fiscal years). Scan recent
    // days newest-first (weekends skipped — EDINET publishes nothing), each
    // day's index cached so a second company lookup reuses the walk instead
    // of re-spending the API quota.
    let today = today_utc();
    let mut probed = 0usize;
    let mut day = today;
    while probed < 400 {
        if is_weekend(&day) {
            day = prev_date(&day);
            continue;
        }
        let hits = search_day(&day, query, api_key)?;
        if let Some(doc) = hits.into_iter().max_by(|a, b| a.submitted.cmp(&b.submitted)) {
            let period_end = doc
                .period_end
                .clone()
                .unwrap_or_else(|| day.clone());
            let tsvs = fetch_csv_bundle(&doc.doc_id, api_key)?;
            let mut ifrs: Map<String, Value> = Map::new();
            for t in &tsvs {
                merge_edinet_tsv(t, &period_end, &doc.submitted.chars().take(10).collect::<String>(), &mut ifrs);
            }
            if ifrs.is_empty() {
                return Err(format!(
                    "found {}'s annual report ({}) but couldn't read structured figures from its CSV bundle.",
                    doc.filer_name, doc.doc_id
                ));
            }
            return Ok(json!({
                "entityName": doc.filer_name,
                "facts": { "ifrs-full": Value::Object(ifrs) },
            }));
        }
        day = prev_date(&day);
        probed += 1;
    }
    Err(format!(
        "no annual securities report matching \"{query}\" in the last ~13 months of EDINET filings."
    ))
}

/// Today (UTC) as `YYYY-MM-DD` — civil-from-days, no chrono.
fn today_utc() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let mut z = secs / 86_400 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    z -= era * 146_097;
    let yoe = (z - z / 1460 + z / 36_524 - z / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = z - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

/// Day-of-week (0 = Sunday) for `YYYY-MM-DD` — Sakamoto's method. EDINET
/// publishes nothing on weekends; skipping them cuts the scan ~29%.
fn is_weekend(date: &str) -> bool {
    let mut it = date.splitn(3, '-');
    let (Some(y), Some(m), Some(d)) = (it.next(), it.next(), it.next()) else {
        return false;
    };
    let (Ok(mut y), Ok(m), Ok(d)) = (y.parse::<i64>(), m.parse::<i64>(), d.parse::<i64>()) else {
        return false;
    };
    const T: [i64; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    if m < 3 {
        y -= 1;
    }
    let dow = (y + y / 4 - y / 100 + y / 400 + T[(m - 1) as usize] + d).rem_euclid(7);
    dow == 0 || dow == 6
}
/// The day before `YYYY-MM-DD` (civil arithmetic, no chrono).
fn prev_date(date: &str) -> String {
    let mut it = date.splitn(3, '-');
    let (Some(y), Some(m), Some(d)) = (it.next(), it.next(), it.next()) else {
        return date.to_string();
    };
    let (Ok(mut y), Ok(mut m), Ok(mut d)) = (y.parse::<i64>(), m.parse::<u32>(), d.parse::<u32>())
    else {
        return date.to_string();
    };
    if d > 1 {
        d -= 1;
    } else if m > 1 {
        m -= 1;
        d = match m {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            _ => {
                if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 {
                    29
                } else {
                    28
                }
            }
        };
    } else {
        y -= 1;
        m = 12;
        d = 31;
    }
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Constructed from the documented CSV layout (headers by name) — NOT a
    /// live capture (the format spec portal is session-gated; the live
    /// ignored test below is the first-key verification path).
    fn tsv() -> String {
        [
            "\"要素ID\"\t\"項目名\"\t\"コンテキストID\"\t\"相対年度\"\t\"連結・個別\"\t\"期間・時点\"\t\"ユニットID\"\t\"単位\"\t\"値\"",
            "\"jppfs_cor:NetSales\"\t\"売上高\"\t\"CurrentYearDuration\"\t\"当期\"\t\"連結\"\t\"期間\"\t\"JPY\"\t\"円\"\t\"45095325000000\"",
            "\"jppfs_cor:NetSales\"\t\"売上高\"\t\"Prior1YearDuration\"\t\"前期\"\t\"連結\"\t\"期間\"\t\"JPY\"\t\"円\"\t\"43300000000000\"",
            "\"jppfs_cor:NetSales\"\t\"売上高\"\t\"CurrentYearDuration_NonConsolidatedMember\"\t\"当期\"\t\"個別\"\t\"期間\"\t\"JPY\"\t\"円\"\t\"20000000000000\"",
            "\"jppfs_cor:OperatingIncome\"\t\"営業利益\"\t\"CurrentYearDuration\"\t\"当期\"\t\"連結\"\t\"期間\"\t\"JPY\"\t\"円\"\t\"5352934000000\"",
            "\"jppfs_cor:Assets\"\t\"総資産\"\t\"CurrentYearInstant\"\t\"当期\"\t\"連結\"\t\"時点\"\t\"JPY\"\t\"円\"\t\"93601350000000\"",
            "\"jppfs_cor:SomethingElse\"\t\"その他\"\t\"CurrentYearDuration\"\t\"当期\"\t\"連結\"\t\"期間\"\t\"JPY\"\t\"円\"\t\"1\"",
        ]
        .join("\n")
    }

    #[test]
    fn tsv_converts_current_year_consolidated_totals_only() {
        let mut out = Map::new();
        merge_edinet_tsv(&tsv(), "2026-03-31", "2026-06-25", &mut out);
        let rev = out["Revenue"]["units"]["JPY"].as_array().unwrap();
        // ONE row: prior-year and non-consolidated contexts are rejected.
        assert_eq!(rev.len(), 1, "{rev:?}");
        assert_eq!(rev[0]["val"], 45_095_325_000_000.0);
        assert_eq!(rev[0]["end"], "2026-03-31");
        assert_eq!(rev[0]["form"], "EDINET-CSV");
        assert_eq!(rev[0]["start"], "2025-03-31");
        // Instant fact: no start.
        let assets = out["Assets"]["units"]["JPY"].as_array().unwrap();
        assert!(assets[0].get("start").is_none());
        // Unmapped elements never leak.
        assert!(!out.contains_key("SomethingElse"));
        // Operating income mapped to the ifrs-full tag downstream expects.
        assert!(out.contains_key("ProfitLossFromOperatingActivities"));
    }

    #[test]
    fn unknown_layout_contributes_nothing() {
        let mut out = Map::new();
        merge_edinet_tsv("colA\tcolB\n1\t2", "2026-03-31", "2026-06-25", &mut out);
        assert!(out.is_empty(), "unknown headers must never produce facts");
    }

    #[test]
    fn keyless_call_fails_with_a_settings_pointer() {
        let err = fetch_edinet_companyfacts("トヨタ自動車", "").unwrap_err();
        assert!(err.contains("Settings"), "{err}");
    }

    /// LIVE (network + EDINET key in EDINET_API_KEY): first-run verification
    /// once a key exists. Run:
    /// EDINET_API_KEY=… cargo test -p fm-fetch live_edinet -- --ignored --nocapture
    #[test]
    #[ignore]
    fn live_edinet_toyota() {
        let key = std::env::var("EDINET_API_KEY").expect("set EDINET_API_KEY");
        let facts = fetch_edinet_companyfacts("トヨタ自動車", &key).expect("fetch");
        let rev = facts["facts"]["ifrs-full"]["Revenue"]["units"]["JPY"]
            .as_array()
            .expect("JPY revenue");
        println!("Toyota revenue rows: {rev:?}");
        assert!(!rev.is_empty());
    }
}
