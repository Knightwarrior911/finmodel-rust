//! In-memory cell-model — the single source of truth for both the rendered
//! `.xlsx` and the snapshot parity comparison.
//!
//! The Python writer emits cells straight into an `xlsxwriter` workbook; here we
//! build an explicit sparse cell grid first, then (a) render it via
//! `rust_xlsxwriter` and (b) diff it against the committed `excel_snapshots`.
//! What we test is exactly what we write.

use std::collections::BTreeMap;

// ── Fill palette ────────────────────────────────────────────────────────────
// openpyxl records `fgColor.rgb` as 8-hex ARGB. These are the only three fills
// that survive into the committed snapshots (see tieout/excel_snapshots).
/// Title bars and grand-total rows (blue).
pub const BLUE: &str = "FF255BE3";
/// Section headers (tan).
pub const TAN: &str = "FFEAE0D3";
/// Critical-failure cells on the Sources tab (red).
pub const RED: &str = "FFFF3C28";
/// Sensitivity base-row highlight (light blue).
pub const LIGHT_BLUE: &str = "FFDEEAF1";

// ── Number formats (product polish; NOT part of the snapshot gate) ───────────
// Ported verbatim from writer.py `_Fmt` (Section 3 codes).
/// Plain number, thousands-separated: `#,##0`.
pub const FMT_NUM: &str = "#,##0_);(#,##0);\"-\";@";
/// Percentage, one decimal: `0.0%`.
pub const FMT_PCT: &str = "0.0%_);(0.0%);\"-\";@";
/// EV/EBITDA-style multiple: `0.0"x"`.
pub const FMT_MULT: &str = "0.0\"x\";(0.0\"x\");\"-\";@";
/// Check rows — always render a dash.
pub const FMT_CHECK: &str = "\"-\";;\"-\"";

/// Currency (dollar) format. Identical to [`FMT_NUM`] for non-USD reporting
/// currencies (writer.py only prefixes `$` for USD).
pub fn fmt_dollar(currency: &str) -> &'static str {
    if currency == "USD" { "$#,##0_);($#,##0);\"-\";@" } else { FMT_NUM }
}

// ── Column layout (0-based), mirrors writer.py ──────────────────────────────
/// Col A — left gutter.
pub const MARGIN: u32 = 0;
/// Col C — row labels.
pub const LABEL: u32 = 2;
/// Col D — first period column, also the Assumptions scalar/toggle column.
pub const DATA0: u32 = 3;

/// A scalar cell value: number or text. A cell is never both a value and a
/// formula (matches openpyxl's characterization: a `"="`-prefixed string is a
/// formula, everything else a value).
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Number(f64),
    Text(String),
}

/// A single populated cell. Empty fields mean "not set" (an absent attribute in
/// the snapshot).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Cell {
    pub value: Option<Value>,
    /// Formula string INCLUDING the leading `=`, exactly as the snapshot stores it.
    pub formula: Option<String>,
    /// 8-hex ARGB fill, e.g. `"FF255BE3"`.
    pub fill: Option<String>,
    /// Excel number-format code (e.g. [`FMT_PCT`]). Invisible to the snapshot
    /// gate (openpyxl doesn't characterize number formats) — product polish only.
    pub num_fmt: Option<&'static str>,
}

impl Cell {
    /// True when the cell carries any characterized content (value / formula / fill).
    pub fn has_content(&self) -> bool {
        self.value.is_some() || self.formula.is_some() || self.fill.is_some()
    }
}

/// One worksheet: a sparse map from 0-based `(row, col)` to [`Cell`].
#[derive(Clone, Debug)]
pub struct Sheet {
    pub name: String,
    pub cells: BTreeMap<(u32, u32), Cell>,
}

impl Sheet {
    pub fn new(name: impl Into<String>) -> Self {
        Sheet { name: name.into(), cells: BTreeMap::new() }
    }

    /// Merge content into a cell (later writes overlay earlier ones field-by-field).
    fn merge(&mut self, row: u32, col: u32, patch: Cell) {
        let c = self.cells.entry((row, col)).or_default();
        if patch.value.is_some() {
            c.value = patch.value;
            c.formula = None;
        }
        if patch.formula.is_some() {
            c.formula = patch.formula;
            c.value = None;
        }
        if patch.fill.is_some() {
            c.fill = patch.fill;
        }
        if patch.num_fmt.is_some() {
            c.num_fmt = patch.num_fmt;
        }
    }

    pub fn text(&mut self, row: u32, col: u32, s: impl Into<String>) {
        self.merge(row, col, Cell { value: Some(Value::Text(s.into())), ..Default::default() });
    }

    pub fn number(&mut self, row: u32, col: u32, n: f64) {
        self.merge(row, col, Cell { value: Some(Value::Number(n)), ..Default::default() });
    }

    /// Store a formula. `f` may be given with or without a leading `=`; it is
    /// normalized to include it (matching the snapshot).
    pub fn formula(&mut self, row: u32, col: u32, f: impl AsRef<str>) {
        let f = f.as_ref();
        let f = if f.starts_with('=') { f.to_string() } else { format!("={f}") };
        self.merge(row, col, Cell { formula: Some(f), ..Default::default() });
    }

