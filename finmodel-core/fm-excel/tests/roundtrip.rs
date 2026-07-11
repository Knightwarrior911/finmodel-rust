//! Render faithfulness: render the cell-model to a real `.xlsx`, read it back
//! with calamine, and assert every model value/formula survives the round-trip.
//! This proves the parity comparison (which runs against the model) reflects the
//! actual file rust_xlsxwriter emits.

use calamine::{open_workbook, Data, Reader, Xlsx};

use fm_excel::model::{cell_ref, Value};
use fm_excel::render::render;
use fm_excel::sheets::build_workbook;
use fm_excel::snapshot::{load_snapshot, workbook_input_from_snapshot};

fn snapshot_path(name: &str) -> String {
    format!("{}/../../tieout/excel_snapshots/{}_snapshot.json", env!("CARGO_MANIFEST_DIR"), name)
}

#[test]
fn render_roundtrips_values_and_formulas() {
    let snap = load_snapshot(&snapshot_path("SAND_ST")).expect("load");
    let input = workbook_input_from_snapshot(&snap).expect("input");
    let wb = build_workbook(&input);

    let mut path = std::env::temp_dir();
    path.push("fm_excel_roundtrip_SAND.xlsx");
    let path = path.to_string_lossy().to_string();
    render(&wb, &path).expect("render");

    let mut book: Xlsx<_> = open_workbook(&path).expect("open xlsx");

    let mut checked_values = 0usize;
    let mut checked_formulas = 0usize;

    for sheet in &wb.sheets {
        let values = book.worksheet_range(&sheet.name).expect("range");
        let formulas = book.worksheet_formula(&sheet.name).expect("formulas");

        for ((row, col), cell) in &sheet.cells {
            let pos = (*row, *col);
            match (&cell.value, &cell.formula) {
                (Some(Value::Number(n)), _) => {
                    let got = values.get_value(pos);
                    let gv = match got {
                        Some(Data::Float(f)) => *f,
                        Some(Data::Int(i)) => *i as f64,
                        other => panic!("{}!{} expected number {n}, got {other:?}", sheet.name, cell_ref(*row, *col)),
                    };
                    assert!((n - gv).abs() <= 1e-6, "{}!{} number {n} != {gv}", sheet.name, cell_ref(*row, *col));
                    checked_values += 1;
                }
                (Some(Value::Text(t)), _) => {
                    let got = values.get_value(pos);
                    match got {
                        Some(Data::String(s)) if s == t => {}
                        other => panic!("{}!{} expected text {t:?}, got {other:?}", sheet.name, cell_ref(*row, *col)),
                    }
                    checked_values += 1;
                }
                (None, Some(f)) => {
                    // calamine returns formulas without the leading '='.
                    let want = f.strip_prefix('=').unwrap_or(f);
                    let got = formulas.get_value(pos).cloned().unwrap_or_default();
                    assert_eq!(got, want, "{}!{} formula mismatch", sheet.name, cell_ref(*row, *col));
                    checked_formulas += 1;
                }
                _ => {}
            }
        }
    }

    let _ = std::fs::remove_file(&path);
    assert!(checked_values > 50, "too few values checked: {checked_values}");
    assert!(checked_formulas > 20, "too few formulas checked: {checked_formulas}");
}
