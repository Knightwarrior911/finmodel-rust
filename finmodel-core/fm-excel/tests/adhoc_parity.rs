//! Ad-hoc / benchmark parity gate — proves the Rust `AdHocTable::build_sheet`
//! reproduces the Python `AdHocExcelWriter.write_research` oracle
//! (`tieout/build_adhoc_oracle.py` → `ADHOC_bench_snapshot.json`) cell-for-cell
//! (value + formula + fill). Mirrors the fixed benchmark input verbatim.

use std::collections::HashMap;

use fm_excel::adhoc::{AdHocTable, CellVal, ColKind, ColumnSpec, Grain};
use fm_excel::model::Workbook;
use fm_excel::snapshot::{compare_workbook, load_snapshot};

/// Same pinned footer stamp the oracle normalizes to.
const PINNED_GENERATED: &str =
    "Generated: 2026-01-01 00:00 | Source: SEC EDGAR / yfinance / Company filings";

fn snap_path() -> String {
    format!(
        "{}/../../tieout/excel_snapshots/ADHOC_bench_snapshot.json",
        env!("CARGO_MANIFEST_DIR")
    )
}

fn row(
    ticker: &str,
    revenue: f64,
    ebitda: f64,
    net_income: f64,
    ebitda_margin: f64,
    net_margin: f64,
    ev_ebitda: f64,
) -> HashMap<String, CellVal> {
    let mut m = HashMap::new();
    m.insert("ticker".to_string(), CellVal::Text(ticker.into()));
    m.insert("revenue".to_string(), CellVal::Number(revenue));
    m.insert("ebitda".to_string(), CellVal::Number(ebitda));
    m.insert("net_income".to_string(), CellVal::Number(net_income));
    m.insert("ebitda_margin".to_string(), CellVal::Number(ebitda_margin));
    m.insert("net_margin".to_string(), CellVal::Number(net_margin));
    m.insert("ev_ebitda".to_string(), CellVal::Number(ev_ebitda));
    m
}

fn fixed_table() -> AdHocTable {
    let columns = vec![
        ColumnSpec::label("ticker", "Ticker"),
        ColumnSpec::metric("revenue", "Revenue", ColKind::Dollar)
            .with_group("Financials")
            .with_units("USD millions")
            .with_definition("Total net revenue, latest FY"),
        ColumnSpec::metric("ebitda", "EBITDA", ColKind::Dollar)
            .with_group("Financials")
            .with_units("USD millions"),
        ColumnSpec::metric("net_income", "Net Income", ColKind::Dollar)
            .with_group("Financials")
            .with_units("USD millions"),
        ColumnSpec::metric("ebitda_margin", "EBITDA Margin", ColKind::Percent)
            .with_group("Profitability"),
        ColumnSpec::metric("net_margin", "Net Margin", ColKind::Percent)
            .with_group("Profitability"),
        ColumnSpec::metric("ev_ebitda", "EV / EBITDA", ColKind::Multiple).with_group("Valuation"),
    ];

    let rows = vec![
        row("AAPL", 391035.0, 134661.0, 93736.0, 0.3444, 0.2397, 22.5),
        row("MSFT", 245122.0, 133558.0, 88136.0, 0.5449, 0.3596, 24.1),
        row("GOOGL", 350018.0, 123456.0, 100118.0, 0.3527, 0.286, 15.3),
        row("AMZN", 637959.0, 111500.0, 59248.0, 0.1748, 0.0929, 18.7),
        row("META", 164501.0, 87200.0, 62360.0, 0.5301, 0.3791, 14.2),
    ];

    let mut sources = HashMap::new();
    sources.insert(
        ("AAPL".to_string(), "revenue".to_string()),
        "AAPL 10-K FY2024 p.31 (us-gaap:RevenueFromContractWithCustomerExcludingAssessedTax)"
            .to_string(),
    );
    sources.insert(
        ("MSFT".to_string(), "revenue".to_string()),
        "MSFT 10-K FY2024 p.55".to_string(),
    );
    sources.insert(
        ("AMZN".to_string(), "net_income".to_string()),
        "AMZN 10-K FY2024 p.38".to_string(),
    );

    AdHocTable {
        title: "Big Tech - Peer Benchmark (FY2024)".to_string(),
        units: String::new(),
        columns,
        rows,
        sources,
        grain: Grain::Company,
        is_comparative: true,
        needs_sort_filter: true,
        layout_override: None,
    }
}

#[test]
fn adhoc_bench_reproduces_oracle() {
    let table = fixed_table();
    table.validate().expect("table must validate");
    let sheet = table.build_sheet(PINNED_GENERATED);
    let mut wb = Workbook::new();
    wb.push(sheet);

    let snap = load_snapshot(&snap_path()).expect("load ADHOC oracle snapshot");
    let diffs = compare_workbook(&wb, &snap);
    if !diffs.is_empty() {
        let shown: Vec<String> = diffs.iter().take(40).map(|d| d.to_string()).collect();
        panic!(
            "{} cell diff(s) vs ADHOC oracle:\n{}",
            diffs.len(),
            shown.join("\n")
        );
    }
}