    pub fn fill(&mut self, row: u32, col: u32, argb: &str) {
        self.merge(row, col, Cell { fill: Some(argb.to_string()), ..Default::default() });
    }

    /// Section-header cell: tan-filled text at [`LABEL`].
    pub fn section(&mut self, row: u32, text: impl Into<String>) {
        self.merge(row, LABEL, Cell {
            value: Some(Value::Text(text.into())),
            fill: Some(TAN.to_string()),
            ..Default::default()
        });
    }

    /// Title-bar cell: blue-filled text at [`LABEL`].
    pub fn title(&mut self, row: u32, text: impl Into<String>) {
        self.merge(row, LABEL, Cell {
            value: Some(Value::Text(text.into())),
            fill: Some(BLUE.to_string()),
            ..Default::default()
        });
    }

    /// Force a number format onto every numeric/formula cell in `row` from
    /// [`DATA0`] rightward (row-level format assignment, mirroring writer.py
    /// where a row shares one format).
    pub fn stamp_row(&mut self, row: u32, fmt: &'static str) {
        for ((r, c), cell) in self.cells.iter_mut() {
            if *r == row && *c >= DATA0 && (matches!(cell.value, Some(Value::Number(_))) || cell.formula.is_some()) {
                cell.num_fmt = Some(fmt);
            }
        }
    }

    /// Fill in `fmt` on every numeric/formula data cell that has no format yet
    /// (the default for monetary statement cells).
    pub fn stamp_numeric_default(&mut self, fmt: &'static str) {
        for ((_, c), cell) in self.cells.iter_mut() {
            if *c >= DATA0 && cell.num_fmt.is_none()
                && (matches!(cell.value, Some(Value::Number(_))) || cell.formula.is_some())
            {
                cell.num_fmt = Some(fmt);
            }
        }
    }
}

/// A full workbook: an ordered list of sheets.
#[derive(Clone, Debug)]
pub struct Workbook {
    pub sheets: Vec<Sheet>,
}

impl Workbook {
    pub fn new() -> Self {
        Workbook { sheets: Vec::new() }
    }

    pub fn push(&mut self, sheet: Sheet) {
        self.sheets.push(sheet);
    }

    pub fn sheet(&self, name: &str) -> Option<&Sheet> {
        self.sheets.iter().find(|s| s.name == name)
    }
}

impl Default for Workbook {
    fn default() -> Self {
        Self::new()
    }
}

// ── Address helpers ─────────────────────────────────────────────────────────

/// 0-based column index → Excel column name (`0 -> "A"`, `26 -> "AA"`).
pub fn col_name(mut col: u32) -> String {
    let mut name = String::new();
    loop {
        let rem = (col % 26) as u8;
        name.insert(0, (b'A' + rem) as char);
        if col < 26 {
            break;
        }
        col = col / 26 - 1;
    }
    name
}

/// 0-based `(row, col)` → Excel address (`(9, 2) -> "C10"`).
pub fn cell_ref(row: u32, col: u32) -> String {
    format!("{}{}", col_name(col), row + 1)
}

/// Parse an Excel address (`"C10"`) → 0-based `(row, col)`.
pub fn parse_ref(reference: &str) -> Option<(u32, u32)> {
    let split = reference.find(|c: char| c.is_ascii_digit())?;
    let (letters, digits) = reference.split_at(split);
    if letters.is_empty() || digits.is_empty() {
        return None;
    }
    let mut col: u32 = 0;
    for ch in letters.bytes() {
        if !ch.is_ascii_uppercase() {
            return None;
        }
        col = col * 26 + (ch - b'A' + 1) as u32;
    }
    let row: u32 = digits.parse().ok()?;
    Some((row - 1, col - 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn col_names() {
        assert_eq!(col_name(0), "A");
        assert_eq!(col_name(2), "C");
        assert_eq!(col_name(9), "J");
        assert_eq!(col_name(25), "Z");
        assert_eq!(col_name(26), "AA");
    }

    #[test]
    fn refs_roundtrip() {
        assert_eq!(cell_ref(9, 2), "C10");
        assert_eq!(cell_ref(0, 3), "D1");
        assert_eq!(parse_ref("C10"), Some((9, 2)));
        assert_eq!(parse_ref("D1"), Some((0, 3)));
        assert_eq!(parse_ref("AA5"), Some((4, 26)));
    }

    #[test]
    fn value_overrides_formula() {
        let mut s = Sheet::new("X");
        s.formula(0, 0, "=A2");
        s.number(0, 0, 5.0);
        let c = &s.cells[&(0, 0)];
        assert_eq!(c.value, Some(Value::Number(5.0)));
        assert!(c.formula.is_none());
    }

    #[test]
    fn fill_merges_with_value() {
        let mut s = Sheet::new("X");
        s.number(0, 0, 5.0);
        s.fill(0, 0, BLUE);
        let c = &s.cells[&(0, 0)];
        assert_eq!(c.value, Some(Value::Number(5.0)));
        assert_eq!(c.fill.as_deref(), Some(BLUE));
    }
}
