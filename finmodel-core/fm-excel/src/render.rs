//! Render the [`Workbook`] cell-model to a polished `.xlsx` via `rust_xlsxwriter`.
//!
//! The cell-model is the source of truth for *content* (value / formula / fill),
//! which is what the snapshot parity gate compares. Visual finish — fonts,
//! semantic colors, borders, alignment, column widths, frozen panes, hidden
//! gridlines — is applied here at render time (invisible to the content gates,
//! exactly like number formats) so the output matches the Python writer's
//! IB-grade look.
//!
//! Color system (mirrors `writer.py::_Fmt`):
//!   * hardcoded number input      → blue   (`#0000FF`)
//!   * cross-tab formula (`=X!…`)  → green  (`#008000`)
//!   * same-tab formula / text     → ink    (`#0F1632`)
//!   * navy/red fill               → white bold (titles / totals / failures)
//!   * sand fill                   → ink bold  (section headers)
//!   * explicit `font_hex`         → overrides the above (navy headers, gray drivers)

use std::collections::HashMap;

use rust_xlsxwriter::{
    Color, Format, FormatAlign, FormatBorder, FormatPattern, Formula, Workbook as XlsxWorkbook,
};

use crate::model::{Cell, Value, Workbook, DATA0, LABEL};
use crate::Result;

// Font colors (writer.py `_Fmt`).
const INK: u32 = 0x0F_1632; // same-tab formula / labels
const INPUT_BLUE: u32 = 0x00_00FF; // hardcoded input
const XTAB_GREEN: u32 = 0x00_8000; // cross-tab link
const WHITE: u32 = 0xFF_FFFF;
const SAND_ARGB: &str = "FFEAE0D3";

const FONT: &str = "Arial";
const FONT_SZ: f64 = 10.0;

/// Parse an 8-hex ARGB string into an opaque RGB color (drop alpha).
fn argb_to_color(argb: &str) -> Color {
    let hex = if argb.len() == 8 { &argb[2..] } else { argb };
    Color::RGB(u32::from_str_radix(hex, 16).unwrap_or(0))
}

