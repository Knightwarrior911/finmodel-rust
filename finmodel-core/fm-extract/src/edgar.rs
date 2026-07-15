//! SEC EDGAR XBRL data pull.
//!
//! Fetches XBRL-formatted financial data from the SEC EDGAR system.
//! Uses fm-fetch for CIK lookup and companyfacts retrieval, then
//! parses with the xbrl module.

use fm_fetch::edgar::{cik_from_ticker, fetch_companyfacts_raw};
use crate::extract::{ExtractionResult, ExtractError};
use crate::xbrl;

/// Fetch structured financial data from SEC EDGAR XBRL for the given ticker.
///
/// Uses live SEC API calls and returns `Err` on any network/lookup failure —
/// never a placeholder. Non-US tickers not in EDGAR should use the PDF path
/// ([`fetch_non_us_filing`]) instead.
pub fn fetch_xbrl(ticker: &str) -> Result<ExtractionResult, ExtractError> {
    fetch_xbrl_with_provenance(ticker).map(|(r, _)| r)
}

/// Fetch companyfacts JSON + detected reporting currency (one HTTP call).
fn companyfacts_for(ticker: &str) -> Result<(serde_json::Value, String), ExtractError> {
    let cik = cik_from_ticker(ticker).map_err(|_| {
        ExtractError::Other(format!(
            "{ticker} not found in SEC EDGAR (non-US?) — use the PDF extraction path"
        ))
    })?;
    let facts = fetch_companyfacts_raw(&cik)
        .map_err(|e| ExtractError::Other(format!("XBRL fetch failed for {ticker}: {e}")))?;
    let currency = detect_currency(&facts).unwrap_or_else(|| "USD".to_string());
    Ok((facts, currency))
}

/// Build the annual `ExtractionResult` + tag provenance from fetched facts.
fn build_result(
    ticker: &str,
    facts: &serde_json::Value,
    currency: &str,
) -> Result<(ExtractionResult, std::collections::HashMap<String, String>), ExtractError> {
    let (parsed, prov) = xbrl::parse_xbrl_to_raw_with_provenance(facts, 3, currency)
        .map_err(|e| ExtractError::Other(format!("XBRL parse error for {ticker}: {e}")))?;
    let years = detect_years(&parsed).unwrap_or_else(|| {
        // Fallback: last full year and the two prior (labels track the clock).
        let latest = crate::current_year() - 1;
        vec![
            (latest - 2).to_string(),
            (latest - 1).to_string(),
            latest.to_string(),
        ]
    });
    let result = ExtractionResult {
        currency: currency.to_string(),
        years_found: years,
        income_statement: parsed.is,
        balance_sheet: parsed.bs,
        cash_flow_statement: parsed.cfs,
        notes: parsed.notes,
        confidence: 0.95,
        discrepancies: vec![],
    };
    Ok((result, prov))
}

/// Fetch a company's LTM (last-twelve-months) figures. `None` when no usable revenue.
pub fn fetch_ltm(ticker: &str) -> Result<Option<crate::ltm::LtmData>, ExtractError> {
    let (facts, ccy) = companyfacts_for(ticker)?;
    Ok(crate::ltm::extract_ltm(&facts, &ccy))
}

/// Like [`fetch_xbrl`] but also returns a `canonical_key → matched us-gaap tag` map.
pub fn fetch_xbrl_with_provenance(
    ticker: &str,
) -> Result<(ExtractionResult, std::collections::HashMap<String, String>), ExtractError> {
    let (facts, ccy) = companyfacts_for(ticker)?;
    build_result(ticker, &facts, &ccy)
}

/// One companyfacts download → annual extraction + tag provenance + LTM figures.
/// The efficient path for consumers (e.g. the benchmark) that need both bases.
pub fn fetch_xbrl_bundle(
    ticker: &str,
) -> Result<
    (
        ExtractionResult,
        std::collections::HashMap<String, String>,
        Option<crate::ltm::LtmData>,
        serde_json::Value,
    ),
    ExtractError,
