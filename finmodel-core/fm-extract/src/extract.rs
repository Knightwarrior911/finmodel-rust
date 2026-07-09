use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use fm_types::StatementData;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors that can occur during filing extraction.
#[derive(Debug, Error)]
pub enum ExtractError {
    /// JSON serialization / deserialization error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// I/O error (file reads, network, etc.).
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// HTTP / network error.
    #[error("HTTP error: {0}")]
    Reqwest(#[from] reqwest::Error),

    /// Generic extraction failure with message.
    #[error("{0}")]
    Other(String),
}
// ---------------------------------------------------------------------------
// Core extraction types
// ---------------------------------------------------------------------------

/// The full result of extracting financial data from a filing.
///
/// Matches the shape of the Python model cache JSON file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionResult {
    /// Three-letter currency code (e.g. "USD", "SEK").
    pub currency: String,

    /// The fiscal years for which data was found, oldest first.
    pub years_found: Vec<String>,

    /// Income statement line items.
    pub income_statement: StatementData,

    /// Balance sheet line items.
    pub balance_sheet: StatementData,

    /// Cash flow statement line items.
    pub cash_flow_statement: StatementData,

    /// Footnote and supplementary details.
    #[serde(default)]
    pub notes: HashMap<String, serde_json::Value>,

    /// Overall extraction confidence, 0.0 – 1.0.
    #[serde(default = "default_confidence")]
    pub confidence: f64,

    /// Discrepancy / warning messages.
    #[serde(default)]
    pub discrepancies: Vec<String>,
}

fn default_confidence() -> f64 {
    1.0
}

// ---------------------------------------------------------------------------
// Fetch configuration
// ---------------------------------------------------------------------------

/// Configuration for fetching a filing.
#[derive(Debug, Clone)]
pub struct FetchConfig {
    /// Company ticker symbol.
    pub ticker: String,

    /// Optional SEC EDGAR CIK number (used for XBRL lookups).
    pub cik: Option<String>,

    /// Optional explicit URL override (e.g. direct XBRL link).
    pub url: Option<String>,

    /// Optional company name (for display / logging).
    pub company_name: Option<String>,
}

impl FetchConfig {
    /// Create a new `FetchConfig` for the given ticker.
    pub fn new(ticker: &str) -> Self {
        Self {
            ticker: ticker.to_string(),
            cik: None,
            url: None,
            company_name: None,
        }
    }

    /// Set the SEC CIK number.
    pub fn with_cik(mut self, cik: &str) -> Self {
        self.cik = Some(cik.to_string());
        self
    }

    /// Set an explicit fetch URL.
    pub fn with_url(mut self, url: &str) -> Self {
        self.url = Some(url.to_string());
        self
    }
}

// ---------------------------------------------------------------------------
// LLM extraction prompts (ported from Python src/extractor.py)
// ---------------------------------------------------------------------------

/// System prompt for extracting structured financial data from XBRL / filing text.
///
/// Ported from `FINANCIALS_SYSTEM_PROMPT` in the Python extractor, adapted for
/// XBRL-based audit-grade extraction.
pub static EXTRACT_SYSTEM_PROMPT: &str = "\
You are a senior financial analyst performing audit-grade extraction of structured financial data from XBRL (eXtensible Business Reporting Language) filing data.

Extract main income statement, balance sheet, and cash flow statement line items for ALL years present in the report (typically 2-3 comparative years). Also extract key footnote detail.

