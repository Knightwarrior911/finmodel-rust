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
/// Uses live SEC API calls. Falls back to placeholder on network error
/// (non-US tickers that won't be found in EDGAR should use the PDF path instead).
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
    let currency = detect_currency(&facts).unwrap_or("USD").to_string();
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
    let years = detect_years(&parsed)
        .unwrap_or_else(|| vec!["2022".to_string(), "2023".to_string(), "2024".to_string()]);
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
    (ExtractionResult, std::collections::HashMap<String, String>, Option<crate::ltm::LtmData>),
    ExtractError,
> {
    let (facts, ccy) = companyfacts_for(ticker)?;
    let (result, prov) = build_result(ticker, &facts, &ccy)?;
    let ltm = crate::ltm::extract_ltm(&facts, &ccy);
    Ok((result, prov, ltm))
}

/// Try to detect the reporting currency from the companyfacts JSON.
fn detect_currency(facts: &serde_json::Value) -> Option<&str> {
    // Look at the first us-gaap concept with units to find the currency
    let gaap = facts.pointer("/facts/us-gaap")?.as_object()?;
    for (_name, entry) in gaap {
        if let Some(units) = entry.get("units")?.as_object() {
            for unit_name in units.keys() {
                if unit_name.starts_with("USD") || unit_name.starts_with("EUR")
                    || unit_name.starts_with("GBP") || unit_name.starts_with("SEK")
                    || unit_name.starts_with("CHF") || unit_name.starts_with("DKK")
                    || unit_name.starts_with("NOK") || unit_name.starts_with("JPY")
                {
                    // Return just the currency code part (before any suffix like /shares)
                    let ccy = if let Some(idx) = unit_name.find('/') {
                        &unit_name[..idx]
                    } else {
                        unit_name.as_str()
                    };
                    return Some(ccy);
                }
            }
        }
    }
    None
}

/// Detect the fiscal years found in the parsed data.
fn detect_years(parsed: &xbrl::ParsedXbrlData) -> Option<Vec<String>> {
    // Look at the income statement revenue array length
    let rev_len = parsed.is.get("revenue")?.len();
    if rev_len == 0 {
        return None;
    }
    // Compute years based on current date (same logic as Python)
    let today_year = 2026;
    let latest_fy = today_year - 1;
    Some(
        (0..rev_len)
            .map(|i| format!("{}", latest_fy - rev_len + 1 + i))
            .collect()
    )
}

/// Fetch financial data for a non-US company by discovering and extracting its annual report PDF.
///
/// Steps:
/// 1. Discover PDF URL via DDG search + IR page scrape (fm-fetch::discovery)
/// 2. Download PDF to temp file (fm-fetch::pdf)
/// 3. Extract financials via LLM (crate::extract::extract_financials_from_pdf)
/// 4. Return ExtractionResult
pub fn fetch_non_us_filing(
    company_name: &str,
    ticker: &str,
    periods: &[String],
    year: Option<i32>,
) -> Result<super::ExtractionResult, super::ExtractError> {
    // Step 1: Discover PDF URL
    let pdf_url = fm_fetch::discovery::find_annual_report_pdf_url(company_name, ticker, year)
        .map_err(|e| super::ExtractError::Other(format!("PDF discovery failed: {e}")))?;

    // Step 2: Download PDF
    let config = fm_fetch::pdf::DownloadConfig {
        url: pdf_url,
        output_path: None,
        user_agent: Some("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36".into()),
    };
    let pdf_path = fm_fetch::pdf::download_pdf(&config)
        .map_err(|e| super::ExtractError::Other(format!("PDF download failed: {e}")))?;

    // Step 3: Extract financials from PDF
    let result = super::extract::extract_financials_from_pdf(
        &pdf_path.to_string_lossy(),
        periods,
        ticker,
    )?;

    // Step 4: Clean up temp file
    let _ = std::fs::remove_file(&pdf_path);

    Ok(result)
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
        assert_eq!(detect_currency(&val), Some("USD"));
    }
}
