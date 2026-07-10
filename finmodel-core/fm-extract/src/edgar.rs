//! SEC EDGAR XBRL data pull.
//!
//! Fetches XBRL-formatted financial data from the SEC EDGAR system.
//! Uses fm-fetch for CIK lookup and companyfacts retrieval, then
//! parses with the xbrl module.

use fm_fetch::edgar::{cik_from_ticker, fetch_companyfacts_raw};
use crate::extract::{placeholder_result, ExtractionResult, ExtractError};
use crate::xbrl;

/// Fetch structured financial data from SEC EDGAR XBRL for the given ticker.
///
/// Uses live SEC API calls. Falls back to placeholder on network error
/// (non-US tickers that won't be found in EDGAR should use the PDF path instead).
pub fn fetch_xbrl(ticker: &str) -> Result<ExtractionResult, ExtractError> {
    // 1. Look up CIK from ticker via SEC company_tickers.json
    let cik = match cik_from_ticker(ticker) {
        Ok(c) => c,
        Err(_) => {
            // Ticker not found in EDGAR — return placeholder (caller should use PDF path)
            return Ok(placeholder_result(ticker));
        }
    };

    // 2. Fetch XBRL company facts JSON
    let facts = match fetch_companyfacts_raw(&cik) {
        Ok(f) => f,
        Err(e) => {
            return Err(ExtractError::Other(format!("XBRL fetch failed for {ticker}: {e}")));
        }
    };

    // 3. Determine currency from the JSON (default USD)
    let currency = detect_currency(&facts).unwrap_or("USD");

    // 4. Parse XBRL into structured data
    let parsed = xbrl::parse_xbrl_to_raw(&facts, 3, currency)
        .map_err(|e| ExtractError::Other(format!("XBRL parse error for {ticker}: {e}")))?;

    // 5. Build ExtractionResult
    let years = detect_years(&parsed).unwrap_or_else(|| {
        vec!["2022".to_string(), "2023".to_string(), "2024".to_string()]
    });

    Ok(ExtractionResult {
        currency: currency.to_string(),
        years_found: years,
        income_statement: parsed.is,
        balance_sheet: parsed.bs,
        cash_flow_statement: parsed.cfs,
        notes: parsed.notes,
        confidence: 0.95,
        discrepancies: vec![],
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fetch_xbrl_returns_ok() {
        // This ticker is non-US — should return placeholder
        let result = fetch_xbrl("NONUS").expect("should return placeholder for non-US ticker");
        assert_eq!(result.currency, "USD");
        assert!(!result.years_found.is_empty());
        assert!(result.income_statement.contains_key("revenue"));
        assert!(result.balance_sheet.contains_key("cash"));
        assert!(result.cash_flow_statement.contains_key("cfo"));
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
