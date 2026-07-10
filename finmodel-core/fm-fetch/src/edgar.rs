//! SEC EDGAR CIK lookup and XBRL company facts fetching.
//!
//! Ported from `src/fetcher.py` — `get_cik()` and `fetch_xbrl_facts()`.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// HTTP User-Agent required by SEC EDGAR rate-limiting policy.
/// Matches Python `EDGAR_HEADERS`.
const EDGAR_USER_AGENT: &str = "FinancialModelBot vinit.paul@gmail.com";
const COMPANY_TICKERS_URL: &str = "https://www.sec.gov/files/company_tickers.json";
const COMPANY_FACTS_URL: &str = "https://data.sec.gov/api/xbrl/companyfacts/CIK{cik}.json";

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum EdgarError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Ticker not found in EDGAR: {0}")]
    TickerNotFound(String),
    #[error("CIK format error: {0}")]
    CikFormat(String),
}

// ---------------------------------------------------------------------------
// Company ticker entry from the SEC company_tickers.json index
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TickerEntry {
    cik_str: serde_json::Value, // can be number or string
    ticker: String,
    title: String,
}

// ---------------------------------------------------------------------------
// Company facts top-level structure
// ---------------------------------------------------------------------------

/// Top-level schema of the SEC companyfacts API response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompanyFacts {
    #[serde(rename = "entityName")]
    pub entity_name: String,
    /// SEC CIK number (e.g. 320193).
    pub cik: i64,
    pub facts: FactsContainer,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactsContainer {
    #[serde(rename = "us-gaap")]
    pub us_gaap: Option<HashMap<String, FactEntry>>,
    #[serde(rename = "ifrs-full")]
    pub ifrs_full: Option<HashMap<String, FactEntry>>,
    #[serde(flatten)]
    pub other: HashMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactEntry {
    pub label: Option<String>,
    pub description: Option<String>,
    pub units: HashMap<String, Vec<FactValue>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactValue {
    pub end: String,
    pub val: Option<f64>,
    #[serde(default)]
    pub accn: Option<String>,
    #[serde(default)]
    pub fy: Option<String>,
    #[serde(default)]
    pub fp: Option<String>,
    #[serde(default)]
    pub form: Option<String>,
    #[serde(default)]
    pub filed: Option<String>,
    #[serde(default)]
    pub frame: Option<String>,
}

// ---------------------------------------------------------------------------
// Client helpers
// ---------------------------------------------------------------------------

fn client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .user_agent(EDGAR_USER_AGENT)
        .build()
        .expect("reqwest client build")
}

/// Look up the 10-digit SEC CIK number for a ticker symbol.
///
/// Ported from Python `get_cik()` in `src/fetcher.py`.
pub fn cik_from_ticker(ticker: &str) -> Result<String, EdgarError> {
    let ticker_upper = ticker.trim().to_uppercase();
    let resp = client()
        .get(COMPANY_TICKERS_URL)
        .send()?
        .error_for_status()?;
    let entries: HashMap<String, TickerEntry> = resp.json()?;
    for (_key, entry) in &entries {
        if entry.ticker == ticker_upper {
            let cik = match &entry.cik_str {
                Value::Number(n) => n.as_i64().unwrap_or(0).to_string(),
                Value::String(s) => s.clone(),
                _ => return Err(EdgarError::CikFormat(format!("{:?}", entry.cik_str))),
            };
            return Ok(format!("{:0>10}", cik));
        }
    }
    Err(EdgarError::TickerNotFound(ticker.to_string()))
}

/// Fetch the full XBRL company facts JSON for a given CIK.
///
/// The CIK should be a 10-digit zero-padded string.
/// Ported from Python `fetch_xbrl_facts()` in `src/fetcher.py`.
pub fn fetch_companyfacts(cik: &str) -> Result<CompanyFacts, EdgarError> {
    let url = COMPANY_FACTS_URL.replace("{cik}", cik);
    let resp = client()
        .get(&url)
        .header("Accept", "application/json")
        .send()?
        .error_for_status()?;
    Ok(resp.json()?)
}

/// Fetch the raw JSON value of company facts (for flexible downstream parsing).
pub fn fetch_companyfacts_raw(cik: &str) -> Result<Value, EdgarError> {
    let url = COMPANY_FACTS_URL.replace("{cik}", cik);
    let resp = client()
        .get(&url)
        .header("Accept", "application/json")
        .send()?
        .error_for_status()?;
    Ok(resp.json()?)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore]
    fn test_cik_lookup_known_ticker() {
        let cik = cik_from_ticker("AAPL").expect("AAPL should have a CIK");
        assert_eq!(cik.len(), 10, "CIK must be 10 digits");
        assert_eq!(cik, "0000320193");
    }

    #[test]
    #[ignore]
    fn test_cik_lookup_lowercase() {
        let cik = cik_from_ticker("aapl").expect("lowercase AAPL should work");
        assert_eq!(cik, "0000320193");
    }

    #[test]
    #[ignore]
    fn test_cik_lookup_nonexistent() {
        let result = cik_from_ticker("ZZZZZZZ");
        assert!(result.is_err());
    }

    #[test]
    #[ignore]
    fn test_fetch_companyfacts_aapl() {
        let cik = cik_from_ticker("AAPL").expect("CIK lookup");
        let facts = fetch_companyfacts(&cik).expect("company facts fetch");
        assert_eq!(facts.cik, 320193);
        assert!(facts.facts.us_gaap.is_some());
    }

    #[test]
    #[ignore]
    fn test_fetch_companyfacts_has_revenue() {
        let cik = cik_from_ticker("MSFT").expect("CIK lookup");
        let facts = fetch_companyfacts(&cik).expect("company facts fetch");
        let gaap = facts.facts.us_gaap.expect("us-gaap facts");
        let revenue = gaap.get("RevenueFromContractWithCustomerExcludingAssessedTax")
            .or_else(|| gaap.get("Revenues"))
            .or_else(|| gaap.get("RevenueFromContractWithCustomer"))
            .expect("revenue concept should exist");
        assert!(revenue.units.contains_key("USD"));
    }
}
