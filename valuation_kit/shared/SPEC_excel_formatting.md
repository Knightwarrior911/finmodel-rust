# SPEC_excel_formatting.md - Excel Formatting Standards

This file defines **visual conventions** for Excel output. These are generic IB-style standards. **If a firm-specific brand standard exists, it overrides this file.**

---

## 1. Page and Tab Setup

### 1.1 Per-tab Settings
- Gridlines: **off** on every tab.
- Page setup: landscape, fit to 1 page wide, automatic height.
- Footer: tab name on left, page number on right.
- Print area: from B2 to the first row/column below the data.
- Freeze panes: do not freeze unless user requests.

### 1.2 Column Widths
- Columns A and B: width 3 (blank – gutters that give the tab a clean left margin).
- Column C: width ~42 (label column).
- Data columns (D onward): width ~13.

### 1.3 Standard Row Layout
- Row 1: 8px spacer (blank)
- Row 2: 8px spacer (blank)
- Row 3: Tab title bar – colored background, white text, 16pt bold, spans label + data columns
- Row 4: 8px spacer (blank)
- Row 5: Subheader – 11pt bold (Citi Ink color or firm equivalent)
- Row 6: Units label – 10pt italic, e.g., "(USD $ in millions)"
- Row 7: 8px spacer (blank)
- Row 8+: Content begins (for non-statement tabs)
- Row 12: Year headers (for statement tabs – IS, BS, CF use rows 9-11 for case display and circ switch)

---

## 2. Color System (defaults; override with firm brand)

### Primary
| Name          | Hex       | Usage |
|---------------|-----------|-------|
| Ink (dark navy) | `#0F1632` | Body text, labels |
| Brand Blue    | `#2558B3` | Header bars, year headers, table rules, totals fill |
| Red           | `#FF3C28` | Negative highlights only (sparingly) |
| White         | `#FFFFFF` | Header bar text |

### Utility
| Name                | Hex       | Usage |
|---------------------|-----------|-------|
| Light Gray          | `#E6EBED` | Sensitivity table headers, alt row shading, tab strip |
| Mid Gray            | `#D3DADD` | Hist/proj divider, sensitivity base case |
| Light Border Gray   | `#A4ACAF` | Borders, outlines |
| Sand / Beige        | `#EAE0D3` | Section dividers, subsection shading, emphasis |

### Cell Content Colors (Formulas)
| Type                        | Hex       |
|-----------------------------|-----------|
| Blue inputs                 | `#0000FF` |
| Black local formulas        | `#000000` |
| Green cross-sheet           | `#008000` |

---

## 3. Number Formats

| Type                  | Format Code |
|-----------------------|-------------|
| Dollars in $M         | `#,##0_);($#,##0);"-";@` |
| Plain numbers         | `#,##0_);(#,##0);"-";@` |
| Percentages           | `0.0%_);(0.0%);"-";@` |
| Multiples             | `0.0"x";(0.0"x");"-";@` |
| Share prices          | `#,##0.00_);($#,##0.00);"-";@` |

**Rules:**
- No decimals on large dollar figures ($M). Use `#,##0` not `#,##0.0`.
- Share prices and per-share data: 2 decimals.
- Negatives in parentheses, never with a minus sign.
- Right-align all numeric cells.
- Apply `$` format to first row of each section and to per-share/price-related items. Other dollar rows use plain number format without the `$`.
- Zero values display as `"-"` (em dash) by default.

---

## 4. Year and Period Conventions
- Year format: no prefix, with quarter style `Q1 '25`, `Q2 '25`, etc.
- Year header suffixes:
  - **A** for actuals (e.g., `2024A`)
  - **E** for estimates / projections (e.g., `2026E`) — never P
- Units label format: `(USD $ in millions)` – italicized, 10pt, in row 6.
- No explicit "Historical" / "Projected" labels above year headers — the A/E suffix carries this.
- No date row beneath year headers.

---

## 5. Borders and Rules

### Table Rules
- Thin brand-colored 0.75pt line above the first row of data.
- Thin brand-colored 0.75pt line below the last row of data.
- Keep the rest of the table clean — no internal vertical lines unless absolutely needed.

### Subtotals
- Top border (thin) on key subtotals: Gross Profit, EBIT, EBITDA, Net Income, Total Assets, Total Liabilities & Equity, CFO, CFI, CFF, Change in Cash.
- Bold top border on the most senior subtotals (EBITDA, Net Income).
- Double bottom border on terminal lines (Net Income to Common, Total Liabilities and Equity, Ending Cash).

