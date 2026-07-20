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
            m.insert(
                "revenue".to_string(),
                vec![Some(1000.0), Some(1100.0), Some(1200.0)],
            );
            m.insert(
                "cogs".to_string(),
                vec![Some(600.0), Some(660.0), Some(720.0)],
            );
            m.insert(
                "gross_profit".to_string(),
                vec![Some(400.0), Some(440.0), Some(480.0)],
            );
            m.insert(
                "sga".to_string(),
                vec![Some(150.0), Some(160.0), Some(170.0)],
            );
            m.insert("rd".to_string(), vec![Some(50.0), Some(55.0), Some(60.0)]);
            m.insert(
                "ebit".to_string(),
                vec![Some(200.0), Some(225.0), Some(250.0)],
            );
            m.insert(
                "interest_expense".to_string(),
                vec![Some(20.0), Some(22.0), Some(24.0)],
            );
            m.insert(
                "income_tax".to_string(),
                vec![Some(45.0), Some(51.0), Some(57.0)],
            );
            m.insert(
                "net_income".to_string(),
                vec![Some(135.0), Some(152.0), Some(169.0)],
            );
            m.insert(
                "shares_diluted".to_string(),
                vec![Some(50.0), Some(52.0), Some(54.0)],
            );
            m
        },
        balance_sheet: {
            let mut m = HashMap::new();
            m.insert(
                "cash".to_string(),
                vec![Some(100.0), Some(120.0), Some(150.0)],
            );
            m.insert(
                "accounts_receivable".to_string(),
                vec![Some(80.0), Some(90.0), Some(100.0)],
            );
            m.insert(
                "inventory".to_string(),
                vec![Some(70.0), Some(75.0), Some(80.0)],
            );
            m.insert(
                "total_current_assets".to_string(),
                vec![Some(300.0), Some(340.0), Some(380.0)],
            );
            m.insert(
                "ppe_net".to_string(),
                vec![Some(400.0), Some(420.0), Some(450.0)],
            );
            m.insert(
                "total_assets".to_string(),
                vec![Some(800.0), Some(860.0), Some(940.0)],
            );
            m.insert(
                "accounts_payable".to_string(),
                vec![Some(60.0), Some(65.0), Some(70.0)],
            );
            m.insert(
                "long_term_debt".to_string(),
                vec![Some(200.0), Some(180.0), Some(160.0)],
            );
            m.insert(
                "total_liabilities".to_string(),
                vec![Some(400.0), Some(410.0), Some(420.0)],
            );
            m.insert(
                "total_equity".to_string(),
                vec![Some(400.0), Some(450.0), Some(520.0)],
            );
            m
        },
        cash_flow_statement: {
            let mut m = HashMap::new();
            m.insert(
                "cfo".to_string(),
                vec![Some(180.0), Some(200.0), Some(220.0)],
            );
            m.insert(
                "capex".to_string(),
                vec![Some(50.0), Some(55.0), Some(60.0)],
            );
            m.insert(
                "cfi".to_string(),
                vec![Some(-80.0), Some(-90.0), Some(-100.0)],
            );
            m.insert(
                "dividends_paid".to_string(),
                vec![Some(30.0), Some(32.0), Some(35.0)],
            );
            m.insert(
                "cff".to_string(),
                vec![Some(-70.0), Some(-78.0), Some(-80.0)],
            );
            m.insert(
                "net_change_cash".to_string(),
                vec![Some(30.0), Some(32.0), Some(40.0)],
            );
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

// ---------------------------------------------------------------------------
// PDF-based extraction prompts (non-US filings)
// ---------------------------------------------------------------------------

/// System prompt for non-US industrial filing PDF extraction.
/// Ported verbatim from `src/extractor.py::FINANCIALS_SYSTEM_PROMPT`.
pub static FINANCIALS_SYSTEM_PROMPT: &str = "\
You are a senior financial analyst extracting structured financial data from annual report text.

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
- IFRS naming mappings (label in filing → JSON key):
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

/// System prompt for bank extraction.
/// Ported verbatim from `src/extractor.py::_BANK_SYSTEM_PROMPT`.
pub static BANK_SYSTEM_PROMPT: &str = "\
You are a senior financial analyst extracting structured financial data from annual report text.

Extract main income statement, balance sheet, and cash flow statement line items for ALL years present in the report (typically 2-3 comparative years). Also extract key footnote detail.

IMPORTANT RULES:
- All monetary values in MILLIONS (same currency as the filing)
- Arrays: oldest year first, newest year last — same length for every key
- income_tax: positive number (absolute tax charge)
- cfi: SIGNED total (negative = net outflow from investing)
- cff: SIGNED total (negative = net outflow from financing)
- net_change_cash: SIGNED total (positive = increase in cash and equivalents)
- USE ONLY the CONSOLIDATED financial statements — never segment tables, parent-company, or subsidiary statements
- IFRS naming mappings (label in filing → JSON key):
    \"Interest income\" / \"Interest and similar income\" / \"Interest and similar revenue\" → interest_income
    \"Interest expense\" / \"Interest and similar expense\" / \"Interest and similar charges\" → interest_expense
    \"Net interest income\" / \"Net interest and similar income\" → net_interest_income
    \"Fee and commission income\" / \"Net fee and commission income\" / \"Fees and commissions\" → fee_commission_income
    \"Net trading income\" / \"Trading income\" / \"Net gains on financial instruments at fair value\" → trading_income
    \"Total operating income\" / \"Total income\" / \"Operating income\" → total_operating_income
    \"Loan loss provisions\" / \"Impairment losses on loans\" / \"Credit loss expense\" / \"Net impairment on financial assets\" → loan_loss_provisions
    \"Operating expenses\" / \"Total operating expenses\" / \"General and administrative expenses\" → operating_expenses
    \"Profit before tax\" / \"Profit before income tax\" / \"Pre-tax profit\" → pretax_income
    \"Net cash from investing activities\" / \"Net cash used in investing activities\" / \"Cash flow from investment activities\" → cfi
    \"Net cash from financing activities\" / \"Net cash used in financing activities\" / \"Cash flow from financing activities\" → cff
    \"Net change in cash and cash equivalents\" / \"Net increase (decrease) in cash\" / \"Change in cash and cash equivalents\" → net_change_cash
- net_income: the TOTAL \"Profit for the year\" / \"Profit for the period\" / \"Net profit\" for the whole group INCLUDING non-controlling interests — NEVER the \"attributable to owners/shareholders of the parent\" sub-line
- Nordic/European numbers: \"168 343\" means 168,343 (space = thousands separator)
- If a line item is absent from the filing, omit its key entirely (do not include null or 0)

Return ONLY valid JSON in this exact structure (no prose, no markdown):
{
  \"currency\": \"<3-letter code e.g. SEK, EUR, GBP>\",
  \"years_found\": [\"2022\", \"2023\", \"2024\"],
  \"income_statement\": {
    \"interest_income\":         [<2022>, <2023>, <2024>],
    \"interest_expense\":        [<2022>, <2023>, <2024>],
    \"net_interest_income\":     [<2022>, <2023>, <2024>],
    \"fee_commission_income\":   [<2022>, <2023>, <2024>],
    \"trading_income\":          [<2022>, <2023>, <2024>],
    \"total_operating_income\":  [<2022>, <2023>, <2024>],
    \"loan_loss_provisions\":    [<2022>, <2023>, <2024>],
    \"operating_expenses\":      [<2022>, <2023>, <2024>],
    \"pretax_income\":           [<2022>, <2023>, <2024>],
    \"income_tax\":              [<2022>, <2023>, <2024>],
    \"net_income\":              [<2022>, <2023>, <2024>]
  },
  \"balance_sheet\": {
    \"cash_and_central_bank\":  [<2022>, <2023>, <2024>],
    \"loans_to_customers\":     [<2022>, <2023>, <2024>],
    \"investment_securities\":  [<2022>, <2023>, <2024>],
    \"total_assets\":           [<2022>, <2023>, <2024>],
    \"customer_deposits\":      [<2022>, <2023>, <2024>],
    \"debt_securities_issued\": [<2022>, <2023>, <2024>],
    \"total_liabilities\":      [<2022>, <2023>, <2024>],
    \"total_equity\":           [<2022>, <2023>, <2024>]
  },
  \"cash_flow_statement\": {
    \"cfo\":             [<2022>, <2023>, <2024>],
    \"cfi\":             [<2022>, <2023>, <2024>],
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

