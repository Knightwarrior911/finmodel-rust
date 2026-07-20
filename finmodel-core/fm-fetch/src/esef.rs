//! European structured financials via filings.xbrl.org (ESEF annual reports
//! as xBRL-JSON) — the EU/UK/UA leg of the numbers pipeline.
//!
//! Output contract: EDGAR-companyfacts-shaped JSON (`facts.ifrs-full.Tag.
//! units.EUR = [{start, end, val, fy, fp:"FY", form:"ESEF", filed}]`) so the
//! entire downstream pipeline — annual spread, LTM, comps, verification —
//! consumes ESEF filings through the exact code path proven on EDGAR.
//!
//! Design notes (observed live against the API, 2026-07-20):
//! - `/api/entities` (~7.3k rows) is the resolver: only companies that
//!   actually FILED exist there, which kills the GLEIF fund/wrapper-noise
//!   class entirely. The index is small; we page it once and cache 24h.
//! - `/api/entities/{identifier}/filings` lists filings; `json_url` is the
//!   xBRL-JSON artifact. A filing may appear once per language — dedupe.
//! - The filing attribute `period_end` is sometimes junk (a 2022 filing
//!   carrying 2031-01-01); periods are derived from the FACTS, never the
//!   metadata.
//! - Coverage is per-regulator: strong for FR/NL/FI/DK/SE/ES/IT/GB/UA, and
//!   Germany does not feed the aggregator at all — callers surface that
//!   honestly instead of pretending.

use std::time::Duration;

use serde_json::{json, Map, Value};

use crate::cache::SyncCache;

const BASE: &str = "https://filings.xbrl.org";
const UA: &str = "finmodel (research tool; contact via app settings)";

/// One resolvable filer in the index.
#[derive(Clone, Debug)]
pub struct EsefEntity {
    /// API identifier (usually the LEI; UA filers use a local scheme).
    pub identifier: String,
    pub name: String,
}

fn client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(45))
        .user_agent(UA)
        .build()
        .map_err(|e| format!("esef client: {e}"))
}

fn get_json(url: &str) -> Result<Value, String> {
    let resp = client()?
        .get(url)
        .send()
        .map_err(|e| format!("esef transport: {e}"))?
        .error_for_status()
        .map_err(|e| format!("esef http: {e}"))?;
    resp.json().map_err(|e| format!("esef body: {e}"))
}

// ── entity index (cached 24h) ───────────────────────────────────────

fn index_cache() -> &'static SyncCache<String, Vec<(String, String)>> {
    static CACHE: std::sync::LazyLock<SyncCache<String, Vec<(String, String)>>> =
        std::sync::LazyLock::new(|| SyncCache::new(2, Duration::from_secs(24 * 3600)));
    &CACHE
}

/// Download the full entity index (paged). ~7.3k rows — one bounded loop.
fn entities_index() -> Result<Vec<(String, String)>, String> {
    if let Some(v) = index_cache().get(&"idx".to_string()) {
        return Ok(v);
    }
    let mut out: Vec<(String, String)> = Vec::new();
    let mut page = 1usize;
    loop {
        let url = format!("{BASE}/api/entities?page%5Bsize%5D=500&page%5Bnumber%5D={page}");
        let v = get_json(&url)?;
        let data = v["data"].as_array().cloned().unwrap_or_default();
        if data.is_empty() {
            break;
        }
        for e in &data {
            let name = e["attributes"]["name"].as_str().unwrap_or("").trim();
            let ident = e["attributes"]["identifier"].as_str().unwrap_or("").trim();
            if !name.is_empty() && !ident.is_empty() {
                out.push((ident.to_string(), name.to_string()));
            }
        }
        page += 1;
        if page > 40 {
            break; // hard bound: the index is ~15 pages today
        }
    }
    if out.is_empty() {
        return Err("esef entity index came back empty".into());
    }
    index_cache().insert("idx".to_string(), out.clone());
    Ok(out)
}

