//! Snapshot parity gate (R.5).
//!
//! Reads the real `tieout/excel_snapshots/*.json` format
//! (`sheets → name → [{row, cells:[{ref, value?, formula?, fill?}]}]`),
//! reconstructs the writer's [`WorkbookInput`] from it, and diffs a generated
//! [`Workbook`] against it cell-by-cell (value + formula + fill).

use std::collections::{BTreeSet, HashMap};

use serde_json::Value as Json;

use crate::input::{Meta, ModelOutput, Statement, Verification, WorkbookInput};
use crate::model::{cell_ref, Cell, Value, Workbook};
use crate::{ExcelError, Result};

/// Numeric equality tolerance. Derived drivers are `round(_, 6)`; injected /
/// historical values are exact. Half-a-unit in the 6th decimal is tight yet
/// absorbs f64 representation noise.
const NUM_TOL: f64 = 5e-7;

/// Load a snapshot JSON from disk.
pub fn load_snapshot(path: &str) -> Result<Json> {
    let file = std::fs::File::open(path)?;
    Ok(serde_json::from_reader(file)?)
}

// ── Snapshot cell lookup ─────────────────────────────────────────────────────

/// Find a cell object (`{ref, value?, formula?, fill?}`) in a snapshot sheet.
fn snap_cell<'a>(snap: &'a Json, sheet: &str, reference: &str) -> Option<&'a Json> {
    let rows = snap.get("sheets")?.get(sheet)?.as_array()?;
    for row in rows {
        for cell in row.get("cells")?.as_array()? {
            if cell.get("ref").and_then(Json::as_str) == Some(reference) {
                return Some(cell);
            }
        }
    }
    None
}

fn snap_cell_num(snap: &Json, sheet: &str, reference: &str) -> Option<f64> {
    snap_cell(snap, sheet, reference)?.get("value")?.as_f64()
}

fn snap_cell_text(snap: &Json, sheet: &str, reference: &str) -> Option<String> {
    Some(snap_cell(snap, sheet, reference)?.get("value")?.as_str()?.to_string())
}

// ── WorkbookInput reconstruction ─────────────────────────────────────────────

fn parse_statement(obj: &Json) -> Statement {
    let mut out = Statement::new();
    if let Some(map) = obj.as_object() {
        for (k, v) in map {
            if let Some(arr) = v.as_array() {
                out.insert(k.clone(), arr.iter().map(|x| x.as_f64()).collect());
            }
        }
    }
    out
}

/// Rebuild the exact [`WorkbookInput`] the Python writer received for this
/// snapshot: `model_output` + metadata + verification, plus the derived
/// assumptions. The genuinely-external inputs the writer does not compute
/// (market rate/price, fiscal-year-end, as-of date) are read back from the
/// snapshot's own cells — they were inputs, not writer outputs.
pub fn workbook_input_from_snapshot(snap: &Json) -> Result<WorkbookInput> {
    let err = |m: &str| ExcelError::Snapshot(m.to_string());

    let periods: Vec<String> = snap
        .get("periods")
        .and_then(Json::as_array)
        .ok_or_else(|| err("missing periods"))?
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();

    let mo = snap.get("model_output").ok_or_else(|| err("missing model_output"))?;
    let model = ModelOutput {
        periods,
        income_statement: parse_statement(&mo["income_statement"]),
        balance_sheet: parse_statement(&mo["balance_sheet"]),
        cash_flow_statement: parse_statement(&mo["cash_flow_statement"]),
        plug_used: false,
    };

    let sector = "standard".to_string();
    // As-of date: parse "As of YYYY-MM-DD  | …" off Cover C6.
    let as_of = snap_cell_text(snap, "Cover", "C6")
        .and_then(|t| t.strip_prefix("As of ").map(|r| r.split_whitespace().next().unwrap_or("").to_string()))
        .unwrap_or_default();
    let meta = Meta {
        company: snap.get("company").and_then(Json::as_str).unwrap_or_default().to_string(),
        ticker: snap.get("ticker").and_then(Json::as_str).unwrap_or_default().to_string(),
        currency: snap.get("currency").and_then(Json::as_str).unwrap_or_default().to_string(),
        fiscal_year_end: snap_cell_text(snap, "Cover", "D14").unwrap_or_else(|| "Dec".into()),
        sector: sector.clone(),
        as_of,
    };

    let ver = snap.get("verification").cloned().unwrap_or(Json::Null);
    let str_list = |k: &str| -> Vec<String> {
        ver.get(k)
            .and_then(Json::as_array)
            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default()
    };
    let verification = Verification {
        passed: ver.get("passed").and_then(Json::as_bool).unwrap_or(false),
        critical_failures: str_list("critical_failures"),
        warnings: str_list("warnings"),
        notes: str_list("notes"),
    };

    // External market inputs (live yfinance in Python) — read from Assumptions.
    let risk_free = snap_cell_num(snap, "Assumptions", "D86").unwrap_or(0.045);
    let share_price = snap_cell_num(snap, "Assumptions", "D90").unwrap_or(0.0);
    let params = crate::derive::ValuationParams {
        risk_free_rate: risk_free,
        share_price,
        ..Default::default()
    };
    let assumptions = crate::derive::build_assumptions_block(&model, &sector, &params);

    Ok(WorkbookInput {
        meta,
        model,
        assumptions,
        verification,
        is_structure: Vec::new(),
        wacc: None,
        peer_source: "fallback".into(),
        dcf: None,
        public_comps: None,
    })
}

