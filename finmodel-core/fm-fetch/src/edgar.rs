//! SEC EDGAR CIK lookup and XBRL company facts fetching.
//!
//! Ported from `src/fetcher.py` — `get_cik()` and `fetch_xbrl_facts()`.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// HTTP User-Agent required by SEC EDGAR rate-limiting policy.
/// Matches Python `EDGAR_HEADERS`.
const EDGAR_USER_AGENT: &str = "FinancialModelBot vinit.paul@gmail.com";
const COMPANY_TICKERS_URL: &str = "https://www.sec.gov/files/company_tickers.json";
const COMPANY_FACTS_URL: &str = "https://data.sec.gov/api/xbrl/companyfacts/CIK{cik}.json";
const SUBMISSIONS_URL: &str = "https://data.sec.gov/submissions/CIK{cik}.json";

/// Shared blocking client — one TLS connection pool for every EDGAR call
/// (replaces the per-call `Client::builder()`). The User-Agent is resolved once
/// at first use, so [`set_edgar_contact`] must run before the first request.
static EDGAR_CLIENT: LazyLock<reqwest::blocking::Client> = LazyLock::new(|| {
    reqwest::blocking::Client::builder()
        .user_agent(edgar_user_agent())
        .build()
        .unwrap_or_else(|_| reqwest::blocking::Client::new())
});

/// Runtime-set EDGAR contact (SEC policy requires a real contact in the UA).
/// `OnceLock`, not `LazyLock`: the value is supplied at runtime, not declaration.
static EDGAR_CONTACT: OnceLock<String> = OnceLock::new();

/// ticker(upper) → 10-digit padded CIK, downloaded once and cached — kills the
/// ~1 MB `company_tickers.json` re-download on every lookup. Populated from a
/// fallible network fetch, so `OnceLock` (only a success is cached), not `LazyLock`.
static TICKER_MAP: OnceLock<HashMap<String, String>> = OnceLock::new();

/// Instant of the last EDGAR request, for the rate-limit gate.
static EDGAR_GATE: LazyLock<Mutex<Instant>> =
    LazyLock::new(|| Mutex::new(Instant::now() - Duration::from_secs(1)));

/// Minimum spacing between EDGAR requests (SEC policy is ≤10 req/s).
const EDGAR_MIN_INTERVAL: Duration = Duration::from_millis(120);

/// Set the EDGAR contact string (e.g. an email). Used in the User-Agent per SEC
/// policy. Call once at startup / command entry, BEFORE the first EDGAR request
/// (the client caches the UA on first use). First value wins.
pub fn set_edgar_contact(contact: String) {
    let _ = EDGAR_CONTACT.set(contact);
}

/// The EDGAR User-Agent: the runtime-set contact, else `FINMODEL_EDGAR_CONTACT`
/// from the environment, else the built-in default.
pub fn edgar_user_agent() -> String {
    let contact = EDGAR_CONTACT
        .get()
        .filter(|c| !c.trim().is_empty())
        .cloned()
        .or_else(|| {
            std::env::var("FINMODEL_EDGAR_CONTACT")
                .ok()
                .filter(|c| !c.trim().is_empty())
        });
    match contact {
        Some(c) => format!("finmodel {}", c.trim()),
        None => EDGAR_USER_AGENT.to_string(),
    }
}

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
    #[error("EDGAR request failed after retries: {0}")]
    Request(String),
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

/// Block until at least [`EDGAR_MIN_INTERVAL`] has passed since the last EDGAR
/// request, then record now as the last request. Serialized via a global mutex.
fn rate_limit() {
    let mut last = EDGAR_GATE.lock().unwrap_or_else(|e| e.into_inner());
    let elapsed = last.elapsed();
    if elapsed < EDGAR_MIN_INTERVAL {
        std::thread::sleep(EDGAR_MIN_INTERVAL - elapsed);
    }
    *last = Instant::now();
}

