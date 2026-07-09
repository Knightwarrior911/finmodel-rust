use serde_json::Value;

use crate::Result;
use crate::writer::{CellValue, SheetData};

/// Load a JSON snapshot from disk.
pub fn load_snapshot(path: &str) -> Result<Value> {
    let file = std::fs::File::open(path)?;
    let value: Value = serde_json::from_reader(file)?;
    Ok(value)
}

/// Compare actual `SheetData` against a JSON snapshot value.
///
/// The snapshot must contain a `"sheets"` key whose value is an object mapping
/// sheet-name → `{ "headers": […], "rows": [{ "label": …, "values": […] }, …] }`.
///
/// Returns a list of human-readable difference descriptions. An empty `Vec`
/// means the actual data matches the snapshot.
pub fn compare_sheets(actual: &[SheetData], snapshot: &Value) -> Vec<String> {
    let mut diffs: Vec<String> = Vec::new();

    let sheets_obj = match snapshot.get("sheets") {
        Some(Value::Object(map)) => map,
        _ => return diffs, // No sheets key → nothing to compare.
    };

    for sheet_data in actual {
        let name = &sheet_data.name;

        let sheet_val = match sheets_obj.get(name) {
            Some(v) => v,
            None => {
                diffs.push(format!("[{name}] Sheet not found in snapshot"));
                continue;
            }
        };

        // --- Compare headers ---
        let expected_headers = sheet_val
            .get("headers")
            .and_then(|h| h.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let max_header_len = sheet_data.headers.len().max(expected_headers.len());
        for i in 0..max_header_len {
            let actual_h = sheet_data.headers.get(i).map(String::as_str).unwrap_or("");
            let expected_h = expected_headers.get(i).map(String::as_str).unwrap_or("");
            if actual_h != expected_h {
                diffs.push(format!(
                    "[{name}] Header col {i}: got \"{actual_h}\", expected \"{expected_h}\""
                ));
            }
        }

        // --- Compare rows ---
        let expected_rows = sheet_val
            .get("rows")
            .and_then(|r| r.as_array())
            .map(|arr| arr.to_vec())
            .unwrap_or_default();

        let max_row_len = sheet_data.rows.len().max(expected_rows.len());
        for i in 0..max_row_len {
            let (actual_label, actual_values) = match sheet_data.rows.get(i) {
                Some((l, vs)) => (l.as_str(), vs),
                None => {
                    // Extra row in snapshot.
                    if let Some(row_obj) = expected_rows.get(i) {
                        let lbl = row_obj
                            .get("label")
                            .and_then(|v| v.as_str())
                            .unwrap_or("?");
                        diffs.push(format!("[{name}] Missing row {i}: \"{lbl}\""));
                    }
                    continue;
                }
            };

            let expected_row = match expected_rows.get(i) {
                Some(v) => v,
                None => {
                    diffs.push(format!(
                        "[{name}] Extra row {i}: \"{actual_label}\" not in snapshot"
                    ));
                    continue;
                }
            };

            // Compare label.
            let expected_label = expected_row
                .get("label")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if actual_label != expected_label {
                diffs.push(format!(
                    "[{name}] Row {i} label: got \"{actual_label}\", expected \"{expected_label}\""
                ));
            }

            // Compare cell values.
            let expected_values = expected_row
                .get("values")
                .and_then(|v| v.as_array())
                .map(|arr| arr.to_vec())
                .unwrap_or_default();

            let max_col_len = actual_values.len().max(expected_values.len());
            for j in 0..max_col_len {
                let actual_cell = actual_values.get(j);
                let expected_cell = expected_values.get(j);
                let diff = compare_cell(actual_cell, expected_cell, name, i, j);
                diffs.extend(diff);
            }
        }
    }

    diffs
}

/// Compare a single actual cell against an expected JSON value.
fn compare_cell(
    actual: Option<&CellValue>,
    expected: Option<&Value>,
    sheet: &str,
    row: usize,
    col: usize,
) -> Vec<String> {
    let mut diffs = Vec::new();

    match (actual, expected) {
        (None, None) => {}
        (None, Some(_expected)) => {
            diffs.push(format!("[{sheet}] Row {row}, col {col}: missing actual cell"))
        }
        (Some(actual), None) => match actual {
            CellValue::Empty => { /* both absent, OK */ }
            _ => diffs.push(format!(
                "[{sheet}] Row {row}, col {col}: extra cell in actual"
            )),
        },
        (Some(CellValue::Empty), Some(_expected)) => {
            diffs.push(format!(
                "[{sheet}] Row {row}, col {col}: actual is empty but snapshot has a value"
            ))
        }
        (Some(CellValue::Value(actual_v)), Some(expected_v)) => {
            match expected_v.as_f64() {
                Some(expected_f) => {
                    if (*actual_v - expected_f).abs() > f64::EPSILON {
                        diffs.push(format!(
                            "[{sheet}] Row {row}, col {col}: value mismatch — got {actual_v}, expected {expected_f}"
                        ));
                    }
                }
                None => diffs.push(format!(
                    "[{sheet}] Row {row}, col {col}: expected non-numeric, got number {actual_v}"
                )),
            }
        }
        (Some(CellValue::Formula(actual_f)), Some(expected_v)) => {
            let expected_str = expected_v.as_str().unwrap_or("");
            if actual_f != expected_str {
                diffs.push(format!(
                    "[{sheet}] Row {row}, col {col}: formula mismatch — got \"{actual_f}\", expected \"{expected_str}\""
                ));
            }
        }
        (Some(CellValue::Text(actual_t)), Some(expected_v)) => {
            match expected_v.as_str() {
                Some(expected_s) => {
                    if actual_t != expected_s {
                        diffs.push(format!(
                            "[{sheet}] Row {row}, col {col}: text mismatch — got \"{actual_t}\", expected \"{expected_s}\""
                        ));
                    }
                }
                None => {
                    diffs.push(format!(
                        "[{sheet}] Row {row}, col {col}: expected non-text, got \"{actual_t}\""
                    ));
                }
            }
        }
    }

    diffs
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use crate::writer::CellValue;

    #[test]
    fn test_compare_exact_match() {
        let actual = vec![SheetData {
            name: "Income".to_string(),
            headers: vec!["Item".to_string(), "2023".to_string()],
            rows: vec![(
                "Revenue".to_string(),
                vec![CellValue::Value(100.0)],
            )],
        }];

        let snapshot: Value = serde_json::from_str(
            r#"{
            "sheets": {
                "Income": {
                    "headers": ["Item", "2023"],
                    "rows": [
                        {"label": "Revenue", "values": [100.0]}
                    ]
                }
            }
        }"#,
        )
        .unwrap();

        let diffs = compare_sheets(&actual, &snapshot);
        assert!(diffs.is_empty(), "Expected no diffs, got: {diffs:?}");
    }

    #[test]
    fn test_compare_value_mismatch() {
        let actual = vec![SheetData {
            name: "Income".to_string(),
            headers: vec!["Item".to_string(), "2023".to_string()],
            rows: vec![(
                "Revenue".to_string(),
                vec![CellValue::Value(99.0)],
            )],
        }];

        let snapshot: Value = serde_json::from_str(
            r#"{
            "sheets": {
                "Income": {
                    "headers": ["Item", "2023"],
                    "rows": [
                        {"label": "Revenue", "values": [100.0]}
                    ]
                }
            }
        }"#,
        )
        .unwrap();

        let diffs = compare_sheets(&actual, &snapshot);
        assert!(!diffs.is_empty(), "Expected diffs for value mismatch");
        assert!(
            diffs[0].contains("value mismatch"),
            "Expected 'value mismatch' in: {}",
            diffs[0]
        );
    }

    #[test]
    fn test_compare_formula_match() {
        let actual = vec![SheetData {
            name: "Calc".to_string(),
            headers: vec!["Item".to_string(), "Val".to_string()],
            rows: vec![(
                "Total".to_string(),
                vec![CellValue::Formula("=SUM(B2:B10)".to_string())],
            )],
        }];

        let snapshot: Value = serde_json::from_str(
            r#"{
            "sheets": {
                "Calc": {
                    "headers": ["Item", "Val"],
                    "rows": [
                        {"label": "Total", "values": ["=SUM(B2:B10)"]}
                    ]
                }
            }
        }"#,
        )
        .unwrap();

        let diffs = compare_sheets(&actual, &snapshot);
        assert!(diffs.is_empty(), "Expected no diffs, got: {diffs:?}");
    }

    #[test]
    fn test_compare_missing_sheet() {
        let actual = vec![SheetData {
            name: "Unknown".to_string(),
            headers: vec!["X".to_string()],
            rows: vec![],
        }];

        let snapshot: Value = serde_json::from_str(
            r#"{
            "sheets": {
                "Other": { "headers": ["X"], "rows": [] }
            }
        }"#,
        )
        .unwrap();

        let diffs = compare_sheets(&actual, &snapshot);
        assert!(!diffs.is_empty(), "Expected diffs for missing sheet");
        assert!(diffs[0].contains("not found"));
    }

    #[test]
    fn test_load_snapshot_nonexistent() {
        let result = load_snapshot("C:\\nonexistent_snapshot_file.json");
        assert!(result.is_err(), "Expected error for missing file");
    }
}
