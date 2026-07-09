//! SEC EDGAR XBRL data pull (stub).
//!
//! This module provides the interface for fetching XBRL-formatted financial
//! data from the SEC EDGAR system. The current implementation returns
//! placeholder data; actual HTTP-based XBRL ingestion will replace it.

use crate::extract::{placeholder_result, ExtractionResult, ExtractError};

/// Fetch structured financial data from SEC EDGAR XBRL for the given ticker.
///
/// This is a **stub** — it returns placeholder data without making any
/// network requests. Actual EDGAR XBRL fetching (via the SEC's REST API or
/// direct XBRL instance document parsing) will replace this implementation
/// in a future phase.
pub fn fetch_xbrl(ticker: &str) -> Result<ExtractionResult, ExtractError> {
    //   1. Look up CIK from ticker via SEC companyfacts API.
    //   2. Fetch XBRL instance document or companyfacts JSON.
    //   3. Map GAAP/IFRS concepts to the standard financial line items.
    //   4. Return populated ExtractionResult.
    Ok(placeholder_result(ticker))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fetch_xbrl_returns_ok() {
        let result = fetch_xbrl("AAPL").expect("stub fetch should succeed");
        assert_eq!(result.currency, "USD");
        assert!(!result.years_found.is_empty());
        assert!(result.income_statement.contains_key("revenue"));
        assert!(result.balance_sheet.contains_key("cash"));
        assert!(result.cash_flow_statement.contains_key("cfo"));
    }

    #[test]
    fn test_fetch_xbrl_different_tickers() {
        // Stub should work for any ticker — data is not network-dependent.
        for ticker in &["MSFT", "GOOGL", "NVDA"] {
            let result = fetch_xbrl(ticker).unwrap_or_else(|e| {
                panic!("fetch_xbrl({ticker}) failed: {e}")
            });
            assert!(
                !result.income_statement.is_empty(),
                "{ticker}: income_statement should not be empty"
            );
        }
    }
}