### Historical / Projection Divider
- Right border on the last historical column, in mid-gray (`#D3DADD`).
- Subtle vertical line separating actuals from forecasts. Spans all data rows.
- No blank gap column between historicals and projections — the right border alone is the divider.

### Year Headers
- Brand-colored text, bold, center-aligned.
- Bottom border (thin, brand color) — underline effect.
- No background fill.

---

## 6. Bold Tiers
- **Bold**: Key subtotals — Revenue, Gross Profit, EBITDA, EBIT, Net Income, Total Assets, Total Equity, Ending Cash.
- **Regular weight**: Cost line items — COGS, SG&A, D&A, Interest Expense, Taxes.
- **Italic**: Margins, growth rates, assumptions, memo / driver rows, check rows.

---

## 7. Section Dividers (within tabs)
- Background: Sand / beige (`#EAE0D3` or firm equivalent).
- Text: Ink color, bold.
- Spans the full width of the section.
- Use sparingly — only for meaningfully distinct blocks (e.g., "Assets" vs. "Liabilities" vs. "Equity"; "Working Capital Schedule" vs. "PP&E Schedule" vs. "Debt Schedule").
- Do not use to label every subsection of a continuous statement. Within a statement, use bold line items and borders for hierarchy.
- Do not confuse with the tab title bar in row 3.

---

## 8. Margin and Memo Rows
- Sit directly below the line item they describe.
- Include a height-5 spacer row below each margin row to visually separate from the next line item.
- Display `"-"` (dash) when zero — the standard percentage format handles this.

---

## 9. Sensitivity Tables
- Column headers (top row) and row headers (left column): light gray (`#E6EBED` or equivalent) fill, ink text, bold, center-aligned.
- Column header cells get a bottom border separating them from data.
- Row header cells get a right border separating them from data.
- Base case cell: mid-gray (`#D3DADD`) fill.
- Data cells: unshaded, standard number format.
- Italic label above the table identifying the column axis (e.g., _Exit Multiple_).
- Italic label to the left identifying the row axis (e.g., _Base Year_).
- Top-left corner cell: meaningful label (metric name or base case value), never blank.
- Use mixed cell references for the formula: `$A5` for row input, `B$4` for column input.
- Do **NOT** use Excel’s DATA TABLE feature — it does not survive openpyxl/LibreOffice round-trip.

---

## 10. Tab Naming and Tab Color
- Tab names: short and consistent. Use abbreviations (IS, BS, CF, Assumptions, Cover). Schedule tabs may use longer names (Debt Schedule, Working Capital Detail).
- Tab strip color:
  - Statement tabs (IS, BS, CF, Assumptions): mid-light gray (`#E6EBED`)
  - Supporting/schedule tabs: slightly darker gray (`#D3DADD`)
  - Cover: any neutral

---

## 11. Alignment
- Vertically middle-align all text.
- **Center-align all column headers** (year headers, period headers). This is critical — never skip.
- Right-align numeric cells.
- Left-align row labels in the label column.

---

## 12. Financial Conventions

| Convention                        | Rule |
|-----------------------------------|------|
| Currency approximations in narrative | Use `~` (e.g., ~$50m) |
| Years — Actual                    | `YYYYA` (e.g., 2024A) |
| Years — Estimate                  | `YYYYE` (e.g., 2026E) — never P |
| Negatives in tables               | Parentheses: (50) |
| Negatives in charts               | Minus sign: -50 |
| Not applicable                    | "n/a" |
| Multiples                         | Lowercase x: 6.0x |
| Percentages                       | No space: 78.2% |

---

## 13. What to Avoid
- Yellow input highlighting (blue text is sufficient).
- Cell notes / annotations beyond the standard citation comment.
- Watermarks, logos, branding embedded in the worksheet.
- Cover tab unless explicitly requested.
- Merged cells (use "center across selection" instead).
- Alternating row shading across default — keep tables clean with white backgrounds.
- 3D effects, gradients, or graphic flourishes on charts. 2D only.
- Vendor formula highlighting (e.g., purple for Bloomberg / FactSet) — use ink color for all formulas regardless of source.

---

## 14. Firm Brand Override

If a firm-specific Excel brand standard is loaded (e.g., a Citi, Morgan Stanley, JPMorgan, or boutique brand spec), it overrides this file. In particular:
- Color hex codes
- Specific fonts (Citi Sans Text vs. Arial vs. Calibri)
- Tab title bar style
- Section divider color
- Number format details (some firms include trailing zeros, others don’t)
- Year suffix style (some use `'` instead of A/E)

The **substance** of this file (formulas-not-hardcodes, font color = cell type, two-hop pattern, no plugs, validator gating) is universal and never overridden by brand standards.

---

**End of SPEC_excel_formatting.md**