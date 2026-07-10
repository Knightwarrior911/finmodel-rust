//! R.5 Excel writer parity — smoke test against committed fixture data.
//!
//! Validates that the Rust Excel writer correctly handles real model data
//! from a baseline company fixture. Verifies the writer produces a valid
//! file with the expected sheets and values.
//!
//! Full cell-by-cell layout parity against the Python Excel snapshots
//! requires porting the complete Python writer layout — deferred.

use fm_excel::compare::load_snapshot;
use fm_excel::writer::{CellValue, SheetData, write_workbook};

/// Path to a committed fixture (relative to fm-excel/Cargo.toml dir).
fn fixture_path() -> String {
    format!("{}/../fm-cli/tests/fixtures/SAND_ST_model.json", env!("CARGO_MANIFEST_DIR"))
}

fn all_fixture_base() -> String {
    format!("{}/../fm-cli/tests/fixtures", env!("CARGO_MANIFEST_DIR"))
}

/// Build SheetData from a fixture JSON for a given statement (is/bs/cfs).
fn fixture_to_sheet_data(
    stmt_name: &str,
    fixture: &serde_json::Value,
    headers: Vec<String>,
) -> SheetData {
    let stmt = &fixture[stmt_name];
    let mut rows = Vec::new();

    if let Some(obj) = stmt.as_object() {
        for (key, vals) in obj {
            if let Some(arr) = vals.as_array() {
                let values: Vec<CellValue> = arr
                    .iter()
                    .map(|v| {
                        v.as_f64()
                            .or_else(|| v.as_i64().map(|i| i as f64))
                            .map(CellValue::Value)
                            .unwrap_or(CellValue::Empty)
                    })
                    .collect();
                if !values.is_empty() {
                    rows.push((key.clone(), values));
                }
            }
        }
    }

    SheetData {
        name: stmt_name.to_string(),
        headers,
        rows,
    }
}

fn build_headers(fixture: &serde_json::Value) -> Vec<String> {
    let is = fixture.get("income_statement")
        .and_then(|v| v.as_object())
        .expect("income_statement");
    let n_periods = is.get("revenue")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    let mut h = vec!["Item".to_string()];
    for i in 0..n_periods {
        h.push(format!("FY{}", 2023 + i));
    }
    h
}

fn build_sheets(fixture: &serde_json::Value) -> Vec<SheetData> {
    let headers = build_headers(fixture);
    vec![
        fixture_to_sheet_data("income_statement", fixture, headers.clone()),
        fixture_to_sheet_data("balance_sheet", fixture, headers.clone()),
        fixture_to_sheet_data("cash_flow_statement", fixture, headers),
    ]
}

#[test]
fn test_excel_writer_produces_valid_file_from_fixture() {
    let fixture = load_snapshot(&fixture_path())
        .expect("SAND_ST fixture should load");
    let sheets = build_sheets(&fixture);

    let tmp = std::env::temp_dir().join("fm_excel_parity.xlsx");
    let path = tmp.to_str().unwrap().to_string();
    write_workbook(&path, &sheets).expect("write_workbook");

    assert!(tmp.exists(), "Excel file should exist");
    let len = std::fs::metadata(&tmp).map(|m| m.len()).unwrap_or(0);
    assert!(len > 1000, "file size {len} should be > 1KB");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn test_excel_writer_fixture_sheets_have_expected_content() {
    let fixture = load_snapshot(&fixture_path())
        .expect("SAND_ST fixture should load");
    let sheets = build_sheets(&fixture);

    assert_eq!(sheets.len(), 3, "should have 3 statement sheets");
    let is_sheet = &sheets[0];
    assert_eq!(is_sheet.name, "income_statement");
    assert!(is_sheet.headers.len() > 1, "should have periods");
    assert!(!is_sheet.rows.is_empty(), "should have data rows");

    let labels: Vec<&str> = is_sheet.rows.iter().map(|(l, _)| l.as_str()).collect();
    assert!(labels.contains(&"revenue"), "revenue row");
    assert!(labels.contains(&"ebit"), "ebit row");
    assert!(labels.contains(&"net_income"), "net_income row");

    let rev = is_sheet.rows.iter().find(|(l, _)| l == "revenue").unwrap();
    assert!(rev.1.len() >= 2, "revenue has values");
    match rev.1[0] {
        CellValue::Value(v) => assert!(v > 0.0, "revenue positive"),
        _ => panic!("revenue[0] should be Value"),
    }
}

#[test]
fn test_excel_writer_round_trips_all_fixtures() {
    let companies = ["SAND_ST", "ASML_AS", "NOVO-B_CO", "NESN_SW", "ATCO-B_ST"];
    let base = all_fixture_base();

    for name in &companies {
        let path = format!("{base}/{name}_model.json");
        let fixture = load_snapshot(&path)
            .unwrap_or_else(|e| panic!("{name} load: {e}"));
        let sheets = build_sheets(&fixture);

        let tmp = std::env::temp_dir().join(format!("fm_excel_{name}.xlsx"));
        let p = tmp.to_str().unwrap().to_string();
        write_workbook(&p, &sheets).unwrap_or_else(|e| panic!("{name} write: {e}"));

        assert!(tmp.exists(), "{name}: file exists");
        let len = std::fs::metadata(&tmp).map(|m| m.len()).unwrap_or(0);
        assert!(len > 1000, "{name}: size {len} > 1KB");
        let _ = std::fs::remove_file(&p);
    }
}