> {
    let (facts, ccy) = companyfacts_for(ticker)?;
    let (result, prov) = build_result(ticker, &facts, &ccy)?;
    let ltm = crate::ltm::extract_ltm(&facts, &ccy);
    Ok((result, prov, ltm, facts))
}

/// Detect the dominant reporting currency from companyfacts (any taxonomy).
/// Picks the most frequent 3-letter ISO currency-code unit (USD, EUR, TWD, …),
/// so a foreign filer with a few incidental USD facts still resolves correctly.
fn detect_currency(facts: &serde_json::Value) -> Option<String> {
    for tax in ["us-gaap", "ifrs-full"] {
        let obj = match facts.pointer(&format!("/facts/{tax}")).and_then(|v| v.as_object()) {
            Some(o) if !o.is_empty() => o,
            _ => continue,
        };
        let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for (_name, entry) in obj {
            if let Some(units) = entry.get("units").and_then(|u| u.as_object()) {
                for unit_name in units.keys() {
                    let code = unit_name.split('/').next().unwrap_or("");
                    if code.len() == 3 && code.chars().all(|c| c.is_ascii_uppercase()) {
                        *counts.entry(code.to_string()).or_default() += 1;
                    }
                }
            }
        }
        if let Some((ccy, _)) = counts.into_iter().max_by_key(|(_, c)| *c) {
            return Some(ccy);
        }
    }
    None
}

/// Detect the fiscal years found in the parsed data (labels track the current
/// calendar year, not a hardcoded constant).
fn detect_years(parsed: &xbrl::ParsedXbrlData) -> Option<Vec<String>> {
    detect_years_at(crate::current_year(), parsed)
}

/// [`detect_years`] with the reference year injected (testable). The latest
/// fiscal year is the last full year (`year - 1`); labels count back from it.
fn detect_years_at(year: i32, parsed: &xbrl::ParsedXbrlData) -> Option<Vec<String>> {
    // Look at the income statement revenue array length
    let rev_len = parsed.is.get("revenue")?.len();
    if rev_len == 0 {
        return None;
    }
    let latest_fy = year - 1;
    Some(
        (0..rev_len)
            .map(|i| format!("{}", latest_fy as usize - rev_len + 1 + i))
            .collect(),
    )
}

/// Fetch financial data for a non-US company by discovering and extracting its annual report PDF.
///
/// Discovery order (7.3):
/// 1. Authoritative regulator-site candidates (`site:{regulator}` via
///    `fm_fetch::jurisdiction`), tried first when the ticker suffix maps to an
///    indexed filing database (BSE, HKEX, ASX, EDINET, SGX).
/// 2. Generic DuckDuckGo annual-report discovery (`fm_fetch::discovery`).
/// 3. Honest error naming what was tried.
///
/// After extracting the annual filing, if a newer interim (half-year /
/// quarterly) report is discovered its balance-sheet items overlay the annual
/// figures (income statement is kept from the annual). See
/// [`crate::report::ExtractedFinancials::overlay_interim_bs`].
pub fn fetch_non_us_filing(
    company_name: &str,
    ticker: &str,
    periods: &[String],
    year: Option<i32>,
    llm_cfg: Option<&super::llm::LlmConfig>,
) -> Result<super::ExtractionResult, super::ExtractError> {
    // Step 1: Discover the annual report PDF (regulator → DDG → honest error).
    let (pdf_url, tried) = discover_non_us_pdf(company_name, ticker, year)?;

    // Step 2: Download PDF.
    let pdf_path = download_to_temp(&pdf_url)?;

    // Step 3: Extract financials from PDF.
    let mut result = super::extract::extract_financials_from_pdf(
        &pdf_path.to_string_lossy(),
        periods,
        ticker,
        llm_cfg,
    )?;
    let _ = std::fs::remove_file(&pdf_path);

    // Step 4: Overlay a fresher interim balance sheet when one is available.
    let _ = tried; // (retained for the error path)
    if let Some(interim) = discover_and_extract_interim(company_name, ticker, year) {
        overlay_interim_bs_into_result(&mut result, &interim);
    }

    Ok(result)
}

