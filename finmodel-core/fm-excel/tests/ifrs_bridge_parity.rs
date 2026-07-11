//! IFRS-16 bridge worksheet parity gate — proves
//! `fm_excel::bridge::build_ifrs_bridge_sheet` reproduces the Python
//! `ResearchExcelWriter.write_ifrs_bridge` oracle
//! (`tieout/build_ifrs_bridge_oracle.py` → `IFRS_BRIDGE{,_SIMPLE}_snapshot.json`)
//! cell-for-cell (value + formula + fill). Two variants pin the branchy layout:
//! FULL (adjusted EBITDA + EBITA + margins, IFRS→US GAAP) and SIMPLE (computed
//! EBITDA, no EBITA, no margins, US GAAP→IFRS).

use fm_excel::bridge::{build_ifrs_bridge_sheet, IfrsBridgeInput};
use fm_excel::model::Workbook;
use fm_excel::snapshot::{compare_workbook, load_snapshot};

const PINNED_GENERATED: &str =
    "Generated: 2026-01-01 00:00 | Source: SEC EDGAR / yfinance / Company filings";

fn excluded() -> Vec<String> {
    vec!["Short-term rent (already OPEX in both frameworks)".to_string()]
}

fn snap(name: &str) -> String {
    format!("{}/../../tieout/excel_snapshots/{}_snapshot.json", env!("CARGO_MANIFEST_DIR"), name)
}

fn gate(inp: &IfrsBridgeInput, oracle: &str) {
    let mut wb = Workbook::new();
    wb.push(build_ifrs_bridge_sheet(inp, PINNED_GENERATED));
    let s = load_snapshot(&snap(oracle)).expect("load IFRS oracle");
    let diffs = compare_workbook(&wb, &s);
    if !diffs.is_empty() {
        let shown: Vec<String> = diffs.iter().take(40).map(|d| d.to_string()).collect();
        panic!("{} cell diff(s) vs {oracle}:\n{}", diffs.len(), shown.join("\n"));
    }
}

#[test]
fn ifrs_bridge_full_reproduces_oracle() {
    let inp = IfrsBridgeInput {
        company: "FullCo".into(),
        period: "FY2024".into(),
        ifrs_to_us_gaap: true,
        reported_ebit: 1000.0,
        reported_ebitda: 1400.0,
        reported_ebita: 1100.0,
        standard_depreciation: 200.0,
        standard_amortization: 50.0,
        rou_depreciation: 80.0,
        lease_interest: 20.0,
        short_term_rent: 30.0,
        revenue: 5000.0,
        items_excluded: excluded(),
    };
    gate(&inp, "IFRS_BRIDGE");
}

#[test]
fn ifrs_bridge_simple_reproduces_oracle() {
    let inp = IfrsBridgeInput {
        company: "SimpleCo".into(),
        period: "FY2024".into(),
        ifrs_to_us_gaap: false,
        reported_ebit: 500.0,
        reported_ebitda: 0.0,
        reported_ebita: 500.0,
        standard_depreciation: 100.0,
        standard_amortization: 0.0,
        rou_depreciation: 40.0,
        lease_interest: 10.0,
        short_term_rent: 0.0,
        revenue: 0.0,
        items_excluded: excluded(),
    };
    gate(&inp, "IFRS_BRIDGE_SIMPLE");
}
