//! Regenerate a committed model-workbook tie-out snapshot from the CURRENT Rust
//! pipeline.
//!
//! Needed after the fm-engine gross-margin fix (derive gross_profit = revenue −
//! cogs when a filing reports COGS without a gross-profit subtotal). The
//! corrected NESN projection intentionally diverges from the old Python oracle
//! (`tieout/build_excel_snapshots.py`), which produced a spurious loss for NESN
//! and is defunct here (its extraction cache is gone; NESN is already excluded
//! from `full_is_parity`). This re-pins the snapshot to the CORRECT behavior so
//! `fm verify`, `parity`, and `snapshot_parity` gate correctness, not a bug.
//!
//! Run:  cargo run -p fm-cli --example regen_snapshot -- NESN.SW
//!
//! Self-consistent by construction: it rebuilds the minimal workbook exactly the
//! way `fm verify` does (`workbook_input_from_snapshot` → `build_workbook`), then
//! dumps that workbook's cells back into `sheets`. A self-check diff must be 0.

use std::collections::BTreeMap;
use std::path::PathBuf;

use fm_excel::model::{Value as CellValue, Workbook, cell_ref};
use serde_json::{Value as J, json};

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Load a committed extraction fixture (offline), injecting the ticker's
/// reporting currency when the fixture omits it (mirrors the CLI build path).
fn load_fixture(ticker: &str) -> fm_extract::ExtractionResult {
    let stem = fm_build::ticker_to_stem(ticker);
    let p = manifest().join(format!("tests/fixtures/{stem}_model.json"));
    let text = std::fs::read_to_string(&p).expect("read fixture");
    let mut val: J = serde_json::from_str(&text).expect("parse fixture");
    if val.get("currency").is_none() {
        val["currency"] = json!(fm_build::currency_for_ticker(ticker));
    }
    serde_json::from_value(val).expect("deserialize ExtractionResult")
}

/// Serialize a statement (line -> per-period values) with sorted keys for a
/// stable committed file.
fn stmt_to_json(s: &fm_excel::input::Statement) -> J {
    let sorted: BTreeMap<&String, &Vec<Option<f64>>> = s.iter().collect();
    serde_json::to_value(sorted).expect("serialize statement")
}

/// Dump a built workbook into the committed snapshot `sheets` shape:
/// `{ name: [ { row, cells: [ { ref, value?, formula?, fill? } ] } ] }`.
/// Only content cells are emitted (value / formula / fill) — exactly the cells
/// `compare_workbook` characterizes.
fn dump_sheets(wb: &Workbook) -> J {
    let mut sheets = serde_json::Map::new();
    for sh in &wb.sheets {
        let mut rows: BTreeMap<u32, Vec<J>> = BTreeMap::new();
        for ((r, c), cell) in &sh.cells {
            if !cell.has_content() {
                continue;
            }
            let mut obj = serde_json::Map::new();
            obj.insert("ref".into(), json!(cell_ref(*r, *c)));
            match &cell.value {
                Some(CellValue::Number(n)) => {
                    obj.insert("value".into(), json!(n));
                }
                Some(CellValue::Text(t)) => {
                    obj.insert("value".into(), json!(t));
                }
                None => {}
            }
            if let Some(f) = &cell.formula {
                obj.insert("formula".into(), json!(f));
            }
            if let Some(fill) = &cell.fill {
                obj.insert("fill".into(), json!(fill));
            }
            rows.entry(*r).or_default().push(J::Object(obj));
        }
        let arr: Vec<J> = rows
            .into_iter()
            .map(|(r, cells)| json!({ "row": r + 1, "cells": cells }))
            .collect();
        sheets.insert(sh.name.clone(), J::Array(arr));
    }
    J::Object(sheets)
}

fn main() {
    let ticker = std::env::args().nth(1).unwrap_or_else(|| "NESN.SW".to_string());
    let stem = fm_build::ticker_to_stem(&ticker);
    let snap_path = manifest().join(format!("../../tieout/excel_snapshots/{stem}_snapshot.json"));

    // 1. Corrected projection + merged model_output + verification (data only).
    let extraction = load_fixture(&ticker);
    let opts = fm_build::BuildOptions {
        proj_years: 5,
        ..Default::default()
    };
    let out = fm_build::build_with(&extraction, &ticker, &opts);
    let (input, _warn) =
        fm_build::build_workbook_input_with(&extraction, &out.projected, &ticker, &opts);

    // 2. Patch the OLD snapshot's model_output + verification (periods kept, so
    //    the market-input cells the writer reads back stay identical).
    let old: J = serde_json::from_str(&std::fs::read_to_string(&snap_path).expect("read snapshot"))
        .expect("parse snapshot");
    let mut patched = old.clone();
    patched["model_output"] = json!({
        "income_statement": stmt_to_json(&input.model.income_statement),
        "balance_sheet": stmt_to_json(&input.model.balance_sheet),
        "cash_flow_statement": stmt_to_json(&input.model.cash_flow_statement),
    });
    patched["verification"] = json!({
        "passed": input.verification.passed,
        "critical_failures": input.verification.critical_failures,
        "warnings": input.verification.warnings,
        "notes": input.verification.notes,
    });

    let periods_len = patched["periods"].as_array().map(|a| a.len()).unwrap_or(0);
    let series_len = input
        .model
        .income_statement
        .values()
        .next()
        .map(|v| v.len())
        .unwrap_or(0);
    eprintln!(
        "periods={periods_len} series_len={series_len} model.periods={:?}",
        input.model.periods
    );

    // 3. Rebuild the minimal workbook exactly as `fm verify` does, then dump.
    let vin = fm_excel::snapshot::workbook_input_from_snapshot(&patched)
        .expect("workbook_input_from_snapshot");
    let wb = fm_excel::sheets::build_workbook(&vin);
    let mut final_snap = patched.clone();
    final_snap["sheets"] = dump_sheets(&wb);

    // 4. Self-check: replay `fm verify` against the FINAL snapshot — must be 0.
    let vin2 = fm_excel::snapshot::workbook_input_from_snapshot(&final_snap)
        .expect("workbook_input_from_snapshot (final)");
    let wb2 = fm_excel::sheets::build_workbook(&vin2);
    let diffs = fm_excel::snapshot::compare_workbook(&wb2, &final_snap);
    eprintln!("self-check diffs: {}", diffs.len());
    for d in diffs.iter().take(30) {
        eprintln!("  {d}");
    }
    if !diffs.is_empty() {
        eprintln!("REFUSING to write: self-check found {} diff(s)", diffs.len());
        std::process::exit(1);
    }

    std::fs::write(
        &snap_path,
        serde_json::to_string_pretty(&final_snap).expect("serialize snapshot"),
    )
    .expect("write snapshot");
    eprintln!("wrote {}", snap_path.display());
}