fn hex_to_u32(hex: &str) -> u32 {
    u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap_or(INK)
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Align {
    Left,
    Center,
    Right,
}

/// Resolved visual style for one cell — the cache key for shared `Format`s.
#[derive(Clone, PartialEq, Eq, Hash)]
struct Style {
    fill: Option<String>,
    num_fmt: Option<&'static str>,
    font_color: u32,
    bold: bool,
    italic: bool,
    align: Align,
    top_border: bool,
    bottom_border: bool,
}

fn build_format(st: &Style) -> Format {
    let mut f = Format::new()
        .set_font_name(FONT)
        .set_font_size(FONT_SZ)
        .set_align(FormatAlign::VerticalCenter)
        .set_font_color(Color::RGB(st.font_color));

    f = match st.align {
        Align::Left => f.set_align(FormatAlign::Left),
        Align::Center => f.set_align(FormatAlign::Center),
        Align::Right => f.set_align(FormatAlign::Right),
    };
    if st.bold {
        f = f.set_bold();
    }
    if st.italic {
        f = f.set_italic();
    }
    if let Some(argb) = &st.fill {
        f = f
            .set_pattern(FormatPattern::Solid)
            .set_background_color(argb_to_color(argb));
    }
    if let Some(nf) = st.num_fmt {
        f = f.set_num_format(nf);
    }
    if st.top_border {
        f = f.set_border_top(FormatBorder::Thin);
    }
    if st.bottom_border {
        f = f.set_border_bottom(FormatBorder::Thin);
    }
    f
}

/// Derive the visual style of a cell from its content + fill + explicit emphasis.
fn style_for(cell: &Cell, col: u32) -> Style {
    let is_number = matches!(cell.value, Some(Value::Number(_)));
    let is_text = matches!(cell.value, Some(Value::Text(_)));
    let is_formula = cell.formula.is_some();
    let cross_tab = cell.formula.as_deref().map(|f| f.contains('!')).unwrap_or(false);

    // Fill-driven emphasis first (titles, totals, section headers).
    let (mut font_color, mut bold, filled) = match cell.fill.as_deref() {
        Some(SAND_ARGB) => (INK, true, true),          // section header
        Some(_) => (WHITE, true, true),                // navy title / total / red fail
        None => {
            if is_number {
                (INPUT_BLUE, false, false) // hardcoded input
            } else if is_formula && cross_tab {
                (XTAB_GREEN, false, false) // cross-tab link
            } else {
                (INK, false, false) // same-tab formula or text label
            }
        }
    };

    // Explicit overrides from the builders (writer.py `_Fmt` roles that content
    // inference can't recover): gray drivers/memos, navy headers, forced bold.
    if let Some(hex) = cell.font_hex {
        font_color = hex_to_u32(hex);
    }
    if cell.bold {
        bold = true;
    }
    let italic = cell.italic;

    // Alignment: explicit center wins; else numbers/formulas right, text left in
    // the label column and centered in data columns (period headers).
    let align = if cell.center {
        Align::Center
    } else if is_number || is_formula {
        Align::Right
    } else if is_text {
        if filled || col <= LABEL {
            Align::Left
        } else {
            Align::Center
        }
    } else {
        Align::Left
    };

    Style {
        fill: cell.fill.clone(),
        num_fmt: cell.num_fmt,
        font_color,
        bold,
        italic,
        align,
        top_border: cell.top_border,
        bottom_border: cell.bottom_border,
    }
}

/// Render `wb` to an `.xlsx` file at `path`.
pub fn render(wb: &Workbook, path: &str) -> Result<()> {
    let mut book = XlsxWorkbook::new();
    let mut fmts: HashMap<Style, Format> = HashMap::new();

    for sheet in &wb.sheets {
        let ws = book.add_worksheet();
        ws.set_name(&sheet.name)?;

        // ── Sheet-level polish ────────────────────────────────────────────
        ws.set_screen_gridlines(false);
        ws.set_landscape();

        // Column widths: A/B gutter narrow, C label wide, data cols medium.
        let max_col = sheet.cells.keys().map(|(_, c)| *c).max().unwrap_or(DATA0);
        ws.set_column_range_width(0, (LABEL - 1) as u16, 2.6)?;
        ws.set_column_range_width(LABEL as u16, LABEL as u16, 34.0)?;
        if max_col >= DATA0 {
            ws.set_column_range_width(DATA0 as u16, max_col as u16, 12.5)?;
        }

        // Freeze header rows + label column at the first row carrying a
        // numeric/formula value in a data column (keeps titles + period headers
        // + row labels pinned while scrolling the model body).
        let freeze_row = sheet
            .cells
            .iter()
            .filter(|((_, c), cell)| {
                *c >= DATA0
                    && (matches!(cell.value, Some(Value::Number(_))) || cell.formula.is_some())
            })
            .map(|((r, _), _)| *r)
            .min()
            .unwrap_or(0);
        ws.set_freeze_panes(freeze_row, DATA0 as u16)?;

        // ── Cells ─────────────────────────────────────────────────────────
        for ((row, col), cell) in &sheet.cells {
            let style = style_for(cell, *col);
            let f = fmts.entry(style.clone()).or_insert_with(|| build_format(&style)).clone();

            let formula_obj = |formula: &str, cached: Option<f64>| -> Formula {
                let mut fo = Formula::new(formula);
                if let Some(n) = cached {
                    fo = fo.set_result(format!("{n}"));
                }
                fo
            };

            match (&cell.value, &cell.formula) {
                (Some(Value::Number(n)), _) => {
                    ws.write_number_with_format(*row, *col as u16, *n, &f)?;
                }
                (Some(Value::Text(t)), _) => {
                    ws.write_string_with_format(*row, *col as u16, t, &f)?;
                }
                (None, Some(formula)) => {
                    ws.write_formula_with_format(
                        *row,
                        *col as u16,
                        formula_obj(formula, cell.cached),
                        &f,
                    )?;
                }
                (None, None) => {
                    ws.write_blank(*row, *col as u16, &f)?;
                }
            }
        }
    }

    book.save(path)?;
    Ok(())
}