/// Resolve a company name (or a 20-char LEI, passed through) against the
/// filings index. Exact case-insensitive match wins; else the shortest name
/// containing the query (the parent usually has the shortest legal name —
/// "Fiskars Oyj Abp" beats "Fiskars Finance Oy").
pub fn resolve_esef_entity(query: &str) -> Result<EsefEntity, String> {
    let q = query.trim();
    if q.len() == 20 && q.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Ok(EsefEntity {
            identifier: q.to_uppercase(),
            name: q.to_uppercase(),
        });
    }
    let ql = q.to_lowercase();
    let index = entities_index()?;
    if let Some((id, name)) = index.iter().find(|(_, n)| n.to_lowercase() == ql) {
        return Ok(EsefEntity {
            identifier: id.clone(),
            name: name.clone(),
        });
    }
    index
        .iter()
        .filter(|(_, n)| n.to_lowercase().contains(&ql))
        .min_by_key(|(_, n)| n.len())
        .map(|(id, name)| EsefEntity {
            identifier: id.clone(),
            name: name.clone(),
        })
        .ok_or_else(|| {
            format!(
                "\"{q}\" isn't in the European filings index — coverage varies by country (Germany doesn't feed it). Try the exact legal name, or the US listing if one exists."
            )
        })
}

// ── filings ─────────────────────────────────────────────────────────

/// The newest `max` distinct filings with an xBRL-JSON artifact, one per
/// (rough) period. Language variants of the same report are deduped by the
/// URL's `{identifier}/{date}` prefix.
fn entity_filing_urls(identifier: &str, max: usize) -> Result<Vec<(String, String)>, String> {
    let v = get_json(&format!("{BASE}/api/entities/{identifier}/filings"))?;
    let mut rows: Vec<(String, String, String)> = v["data"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|f| {
                    let a = &f["attributes"];
                    let json_url = a["json_url"].as_str()?.trim();
                    if json_url.is_empty() {
                        return None;
                    }
                    let filed = a["date_added"].as_str().unwrap_or("").chars().take(10).collect::<String>();
                    // Dedupe key: everything before the final path segment —
                    // language variants share it.
                    let prefix = json_url.rsplit_once('/').map(|(p, _)| p.to_string())?;
                    Some((prefix, json_url.to_string(), filed))
                })
                .collect()
        })
        .unwrap_or_default();
    // Newest first by the date embedded in the prefix (path carries the
    // period); stable fallback on filed date.
    rows.sort_by(|a, b| b.0.cmp(&a.0).then(b.2.cmp(&a.2)));
    rows.dedup_by(|a, b| a.0 == b.0);
    Ok(rows
        .into_iter()
        .take(max)
        .map(|(_, url, filed)| (format!("{BASE}{url}"), filed))
        .collect())
}

// ── conversion (pure) ───────────────────────────────────────────────

