//! Pure regex financial extractor for non-US annual-report PDF text.
//!
//! Ported from `src/research/browser_pipeline.py` — the PURE extraction core
//! only (`extract_financials`, `_extract_amount`, `filing_type`,
//! `is_valid_filing`, `_overlay_interim_bs`). The anti-bot browser-discovery
//! glue is NOT ported (a later phase wires the Roam MCP browser).
//!
//! Regex patterns are ported VERBATIM from the Python source. Two flag regimes
//! mirror the original exactly:
//! - `_extract_amount` patterns run with `IGNORECASE | DOTALL`.
//! - the income/balance-sheet ANCHOR regexes run case-sensitively, no DOTALL.
//!
//! Text is normalized (locale thousand-separators → commas) up front, matching
//! Python's `extract_text()` preprocessing, so a raw pdf-extract dump can be
//! fed directly.

use std::collections::HashMap;

use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};

use crate::extract::ExtractionResult;
use fm_types::StatementData;

// ---------------------------------------------------------------------------
// Data model
// ---------------------------------------------------------------------------

/// Financial data extracted from annual report text.
///
/// Field-for-field port of the Python `ExtractedFinancials` dataclass. Every
/// monetary field is `Option<f64>`; a field the filing lacks stays `None`
/// (never fabricated).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExtractedFinancials {
    #[serde(default)]
    pub company: String,
    #[serde(default)]
    pub year: String,

    pub revenue: Option<f64>,
    /// EBIT / Operating Result.
    pub operating_income: Option<f64>,
    pub net_income: Option<f64>,
    pub total_assets: Option<f64>,
    pub total_equity: Option<f64>,
    pub total_debt: Option<f64>,
    pub cash: Option<f64>,
    pub goodwill: Option<f64>,
    pub short_term_investments: Option<f64>,

    // EBITDA / EBITA hierarchy (preference order).
    /// Tier 1: company-reported adjusted (one-offs removed).
    pub adjusted_ebitda: Option<f64>,
    /// Tier 2: company-reported EBITDA.
    pub reported_ebitda: Option<f64>,
    /// EBIT + amortisation of intangibles (IFRS KPI).
    pub ebita: Option<f64>,

    // IFRS 16 lease data.
    pub rou_depreciation: Option<f64>,
    pub lease_interest: Option<f64>,
    pub short_term_rent: Option<f64>,
    pub lease_liabilities_current: Option<f64>,
    pub lease_liabilities_noncurrent: Option<f64>,
    pub rou_assets: Option<f64>,

    // D&A breakdown.
    pub depreciation_total: Option<f64>,
    pub amortisation_total: Option<f64>,

    // EV bridge items (balance sheet / notes).
    /// Non-controlling interest.
    pub minority_interest: Option<f64>,
    pub preferred_stock: Option<f64>,
    /// Equity method investments / associates.
    pub equity_investments: Option<f64>,
    /// Non-operating financial investments.
    pub financial_investments: Option<f64>,
    pub assets_held_for_sale: Option<f64>,
    pub discontinued_ops_assets: Option<f64>,
    /// NOL / Deferred Tax Assets (non-operating).
    pub nol_dta: Option<f64>,
    /// Projected/Defined Benefit Obligation.
    pub pension_pbo: Option<f64>,
    /// Plan assets at fair value.
    pub pension_plan_assets: Option<f64>,
    /// Operating leases per note.
    pub operating_lease_liabilities: Option<f64>,
    /// Finance/capital leases per note.
    pub finance_lease_liabilities: Option<f64>,

    // Debt components (used to sum total_debt when no explicit total).
    pub current_borrowings: Option<f64>,
    pub noncurrent_borrowings: Option<f64>,

    // Metadata.
    #[serde(default)]
    pub currency: String,
    /// IFRS or US GAAP.
    #[serde(default)]
    pub accounting_standard: String,
    #[serde(default)]
    pub source_sections: HashMap<String, String>,
    #[serde(default)]
    pub extraction_confidence: HashMap<String, f64>,
    #[serde(default)]
    pub raw_snippets: HashMap<String, String>,
    /// Maps field_name → PDF URL that was the source of that value.
    #[serde(default)]
    pub field_sources: HashMap<String, String>,
}

