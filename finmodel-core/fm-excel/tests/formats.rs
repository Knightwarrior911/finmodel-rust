//! Number-format assignment (product polish; not covered by the snapshot gate,
//! which is blind to number formats). Verifies percent vs number formats land on
//! the right cells, mirroring writer.py's `_Fmt` per-row assignment.

use fm_excel::model::{Cell, FMT_NUM, FMT_PCT, Sheet, parse_ref};
use fm_excel::sheets::build_workbook;
use fm_excel::snapshot::{load_snapshot, workbook_input_from_snapshot};

fn snapshot_path(name: &str) -> String {
    format!(
        "{}/../../tieout/excel_snapshots/{}_snapshot.json",
        env!("CARGO_MANIFEST_DIR"),
        name
    )
}

fn cell<'a>(s: &'a Sheet, reference: &str) -> &'a Cell {
    let (r, c) = parse_ref(reference).expect("ref");
    s.cells
        .get(&(r, c))
        .unwrap_or_else(|| panic!("no cell {reference}"))
}

#[test]
fn number_formats_are_assigned() {
    let snap = load_snapshot(&snapshot_path("SAND_ST")).expect("load");
    let input = workbook_input_from_snapshot(&snap).expect("input");
    let wb = build_workbook(&input);

    let asmp = wb.sheet("Assumptions").expect("Assumptions");
    // Percent drivers.
    assert_eq!(
        cell(asmp, "D33").num_fmt,
        Some(FMT_PCT),
        "base rev-growth % "
    );
    assert_eq!(
        cell(asmp, "D34").num_fmt,
        Some(FMT_PCT),
        "base gross-margin %"
    );
    assert_eq!(
        cell(asmp, "D15").num_fmt,
        Some(FMT_PCT),
        "active rev-growth CHOOSE %"
    );
    // Number drivers (days / dividend / exit multiple).
    assert_eq!(cell(asmp, "D41").num_fmt, Some(FMT_NUM), "DSO days");
    assert_eq!(cell(asmp, "D46").num_fmt, Some(FMT_NUM), "exit multiple");
    assert_eq!(
        cell(asmp, "D26").num_fmt,
        Some(FMT_NUM),
        "active dividend/share"
    );
    // Shared inputs: rf/erp/kd pct; de/price/shares num.
    assert_eq!(cell(asmp, "D86").num_fmt, Some(FMT_PCT), "risk-free %");
    assert_eq!(cell(asmp, "D88").num_fmt, Some(FMT_NUM), "target D/E (num)");
    assert_eq!(
        cell(asmp, "D90").num_fmt,
        Some(FMT_NUM),
        "share price (num)"
    );

    // BS: monetary default, interest-rate schedule row is percent.
    let bs = wb.sheet("BS").expect("BS");
    assert_eq!(cell(bs, "D12").num_fmt, Some(FMT_NUM), "cash");
    assert_eq!(
        cell(bs, "F19").num_fmt,
        Some(FMT_NUM),
        "total assets formula"
    );
    assert_eq!(
        cell(bs, "F58").num_fmt,
        Some(FMT_PCT),
        "debt interest-rate %"
    );

    // CF: monetary default, CapEx%-of-revenue driver row is percent.
    let cf = wb.sheet("CF").expect("CF");
    assert_eq!(cell(cf, "F24").num_fmt, Some(FMT_PCT), "capex % driver");
    assert_eq!(cell(cf, "D12").num_fmt, Some(FMT_NUM), "net income link");
}