IMPORTANT RULES:
- All monetary values in MILLIONS (same currency as the filing)
- Arrays: oldest year first, newest year last — same length for every key
- capex: positive number (absolute cash outflow for PP&E purchases)
- income_tax: positive number (absolute tax charge)
- dividends_paid: positive number (absolute cash outflow)
- cfi: SIGNED total (negative = net outflow from investing; typical for industrial/manufacturing companies)
- cff: SIGNED total (negative = net outflow from financing)
- net_change_cash: SIGNED total (positive = increase in cash and equivalents)
- USE ONLY the CONSOLIDATED financial statements — never segment tables, parent-company, or subsidiary statements
    \"Revenue\" / \"Net revenue\" / \"Net sales\" / \"Revenues\" → revenue
    \"Cost of sales\" / \"Cost of revenue\" / \"Cost of goods sold\" → cogs
    \"Gross profit\" → gross_profit
    \"Marketing expenses\" + \"Selling expenses\" + \"Administrative expenses\" / \"SG&A\" → sga (SUM them if split). EXCLUDE \"Distribution expenses\" / \"Logistics\" / \"Fulfilment\" — those are COGS-type, NOT sga
    \"Research and development expenses\" / \"R&D expenses\" → rd
    \"Operating profit\" / \"Operating income\" / \"EBIT\" → ebit
    \"EBITA\" / \"Earnings before interest, taxes and amortisation\" → ebita
    \"Financial expenses\" / \"Interest expense\" / \"Finance costs\" → interest_expense
    \"Financial income\" / \"Interest income\" / \"Finance income\" → interest_income
    \"Depreciation and amortization\" / \"D&A\" from cash flow statement → da
    \"Net cash from investing activities\" / \"Net cash used in investing activities\" / \"Cash flow from investment activities\" → cfi
    \"Net cash from financing activities\" / \"Net cash used in financing activities\" / \"Cash flow from financing activities\" → cff
    \"Net change in cash and cash equivalents\" / \"Net increase (decrease) in cash\" / \"Change in cash and cash equivalents\" → net_change_cash
- da: take from the cash flow statement add-back line (most reliable source), NOT the income statement
- net_income: the TOTAL \"Profit for the year\" / \"Profit for the period\" / \"Net profit\" for the whole group INCLUDING non-controlling interests — NEVER the \"attributable to owners/shareholders of the parent\" sub-line
- shares_diluted: weighted average DILUTED shares in MILLIONS — NOT earnings per share
- If gross profit not shown separately and cogs not shown: omit both cogs and gross_profit
- Nordic/European numbers: \"168 343\" means 168,343 (space = thousands separator)
- If a line item is absent from the filing, omit its key entirely (do not include null or 0)

Return ONLY valid JSON in this exact structure (no prose, no markdown):
{
  \"currency\": \"<3-letter code e.g. SEK, EUR, GBP>\",
  \"years_found\": [\"2022\", \"2023\", \"2024\"],
  \"income_statement\": {
    \"revenue\":          [<2022>, <2023>, <2024>],
    \"cogs\":             [<2022>, <2023>, <2024>],
    \"gross_profit\":     [<2022>, <2023>, <2024>],
    \"sga\":              [<2022>, <2023>, <2024>],
    \"rd\":               [<2022>, <2023>, <2024>],
    \"da\":               [<2022>, <2023>, <2024>],
    \"ebit\":             [<2022>, <2023>, <2024>],
    \"ebita\":            [<2022>, <2023>, <2024>],
    \"interest_expense\": [<2022>, <2023>, <2024>],
    \"interest_income\":  [<2022>, <2023>, <2024>],
    \"income_tax\":       [<2022>, <2023>, <2024>],
    \"net_income\":       [<2022>, <2023>, <2024>],
    \"shares_diluted\":   [<2022>, <2023>, <2024>]
  },
  \"balance_sheet\": {
    \"cash\":                 [<2022>, <2023>, <2024>],
    \"accounts_receivable\":  [<2022>, <2023>, <2024>],
    \"inventory\":            [<2022>, <2023>, <2024>],
    \"total_current_assets\": [<2022>, <2023>, <2024>],
    \"ppe_net\":              [<2022>, <2023>, <2024>],
    \"goodwill\":             [<2022>, <2023>, <2024>],
    \"intangibles_net\":      [<2022>, <2023>, <2024>],
    \"total_assets\":         [<2022>, <2023>, <2024>],
    \"accounts_payable\":     [<2022>, <2023>, <2024>],
    \"long_term_debt\":       [<2022>, <2023>, <2024>],
    \"total_liabilities\":    [<2022>, <2023>, <2024>],
    \"total_equity\":         [<2022>, <2023>, <2024>]
  },
  \"cash_flow_statement\": {
    \"cfo\":             [<2022>, <2023>, <2024>],
    \"capex\":           [<2022>, <2023>, <2024>],
    \"cfi\":             [<2022>, <2023>, <2024>],
    \"dividends_paid\":  [<2022>, <2023>, <2024>],
    \"cff\":             [<2022>, <2023>, <2024>],
    \"net_change_cash\": [<2022>, <2023>, <2024>]
  },
  \"notes\": {
    \"tax_rate\":          {\"values\": {\"2022A\": <decimal>, \"2023A\": <decimal>, \"2024A\": <decimal>}},
    \"debt_maturities\":   {\"2025\": <val>, \"2026\": <val>, \"2027\": <val>},
    \"sbc_expense\":       {\"values\": {\"2022A\": <val>, \"2023A\": <val>, \"2024A\": <val>}},
    \"lease_obligations\": {\"operating\": <val>, \"finance\": <val>},
    \"dso_days\": <number or null>,
    \"dpo_days\": <number or null>,
    \"dio_days\": <number or null>
  },
  \"confidence\": <0.0 to 1.0>,
  \"discrepancies\": [\"description of any conflicts or missing items\"]
}";