impl ExtractedFinancials {
    /// The 33 numeric fields, in dataclass declaration order, with their
    /// canonical names — used for provenance tagging and downstream mapping.
    fn numeric_fields(&self) -> [(&'static str, Option<f64>); 33] {
        [
            ("revenue", self.revenue),
            ("operating_income", self.operating_income),
            ("net_income", self.net_income),
            ("total_assets", self.total_assets),
            ("total_equity", self.total_equity),
            ("total_debt", self.total_debt),
            ("cash", self.cash),
            ("goodwill", self.goodwill),
            ("short_term_investments", self.short_term_investments),
            ("adjusted_ebitda", self.adjusted_ebitda),
            ("reported_ebitda", self.reported_ebitda),
            ("ebita", self.ebita),
            ("rou_depreciation", self.rou_depreciation),
            ("lease_interest", self.lease_interest),
            ("short_term_rent", self.short_term_rent),
            ("lease_liabilities_current", self.lease_liabilities_current),
            (
                "lease_liabilities_noncurrent",
                self.lease_liabilities_noncurrent,
            ),
            ("rou_assets", self.rou_assets),
            ("depreciation_total", self.depreciation_total),
            ("amortisation_total", self.amortisation_total),
            ("minority_interest", self.minority_interest),
            ("preferred_stock", self.preferred_stock),
            ("equity_investments", self.equity_investments),
            ("financial_investments", self.financial_investments),
            ("assets_held_for_sale", self.assets_held_for_sale),
            ("discontinued_ops_assets", self.discontinued_ops_assets),
            ("nol_dta", self.nol_dta),
            ("pension_pbo", self.pension_pbo),
            ("pension_plan_assets", self.pension_plan_assets),
            (
                "operating_lease_liabilities",
                self.operating_lease_liabilities,
            ),
            ("finance_lease_liabilities", self.finance_lease_liabilities),
            ("current_borrowings", self.current_borrowings),
            ("noncurrent_borrowings", self.noncurrent_borrowings),
        ]
    }

    /// Override balance-sheet items with fresher interim data; income-statement
    /// items stay from the annual report. Ported from `_overlay_interim_bs()`.
    pub fn overlay_interim_bs(&mut self, interim: &ExtractedFinancials) {
        macro_rules! ov {
            ($f:ident) => {
                if let Some(v) = interim.$f {
                    self.$f = Some(v);
                    if let Some(src) = interim.field_sources.get(stringify!($f)) {
                        self.field_sources
                            .insert(stringify!($f).to_string(), src.clone());
                    }
                }
            };
        }
        ov!(total_assets);
        ov!(total_equity);
        ov!(total_debt);
        ov!(cash);
        ov!(goodwill);
        ov!(short_term_investments);
        ov!(minority_interest);
        ov!(preferred_stock);
        ov!(equity_investments);
        ov!(financial_investments);
        ov!(assets_held_for_sale);
        ov!(discontinued_ops_assets);
        ov!(nol_dta);
        ov!(pension_pbo);
        ov!(pension_plan_assets);
        ov!(operating_lease_liabilities);
        ov!(finance_lease_liabilities);
        ov!(rou_assets);
        ov!(lease_liabilities_current);
        ov!(lease_liabilities_noncurrent);
        ov!(rou_depreciation);
        ov!(lease_interest);
    }

    /// Convert into the crate's [`ExtractionResult`] so `fetch_non_us_filing`
    /// can consume regex-extracted data. Populates currency, `years_found`
    /// (`[year]`), the three statements (single-period column), and a
    /// coverage-based confidence. Values are in the filing's reported unit.
    pub fn to_extraction_result(&self) -> ExtractionResult {
        let mut is = StatementData::new();
        let mut bs = StatementData::new();
        let cfs = StatementData::new();

        let put = |sd: &mut StatementData, key: &str, v: Option<f64>| {
            if let Some(x) = v {
                sd.insert(key.to_string(), vec![Some(x)]);
            }
        };

        put(&mut is, "revenue", self.revenue);
        put(&mut is, "ebit", self.operating_income);
        put(&mut is, "ebita", self.ebita);
        put(&mut is, "net_income", self.net_income);
        put(&mut is, "da", self.depreciation_total);

        put(&mut bs, "total_assets", self.total_assets);
        put(&mut bs, "total_equity", self.total_equity);
        put(&mut bs, "cash", self.cash);
        put(&mut bs, "goodwill", self.goodwill);
        put(&mut bs, "long_term_debt", self.total_debt);
        put(
            &mut bs,
            "short_term_investments",
            self.short_term_investments,
        );

        let years_found = if self.year.is_empty() {
            Vec::new()
        } else {
            vec![self.year.clone()]
        };

        // Coverage-based confidence over the core solvency/earnings fields.
        let core = [
            self.revenue,
            self.operating_income,
            self.net_income,
            self.total_assets,
            self.total_equity,
            self.cash,
        ];
        let found = core.iter().filter(|v| v.is_some()).count();
        let confidence = (0.3 + 0.65 * (found as f64 / core.len() as f64)).clamp(0.3, 0.95);

        ExtractionResult {
            currency: self.currency.clone(),
            years_found,
            income_statement: is,
            balance_sheet: bs,
            cash_flow_statement: cfs,
            notes: HashMap::new(),
            confidence,
            discrepancies: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Codepoint-indexed view over the text (matches Python str semantics)
// ---------------------------------------------------------------------------

/// A view over a string that supports Python-style codepoint indexing so the
/// anchor arithmetic (`+500`, `-3000`, `text[a:b]`) is byte/char-boundary safe
/// and matches the Python source exactly.
struct Indexed<'a> {
    s: &'a str,
    ascii_lower: String,
    /// Byte offset of char `i`; `b_at_c[nchars]` == `s.len()`.
    b_at_c: Vec<usize>,
}

impl<'a> Indexed<'a> {
    fn new(s: &'a str) -> Self {
        let mut b_at_c = Vec::with_capacity(s.len() + 1);
        for (b, _) in s.char_indices() {
            b_at_c.push(b);
        }
        b_at_c.push(s.len());
        Indexed {
            s,
            ascii_lower: s.to_ascii_lowercase(),
            b_at_c,
        }
    }

    fn nchars(&self) -> usize {
        self.b_at_c.len() - 1
    }

    /// Char index of a char-boundary byte offset (as returned by `str::find`).
    fn char_of_byte(&self, b: usize) -> usize {
        self.b_at_c.binary_search(&b).unwrap_or_else(|i| i)
    }

    /// Slice by char indices; clamps like Python `text[a:b]`.
    fn slice(&self, a: i64, b: i64) -> &str {
        let n = self.nchars() as i64;
        let a = a.clamp(0, n) as usize;
        let b = b.clamp(0, n) as usize;
        if a >= b {
            return "";
        }
        &self.s[self.b_at_c[a]..self.b_at_c[b]]
    }

    /// Python `text.find(pat)` → char index, or -1.
    fn find(&self, pat: &str) -> i64 {
        match self.s.find(pat) {
            Some(b) => self.char_of_byte(b) as i64,
            None => -1,
        }
    }

    /// Python `text.find(pat, start)` (start = char index) → char index, or -1.
    fn find_from(&self, pat: &str, start_char: i64) -> i64 {
        let start = start_char.clamp(0, self.nchars() as i64) as usize;
        let base = self.b_at_c[start];
        match self.s[base..].find(pat) {
            Some(o) => self.char_of_byte(base + o) as i64,
            None => -1,
        }
    }

    /// Python `text.rfind(pat, 0, end)` (end = char index) → char index, or -1.
    fn rfind_before(&self, pat: &str, end_char: i64) -> i64 {
        let end = end_char.clamp(0, self.nchars() as i64) as usize;
        let end_b = self.b_at_c[end];
        match self.s[..end_b].rfind(pat) {
            Some(b) => self.char_of_byte(b) as i64,
            None => -1,
        }
    }

    /// Case-insensitive find (ASCII fold) → char index, or -1. Mirrors
    /// `text.lower().find(marker.lower())` for the ASCII markers used here.
    fn find_ci(&self, pat_ascii_lower: &str) -> i64 {
        match self.ascii_lower.find(pat_ascii_lower) {
            Some(b) => self.char_of_byte(b) as i64,
            None => -1,
        }
    }
}

// ---------------------------------------------------------------------------
// Text normalization (ported from Python `extract_text()`)
// ---------------------------------------------------------------------------

/// Locale thousand-separators that some PDFs use between digit groups.
/// U+202F narrow no-break, U+00A0 nbsp, U+2009 thin, U+2007 figure, ASCII space.
const NUM_SEPS: [char; 5] = ['\u{202f}', '\u{00a0}', '\u{2009}', '\u{2007}', ' '];

fn is_num_sep(c: char) -> bool {
    NUM_SEPS.contains(&c)
}

/// One left-to-right non-overlapping pass of the Python regex
/// `(\d)[<seps>](\d{3})(?!\d)` → `\1,\2`.
fn norm_pass(chars: &[char]) -> Vec<char> {
    let n = chars.len();
    let mut out = Vec::with_capacity(n);
    let mut i = 0;
    while i < n {
        if i + 4 < n
            && chars[i].is_ascii_digit()
            && is_num_sep(chars[i + 1])
            && chars[i + 2].is_ascii_digit()
            && chars[i + 3].is_ascii_digit()
            && chars[i + 4].is_ascii_digit()
            && (i + 5 >= n || !chars[i + 5].is_ascii_digit())
        {
            out.push(chars[i]);
            out.push(',');
            out.push(chars[i + 2]);
            out.push(chars[i + 3]);
            out.push(chars[i + 4]);
            i += 5;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// Normalize report text: convert locale thousand-separators to commas
/// (two passes, for `1 234 567`) and em-dash minus → hyphen. Idempotent on
/// already-normalized text.
pub fn normalize_report_text(text: &str) -> String {
    // Fold CRLF → LF first: Python's fitz text is LF-only, so this is a no-op
    // on Python-equivalent input and hardens the Rust path against CRLF PDF
    // dumps (whose literal-`\n` anchor patterns would otherwise miss).
    let text = text.replace("\r\n", "\n");
    let pass1 = norm_pass(&text.chars().collect::<Vec<_>>());
    let pass2 = norm_pass(&pass1);
    pass2
        .into_iter()
        .collect::<String>()
        .replace('\u{2212}', "-")
}

// ---------------------------------------------------------------------------
// Regex helpers
// ---------------------------------------------------------------------------

/// Compile a pattern with `IGNORECASE | DOTALL` (the `_extract_amount` regime).
fn re_id(pat: &str) -> Regex {
    RegexBuilder::new(pat)
        .case_insensitive(true)
        .dot_matches_new_line(true)
        .build()
        .expect("valid ported regex")
}

/// Compile a pattern with no flags (the anchor / findall regime).
fn re_plain(pat: &str) -> Regex {
    Regex::new(pat).expect("valid ported regex")
}

fn strip_num(raw: &str) -> String {
    raw.chars().filter(|c| *c != ',' && *c != ' ').collect()
}

// ---------------------------------------------------------------------------
// _extract_amount
// ---------------------------------------------------------------------------

/// Extract a financial amount from text using regex patterns.
///
/// Ported verbatim from `_extract_amount()`. Returns the first match (across
/// patterns in order, then matches in order) that passes the section-specific
/// sanity check, in the filing's reported unit (thousands as-is or millions
/// scaled to thousands for income lines).
pub fn extract_amount(text: &str, patterns: &[&str], section: &str, _scale: &str) -> Option<f64> {
    for pat in patterns {
        let re = re_id(pat);
        for caps in re.captures_iter(text) {
            let raw = match caps.get(1) {
                Some(m) => m.as_str(),
                None => continue,
            };
            let cleaned = strip_num(raw);
            let val: f64 = match cleaned.parse() {
                Ok(v) => v,
                Err(_) => continue,
            };
            match section {
                "income_statement" | "adjusted_ebitda" => {
                    if val > 10.0 {
                        if raw.contains('.') && val < 10000.0 {
                            return Some(val * 1_000.0);
                        }
                        if raw.contains(',') {
                            return Some(val);
                        }
                        if val >= 100.0 && !raw.contains('.') {
                            return Some(val * 1_000.0);
                        }
                        return Some(val);
                    }
                }
                "nci" => {
                    if val >= 0.0 {
                        return Some(val);
                    }
                }
                "lease_note" | "finance_note" => {
                    if val > 10.0 {
                        return Some(val);
                    }
                }
                _ => {
                    if val > 1000.0 {
                        return Some(val);
                    }
                }
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// filing_type / is_valid_filing
// ---------------------------------------------------------------------------

/// Classify filing as `annual`, `quarterly`, `semi-annual`, or `unknown`.
/// Ported verbatim from `filing_type()`.
pub fn filing_type(text: &str) -> &'static str {
    let tl = text.to_lowercase();
    let any = |kws: &[&str]| kws.iter().any(|k| tl.contains(k));
    if any(&[
        "annual report",
        "full year",
        "full-year",
        "geschäftsbericht",
        "årsredovisning",
        "jaarverslag",
        "rapport annuel",
    ]) {
        return "annual";
    }
    if any(&[
        "q1 ",
        "q3 ",
        "first quarter",
        "third quarter",
        "nine months",
        "kvartalsrapport",
        "quartalsbericht",
    ]) {
        return "quarterly";
    }
    if any(&[
        "half-year",
        "half year",
        "six months",
        "interim report",
        "h1 ",
        "delårsrapport",
        "zwischenbericht",
    ]) {
        return "semi-annual";
    }
    "unknown"
}

/// Validate the text contains a balance sheet with enough pages for its type.
/// Ported verbatim from `is_valid_filing()` (page count supplied by caller).
pub fn is_valid_filing(text: &str, total_pages: usize) -> bool {
    let tl = text.to_lowercase();
    let any = |kws: &[&str]| kws.iter().any(|k| tl.contains(k));

    let has_balance_sheet = any(&[
        "balance sheet",
        "statement of financial position",
        "total assets",
        "total equity",
        "balansräkning",
        "bilanz",
        "bilanzsumme",
        "bilan",
    ]);
    if !has_balance_sheet {
        return false;
    }

    let is_quarterly_or_interim = any(&[
        "interim report",
        "quarterly report",
        "half-year report",
        "half year report",
        "q1 ",
        "q2 ",
        "q3 ",
        "first quarter",
        "second quarter",
        "third quarter",
        "six months",
        "delårsrapport",
        "kvartalsrapport",
        "zwischenbericht",
        "quartalsbericht",
    ]);

    let min_pages = if is_quarterly_or_interim { 8 } else { 40 };
    total_pages >= min_pages
}

// ---------------------------------------------------------------------------
// extract_financials
// ---------------------------------------------------------------------------

/// Extract structured financial data from annual report text.
///
/// Ported verbatim from `extract_financials()`. Input text is normalized up
/// front (see [`normalize_report_text`]) so a raw pdf-extract dump can be fed.
pub fn extract_financials(
    text: &str,
    company: &str,
    year: &str,
    pdf_url: &str,
) -> ExtractedFinancials {
    let text = normalize_report_text(text);
    let ix = Indexed::new(&text);

    let mut fin = ExtractedFinancials {
        company: company.to_string(),
        year: year.to_string(),
        ..Default::default()
    };

    // Detect accounting standard (first 50k chars).
    let head50 = ix.slice(0, 50_000);
    if head50.contains("IFRS") || head50.to_lowercase().contains("ifrs") {
        fin.accounting_standard = "IFRS".to_string();
    } else if head50.contains("US GAAP") || head50.contains("GAAP") {
        fin.accounting_standard = "US GAAP".to_string();
    }

    // Detect currency (first 10k chars, case-sensitive, order matters).
    let head10 = ix.slice(0, 10_000);
    const CURRENCIES: &[(&str, &[&str])] = &[
        ("SEK", &["MSEK", "BSEK", "SEK", "Swedish krona", "kronor"]),
        ("EUR", &["€", "EUR", "euro", "MEUR"]),
        ("USD", &["$", "USD", "dollar"]),
        ("GBP", &["£", "GBP", "sterling"]),
        ("DKK", &["MDKK", "DKK", "Danish krone"]),
        ("NOK", &["MNOK", "NOK", "Norwegian krone"]),
        ("CHF", &["MCHF", "CHF", "Swiss franc"]),
        ("INR", &["₹", "INR", "rupee"]),
    ];
    for &(curr, symbols) in CURRENCIES {
        if symbols.iter().any(|s| head10.contains(s)) {
            fin.currency = curr.to_string();
            break;
        }
    }

    // --- Section anchors -----------------------------------------------------
    // Balance sheet: anchor on "TOTAL ASSETS", search backwards for header.
    let ta_pos = ix.find("TOTAL ASSETS").max(ix.find("Total assets"));
    let bs_start = if ta_pos > 0 {
        ix.rfind_before("Consolidated balance sheet", ta_pos)
            .max(ix.rfind_before("Balance sheet", ta_pos))
    } else {
        ix.find("Consolidated balance sheet")
            .max(ix.find("Balance sheet"))
    };

    // Income statement: "Revenues\n<note>\n<value>" distinguishes the real IS.
    let rev_note_re = re_plain(r"Revenues?\n\d{1,3}\n\d{1,3}(?:,\d{3})+");
    let mut is_start: i64;
    if let Some(m) = rev_note_re.find(&text) {
        let anchor = ix.char_of_byte(m.start()) as i64;
        is_start = ix
            .rfind_before("Consolidated income statement", anchor + 500)
            .max(ix.rfind_before("Income statement", anchor + 500));
        if is_start < 0 {
            is_start = (anchor - 3000).max(0);
        }
    } else {
        let rev_inline_re = re_plain(r"Revenue\s+\d+\s+\d{1,3}(?:,\d{3})+");
        let last = rev_inline_re.find_iter(&text).last();
        if let Some(m) = last {
            let anchor = ix.char_of_byte(m.start()) as i64;
            is_start = ix
                .rfind_before("Consolidated income statement", anchor + 500)
                .max(ix.rfind_before("Income statement", anchor + 500));
            if is_start < 0 {
                is_start = (anchor - 3000).max(0);
            }
        } else {
            is_start = ix
                .find("Consolidated income statement")
                .max(ix.find("Income statement"))
                .max(0);
        }
    }
    if is_start < 0 {
        is_start = 0;
    }

    let is_end = if bs_start > is_start && is_start > 0 {
        bs_start
    } else {
        is_start + 30_000
    };

    let bs_end_markers = [
        "Consolidated statement of changes in equity",
        "Statement of changes in equity",
        "Parent company",
        "PARENT COMPANY",
        "Financial statements (Parent)",
    ];
    let mut bs_end = ix.nchars() as i64;
    for m in bs_end_markers {
        let pos = if bs_start > 0 {
            ix.find_from(m, bs_start + 100)
        } else {
            -1
        };
        if pos > 0 && pos < bs_end {
            bs_end = pos + 5000;
        }
    }

    let fs_text = if is_start > 0 {
        ix.slice(is_start, is_end)
    } else {
        ix.slice(0, 30_000)
    };
    let bs_text = if bs_start > 0 {
        ix.slice(bs_start, bs_end)
    } else {
        &text
    };

    // --- Revenue -------------------------------------------------------------
    fin.revenue = extract_amount(
        fs_text,
        &[
            r"Revenues?\n\d+\n(\d{1,3}(?:,\d{3})+)",
            r"Revenue\s+\d+\s+(\d{1,3}(?:,\d{3})+)",
            r"Revenues?\s+(\d{1,3}(?:,\d{3}){2,})\s+\d{1,3}(?:,\d{3}){2,}",
            r"(?:Net\s+)?[Rr]evenue[s]?\s+(\d{1,3}(?:,\d{3}){2,})",
        ],
        "income_statement",
        "auto",
    );
    if fin.revenue.is_none() {
        fin.revenue = extract_amount(
            &text,
            &[
                r"Revenues?\n\d+\n(\d{1,3}(?:,\d{3})+)",
                r"Revenue\s+\d+\s+(\d{1,3}(?:,\d{3})+)",
            ],
            "income_statement",
            "auto",
        );
    }

    // --- Operating income / EBIT --------------------------------------------
    fin.operating_income = extract_amount(
        fs_text,
        &[
            r"Operating\s+profit\n[^\n]+\n(\d{1,3}(?:,\d{3})+)",
            r"Operating\s+profit\n\s*\n(\d{1,3}(?:,\d{3})+)",
            r"Operating\s+profit\s{2,}[\d,\s]+?(\d{1,3}(?:,\d{3})+)",
            r"(?:Operating|Trading)\s+result\n[^\n]+\n(\d{1,3}(?:,\d{3})+)",
            r"(?:Operating|Trading)\s+result\s+.*?(\d{1,3}(?:,\d{3})+)",
            r"Operating\s+income\n[^\n]+\n(\d{1,3}(?:,\d{3})+)",
            r"Operating\s+income\s+.*?(\d{1,3}(?:,\d{3})+)",
            r"Result\s+from\s+operations?\s+.*?(\d{1,3}(?:,\d{3})+)",
            r"EBIT\s+.*?(\d{1,3}(?:,\d{3})+)",
        ],
        "income_statement",
        "auto",
    );

    // --- EBITA ---------------------------------------------------------------
    fin.ebita = extract_amount(
        fs_text,
        &[
            r"EBITA\s+\n?\s*(\d{1,3}(?:,\d{3})+)",
            r"EBITA\s{2,}.*?(\d{1,3}(?:,\d{3})+)",
            r"Adjusted\s+EBITA\s+\n?\s*(\d{1,3}(?:,\d{3})+)",
        ],
        "income_statement",
        "auto",
    );

    // --- Net income ----------------------------------------------------------
    fin.net_income = extract_amount(
        fs_text,
        &[
            r"Profit\s+for\s+the\s+year\n\s*\n(\d{1,3}(?:,\d{3})+)",
            r"Profit\s+for\s+the\s+(?:financial\s+)?year\s+.*?(\d{1,3}(?:,\d{3})+)",
            r"Net\s+result\s+.*?(\d{1,3}(?:,\d{3})+)",
            r"Net\s+income\s+.*?(\d{1,3}(?:,\d{3})+)",
            r"Net\s+profit\s+(?:for\s+the\s+year\s+)?.*?(\d{1,3}(?:,\d{3})+)",
        ],
        "income_statement",
        "auto",
    );

    // --- Total assets --------------------------------------------------------
    fin.total_assets = extract_amount(
        bs_text,
        &[
            r"TOTAL\s+ASSETS\s+\n?\s*(\d{1,3}(?:,\d{3})+)",
            r"Total\s+assets\s+\n?\s*(\d{1,3}(?:,\d{3})+)",
        ],
        "balance_sheet",
        "auto",
    );

    // --- Total equity --------------------------------------------------------
    fin.total_equity = extract_amount(
        bs_text,
        &[
            r"TOTAL\s+EQUITY\s+\n?\s*(\d{1,3}(?:,\d{3})+)",
            r"Total\s+equity\s+\n?\s*(\d{1,3}(?:,\d{3})+)",
            r"(?:Group\s+)?(?:Total\s+)?[Ee]quity\s+\n?\s*(\d{1,3}(?:,\d{3})+)",
        ],
        "balance_sheet",
        "auto",
    );

    // --- Cash ----------------------------------------------------------------
    fin.cash = extract_amount(
        bs_text,
        &[
            r"Cash\s+and\s+cash\s+equivalents\n\d*\n(\d{1,3}(?:,\d{3})+)",
            r"Cash\s+and\s+cash\s+equivalents\s+\d*\s+(\d{1,3}(?:,\d{3})+)",
            r"Cash\s+and\s+cash\s+equivalents\s+(\d{1,3}(?:,\d{3})+)",
        ],
        "balance_sheet",
        "auto",
    );

    // --- Total debt ----------------------------------------------------------
    fin.total_debt = extract_amount(
        bs_text,
        &[
            r"(?:Total\s+)?[Ff]inancial\s+(?:debt|indebtedness)\s+\n?\s*(\d{1,3}(?:,\d{3})+)",
            r"[Tt]otal\s+(?:interest[\s-]bearing\s+)?[Bb]orrowings\s+\n?\s*(\d{1,3}(?:,\d{3})+)",
            r"[Tt]otal\s+[Dd]ebt\s+\n?\s*(\d{1,3}(?:,\d{3})+)",
            r"[Ll]oans\s+and\s+borrowings\s+.*?(\d{1,3}(?:,\d{3})+)",
            r"[Ii]nterest[-\s]bearing\s+(?:debt|liabilities)\s+.*?(\d{1,3}(?:,\d{3})+)",
        ],
        "balance_sheet",
        "auto",
    );
    if fin.total_debt.is_none() {
        let nc_re = re_plain(r"[Bb]orrowings\n\s*\d+\s*\n(\d{1,3}(?:,\d{3})+)");
        let inline_re = re_plain(r"[Bb]orrowings\s+\d+\s+(\d{1,3}(?:,\d{3})+)");
        let nc: Vec<String> = nc_re
            .captures_iter(bs_text)
            .filter_map(|c| c.get(1).map(|m| m.as_str().to_string()))
            .collect();
        let all_vals: Vec<String> = if !nc.is_empty() {
            nc
        } else {
            inline_re
                .captures_iter(bs_text)
                .filter_map(|c| c.get(1).map(|m| m.as_str().to_string()))
                .collect()
        };
        if all_vals.len() >= 2 {
            fin.total_debt = Some(
                all_vals[..2]
                    .iter()
                    .map(|v| v.replace(',', "").parse::<f64>().unwrap_or(0.0))
                    .sum(),
            );
        } else if let Some(first) = all_vals.first() {
            fin.total_debt = first.replace(',', "").parse::<f64>().ok();
        }
        if fin.total_debt.is_none() {
            fin.total_debt = extract_amount(
                bs_text,
                &[r"[Ll]ong[-\s]term\s+debt.{0,40}?(\d{1,3}(?:,\d{3})+)"],
                "balance_sheet",
                "auto",
            );
        }
    }

    // --- Goodwill ------------------------------------------------------------
    fin.goodwill = extract_amount(
        bs_text,
        &[
            r"Goodwill\n\d+\n(\d{1,3}(?:,\d{3})+)",
            r"Goodwill\s+\d+\s+(\d{1,3}(?:,\d{3})+)",
            r"Goodwill\s+(\d{1,3}(?:,\d{3})+)",
        ],
        "balance_sheet",
        "auto",
    );

    // --- Short-term investments ---------------------------------------------
    fin.short_term_investments = extract_amount(
        bs_text,
        &[
            r"[Ss]hort[-\s]term\s+investments\s+.*?(\d{1,3}(?:,\d{3})+)",
            r"[Mm]arketable\s+securities\s+.*?(\d{1,3}(?:,\d{3})+)",
        ],
        "balance_sheet",
        "auto",
    );

    // --- Adjusted EBITDA (Tier 1) -------------------------------------------
    let tg_re = re_id(
        r"Total\s+Group[\s\S]{0,200}?(\d{1,3}(?:,\d{3})+)\s+(\d{1,3}(?:,\d{3})*\.\d+)\s+(\d{1,3}(?:,\d{3})+)",
    );
    if let Some(caps) = tg_re.captures(ix.slice(0, 200_000)) {
        if let Some(g2) = caps.get(2) {
            let tg_adj = g2.as_str().replace(',', "").parse::<f64>().unwrap_or(0.0);
            if tg_adj > 100.0 && tg_adj < 10000.0 {
                fin.adjusted_ebitda = Some(tg_adj * 1_000.0);
            }
        }
    }
    if fin.adjusted_ebitda.is_none() {
        fin.adjusted_ebitda = extract_amount(
            &text,
            &[
                r"[Aa]djusted\s+EBITDA.{0,60}?(?:EUR|EUR|of\s+)?\s*(\d+\.?\d*)\s*(?:million|mln)",
                r"[Aa]djusted\s+EBITDA\s+was.{0,40}?(?:EUR|EUR)?\s*(\d+\.?\d*)\s*(?:million|mln)",
                r"[Uu]nderlying\s+EBITDA.{0,30}?(?:EUR|EUR)?\s*(\d+\.?\d*)\s*(?:million|mln)",
            ],
            "adjusted_ebitda",
            "auto",
        );
    }

    // --- Reported EBITDA (Tier 2) -------------------------------------------
    fin.reported_ebitda = extract_amount(
        &text,
        &[
            r"(?:^|\n)\s*EBITDA\s+[^\n]*?(\d{1,3}(?:,\d{3})+)",
            r"[Rr]eported\s+EBITDA[^\n]{0,30}?(\d{1,3}(?:,\d{3})+)",
            r"EBITDA\n[^\n]+\n(\d{1,3}(?:,\d{3})+)",
        ],
        "reported_ebitda",
        "auto",
    );

    // Sanity checks vs EBIT.
    if let (Some(ae), Some(oi)) = (fin.adjusted_ebitda, fin.operating_income) {
        if ae < oi * 0.5 || ae > oi * 5.0 {
            fin.adjusted_ebitda = None;
        }
    }
    if let (Some(re), Some(oi)) = (fin.reported_ebitda, fin.operating_income) {
        if re < oi * 0.5 || re > oi * 5.0 {
            fin.reported_ebitda = None;
        }
    }

    // --- D&A total (from cash flow statement) -------------------------------
    fin.depreciation_total = extract_amount(
        &text,
        &[
            r"Depreciation,\s*amorti[sz]ation\s+and\s+impairment\n[^\n]+\n(\d{1,3}(?:,\d{3})+)",
            r"Depreciation\s+and\s+amorti[sz]ation\n[^\n]*\n(\d{1,3}(?:,\d{3})+)",
            r"Depreciation\s+and\s+amorti[sz]ation\s+\(?\s*(\d{1,3}(?:,\d{3})+)\s*\)?",
            r"Depreciation,\s*amorti[sz]ation\s+\(?\s*(\d{1,3}(?:,\d{3})+)\s*\)?",
        ],
        "income_statement",
        "auto",
    );

    // --- EV bridge balance sheet items --------------------------------------
    fin.minority_interest = extract_amount(
        bs_text,
        &[
            r"[Nn]on[-\s]controlling\s+interests?\n\s*\n(\d{1,3}(?:,\d{3})*)",
            r"[Nn]on[-\s]controlling\s+interests?\s+\n?\s*(\d{1,3}(?:,\d{3})*)",
            r"[Mm]inority\s+(?:interest|equity)\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)",
            r"[Nn]on[-\s]controlling\s+interests?\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)",
        ],
        "nci",
        "auto",
    );

    fin.preferred_stock = extract_amount(
        &text,
        &[
            r"[Pp]referred\s+(?:stock|shares|equity)\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)",
            r"[Pp]reference\s+shares?\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)",
        ],
        "balance_sheet",
        "auto",
    );

    fin.equity_investments = extract_amount(
        &text,
        &[
            r"[Ii]nvestments?\s+(?:in|accounted\s+for\s+using\s+the\s+)?(?:equity[-\s]?(?:accounted|method)\s+)?(?:associates?|joint\s+ventures?)\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)",
            r"[Ee]quity[-\s]?(?:accounted|method)\s+invest(?:ments|ees)\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)",
            r"[Ii]nvestments?\s+in\s+associates?\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)",
            r"[Ii]nterests?\s+in\s+associates?\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)",
        ],
        "balance_sheet",
        "auto",
    );

    fin.financial_investments = extract_amount(
        &text,
        &[
            r"[Ff]inancial\s+(?:assets?\s+at\s+fair\s+value\s+through\s+(?:profit|OCI|other)|investments?\s+\(?non[-\s]?(?:operating|current)\)?)\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)",
            r"[Oo]ther\s+(?:long[-\s]term\s+)?(?:financial\s+)?investments?\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)",
            r"[Nn]on[-\s]current\s+financial\s+assets\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)",
            r"[Ll]ong[-\s]term\s+financial\s+investments?\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)",
        ],
        "balance_sheet",
        "auto",
    );

    fin.assets_held_for_sale = extract_amount(
        &text,
        &[
            r"[Aa]ssets?\s+(?:classified\s+as\s+)?held\s+for\s+sale\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)",
            r"[Nn]on[-\s]current\s+assets?\s+held\s+for\s+sale\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)",
        ],
        "balance_sheet",
        "auto",
    );

    fin.discontinued_ops_assets = extract_amount(
        &text,
        &[
            r"[Dd]iscontinued\s+operations?\s+(?:assets?|net\s+assets?)\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)",
            r"[Aa]ssets?\s+(?:of|from)\s+discontinued\s+operations?\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)",
        ],
        "balance_sheet",
        "auto",
    );

    fin.nol_dta = extract_amount(
        &text,
        &[
            r"[Dd]eferred\s+tax\s+assets?\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)",
            r"[Nn]et\s+operating\s+loss\s+(?:carry[-\s]?forwards?|DTA)\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)",
            r"[Tt]ax\s+loss\s+carry[-\s]?forwards?\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)",
        ],
        "balance_sheet",
        "auto",
    );

    // --- Pension footnote data ----------------------------------------------
    let pension_markers = [
        "Defined benefit",
        "Pension commitment",
        "Post-employment benefit",
        "Pension obligations",
        "Employee benefit obligations",
        "Retirement benefit obligation",
        "Pension plans",
        "defined benefit obligation",
        "pension liability",
    ];
    let mut pension_section = "";
    for marker in pension_markers {
        let idx = ix.find_ci(&marker.to_ascii_lowercase());
        if idx > 0 {
            pension_section = ix.slice(idx, idx + 15_000);
            break;
        }
    }
    if !pension_section.is_empty() {
        fin.pension_pbo = extract_amount(
            pension_section,
            &[
                r"[Pp]resent\s+value\s+of\s+(?:the\s+)?(?:defined\s+benefit\s+)?obligations?\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)",
                r"[Dd]efined\s+benefit\s+obligations?\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)",
                r"[Pp]rojected\s+benefit\s+obligations?\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)",
                r"[Bb]enefit\s+obligations?(?:\s+at\s+(?:fair\s+value|present\s+value))?\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)",
                r"[Pp]ension\s+(?:obligations?|liability)\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)",
            ],
            "pension_note",
            "auto",
        );
        fin.pension_plan_assets = extract_amount(
            pension_section,
            &[
                r"[Ff]air\s+value\s+of\s+(?:plan\s+)?assets?\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)",
                r"[Pp]lan\s+assets?\s+(?:at\s+fair\s+value\s+)?.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)",
                r"[Aa]ssets?\s+of\s+(?:the\s+)?(?:defined\s+benefit\s+)?plans?\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)",
            ],
            "pension_note",
            "auto",
        );
        if fin.pension_pbo.is_some() && fin.pension_plan_assets.is_none() {
            fin.pension_plan_assets = extract_amount(
                pension_section,
                &[r"(?:^|\n)\s*(?:Plan\s+)?[Aa]ssets?\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)"],
                "pension_note",
                "auto",
            );
        }
    }

    // --- Operating vs finance lease liabilities (from lease footnote) -------
    let lease_markers = [
        "Lease liabilities",
        "Lease commitments",
        "Right-of-use",
        "IFRS 16",
        "ASC 842",
        "Leases (Note",
        "Note 15",
        "lease liability maturity",
    ];
    let mut lease_note_section = "";
    for marker in lease_markers {
        let idx = ix.find(marker);
        if idx > 0 {
            lease_note_section = ix.slice(idx, idx + 10_000);
            break;
        }
    }
    if !lease_note_section.is_empty() {
        fin.operating_lease_liabilities = extract_amount(
            lease_note_section,
            &[
                r"[Oo]perating\s+lease\s+(?:liabilit|obligation).{0,40}?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)",
                r"[Nn]on[-\s]current\s+lease\s+liabilit.{0,40}?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)",
            ],
            "lease_note",
            "auto",
        );
        if fin.operating_lease_liabilities.is_none() {
            if let Some(nc) = fin.lease_liabilities_noncurrent {
                fin.operating_lease_liabilities = Some(nc);
            }
        }
        fin.finance_lease_liabilities = extract_amount(
            lease_note_section,
            &[
                r"[Ff]inance\s+lease\s+(?:liabilit|obligation).{0,40}?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)",
                r"[Cc]apital\s+lease\s+(?:liabilit|obligation).{0,40}?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)",
            ],
            "lease_note",
            "auto",
        );
    }

    // --- IFRS 16 lease data --------------------------------------------------
    fin.rou_depreciation = extract_amount(
        &text,
        &[
            r"Depreciation\s+expense\s+of\s+right[-\s]of[-\s]use\s+assets?\s+.*?(\d{1,3}(?:,\d{3})+)",
            r"[Dd]epreciation.{0,30}right[-\s]of[-\s]use.{0,30}?(\d{1,3}(?:,\d{3})+)",
            r"[Dd]epreciation.{0,50}right[-\s]of[-\s]use.{0,100}\n[^\n\d]*(\d{1,3}(?:,\d{3})+)",
            r"[Rr]ight[-\s]of[-\s]use\s+assets?\s+(?:depreciation|amortisation).{0,80}?(\d{1,3}(?:,\d{3})+)",
            r"[Dd]epreciation,?\s+right[-\s]of[-\s]use\s+assets?\s+(\d{1,3}(?:,\d{3})+)",
            r"[Dd]epreciation\s+of\s+right[-\s]of[-\s]use\s+assets?\s+(\d{1,3}(?:,\d{3})+)",
        ],
        "lease_note",
        "auto",
    );

    fin.lease_interest = extract_amount(
        &text,
        &[
            r"Interest\s+expense\s+on\s+lease\s+liabilities[^\n]*\n[^\n\d]*(\d{1,3}(?:,\d{3})*)",
            r"Interest\s+expense\s+on\s+lease\s+liabilities[^\n]*?(\d{1,3}(?:,\d{3})+)",
            r"[Ii]nterest.{0,30}lease\s+liabilit[^\n]*\n[^\n\d]*(\d{1,3}(?:,\d{3})*)",
        ],
        "finance_note",
        "auto",
    );

    fin.short_term_rent = extract_amount(
        &text,
        &[
            r"[Rr]ent\s+expenses?\s+.*?short[-\s]term\s+leases?\s+.*?(\d{1,3}(?:,\d{3})+)",
            r"[Ss]hort[-\s]term\s+lease.{0,80}?(\d{1,3}(?:,\d{3}){2,})",
        ],
        "lease_note",
        "auto",
    );

    fin.lease_liabilities_current = extract_amount(
        bs_text,
        &[
            r"[Ll]ease\s+liabilities?\n\d*\n(\d{1,3}(?:,\d{3})+)",
            r"[Cc]urrent\s+lease\s+liabilities?\s+\n?\s*(\d{1,3}(?:,\d{3})+)",
        ],
        "balance_sheet",
        "auto",
    );

    fin.lease_liabilities_noncurrent = extract_amount(
        bs_text,
        &[r"[Nn]on[-\s]current\s+lease\s+liabilities?\s+\n?\s*(\d{1,3}(?:,\d{3})+)"],
        "balance_sheet",
        "auto",
    );

    fin.rou_assets = extract_amount(
        bs_text,
        &[
            r"Right[-\s]of[-\s]use\s+assets?\n\d+\s*\n(\d{1,3}(?:,\d{3})+)",
            r"Right[-\s]of[-\s]use\s+assets?\s+\d+\s+(\d{1,3}(?:,\d{3})+)",
            r"Right[-\s]of[-\s]use\s+assets?\s+(\d{1,3}(?:,\d{3})+)",
        ],
        "balance_sheet",
        "auto",
    );

    // Tag every extracted field with its source PDF URL for audit trail.
    if !pdf_url.is_empty() {
        for (name, val) in fin.numeric_fields() {
            if val.is_some() {
                fin.field_sources
                    .insert(name.to_string(), pdf_url.to_string());
            }
        }
    }

    fin
}

#[cfg(test)]
mod tests;