/// GET an EDGAR URL through the shared client, rate-limited, with 3 attempts and
/// 500ms→1s backoff on 429 / 5xx / transport errors. Other error statuses are
/// terminal (no retry). Returns the successful response or the last error.
fn edgar_get(url: &str) -> Result<reqwest::blocking::Response, EdgarError> {
    let mut backoff = Duration::from_millis(500);
    let mut last_err: Option<String> = None;
    for attempt in 0..3 {
        rate_limit();
        match EDGAR_CLIENT.get(url).header("Accept", "application/json").send() {
            Ok(resp) => {
                let status = resp.status();
                if status.is_success() {
                    return Ok(resp);
                }
                if status.as_u16() == 429 || status.is_server_error() {
                    last_err = Some(format!("HTTP {status}"));
                } else {
                    // Terminal (e.g. 404) — surface immediately, no panic on 3xx.
                    return Err(EdgarError::Request(format!("HTTP {status} for {url}")));
                }
            }
            Err(e) => last_err = Some(e.to_string()),
        }
        if attempt < 2 {
            std::thread::sleep(backoff);
            backoff *= 2;
        }
    }
    Err(EdgarError::Request(last_err.unwrap_or_else(|| url.to_string())))
}

/// ticker(upper) → padded-CIK map, downloaded once and cached. Only a
/// successful download is cached (a failure returns `Err`, retried next call).
fn ticker_map() -> Result<&'static HashMap<String, String>, EdgarError> {
    if let Some(m) = TICKER_MAP.get() {
        return Ok(m);
    }
    let entries: HashMap<String, TickerEntry> = edgar_get(COMPANY_TICKERS_URL)?.json()?;
    let mut map = HashMap::with_capacity(entries.len());
    for entry in entries.values() {
        let cik = match &entry.cik_str {
            Value::Number(n) => n.as_i64().unwrap_or(0).to_string(),
            Value::String(s) => s.clone(),
            _ => continue,
        };
        map.insert(entry.ticker.to_uppercase(), format!("{:0>10}", cik));
    }
    // First writer wins; a racing thread's identical map is discarded.
    let _ = TICKER_MAP.set(map);
    Ok(TICKER_MAP.get().expect("just set"))
}

/// Look up the 10-digit SEC CIK number for a ticker symbol.
///
/// Ported from Python `get_cik()` in `src/fetcher.py`. Backed by the cached
/// [`ticker_map`], so repeated lookups don't re-download the ~1 MB index.
pub fn cik_from_ticker(ticker: &str) -> Result<String, EdgarError> {
    let ticker_upper = ticker.trim().to_uppercase();
    ticker_map()?
        .get(&ticker_upper)
        .cloned()
        .ok_or_else(|| EdgarError::TickerNotFound(ticker.to_string()))
}

/// Fetch the full XBRL company facts JSON for a given CIK.
///
/// The CIK should be a 10-digit zero-padded string.
/// Ported from Python `fetch_xbrl_facts()` in `src/fetcher.py`.
pub fn fetch_companyfacts(cik: &str) -> Result<CompanyFacts, EdgarError> {
    let url = COMPANY_FACTS_URL.replace("{cik}", cik);
    Ok(edgar_get(&url)?.json()?)
}

/// Fetch the raw JSON value of company facts (for flexible downstream parsing).
pub fn fetch_companyfacts_raw(cik: &str) -> Result<Value, EdgarError> {
    let url = COMPANY_FACTS_URL.replace("{cik}", cik);
    Ok(edgar_get(&url)?.json()?)
}

/// SIC industry classification from the SEC submissions endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SicInfo {
    /// 4-digit SIC code (e.g. "6021").
    pub sic: String,
    /// Human-readable industry (e.g. "National Commercial Banks").
    pub sic_description: String,
}

impl SicInfo {
    /// True for SIC 6000–6799 (finance / insurance / real estate) — sectors
    /// where industrial leverage / coverage metrics don't apply cleanly.
    pub fn is_financial(&self) -> bool {
        self.sic
            .parse::<u32>()
            .map(|c| (6000..=6799).contains(&c))
            .unwrap_or(false)
    }
}

/// Fetch and parse the SEC submissions JSON for a CIK (filing history +
/// company metadata such as SIC). Shared by [`fetch_company_sic`] and the
/// filing-index functions below.
fn fetch_submissions_value(cik: &str) -> Result<Value, EdgarError> {
    let url = SUBMISSIONS_URL.replace("{cik}", cik);
    Ok(edgar_get(&url)?.json()?)
}

/// Fetch a company's SIC industry classification (submissions endpoint).
pub fn fetch_company_sic(cik: &str) -> Result<SicInfo, EdgarError> {
    let v = fetch_submissions_value(cik)?;
    let sic = v.get("sic").and_then(|s| match s {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }).unwrap_or_default();
    let sic_description = v
        .get("sicDescription")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    Ok(SicInfo { sic, sic_description })
}

