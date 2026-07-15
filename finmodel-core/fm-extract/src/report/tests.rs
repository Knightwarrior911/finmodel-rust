use super::*;

const SNIPPET: &str = "\
Nestlé prepares its consolidated accounts under IFRS.
Consolidated income statement
MSEK Note 2024
Revenues
3
122,878
Cost of sales
-73,742
Operating profit
5
18,420
Profit for the year

12,245
Consolidated balance sheet
MSEK Note 2024
Goodwill
14
70,323
Cash and cash equivalents
18
4,528
TOTAL ASSETS
184,384
TOTAL EQUITY
96,999
";

#[test]
fn extracts_core_line_items_from_nordic_format() {
    let fin = extract_financials(SNIPPET, "Sandvik AB", "2024", "");
    assert_eq!(fin.revenue, Some(122_878.0));
    assert_eq!(fin.operating_income, Some(18_420.0));
    assert_eq!(fin.net_income, Some(12_245.0));
    assert_eq!(fin.total_assets, Some(184_384.0));
    assert_eq!(fin.total_equity, Some(96_999.0));
    assert_eq!(fin.cash, Some(4_528.0));
    assert_eq!(fin.goodwill, Some(70_323.0));
    // No debt line present → never fabricated.
    assert_eq!(fin.total_debt, None);
    assert_eq!(fin.currency, "SEK");
    assert_eq!(fin.accounting_standard, "IFRS");
}

#[test]
fn conversion_populates_currency_and_years() {
    let fin = extract_financials(SNIPPET, "Sandvik AB", "2024", "");
    let res = fin.to_extraction_result();
    assert!(!res.currency.is_empty(), "currency must be non-empty");
    assert_eq!(res.currency, "SEK");
    assert!(!res.years_found.is_empty(), "years_found must be non-empty");
    assert_eq!(res.years_found, vec!["2024".to_string()]);
    assert_eq!(
        res.income_statement.get("revenue"),
        Some(&vec![Some(122_878.0)])
    );
    assert_eq!(
        res.balance_sheet.get("total_assets"),
        Some(&vec![Some(184_384.0)])
    );
    assert!(res.confidence > 0.3 && res.confidence <= 0.95);
}

#[test]
fn field_sources_tagged_only_when_pdf_url_given() {
    let no_url = extract_financials(SNIPPET, "X", "2024", "");
    assert!(no_url.field_sources.is_empty());

    let with_url = extract_financials(SNIPPET, "X", "2024", "https://x.com/ar.pdf");
    assert_eq!(
        with_url.field_sources.get("revenue").map(String::as_str),
        Some("https://x.com/ar.pdf")
    );
    // A field the filing lacks is not tagged.
    assert!(!with_url.field_sources.contains_key("total_debt"));
}

#[test]
fn overlay_interim_bs_overrides_balance_sheet_keeps_income() {
    let mut annual = ExtractedFinancials {
        revenue: Some(500.0),
        net_income: Some(60.0),
        total_assets: Some(100.0),
        cash: Some(10.0),
        total_equity: Some(40.0),
        ..Default::default()
    };
    let mut interim = ExtractedFinancials {
        revenue: Some(9999.0), // must NOT overwrite (income statement)
        total_assets: Some(120.0),
        cash: Some(15.0),
        ..Default::default()
    };
    interim
        .field_sources
        .insert("total_assets".to_string(), "https://x/interim.pdf".to_string());

    annual.overlay_interim_bs(&interim);

    assert_eq!(annual.revenue, Some(500.0), "income statement preserved");
    assert_eq!(annual.net_income, Some(60.0));
    assert_eq!(annual.total_assets, Some(120.0), "BS overridden");
    assert_eq!(annual.cash, Some(15.0), "BS overridden");
    assert_eq!(annual.total_equity, Some(40.0), "interim None → keep annual");
    assert_eq!(
        annual.field_sources.get("total_assets").map(String::as_str),
        Some("https://x/interim.pdf"),
        "provenance follows the interim source"
    );
}

#[test]
fn filing_type_classifies() {
    assert_eq!(filing_type("This ANNUAL REPORT for 2024 ..."), "annual");
    assert_eq!(
        filing_type("Interim report — Q3 results for the third quarter"),
        "quarterly"
    );
    assert_eq!(
        filing_type("Half-year report for the six months ended June"),
        "semi-annual"
    );
    assert_eq!(filing_type("A press release with no clues"), "unknown");
}