/// Convert one xBRL-JSON (OIM) document's facts into companyfacts-style
/// entries, merged into `facts_out` under `facts_out[tag].units[cur]`.
///
/// Only undimensioned `ifrs-full:*` facts survive: a fact carrying any
/// dimension beyond concept/entity/period/unit/language is a segment or
/// member slice, not a consolidated total — including it would corrupt the
/// spread with partial numbers.
pub fn merge_xbrl_json_facts(doc: &Value, filed: &str, facts_out: &mut Map<String, Value>) {
    let Some(facts) = doc.get("facts").and_then(|f| f.as_object()) else {
        return;
    };
    const BASE_DIMS: [&str; 5] = ["concept", "entity", "period", "unit", "language"];
    for fact in facts.values() {
        let Some(dims) = fact.get("dimensions").and_then(|d| d.as_object()) else {
            continue;
        };
        if !dims.keys().all(|k| BASE_DIMS.contains(&k.as_str())) {
            continue; // dimensional slice, never a consolidated total
        }
        let Some(concept) = dims.get("concept").and_then(|c| c.as_str()) else {
            continue;
        };
        let Some(tag) = concept.strip_prefix("ifrs-full:") else {
            continue;
        };
        // Money units only ("iso4217:EUR" → "EUR"); shares/pure need no
        // currency handling here and EPS arrives as iso4217:EUR/xbrli:shares.
        let unit_raw = dims.get("unit").and_then(|u| u.as_str()).unwrap_or("");
        let cur = if let Some(c) = unit_raw.strip_prefix("iso4217:") {
            if let Some((num, den)) = c.split_once('/') {
                if den.ends_with("shares") {
                    format!("{num}/shares")
                } else {
                    continue;
                }
            } else {
                c.to_string()
            }
        } else if unit_raw == "xbrli:shares" || unit_raw.ends_with(":shares") {
            "shares".to_string()
        } else {
            continue;
        };
        let Some(val) = fact
            .get("value")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<f64>().ok())
        else {
            continue;
        };
        let period = dims.get("period").and_then(|p| p.as_str()).unwrap_or("");
        // Duration "start/end" (ISO datetimes); instant is a bare datetime.
        // OIM period ends are EXCLUSIVE midnights ("…/2026-01-01T00:00:00"
        // means the year ending 2025-12-31) — normalize to the inclusive
        // last day so periods line up with EDGAR's.
        let (start, end) = match period.split_once('/') {
            Some((s, e)) => (Some(date_of(s)), prev_day(&date_of(e))),
            None => (None, prev_day(&date_of(period))),
        };
        if end.is_empty() {
            continue;
        }
        let fy: i64 = end.get(..4).and_then(|y| y.parse().ok()).unwrap_or(0);
        let mut entry = json!({
            "val": val,
            "end": end,
            "fy": fy,
            "fp": "FY",
            "form": "ESEF",
            "filed": filed,
        });
        if let Some(s) = start {
            entry["start"] = json!(s);
        }
        let tag_obj = facts_out
            .entry(tag.to_string())
            .or_insert_with(|| json!({ "units": {} }));
        let units = tag_obj["units"]
            .as_object_mut()
            .expect("units object created above");
        let arr = units.entry(cur).or_insert_with(|| json!([]));
        let arr = arr.as_array_mut().expect("unit array created above");
        // Restatement semantics: same (start,end) → the later filed wins.
        if let Some(existing) = arr.iter_mut().find(|e| {
            e["end"] == entry["end"] && e.get("start") == entry.get("start")
        }) {
            let old_filed = existing["filed"].as_str().unwrap_or("");
            if filed > old_filed {
                *existing = entry;
            }
            continue;
        }
        arr.push(entry);
    }
}

/// `YYYY-MM-DD` from an OIM ISO datetime (`2026-01-01T00:00:00`).
fn date_of(dt: &str) -> String {
    dt.chars().take(10).collect()
}

/// The day before `YYYY-MM-DD` (OIM period ends are exclusive midnights).
/// Pure civil-date arithmetic — no chrono dependency.
fn prev_day(date: &str) -> String {
    let mut it = date.splitn(3, '-');
    let (Some(y), Some(m), Some(d)) = (it.next(), it.next(), it.next()) else {
        return String::new();
    };
    let (Ok(mut y), Ok(mut m), Ok(mut d)) = (y.parse::<i64>(), m.parse::<u32>(), d.parse::<u32>())
    else {
        return String::new();
    };
    if d > 1 {
        d -= 1;
    } else if m > 1 {
        m -= 1;
        d = days_in_month(y, m);
    } else {
        y -= 1;
        m = 12;
        d = 31;
    }
    format!("{y:04}-{m:02}-{d:02}")
}

fn days_in_month(y: i64, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 {
                29
            } else {
                28
            }
        }
    }
}

// ── entry point ─────────────────────────────────────────────────────