/// System prompt for insurer extraction.
/// Ported verbatim from `src/extractor.py::_INSURER_SYSTEM_PROMPT`.
pub static INSURER_SYSTEM_PROMPT: &str = "\
You are a senior financial analyst extracting structured financial data from annual report text.

Extract main income statement, balance sheet, and cash flow statement line items for ALL years present in the report (typically 2-3 comparative years). Also extract key footnote detail.

IMPORTANT RULES:
- All monetary values in MILLIONS (same currency as the filing)
- Arrays: oldest year first, newest year last — same length for every key
- income_tax: positive number (absolute tax charge)
- cfi: SIGNED total (negative = net outflow from investing)
- cff: SIGNED total (negative = net outflow from financing)
- net_change_cash: SIGNED total (positive = increase in cash and equivalents)
- USE ONLY the CONSOLIDATED financial statements — never segment tables, parent-company, or subsidiary statements
- IFRS naming mappings (label in filing → JSON key):
    \"Gross written premium\" / \"Gross written premiums\" / \"Gross premiums written\" → gross_written_premium
    \"Net earned premium\" / \"Net earned premiums\" / \"Premiums earned, net\" / \"Net insurance revenue\" → net_earned_premium
    \"Net investment income\" / \"Investment income\" / \"Investment result\" → net_investment_income
    \"Net claims incurred\" / \"Claims incurred, net\" / \"Net insurance claims\" / \"Insurance service expense\" → net_claims_incurred
    \"Acquisition expenses\" / \"Acquisition costs\" / \"Deferred acquisition costs amortisation\" / \"Commission expenses\" → acquisition_expenses
    \"Operating expenses\" / \"Total operating expenses\" / \"Administrative expenses\" → operating_expenses
    \"Profit before tax\" / \"Profit before income tax\" / \"Pre-tax profit\" → pretax_income
    \"Net cash from investing activities\" / \"Net cash used in investing activities\" / \"Cash flow from investment activities\" → cfi
    \"Net cash from financing activities\" / \"Net cash used in financing activities\" / \"Cash flow from financing activities\" → cff
    \"Net change in cash and cash equivalents\" / \"Net increase (decrease) in cash\" / \"Change in cash and cash equivalents\" → net_change_cash