#[test]
fn is_valid_filing_gates_on_balance_sheet_and_pages() {
    let annual = "Consolidated balance sheet ... total assets ... total equity";
    assert!(is_valid_filing(annual, 50));
    assert!(!is_valid_filing(annual, 20), "annual needs 40+ pages");

    let interim = "Interim report balance sheet total assets ...";
    assert!(is_valid_filing(interim, 10), "interim needs only 8+ pages");
    assert!(!is_valid_filing(interim, 5));

    let no_bs = "A marketing brochure about sustainability";
    assert!(!is_valid_filing(no_bs, 100));
}

#[test]
fn extract_amount_scaling_and_sanity() {
    // Comma number → returned as-is.
    assert_eq!(
        extract_amount("Revenue 12 1,234,567", &[r"Revenue\s+\d+\s+(\d{1,3}(?:,\d{3})+)"], "income_statement", "auto"),
        Some(1_234_567.0)
    );
    // Decimal < 10000 in an income line → millions → thousands.
    assert_eq!(
        extract_amount("Adjusted EBITDA of 400.3 million", &[r"[Aa]djusted\s+EBITDA.{0,60}?(\d+\.?\d*)\s*(?:million|mln)"], "adjusted_ebitda", "auto"),
        Some(400_300.0)
    );
    // Generic section requires > 1000.
    assert_eq!(
        extract_amount("Item 999", &[r"Item\s+(\d+)"], "balance_sheet", "auto"),
        None
    );
}

#[test]
fn normalize_report_text_converts_locale_separators() {
    // Narrow no-break + regular space thousand separators → commas.
    assert_eq!(normalize_report_text("168\u{202f}343"), "168,343");
    assert_eq!(normalize_report_text("1 234 567"), "1,234,567");
    // Already-normalized text is unchanged (idempotent).
    assert_eq!(normalize_report_text("1,234,567"), "1,234,567");
    // Em-dash minus → hyphen.
    assert_eq!(normalize_report_text("\u{2212}42"), "-42");
}

// ---------------------------------------------------------------------------
// Golden parity gate vs Python `extract_financials` over pinned filings.
//
// Fixtures are generated + committed by `tieout/build_report_extract_oracle.py`
// (downloads the curated pinned PDFs, extracts + normalizes text exactly like
// Python `extract_text()`, runs the Python `extract_financials`, and writes the
// normalized text + golden JSON). This test reads the SAME committed text,
// runs the Rust extractor, and field-by-field tolerance-diffs — mirroring
// fm-tieout's compare pattern (round-to-integer equality).
// ---------------------------------------------------------------------------

fn groundtruth_dir() -> std::path::PathBuf {
    // <crate>/../../tieout/groundtruth
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tieout")
        .join("groundtruth")
}

fn assert_parity(ticker: &str) {
    let gt = groundtruth_dir();
    let text_path = gt.join("report_text").join(format!("{ticker}.txt"));
    let gold_path = gt.join("report_extract").join(format!("{ticker}.json"));
    let text = std::fs::read_to_string(&text_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", text_path.display()));
    let gold_raw = std::fs::read_to_string(&gold_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", gold_path.display()));
    let golden: ExtractedFinancials =
        serde_json::from_str(&gold_raw).expect("golden JSON parses into ExtractedFinancials");

    // Same company/year the oracle used; pdf_url empty so field_sources stays bare.
    let rust = extract_financials(&text, &golden.company, &golden.year, "");

    // Metadata parity.
    assert_eq!(
        rust.currency, golden.currency,
        "[{ticker}] currency: rust={:?} py={:?}",
        rust.currency, golden.currency
    );
    assert_eq!(
        rust.accounting_standard, golden.accounting_standard,
        "[{ticker}] accounting_standard"
    );

    // Field-by-field numeric parity (round-to-integer tolerance).
    let gnum = golden.numeric_fields();
    let rnum = rust.numeric_fields();
    let mut mismatches: Vec<String> = Vec::new();
    for (i, (name, g)) in gnum.iter().enumerate() {
        let r = rnum[i].1;
        let ok = match (r, *g) {
            (None, None) => true,
            (Some(a), Some(b)) => (a - b).abs() <= 0.5,
            _ => false,
        };
        if !ok {
            mismatches.push(format!("{name}: rust={r:?} py={g:?}"));
        }
    }
    assert!(
        mismatches.is_empty(),
        "[{ticker}] {} field mismatch(es):\n  {}",
        mismatches.len(),
        mismatches.join("\n  ")
    );

    // A pinned filing must actually populate something (non-empty golden guard).
    let any_found = gnum.iter().any(|(_, v)| v.is_some());
    assert!(any_found, "[{ticker}] golden has no numeric fields");
}

#[test]
fn parity_vs_python_golden_sandvik() {
    assert_parity("SAND_ST");
}

#[test]
fn parity_vs_python_golden_basf() {
    assert_parity("BAS_DE");
}