// ---------------------------------------------------------------------------
// Filing index (submissions -> filing documents)
// ---------------------------------------------------------------------------

/// Default form types scanned by [`search_filings`] when no narrower filter is
/// wanted — matches the Python `search_filings` default set.
pub const DEFAULT_FORM_TYPES: &[&str] = &["10-K", "10-Q", "8-K", "20-F", "6-K"];

/// A single filing resolved from the SEC submissions history, with a direct URL
/// to its primary document in the EDGAR Archives.
///
/// Ported from the `Filing` records produced by `get_recent_filings` /
/// `search_filings` in `src/research/sec_edgar.py`. Dates are kept as the
/// ISO-8601 strings EDGAR returns (consistent with [`FactValue::end`]); the
/// always-empty Python `company` field and unused `is_amended` flag are omitted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Filing {
    /// Form type, e.g. "10-K", "20-F".
    pub form_type: String,
    /// Filing date (ISO-8601, e.g. "2024-11-01").
    pub filing_date: String,
    /// Fiscal period end / report date (ISO-8601).
    pub fiscal_period_end: String,
    /// Direct URL to the primary document in the EDGAR Archives.
    pub url: String,
    /// Zero-padded 10-digit CIK.
    pub cik: String,
    /// SEC accession number, e.g. "0000320193-24-000123".
    pub accession_number: String,
}