// ---------------------------------------------------------------------------
// Helper to produce placeholder extraction data (stub)
// ---------------------------------------------------------------------------

/// Return minimal placeholder data for the given ticker.
///
/// This is a stub — no network I/O is performed.  Actual XBRL fetching
/// (via SEC EDGAR API) will replace this implementation.
pub fn placeholder_result(_ticker: &str) -> ExtractionResult {
    let years = vec!["2022".to_string(), "2023".to_string(), "2024".to_string()];

    ExtractionResult {
        currency: "USD".to_string(),
        years_found: years.clone(),
        income_statement: {
            let mut m = HashMap::new();
            m.insert("revenue".to_string(), vec![Some(1000.0), Some(1100.0), Some(1200.0)]);
            m.insert("cogs".to_string(), vec![Some(600.0), Some(660.0), Some(720.0)]);
            m.insert("gross_profit".to_string(), vec![Some(400.0), Some(440.0), Some(480.0)]);
            m.insert("sga".to_string(), vec![Some(150.0), Some(160.0), Some(170.0)]);
            m.insert("rd".to_string(), vec![Some(50.0), Some(55.0), Some(60.0)]);
            m.insert("ebit".to_string(), vec![Some(200.0), Some(225.0), Some(250.0)]);
            m.insert("interest_expense".to_string(), vec![Some(20.0), Some(22.0), Some(24.0)]);
            m.insert("income_tax".to_string(), vec![Some(45.0), Some(51.0), Some(57.0)]);
            m.insert("net_income".to_string(), vec![Some(135.0), Some(152.0), Some(169.0)]);
            m.insert("shares_diluted".to_string(), vec![Some(50.0), Some(52.0), Some(54.0)]);
            m
        },
        balance_sheet: {
            let mut m = HashMap::new();
            m.insert("cash".to_string(), vec![Some(100.0), Some(120.0), Some(150.0)]);
            m.insert("accounts_receivable".to_string(), vec![Some(80.0), Some(90.0), Some(100.0)]);
            m.insert("inventory".to_string(), vec![Some(70.0), Some(75.0), Some(80.0)]);
            m.insert("total_current_assets".to_string(), vec![Some(300.0), Some(340.0), Some(380.0)]);
            m.insert("ppe_net".to_string(), vec![Some(400.0), Some(420.0), Some(450.0)]);
            m.insert("total_assets".to_string(), vec![Some(800.0), Some(860.0), Some(940.0)]);
            m.insert("accounts_payable".to_string(), vec![Some(60.0), Some(65.0), Some(70.0)]);
            m.insert("long_term_debt".to_string(), vec![Some(200.0), Some(180.0), Some(160.0)]);
            m.insert("total_liabilities".to_string(), vec![Some(400.0), Some(410.0), Some(420.0)]);
            m.insert("total_equity".to_string(), vec![Some(400.0), Some(450.0), Some(520.0)]);
            m
        },
        cash_flow_statement: {
            let mut m = HashMap::new();
            m.insert("cfo".to_string(), vec![Some(180.0), Some(200.0), Some(220.0)]);
            m.insert("capex".to_string(), vec![Some(50.0), Some(55.0), Some(60.0)]);
            m.insert("cfi".to_string(), vec![Some(-80.0), Some(-90.0), Some(-100.0)]);
            m.insert("dividends_paid".to_string(), vec![Some(30.0), Some(32.0), Some(35.0)]);
            m.insert("cff".to_string(), vec![Some(-70.0), Some(-78.0), Some(-80.0)]);
            m.insert("net_change_cash".to_string(), vec![Some(30.0), Some(32.0), Some(40.0)]);
            m
        },
        notes: {
            let mut m = HashMap::new();
            m.insert(
                "tax_rate".to_string(),
                serde_json::json!({"values": {"2022A": 0.25, "2023A": 0.25, "2024A": 0.25}}),
            );
            m.insert("dso_days".to_string(), serde_json::json!(45));
            m.insert("dpo_days".to_string(), serde_json::json!(30));
            m
        },
        confidence: 0.95,
        discrepancies: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_placeholder_round_trip() {
        let result = placeholder_result("TEST");
        let json = serde_json::to_string(&result).expect("serialize");
        let deserialized: ExtractionResult =
            serde_json::from_str(&json).expect("deserialize");

        assert_eq!(deserialized.currency, "USD");
        assert_eq!(deserialized.years_found.len(), 3);
        assert!(deserialized.income_statement.contains_key("revenue"));
        assert!(deserialized.balance_sheet.contains_key("cash"));
        assert!(deserialized.cash_flow_statement.contains_key("cfo"));
        assert_eq!(deserialized.income_statement["revenue"].len(), 3);
        assert!((deserialized.confidence - 0.95).abs() < 1e-10);
    }

    #[test]
    fn test_prompt_non_empty() {
        assert!(!EXTRACT_SYSTEM_PROMPT.is_empty());
        assert!(EXTRACT_SYSTEM_PROMPT.len() > 500);
    }

    #[test]
    fn test_prompt_contains_key_phrases() {
        // The prompt should describe the financial data to extract
        assert!(
            EXTRACT_SYSTEM_PROMPT.contains("XBRL"),
            "prompt should reference XBRL"
        );
        assert!(
            EXTRACT_SYSTEM_PROMPT.contains("audit-grade"),
            "prompt should reference audit-grade extraction"
        );
        assert!(
            EXTRACT_SYSTEM_PROMPT.contains("income statement"),
            "prompt should reference income statement"
        );
        assert!(
            EXTRACT_SYSTEM_PROMPT.contains("balance sheet"),
            "prompt should reference balance sheet"
        );
        assert!(
            EXTRACT_SYSTEM_PROMPT.contains("cash flow"),
            "prompt should reference cash flow statement"
        );
    }

    #[test]
    fn test_fetch_config_builder() {
        let cfg = FetchConfig::new("AAPL")
            .with_cik("0000320193")
            .with_url("https://www.sec.gov/cgi-bin/browse-edgar?action=getcompany&CIK=0000320193");
        assert_eq!(cfg.ticker, "AAPL");
        assert_eq!(cfg.cik.as_deref(), Some("0000320193"));
        assert!(cfg.url.unwrap().contains("sec.gov"));
    }

    #[test]
    fn test_placeholder_values_aligned() {
        let result = placeholder_result("TEST");
        let n = result.years_found.len();
        for (_key, vals) in &result.income_statement {
            assert_eq!(
                vals.len(),
                n,
                "income_statement entry {:?} has length {} but years_found={}",
                _key,
                vals.len(),
                n
            );
        }
        for (_key, vals) in &result.balance_sheet {
            assert_eq!(vals.len(), n);
        }
        for (_key, vals) in &result.cash_flow_statement {
            assert_eq!(vals.len(), n);
        }
    }

    #[test]
    fn test_placeholder_has_required_statements() {
        let result = placeholder_result("TEST");
        assert!(!result.income_statement.is_empty());
        assert!(!result.balance_sheet.is_empty());
        assert!(!result.cash_flow_statement.is_empty());
        assert_eq!(result.years_found, vec!["2022", "2023", "2024"]);
        assert!(result.confidence > 0.0);
        assert!(result.confidence <= 1.0);
    }
}