// ── Diffing ──────────────────────────────────────────────────────────────────

/// A single cell-level discrepancy.
#[derive(Debug, Clone)]
pub struct Diff {
    pub sheet: String,
    pub reference: String,
    pub message: String,
}

impl std::fmt::Display for Diff {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}!{}] {}", self.sheet, self.reference, self.message)
    }
}

/// Content extracted from a snapshot cell for comparison.
#[derive(Default)]
struct Expected {
    number: Option<f64>,
    text: Option<String>,
    formula: Option<String>,
    fill: Option<String>,
}

impl Expected {
    fn from_json(cell: &Json) -> Self {
        let mut e = Expected::default();
        if let Some(v) = cell.get("value") {
            if let Some(s) = v.as_str() {
                e.text = Some(s.to_string());
            } else if let Some(n) = v.as_f64() {
                e.number = Some(n);
            }
        }
        e.formula = cell.get("formula").and_then(Json::as_str).map(String::from);
        e.fill = cell.get("fill").and_then(Json::as_str).map(String::from);
        e
    }
    fn has_content(&self) -> bool {
        self.number.is_some() || self.text.is_some() || self.formula.is_some() || self.fill.is_some()
    }
}

fn cmp_cell(sheet: &str, reference: &str, exp: &Expected, act: Option<&Cell>) -> Vec<Diff> {
    let mut out = Vec::new();
    let d = |m: String| Diff { sheet: sheet.to_string(), reference: reference.to_string(), message: m };
    let act = act.cloned().unwrap_or_default();

    // Value (text or number).
    match (&exp.text, &exp.number, &act.value) {
        (Some(t), _, Some(Value::Text(a))) if t == a => {}
        (Some(t), _, av) => out.push(d(format!("text: expected {t:?}, got {av:?}"))),
        (_, Some(n), Some(Value::Number(a))) if (n - a).abs() <= NUM_TOL => {}
        (_, Some(n), av) => out.push(d(format!("number: expected {n}, got {av:?}"))),
        (None, None, Some(v)) => out.push(d(format!("expected empty value, got {v:?}"))),
        (None, None, None) => {}
    }

    // Formula (exact string).
    match (&exp.formula, &act.formula) {
        (Some(f), Some(a)) if f == a => {}
        (Some(f), a) => out.push(d(format!("formula: expected {f:?}, got {a:?}"))),
        (None, Some(a)) => out.push(d(format!("expected no formula, got {a:?}"))),
        (None, None) => {}
    }

    // Fill (exact ARGB).
    match (&exp.fill, &act.fill) {
        (Some(f), Some(a)) if f == a => {}
        (Some(f), a) => out.push(d(format!("fill: expected {f:?}, got {a:?}"))),
        (None, Some(a)) => out.push(d(format!("expected no fill, got {a:?}"))),
        (None, None) => {}
    }
    out
}

