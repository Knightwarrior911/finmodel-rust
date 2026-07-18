//! R.5 parity gate — build the workbook cell-model from each committed snapshot
//! and diff it against that snapshot's `sheets`, cell-by-cell.

use std::collections::BTreeMap;

use fm_excel::sheets::build_workbook;
use fm_excel::snapshot::{compare_workbook, load_snapshot, workbook_input_from_snapshot};

const COMPANIES: [&str; 5] = ["SAND_ST", "ASML_AS", "NOVO-B_CO", "NESN_SW", "ATCO-B_ST"];

/// Sheets whose builders are complete and must diff to zero.
const GATED_SHEETS: [&str; 6] = ["Cover", "Assumptions", "IS", "BS", "CF", "Sources"];

fn snapshot_path(name: &str) -> String {
    format!(
        "{}/../../tieout/excel_snapshots/{}_snapshot.json",
        env!("CARGO_MANIFEST_DIR"),
        name
    )
}

/// Diff every company; return sheet -> total diff count and print detail.
fn run() -> BTreeMap<String, usize> {
    let mut totals: BTreeMap<String, usize> = BTreeMap::new();
    for co in COMPANIES {
        let snap = load_snapshot(&snapshot_path(co)).expect("load snapshot");
        let input = workbook_input_from_snapshot(&snap).expect("build input");
        let wb = build_workbook(&input);
        let diffs = compare_workbook(&wb, &snap);

        let mut by_sheet: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for d in &diffs {
            by_sheet
                .entry(d.sheet.clone())
                .or_default()
                .push(d.to_string());
            *totals.entry(d.sheet.clone()).or_default() += 1;
        }
        for (sheet, msgs) in &by_sheet {
            eprintln!("[{co}] {sheet}: {} diffs", msgs.len());
            for m in msgs.iter().take(12) {
                eprintln!("    {m}");
            }
            if msgs.len() > 12 {
                eprintln!("    … +{} more", msgs.len() - 12);
            }
        }
    }
    totals
}

#[test]
fn gated_sheets_zero_diff() {
    let totals = run();
    let failures: Vec<String> = GATED_SHEETS
        .iter()
        .filter_map(|s| {
            let n = totals.get(*s).copied().unwrap_or(0);
            (n > 0).then(|| format!("{s}={n}"))
        })
        .collect();
    assert!(
        failures.is_empty(),
        "non-zero diffs on gated sheets: {failures:?}"
    );
}