- net_income: the TOTAL \"Profit for the year\" / \"Profit for the period\" / \"Net profit\" for the whole group INCLUDING non-controlling interests — NEVER the \"attributable to owners/shareholders of the parent\" sub-line
- Nordic/European numbers: \"168 343\" means 168,343 (space = thousands separator)
- If a line item is absent from the filing, omit its key entirely (do not include null or 0)

Return ONLY valid JSON in this exact structure (no prose, no markdown):
{
  \"currency\": \"<3-letter code e.g. SEK, EUR, GBP>\",
  \"years_found\": [\"2022\", \"2023\", \"2024\"],
  \"income_statement\": {
    \"gross_written_premium\": [<2022>, <2023>, <2024>],
    \"net_earned_premium\":    [<2022>, <2023>, <2024>],
    \"net_investment_income\": [<2022>, <2023>, <2024>],
    \"net_claims_incurred\":   [<2022>, <2023>, <2024>],
    \"acquisition_expenses\":  [<2022>, <2023>, <2024>],
    \"operating_expenses\":    [<2022>, <2023>, <2024>],
    \"pretax_income\":         [<2022>, <2023>, <2024>],
    \"income_tax\":            [<2022>, <2023>, <2024>],
    \"net_income\":            [<2022>, <2023>, <2024>]
  },
  \"balance_sheet\": {
    \"investments\":                    [<2022>, <2023>, <2024>],
    \"cash\":                           [<2022>, <2023>, <2024>],
    \"total_assets\":                   [<2022>, <2023>, <2024>],
    \"insurance_contract_liabilities\": [<2022>, <2023>, <2024>],
    \"total_liabilities\":              [<2022>, <2023>, <2024>],
    \"total_equity\":                   [<2022>, <2023>, <2024>]
  },
  \"cash_flow_statement\": {
    \"cfo\":             [<2022>, <2023>, <2024>],
    \"cfi\":             [<2022>, <2023>, <2024>],
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

/// Notes extraction prompt.
/// Ported verbatim from `src/extractor.py::NOTES_SYSTEM_PROMPT`.
pub static NOTES_SYSTEM_PROMPT: &str = "\
You are a senior financial analyst extracting data from company filing text.
Extract ALL financial data found: D&A schedules, debt maturities, tax rates, working capital details,
CapEx breakdown, SBC expense, lease obligations, segment data, and any other quantitative footnote data.
Return ONLY valid JSON. Use millions as unit. Omit keys where data not present.";