/// Fetch a company's European annual filings as EDGAR-companyfacts-shaped
/// JSON. `query` is a company name or LEI. Reads up to the 3 newest filings
/// (each carries current + prior year, so this typically yields 4+ years).
pub fn fetch_esef_companyfacts(query: &str) -> Result<Value, String> {
    let entity = resolve_esef_entity(query)?;
    let urls = entity_filing_urls(&entity.identifier, 3)?;
    if urls.is_empty() {
        return Err(format!(
            "{} is in the filings index but has no machine-readable annual report yet.",
            entity.name
        ));
    }
    let mut ifrs: Map<String, Value> = Map::new();
    let mut fetched = 0usize;
    for (url, filed) in &urls {
        match get_json(url) {
            Ok(doc) => {
                merge_xbrl_json_facts(&doc, filed, &mut ifrs);
                fetched += 1;
            }
            Err(_) => continue, // one bad artifact never sinks the set
        }
    }
    if fetched == 0 || ifrs.is_empty() {
        return Err(format!(
            "couldn't read any of {}'s filing artifacts — try again shortly.",
            entity.name
        ));
    }
    Ok(json!({
        "entityName": entity.name,
        "facts": { "ifrs-full": Value::Object(ifrs) },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A trimmed real-shape OIM document (Fiskars-style, observed live):
    /// value strings, exclusive-midnight periods, iso4217 units, one
    /// dimensional fact that MUST be skipped.
    fn oim_doc() -> Value {
        json!({
            "documentInfo": { "documentType": "https://xbrl.org/2021/xbrl-json" },
            "facts": {
                "f1": { "value": "18563000", "decimals": 0, "dimensions": {
                    "concept": "ifrs-full:Revenue", "entity": "scheme:LEI1",
                    "period": "2025-01-01T00:00:00/2026-01-01T00:00:00",
                    "unit": "iso4217:EUR" } },
                "f2": { "value": "18482000", "decimals": 0, "dimensions": {
                    "concept": "ifrs-full:Revenue", "entity": "scheme:LEI1",
                    "period": "2024-01-01T00:00:00/2025-01-01T00:00:00",
                    "unit": "iso4217:EUR" } },
                "f3": { "value": "9999000", "decimals": 0, "dimensions": {
                    "concept": "ifrs-full:Revenue", "entity": "scheme:LEI1",
                    "period": "2025-01-01T00:00:00/2026-01-01T00:00:00",
                    "unit": "iso4217:EUR",
                    "ifrs-full:SegmentsAxis": "x:AmericasMember" } },
                "f4": { "value": "70000000", "decimals": 0, "dimensions": {
                    "concept": "ifrs-full:Assets", "entity": "scheme:LEI1",
                    "period": "2026-01-01T00:00:00",
                    "unit": "iso4217:EUR" } },
                "f5": { "value": "1.42", "decimals": 2, "dimensions": {
                    "concept": "ifrs-full:DilutedEarningsLossPerShare",
                    "entity": "scheme:LEI1",
                    "period": "2025-01-01T00:00:00/2026-01-01T00:00:00",
                    "unit": "iso4217:EUR/xbrli:shares" } },
                "f6": { "value": "not-a-number", "dimensions": {
                    "concept": "ifrs-full:Revenue", "entity": "scheme:LEI1",
                    "period": "2025-01-01T00:00:00/2026-01-01T00:00:00",
                    "unit": "iso4217:EUR" } },
                "f7": { "value": "n/a", "dimensions": {
                    "concept": "esef:SomethingLocal", "entity": "scheme:LEI1",
                    "period": "2025-01-01T00:00:00/2026-01-01T00:00:00",
                    "unit": "iso4217:EUR" } }
            }
        })
    }

    #[test]
    fn converts_undimensioned_ifrs_facts_to_companyfacts_shape() {
        let mut out = Map::new();
        merge_xbrl_json_facts(&oim_doc(), "2026-03-01", &mut out);
        // Two revenue years (segment slice + junk value skipped).
        let rev = out["Revenue"]["units"]["EUR"].as_array().unwrap();
        assert_eq!(rev.len(), 2, "{rev:?}");
        let newest = rev.iter().find(|e| e["end"] == "2025-12-31").unwrap();
        // Exclusive midnight normalized to the inclusive last day.
        assert_eq!(newest["start"], "2025-01-01");
        assert_eq!(newest["val"], 18563000.0);
        assert_eq!(newest["fp"], "FY");
        assert_eq!(newest["form"], "ESEF");
        assert_eq!(newest["fy"], 2025);
        // Instant (balance sheet): end only, no start.
        let assets = out["Assets"]["units"]["EUR"].as_array().unwrap();
        assert_eq!(assets[0]["end"], "2025-12-31");
        assert!(assets[0].get("start").is_none());
        // EPS keeps the per-share unit key EDGAR uses.
        assert!(out["DilutedEarningsLossPerShare"]["units"]["EUR/shares"].is_array());
        // Non-ifrs concepts never leak.
        assert!(!out.contains_key("SomethingLocal"));
    }

    #[test]
    fn restatement_later_filed_wins_and_duplicates_never_stack() {
        let mut out = Map::new();
        merge_xbrl_json_facts(&oim_doc(), "2026-03-01", &mut out);
        // The NEXT year's filing restates FY2025 revenue.
        let restated = json!({ "facts": { "g1": { "value": "18600000", "dimensions": {
            "concept": "ifrs-full:Revenue", "entity": "scheme:LEI1",
            "period": "2025-01-01T00:00:00/2026-01-01T00:00:00",
            "unit": "iso4217:EUR" } } } });
        merge_xbrl_json_facts(&restated, "2027-03-01", &mut out);
        let rev = out["Revenue"]["units"]["EUR"].as_array().unwrap();
        assert_eq!(rev.len(), 2, "no duplicate rows: {rev:?}");
        let fy25 = rev.iter().find(|e| e["end"] == "2025-12-31").unwrap();
        assert_eq!(fy25["val"], 18600000.0, "later filed wins");
        // An OLDER filing arriving later never downgrades.
        let stale = json!({ "facts": { "h1": { "value": "1", "dimensions": {
            "concept": "ifrs-full:Revenue", "entity": "scheme:LEI1",
            "period": "2025-01-01T00:00:00/2026-01-01T00:00:00",
            "unit": "iso4217:EUR" } } } });
        merge_xbrl_json_facts(&stale, "2025-01-01", &mut out);
        let fy25 = out["Revenue"]["units"]["EUR"].as_array().unwrap()
            .iter().find(|e| e["end"] == "2025-12-31").unwrap()["val"].clone();
        assert_eq!(fy25, 18600000.0);
    }

    #[test]
    fn prev_day_handles_month_year_and_leap_boundaries() {
        assert_eq!(prev_day("2026-01-01"), "2025-12-31");
        assert_eq!(prev_day("2025-03-01"), "2025-02-28");
        assert_eq!(prev_day("2024-03-01"), "2024-02-29"); // leap
        assert_eq!(prev_day("2025-07-15"), "2025-07-14");
    }

    /// LIVE (network): resolve a Finnish filer by name, fetch its ESEF
    /// filings, and prove an EUR Revenue annual series comes out in
    /// companyfacts shape. Run:
    /// cargo test -p fm-fetch esef -- --ignored --nocapture
    #[test]
    #[ignore]
    fn live_esef_fiskars_revenue() {
        let facts = fetch_esef_companyfacts("Fiskars Oyj Abp").expect("fetch");
        assert_eq!(facts["entityName"], "Fiskars Oyj Abp");
        let rev = facts["facts"]["ifrs-full"]["Revenue"]["units"]["EUR"]
            .as_array()
            .expect("EUR revenue series");
        assert!(!rev.is_empty());
        println!(
            "Fiskars revenue years: {:?}",
            rev.iter().map(|e| e["end"].clone()).collect::<Vec<_>>()
        );
    }
}