/// Diff a generated workbook against a snapshot. Empty result = exact match on
/// every characterized cell across all snapshot sheets.
pub fn compare_workbook(wb: &Workbook, snap: &Json) -> Vec<Diff> {
    let mut diffs = Vec::new();
    let sheets = match snap.get("sheets").and_then(Json::as_object) {
        Some(s) => s,
        None => return vec![Diff { sheet: "*".into(), reference: "*".into(), message: "snapshot has no `sheets`".into() }],
    };

    for (name, rows) in sheets {
        let ws = match wb.sheet(name) {
            Some(w) => w,
            None => {
                diffs.push(Diff { sheet: name.clone(), reference: "*".into(), message: "sheet missing from workbook".into() });
                continue;
            }
        };

        // Expected content cells from the snapshot.
        let mut expected: HashMap<String, Expected> = HashMap::new();
        if let Some(arr) = rows.as_array() {
            for row in arr {
                if let Some(cells) = row.get("cells").and_then(Json::as_array) {
                    for cell in cells {
                        if let Some(r) = cell.get("ref").and_then(Json::as_str) {
                            let e = Expected::from_json(cell);
                            if e.has_content() {
                                expected.insert(r.to_string(), e);
                            }
                        }
                    }
                }
            }
        }

        // Actual content cells from the workbook.
        let actual: HashMap<String, &Cell> = ws
            .cells
            .iter()
            .filter(|(_, c)| c.has_content())
            .map(|((r, c), cell)| (cell_ref(*r, *c), cell))
            .collect();

        let refs: BTreeSet<&String> = expected.keys().chain(actual.keys()).collect();
        for r in refs {
            let exp = expected.get(r).map(|e| e).unwrap_or(&Expected {
                number: None, text: None, formula: None, fill: None,
            });
            diffs.extend(cmp_cell(name, r, exp, actual.get(r).copied()));
        }
    }
    diffs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Sheet, BLUE};
    use serde_json::json;

    fn snap() -> Json {
        json!({
            "sheets": {
                "S": [
                    { "row": 1, "cells": [
                        {"ref": "A1"},
                        {"ref": "C1", "value": "Hi", "fill": "FF255BE3"},
                        {"ref": "D1", "value": 4363}
                    ]},
                    { "row": 2, "cells": [
                        {"ref": "D2", "formula": "=A1+1"}
                    ]}
                ]
            }
        })
    }

    #[test]
    fn exact_match_zero_diffs() {
        let mut s = Sheet::new("S");
        s.title(0, "Hi"); // C1 blue text
        s.number(0, 3, 4363.0); // D1
        s.formula(1, 3, "=A1+1"); // D2
        let mut wb = Workbook::new();
        wb.push(s);
        let diffs = compare_workbook(&wb, &snap());
        assert!(diffs.is_empty(), "unexpected diffs: {diffs:?}");
    }

    #[test]
    fn detects_value_formula_fill_mismatch() {
        let mut s = Sheet::new("S");
        s.text(0, 2, "Hi"); // missing fill
        s.number(0, 3, 9999.0); // wrong number
        s.formula(1, 3, "=A1+2"); // wrong formula
        let mut wb = Workbook::new();
        wb.push(s);
        let diffs = compare_workbook(&wb, &snap());
        assert_eq!(diffs.len(), 3, "got: {diffs:?}");
    }

    #[test]
    fn detects_extra_cell() {
        let mut s = Sheet::new("S");
        s.title(0, "Hi");
        s.number(0, 3, 4363.0);
        s.formula(1, 3, "=A1+1");
        s.number(5, 5, 1.0); // extra content not in snapshot
        let mut wb = Workbook::new();
        wb.push(s);
        let diffs = compare_workbook(&wb, &snap());
        assert_eq!(diffs.len(), 1);
        assert!(diffs[0].message.contains("expected empty"));
    }

    #[test]
    fn detects_missing_sheet() {
        let wb = Workbook::new();
        let diffs = compare_workbook(&wb, &snap());
        assert!(diffs.iter().any(|d| d.message.contains("missing")));
    }

    #[test]
    fn fill_constant_is_stable() {
        assert_eq!(BLUE, "FF255BE3");
    }
}