/// Sector dispatch map matching Python `_SYSTEM_PROMPT_BY_SECTOR`.
/// INVARIANT: keys must stay key-exact with `tieout.config.CANONICAL_BY_SECTOR[sector]`.
pub fn system_prompt_for_sector(sector: &str) -> &'static str {
    match sector {
        "industrial" => FINANCIALS_SYSTEM_PROMPT,
        "bank" => BANK_SYSTEM_PROMPT,
        "insurer" => INSURER_SYSTEM_PROMPT,
        _ => FINANCIALS_SYSTEM_PROMPT,
    }
}

// ---------------------------------------------------------------------------
// Extraction cache (mirrors Python extraction_cache/)
// ---------------------------------------------------------------------------

use std::path::{Path, PathBuf};

/// Default cache directory (relative to project root).
fn cache_dir() -> PathBuf {
    // Try extraction_cache/ relative to CARGO_MANIFEST_DIR, then cwd
    if let Ok(manifest) = std::env::var("CARGO_MANIFEST_DIR") {
        let p = Path::new(&manifest).join("../../extraction_cache");
        if p.exists() || p.parent().map_or(true, |pp| pp.exists()) {
            return p;
        }
    }
    PathBuf::from("extraction_cache")
}

fn cache_filename(ticker: &str) -> String {
    let safe = ticker.replace('/', "_").replace('.', "_");
    format!("{safe}.json")
}

fn cache_path(ticker: &str) -> PathBuf {
    cache_dir().join(cache_filename(ticker))
}

/// Load a cached extraction result for the given ticker.
pub fn load_cache(ticker: &str) -> Option<ExtractionResult> {
    let p = cache_path(ticker);
    if !p.exists() {
        return None;
    }
    let data = std::fs::read_to_string(&p).ok()?;
    serde_json::from_str(&data).ok()
}

/// Save an extraction result to the cache.
pub fn save_extraction_cache(
    ticker: &str,
    result: &ExtractionResult,
) -> Result<PathBuf, ExtractError> {
    let p = cache_path(ticker);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).map_err(|e| ExtractError::Other(format!("mkdir: {e}")))?;
    }
    let json = serde_json::to_string_pretty(result)
        .map_err(|e| ExtractError::Other(format!("serialize: {e}")))?;
    std::fs::write(&p, &json).map_err(|e| ExtractError::Other(format!("write: {e}")))?;
    Ok(p)
}

// ---------------------------------------------------------------------------
// PDF extraction orchestration
// ---------------------------------------------------------------------------