/// Parse the `filings.recent` object of a submissions response into [`Filing`]
/// records, keeping only forms in `form_types`, most-recent-first, up to `limit`.
///
/// Pure (no network) — the deterministic core of the two fetch entry points and
/// the unit-tested parity gate. Faithful to the Python parse: the primary-
/// document URL is
/// `https://www.sec.gov/Archives/edgar/data/{cik}/{accession-no-dashes}/{doc}`
/// with leading zeros stripped from the CIK. Missing per-index fields default to
/// "" (EDGAR's `recent` arrays are parallel and complete in practice).
fn parse_recent_filings(
    recent: &Value,
    form_types: &[&str],
    limit: usize,
    cik: &str,
) -> Vec<Filing> {
    let str_at = |key: &str, i: usize| -> String {
        recent
            .get(key)
            .and_then(Value::as_array)
            .and_then(|a| a.get(i))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    let forms = match recent.get("form").and_then(Value::as_array) {
        Some(f) => f,
        None => return Vec::new(),
    };
    let cik_num = cik.trim_start_matches('0');
    let mut out = Vec::new();
    for (i, form) in forms.iter().enumerate() {
        if out.len() >= limit {
            break;
        }
        let form = form.as_str().unwrap_or("");
        if !form_types.contains(&form) {
            continue;
        }
        let accession_number = str_at("accessionNumber", i);
        let acc_nodash = accession_number.replace('-', "");
        let doc = str_at("primaryDocument", i);
        let url = format!(
            "https://www.sec.gov/Archives/edgar/data/{cik_num}/{acc_nodash}/{doc}"
        );
        out.push(Filing {
            form_type: form.to_string(),
            filing_date: str_at("filingDate", i),
            fiscal_period_end: str_at("reportDate", i),
            url,
            cik: cik.to_string(),
            accession_number,
        });
    }
    out
}

/// Fetch a company's filing history and return the most recent filings whose
/// form type is in `form_types` (e.g. `&["10-K", "20-F"]`), up to `limit`.
///
/// Ported from `search_filings` in `src/research/sec_edgar.py`.
pub fn search_filings(
    cik: &str,
    form_types: &[&str],
    limit: usize,
) -> Result<Vec<Filing>, EdgarError> {
    let subs = fetch_submissions_value(cik)?;
    let recent = subs
        .get("filings")
        .and_then(|f| f.get("recent"))
        .cloned()
        .unwrap_or(Value::Null);
    Ok(parse_recent_filings(&recent, form_types, limit, cik))
}

/// Fetch the most recent filings of a single form type (e.g. `"10-K"`), up to
/// `limit`. Ported from `get_recent_filings` in `src/research/sec_edgar.py`.
pub fn recent_filings(
    cik: &str,
    form_type: &str,
    limit: usize,
) -> Result<Vec<Filing>, EdgarError> {
    search_filings(cik, &[form_type], limit)
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

    // --- Filing-index parser (pure, no network): the parity gate ---

    #[test]
    fn parse_recent_filings_filters_and_builds_urls() {
        let recent = serde_json::json!({
            "form":            ["10-K", "8-K", "10-Q", "10-K", "4"],
            "filingDate":      ["2024-11-01", "2024-10-15", "2024-08-02", "2023-11-03", "2024-09-01"],
            "reportDate":      ["2024-09-28", "", "2024-06-29", "2023-09-30", ""],
            "accessionNumber": ["0000320193-24-000123", "0000320193-24-000120",
                                "0000320193-24-000110", "0000320193-23-000106",
                                "0000320193-24-000115"],
            "primaryDocument": ["aapl-20240928.htm", "8k.htm", "aapl-20240629.htm",
                                "aapl-20230930.htm", "wf-form4.xml"],
        });
        let filings = parse_recent_filings(&recent, &["10-K"], 5, "0000320193");
        assert_eq!(filings.len(), 2, "only the two 10-Ks match");
        assert_eq!(filings[0].form_type, "10-K");
        assert_eq!(filings[0].filing_date, "2024-11-01");
        assert_eq!(filings[0].fiscal_period_end, "2024-09-28");
        assert_eq!(filings[0].accession_number, "0000320193-24-000123");
        assert_eq!(filings[0].cik, "0000320193");
        assert_eq!(
            filings[0].url,
            "https://www.sec.gov/Archives/edgar/data/320193/000032019324000123/aapl-20240928.htm",
            "leading zeros stripped from CIK; dashes stripped from accession"
        );
        // Second-newest 10-K (order preserved from the source arrays).
        assert_eq!(filings[1].filing_date, "2023-11-03");
        assert_eq!(filings[1].fiscal_period_end, "2023-09-30");
    }

    #[test]
    fn parse_recent_filings_respects_limit_and_multi_form() {
        let recent = serde_json::json!({
            "form":            ["10-Q", "10-K", "10-Q", "8-K"],
            "filingDate":      ["2024-08-02", "2024-11-01", "2024-05-02", "2024-10-15"],
            "reportDate":      ["2024-06-29", "2024-09-28", "2024-03-30", ""],
            "accessionNumber": ["a-1", "a-2", "a-3", "a-4"],
            "primaryDocument": ["q1.htm", "k.htm", "q2.htm", "8k.htm"],
        });
        let filings = parse_recent_filings(&recent, &["10-K", "10-Q"], 2, "0000000001");
        assert_eq!(filings.len(), 2, "limit caps the result");
        assert_eq!(filings[0].form_type, "10-Q");
        assert_eq!(filings[1].form_type, "10-K");
        assert_eq!(
            filings[0].url,
            "https://www.sec.gov/Archives/edgar/data/1/a1/q1.htm"
        );
    }

    #[test]
    fn parse_recent_filings_empty_when_no_forms() {
        assert!(parse_recent_filings(&Value::Null, &["10-K"], 5, "1").is_empty());
        assert!(parse_recent_filings(&serde_json::json!({}), &["10-K"], 5, "1").is_empty());
        assert!(
            parse_recent_filings(&serde_json::json!({"form": ["8-K"]}), &["10-K"], 5, "1")
                .is_empty(),
            "no matching form types -> empty"
        );
    }

    #[test]
    #[ignore]
    fn recent_filings_aapl_live() {
        let cik = cik_from_ticker("AAPL").expect("CIK lookup");
        let filings = recent_filings(&cik, "10-K", 3).expect("recent 10-K filings");
        assert!(!filings.is_empty(), "AAPL should have 10-K filings");
        assert!(filings.iter().all(|f| f.form_type == "10-K"));
        assert!(filings[0].url.contains("/Archives/edgar/data/320193/"));
        assert!(filings[0].url.ends_with(".htm"));
    }

    #[test]
    #[ignore]
    fn search_filings_aapl_live_multi_form() {
        let cik = cik_from_ticker("AAPL").expect("CIK lookup");
        let filings = search_filings(&cik, &["10-K", "10-Q"], 5).expect("filings");
        assert_eq!(filings.len(), 5, "AAPL has plenty of 10-K/10-Q filings");
        assert!(filings.iter().all(|f| f.form_type == "10-K" || f.form_type == "10-Q"));
    }
}
