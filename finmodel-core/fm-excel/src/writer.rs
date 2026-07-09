use rust_xlsxwriter::{Color, Format, Formula, Workbook};
use rust_xlsxwriter::XlsxError;

use crate::Result;

/// Top-level configuration metadata embedded in the workbook.
pub struct ExcelConfig {
    pub company: String,
    pub ticker: String,
    pub currency: String,
}

/// A single worksheet's data: name, column headers, and labelled rows.
pub struct SheetData {
    pub name: String,
    pub headers: Vec<String>,
    pub rows: Vec<(String, Vec<CellValue>)>,
}

/// The possible values a cell can hold.
pub enum CellValue {
    /// An `f64` numeric value.
    Value(f64),
    /// An Excel formula string (e.g. `"=SUM(A1:A10)"`).
    Formula(String),
    /// Plain text; if it starts with `'='` it is written as a formula.
    Text(String),
    /// An intentionally empty cell (no content written).
    Empty,
}

/// Maps a tier number (1–5) to a cell background fill colour.
///
/// | Tier | Colour  |
/// |------|---------|
/// | 1    | Blue    |
/// | 2    | Black   |
/// | 3    | Green   |
/// | 4    | Orange  |
/// | 5    | Red     |
pub struct TierColor {
    tier: u8,
}

impl TierColor {
    pub fn new(tier: u8) -> Self {
        Self {
            tier: tier.clamp(1, 5),
        }
    }

    /// The ARGB hex value for this tier's background colour.
    pub const fn background_color(&self) -> u32 {
        match self.tier {
            1 => 0x4472C4, // Blue
            2 => 0x404040, // Black
            3 => 0x70AD47, // Green
            4 => 0xED7D31, // Orange
            5 => 0xFF0000, // Red
            _ => 0xFFFFFF, // White (unreachable due to clamp)
        }
    }

    /// Creates a `rust_xlsxwriter::Format` with the tier's background colour.
    pub fn to_format(&self) -> Format {
        Format::new().set_background_color(Color::RGB(self.background_color()))
    }
}

/// Writes an Excel workbook to `path` with one worksheet per `SheetData`.
///
/// Each sheet contains a bold header row (row 0) followed by data rows.
/// The first column holds the row label; subsequent columns hold period values
/// written according to their `CellValue` variant.
///
/// # Errors
///
/// Returns an error if the workbook cannot be created or saved, or if any
/// worksheet operation fails.
pub fn write_workbook(path: &str, sheets: &[SheetData]) -> Result<()> {
    let mut workbook = Workbook::new();
    let header_format = Format::new().set_bold();

    for sheet_data in sheets {
        let mut worksheet = workbook.add_worksheet();
        worksheet.set_name(&sheet_data.name)?;

        // Write header row.
        for (col, header) in sheet_data.headers.iter().enumerate() {
            worksheet.write_string_with_format(0, col as u16, header, &header_format)?;
        }

        // Write data rows (starting at row 1).
        for (row_idx, (label, cell_values)) in sheet_data.rows.iter().enumerate() {
            let row = (row_idx + 1) as u32;

            // First column: the row label.
            worksheet.write_string(row, 0, label)?;

            for (col_idx, cell_value) in cell_values.iter().enumerate() {
                let col = (col_idx + 1) as u16;
                write_cell(&mut worksheet, row, col, cell_value)?;
            }
        }
    }

    workbook.save(path)?;
    Ok(())
}

/// Writes a single cell according to its `CellValue` variant.
fn write_cell(
    worksheet: &mut rust_xlsxwriter::Worksheet,
    row: u32,
    col: u16,
    value: &CellValue,
) -> std::result::Result<(), XlsxError> {
    match value {
        CellValue::Value(v) => {
            worksheet.write_number(row, col, *v)?;
        }
        CellValue::Formula(f) => {
            worksheet.write_formula(row, col, Formula::new(f.as_str()))?;
        }
        CellValue::Text(t) => {
            if t.starts_with('=') {
                worksheet.write_formula(row, col, Formula::new(t.as_str()))?;
            } else {
                worksheet.write_string(row, col, t.as_str())?;
            }
        }
        CellValue::Empty => {}
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_simple_workbook() {
        let sheets = vec![SheetData {
            name: "TestSheet".to_string(),
            headers: vec![
                "Item".to_string(),
                "2023".to_string(),
                "2024".to_string(),
                "2025".to_string(),
            ],
            rows: vec![
                (
                    "Revenue".to_string(),
                    vec![
                        CellValue::Value(100.0),
                        CellValue::Value(110.0),
                        CellValue::Value(121.0),
                    ],
                ),
                (
                    "Costs".to_string(),
                    vec![
                        CellValue::Value(60.0),
                        CellValue::Value(66.0),
                        CellValue::Value(72.6),
                    ],
                ),
            ],
        }];

        let tmp = std::env::temp_dir().join("test_simple_workbook.xlsx");
        let path = tmp.to_str().unwrap();
        write_workbook(path, &sheets).unwrap();

        assert!(tmp.exists(), "Workbook file should exist");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn test_formula_cells() {
        let sheets = vec![SheetData {
            name: "Formulas".to_string(),
            headers: vec!["Item".to_string(), "Value".to_string()],
            rows: vec![
                (
                    "Total".to_string(),
                    vec![CellValue::Formula("=SUM(B2:B10)".to_string())],
                ),
                (
                    "Growth".to_string(),
                    vec![CellValue::Text("=C2/C1-1".to_string())],
                ),
            ],
        }];

        let tmp = std::env::temp_dir().join("test_formula_cells.xlsx");
        let path = tmp.to_str().unwrap();
        write_workbook(path, &sheets).unwrap();

        assert!(tmp.exists(), "Workbook file should exist");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn test_tier_color_mapping() {
        let tier = TierColor::new(1);
        assert_eq!(tier.background_color(), 0x4472C4);

        let tier = TierColor::new(5);
        assert_eq!(tier.background_color(), 0xFF0000);

        // Out-of-range values are clamped.
        let tier = TierColor::new(0);
        assert_eq!(tier.background_color(), 0x4472C4);
        let tier = TierColor::new(6);
        assert_eq!(tier.background_color(), 0xFF0000);
    }

    #[test]
    fn test_empty_cells_skipped() {
        let sheets = vec![SheetData {
            name: "Empty".to_string(),
            headers: vec!["Item".to_string(), "Val".to_string()],
            rows: vec![(
                "Partial".to_string(),
                vec![CellValue::Value(42.0), CellValue::Empty],
            )],
        }];

        let tmp = std::env::temp_dir().join("test_empty_cells.xlsx");
        let path = tmp.to_str().unwrap();
        write_workbook(path, &sheets).unwrap();

        assert!(tmp.exists());
        std::fs::remove_file(path).unwrap();
    }
}