/// Discover the annual-report PDF URL, trying authoritative regulator sites
/// first, then generic DDG. On total failure returns an error naming what was
/// attempted.
fn discover_non_us_pdf(
    company_name: &str,
    ticker: &str,
    year: Option<i32>,
) -> Result<(String, Vec<String>), super::ExtractError> {
    let mut tried: Vec<String> = Vec::new();

    // 1. Regulator-site candidates (authoritative filing databases).
    let reg_queries = fm_fetch::jurisdiction::regulator_candidates(company_name, ticker, year);
    if !reg_queries.is_empty() {
        if let Some(site) = fm_fetch::jurisdiction::regulator_site(ticker) {
            tried.push(format!("regulator site:{site}"));
        }
        if let Ok(url) = fm_fetch::discovery::find_annual_report_pdf_url_with_queries(
            company_name,
            ticker,
            &reg_queries,
        ) {
            return Ok((url, tried));
        }
    }

    // 2. Generic DuckDuckGo annual-report discovery.
    tried.push("DuckDuckGo annual-report search".to_string());
    match fm_fetch::discovery::find_annual_report_pdf_url(company_name, ticker, year) {
        Ok(url) => Ok((url, tried)),
        Err(e) => Err(super::ExtractError::Other(format!(
            "no annual report PDF found for {company_name} ({ticker}); tried: {}. last error: {e}",
            tried.join(", ")
        ))),
    }
}

/// Best-effort discovery + regex extraction of a newer interim (half-year /
/// quarterly) report. Returns `None` (never an error) so a missing interim
/// never fails the annual path. Live/network path.
fn discover_and_extract_interim(
    company_name: &str,
    ticker: &str,
    year: Option<i32>,
) -> Option<crate::report::ExtractedFinancials> {
    let y = year.unwrap_or(2025);
    let queries = [
        format!("{company_name} interim report {y} filetype:pdf"),
        format!("{company_name} half-year report {y} PDF"),
        format!("{company_name} quarterly report {y} PDF"),
    ];
    let url = fm_fetch::discovery::find_annual_report_pdf_url_with_queries(
        company_name,
        ticker,
        &queries,
    )
    .ok()?;
    let pdf_path = download_to_temp(&url).ok()?;
    let text = super::extract::extract_pdf_text(&pdf_path.to_string_lossy()).ok();
    let _ = std::fs::remove_file(&pdf_path);
    let text = text?;
    // Only overlay a genuinely non-annual (fresher interim) filing.
    if crate::report::filing_type(&text) == "annual" {
        return None;
    }
    let y_str = year.map(|y| y.to_string()).unwrap_or_default();
    Some(crate::report::extract_financials(&text, company_name, &y_str, &url))
}

/// Overlay fresher interim balance-sheet values onto the latest column of an
/// [`ExtractionResult`]'s balance sheet. Income-statement columns are left
/// untouched (mirrors `_overlay_interim_bs`). Pure — unit-tested.
fn overlay_interim_bs_into_result(
    result: &mut super::ExtractionResult,
    interim: &crate::report::ExtractedFinancials,
) {
    let mapping: [(&str, Option<f64>); 6] = [
        ("cash", interim.cash),
        ("total_assets", interim.total_assets),
        ("total_equity", interim.total_equity),
        ("long_term_debt", interim.total_debt),
        ("goodwill", interim.goodwill),
        ("short_term_investments", interim.short_term_investments),
    ];
    for (key, val) in mapping {
        if let Some(v) = val {
            let col = result
                .balance_sheet
                .entry(key.to_string())
                .or_default();
            match col.last_mut() {
                Some(last) => *last = Some(v),
                None => col.push(Some(v)),
            }
        }
    }
}

