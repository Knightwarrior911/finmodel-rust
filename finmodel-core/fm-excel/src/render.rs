//! Render the [`Workbook`] cell-model to an `.xlsx` via `rust_xlsxwriter`.
//!
//! This is a thin, faithful pass over the model: whatever the model holds is
//! what gets written, so the snapshot parity comparison (which runs against the
//! model) reflects the real file.

use std::collections::HashMap;

use rust_xlsxwriter::{Color, Format, FormatPattern, Formula, Workbook as XlsxWorkbook};

use crate::model::{Value, Workbook};
use crate::Result;

/// Parse an 8-hex ARGB string (e.g. `"FF255BE3"`) into an RGB color, dropping
/// the alpha byte (Excel solid fills are opaque).
fn argb_to_color(argb: &str) -> Color {
    let hex = if argb.len() == 8 { &argb[2..] } else { argb };
    let rgb = u32::from_str_radix(hex, 16).unwrap_or(0);
    Color::RGB(rgb)
}

/// Render `wb` to an `.xlsx` file at `path`.
pub fn render(wb: &Workbook, path: &str) -> Result<()> {
    let mut book = XlsxWorkbook::new();
    // Cache formats by (fill ARGB, number-format) so identical styles share one
    // format object.
    let mut fmts: HashMap<(String, String), Format> = HashMap::new();

    for sheet in &wb.sheets {
        let ws = book.add_worksheet();
        ws.set_name(&sheet.name)?;

        for ((row, col), cell) in &sheet.cells {
            let fmt = if cell.fill.is_some() || cell.num_fmt.is_some() {
                let key = (
                    cell.fill.clone().unwrap_or_default(),
                    cell.num_fmt.unwrap_or_default().to_string(),
                );
                Some(
                    fmts.entry(key)
                        .or_insert_with(|| {
                            let mut f = Format::new();
                            if let Some(argb) = &cell.fill {
                                f = f.set_pattern(FormatPattern::Solid)
                                    .set_background_color(argb_to_color(argb));
                            }
                            if let Some(nf) = cell.num_fmt {
                                f = f.set_num_format(nf);
                            }
                            f
                        })
                        .clone(),
                )
            } else {
                None
            };

            let formula_obj = |formula: &str, cached: Option<f64>| -> Formula {
                let mut fo = Formula::new(formula);
                if let Some(n) = cached {
                    // LibreOffice/Excel offline open show this until recalculation.
                    fo = fo.set_result(format!("{n}"));
                }
                fo
            };
            match (&cell.value, &cell.formula, fmt) {
                (Some(Value::Number(n)), _, Some(f)) => { ws.write_number_with_format(*row, *col as u16, *n, &f)?; }
                (Some(Value::Number(n)), _, None) => { ws.write_number(*row, *col as u16, *n)?; }
                (Some(Value::Text(t)), _, Some(f)) => { ws.write_string_with_format(*row, *col as u16, t, &f)?; }
                (Some(Value::Text(t)), _, None) => { ws.write_string(*row, *col as u16, t)?; }
                (None, Some(formula), Some(f)) => {
                    ws.write_formula_with_format(*row, *col as u16, formula_obj(formula, cell.cached), &f)?;
                }
                (None, Some(formula), None) => {
                    ws.write_formula(*row, *col as u16, formula_obj(formula, cell.cached))?;
                }
                (None, None, Some(f)) => { ws.write_blank(*row, *col as u16, &f)?; }
                (None, None, None) => {}
            }
        }
    }

    book.save(path)?;
    Ok(())
}