/// Extract financial data from a PDF filing by shelling to pdfplumber for text
/// extraction, finding the financial section, then calling the LLM.
///
/// Ported from `extract_financials_from_pdf()` in `src/extractor.py`.
pub fn extract_financials_from_pdf(
    pdf_path: &str,
    periods: &[String],
    _ticker: &str,
    llm_cfg: Option<&crate::llm::LlmConfig>,
) -> Result<ExtractionResult, ExtractError> {
    // Step 1: Extract per-page text natively (pure Rust, no Python)
    let pages = extract_pdf_pages(pdf_path)?;

    // Step 3: Detect sector and get right prompt
    let sector = crate::section::detect_sector(&pages);
    let system_prompt = system_prompt_for_sector(sector);

    // Step 4: Find financial section
    let section_text = crate::section::extract_financial_section(&pages, 30);
    // Step 5: Build user prompt matching Python `extract_financials_from_pdf` lines 590-596.
    //   Python: years = [p[:4] for p in periods]
    //   "Extract data for these years (oldest first): {years}\nReturn arrays of length {len} for every key.\n\nAnnual report text:\n{chunk}"
    let years_clean: Vec<&str> = periods.iter().map(|p| &p[..p.len().min(4)]).collect();
    let years_repr = format!(
        "[{}]",
        years_clean
            .iter()
            .map(|y| format!("'{y}'"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let user_prompt = format!(
        "Extract data for these years (oldest first): {years_repr}\n\
         Return arrays of length {} for every key.\n\n\
         Annual report text:\n{section_text}",
        years_clean.len(),
    );

    // Step 6: Call LLM (Python uses max_tokens=8192)
    let raw = crate::llm::llm_complete_with(llm_cfg, system_prompt, &user_prompt, 8192)
        .map_err(|e| ExtractError::Other(format!("LLM call failed: {e}")))?;

    // Step 7: Parse JSON response with salvage fallback matching Python lines 604-615.
    let parsed = parse_llm_json_response(&raw)?;
    extraction_result_from_json(&parsed)
}

/// Parse LLM JSON response with fallback salvage (matches Python's json.loads + find/rfind).
fn parse_llm_json_response(raw: &str) -> Result<serde_json::Value, ExtractError> {
    let raw_trimmed = raw.trim();
    // First attempt: direct parse
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(raw_trimmed) {
        return Ok(v);
    }
    // Fallback: extract outermost { ... } — Python uses find('{') / rfind('}')
    let brace_start = raw_trimmed.find('{');
    let brace_end = raw_trimmed.rfind('}');
    if let (Some(s), Some(e)) = (brace_start, brace_end) {
        if e > s {
            if let Ok(v) = serde_json::from_str(&raw_trimmed[s..=e]) {
                return Ok(v);
            }
        }
    }
    Err(ExtractError::Other(format!(
        "LLM returned invalid JSON; raw (first 200): {}",
        &raw.chars().take(200).collect::<String>()
    )))
}

/// Extract per-page text from a PDF natively (pure Rust via pdf-extract).
///
/// Replaces the former `py -3 pdfplumber` shell-out — no Python dependency.
/// Returns a vector of page texts. Verified to match pdfplumber's page count
/// and figure extraction on the baseline filings (Sandvik: 160 pages, key
/// figures 126,503 / 122,878 extracted identically).
pub fn extract_pdf_pages(pdf_path: &str) -> Result<Vec<String>, ExtractError> {
    // pdf-extract can panic (not just Err) on malformed PDFs — catch it so one
    // bad filing can't crash the whole app/Tauri process.
    let path = pdf_path.to_string();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        pdf_extract::extract_text_by_pages(&path)
    }));
    match result {
        Ok(Ok(pages)) => Ok(pages),
        Ok(Err(e)) => Err(ExtractError::Other(format!(
            "PDF text extraction failed: {e}"
        ))),
        Err(_) => Err(ExtractError::Other(format!(
            "PDF text extraction panicked on {pdf_path} (malformed or unsupported PDF)"
        ))),
    }
}

/// Extract full PDF text as a single newline-joined string (pure Rust).
///
/// Convenience over [`extract_pdf_pages`] for the regex extractor in
/// [`crate::report`], which operates on whole-document text.
pub fn extract_pdf_text(pdf_path: &str) -> Result<String, ExtractError> {
    Ok(extract_pdf_pages(pdf_path)?.join("\n"))
}

/// Convert a JSON value (from LLM response) into an ExtractionResult.
fn extraction_result_from_json(val: &serde_json::Value) -> Result<ExtractionResult, ExtractError> {
    let currency = val
        .get("currency")
        .and_then(|v| v.as_str())
        .unwrap_or("USD")
        .to_string();

    let years_found = val
        .get("years_found")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let income_statement = json_obj_to_statement_data(val.get("income_statement"));
    let balance_sheet = json_obj_to_statement_data(val.get("balance_sheet"));
    let cash_flow_statement = json_obj_to_statement_data(val.get("cash_flow_statement"));

    let notes = val
        .get("notes")
        .and_then(|v| v.as_object())
        .map(|obj| obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();

    let confidence = val
        .get("confidence")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.9);

    let discrepancies = val
        .get("discrepancies")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    Ok(ExtractionResult {
        currency,
        years_found,
        income_statement,
        balance_sheet,
        cash_flow_statement,
        notes,
        confidence,
        discrepancies,
    })
}

/// Convert a JSON object with array values into StatementData.
fn json_obj_to_statement_data(obj: Option<&serde_json::Value>) -> StatementData {
    let mut sd = StatementData::new();
    if let Some(o) = obj.and_then(|v| v.as_object()) {
        for (key, val) in o {
            if let Some(arr) = val.as_array() {
                let vec: Vec<Option<f64>> = arr
                    .iter()
                    .map(|v| v.as_f64().or_else(|| v.as_i64().map(|i| i as f64)))
                    .collect();
                sd.insert(key.clone(), vec);
            }
        }
    }
    sd
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_placeholder_round_trip() {
        let result = placeholder_result("TEST");
        let json = serde_json::to_string(&result).expect("serialize");
        let deserialized: ExtractionResult = serde_json::from_str(&json).expect("deserialize");

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
