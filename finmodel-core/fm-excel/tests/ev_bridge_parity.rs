//! EV-bridge worksheet parity gate — proves `fm_excel::bridge::build_ev_bridge_sheet`
//! reproduces the Python `ResearchExcelWriter.write_ev_bridge` oracle
//! (`tieout/build_ev_bridge_oracle.py` → `EV_BRIDGE_snapshot.json`) cell-for-cell
//! (value + formula + fill). Mirrors the fixed input verbatim.

use fm_excel::bridge::build_ev_bridge_sheet;
use fm_excel::model::Workbook;
use fm_excel::snapshot::{compare_workbook, load_snapshot};
use fm_value::ev_bridge::EvBridgeInput;

const PINNED_GENERATED: &str =
    "Generated: 2026-01-01 00:00 | Source: SEC EDGAR / yfinance / Company filings";

fn snap_path() -> String {
    format!(
        "{}/../../tieout/excel_snapshots/EV_BRIDGE_snapshot.json",
        env!("CARGO_MANIFEST_DIR")
    )
}

fn fixed_input() -> EvBridgeInput {
    EvBridgeInput {
        company: "DemoCo".into(),
        currency: "USD".into(),
        share_price: Some(150.0),
        shares_outstanding: Some(1_000_000_000.0),
        total_debt: Some(50_000_000_000.0),
        finance_leases: Some(5_000_000_000.0),
        operating_leases: Some(8_000_000_000.0),
        underfunded_pension: Some(2_000_000_000.0),
        minority_interest: Some(1_000_000_000.0),
        preferred_stock: Some(500_000_000.0),
        cash: Some(20_000_000_000.0),
        short_term_investments: Some(10_000_000_000.0),
        equity_investments: Some(3_000_000_000.0),
        nol_dta: Some(1_500_000_000.0),
        ltm_revenue: Some(100_000_000_000.0),
        ltm_ebitda: Some(30_000_000_000.0),
        ..Default::default()
    }
}

#[test]
fn ev_bridge_reproduces_oracle() {
    let sheet = build_ev_bridge_sheet(&fixed_input(), PINNED_GENERATED);
    let mut wb = Workbook::new();
    wb.push(sheet);

    let snap = load_snapshot(&snap_path()).expect("load EV_BRIDGE oracle snapshot");
    let diffs = compare_workbook(&wb, &snap);
    if !diffs.is_empty() {
        let shown: Vec<String> = diffs.iter().take(40).map(|d| d.to_string()).collect();
        panic!("{} cell diff(s) vs EV_BRIDGE oracle:\n{}", diffs.len(), shown.join("\n"));
    }
}

fn sparse_input() -> EvBridgeInput {
    EvBridgeInput {
        company: "SparseCo".into(),
        currency: "USD".into(),
        share_price: Some(42.0),
        shares_outstanding: Some(500_000_000.0),
        total_debt: Some(12_000_000_000.0),
        minority_interest: Some(800_000_000.0),
        cash: Some(6_000_000_000.0),
        nol_dta: Some(400_000_000.0),
        ltm_revenue: Some(25_000_000_000.0),
        ltm_ebitda: Some(4_000_000_000.0),
        ..Default::default()
    }
}

#[test]
fn ev_bridge_sparse_reproduces_oracle() {
    // Exercises the dynamic row shifts: several add/sub items absent, so the EV
    // formula and multiples row-refs must track the actual emitted rows.
    let sheet = build_ev_bridge_sheet(&sparse_input(), PINNED_GENERATED);
    let mut wb = Workbook::new();
    wb.push(sheet);
    let path = format!(
        "{}/../../tieout/excel_snapshots/EV_BRIDGE_SPARSE_snapshot.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let snap = load_snapshot(&path).expect("load EV_BRIDGE_SPARSE oracle");
    let diffs = compare_workbook(&wb, &snap);
    if !diffs.is_empty() {
        let shown: Vec<String> = diffs.iter().take(40).map(|d| d.to_string()).collect();
        panic!("{} cell diff(s) vs EV_BRIDGE_SPARSE oracle:\n{}", diffs.len(), shown.join("\n"));
    }
}