/// Download a PDF URL to a temp file.
fn download_to_temp(pdf_url: &str) -> Result<std::path::PathBuf, super::ExtractError> {
    let config = fm_fetch::pdf::DownloadConfig {
        url: pdf_url.to_string(),
        output_path: None,
        user_agent: Some("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36".into()),
    };
    fm_fetch::pdf::download_pdf(&config)
        .map_err(|e| super::ExtractError::Other(format!("PDF download failed: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore]
    fn test_fetch_xbrl_errors_for_non_us() {
        // Non-US / bogus ticker must return Err — never fabricate placeholder data
        // that could masquerade as a real extraction.
        let result = fetch_xbrl("NONUSXYZ");
        assert!(result.is_err(), "non-US ticker should return Err, not placeholder");
    }

    #[test]
    fn test_detect_currency_from_empty() {
        let val = serde_json::json!({});
        assert_eq!(detect_currency(&val), None);
    }

    #[test]
    fn test_detect_currency_from_usd() {
        let val = serde_json::json!({
            "facts": {
                "us-gaap": {
                    "Revenue": {
                        "units": {
                            "USD": [{"end": "2024-12-31", "val": 1000}]
                        }
                    }
                }
            }
        });
        assert_eq!(detect_currency(&val).as_deref(), Some("USD"));
    }

    #[test]
    fn detect_years_track_injected_reference_year() {
        let mut is = fm_types::StatementData::new();
        is.insert("revenue".into(), vec![Some(1.0), Some(2.0), Some(3.0)]);
        let parsed = xbrl::ParsedXbrlData {
            is,
            bs: fm_types::StatementData::new(),
            cfs: fm_types::StatementData::new(),
            notes: std::collections::HashMap::new(),
        };
        // Reference 2027 → latest full FY 2026 → labels 2024/2025/2026.
        let years = detect_years_at(2027, &parsed).expect("years");
        assert_eq!(years, vec!["2024", "2025", "2026"]);
        // Reference 2031 → latest full FY 2030.
        let years = detect_years_at(2031, &parsed).expect("years");
        assert_eq!(years, vec!["2028", "2029", "2030"]);
    }

    #[test]
    fn detect_years_none_without_revenue() {
        let parsed = xbrl::ParsedXbrlData {
            is: fm_types::StatementData::new(),
            bs: fm_types::StatementData::new(),
            cfs: fm_types::StatementData::new(),
            notes: std::collections::HashMap::new(),
        };
        assert_eq!(detect_years_at(2027, &parsed), None);
    }

    #[test]
    fn overlay_interim_bs_into_result_overrides_latest_bs_column() {
        // Annual result with two comparative years on the balance sheet.
        let mut result = crate::extract::placeholder_result("TEST");
        result
            .balance_sheet
            .insert("cash".into(), vec![Some(10.0), Some(12.0)]);
        result
            .balance_sheet
            .insert("total_assets".into(), vec![Some(100.0), Some(110.0)]);

        let interim = crate::report::ExtractedFinancials {
            cash: Some(15.0),
            total_assets: Some(130.0),
            // A key absent from the result is created fresh.
            goodwill: Some(70.0),
            ..Default::default()
        };
        overlay_interim_bs_into_result(&mut result, &interim);

        // Latest (last) column overwritten; prior column untouched.
        assert_eq!(result.balance_sheet.get("cash"), Some(&vec![Some(10.0), Some(15.0)]));
        assert_eq!(
            result.balance_sheet.get("total_assets"),
            Some(&vec![Some(100.0), Some(130.0)])
        );
        // New key seeded as a single-column vec.
        assert_eq!(result.balance_sheet.get("goodwill"), Some(&vec![Some(70.0)]));
    }

    #[test]
    #[ignore] // live network: regulator + DDG discovery
    fn fetch_non_us_filing_live_smoke() {
        let res = fetch_non_us_filing(
            "Sandvik AB",
            "SAND.ST",
            &["2023".to_string(), "2024".to_string()],
            Some(2024),
            None,
        );
        // On the live path this must resolve currency + years or error honestly.
        if let Ok(r) = res {
            assert!(!r.currency.is_empty());
            assert!(!r.years_found.is_empty());
        }
    }
}
