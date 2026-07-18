//! Income Statement tab.
//!
//! With an empty `is_structure` (the committed-snapshot config) only the header
//! block is written. With a populated structure (the app path) the full dynamic
//! IS body is emitted — port of writer.py `_write_is` + `_write_is_data_row` /
//! `_write_is_driver_row` / `_write_is_memo_row` / `_is_hist_formula` /
//! `_is_proj_formula` for the standard sector.

use std::collections::HashMap;

use crate::input::{Statement, WorkbookInput};
use crate::is_structure::{IS_BODY_START, RowType, compute_is_row_map, driver_assump_offset};
use crate::model::{DATA0, FMT_NUM, FMT_PCT, LABEL, Sheet, cell_ref, col_name};
use crate::sheets::{col, formula_maybe_cached, period_headers, tab_header};

// Assumptions active-driver block anchor (0-based) + data col.
const ASSUMP_ACTIVE_DRV0: u32 = 14;
const ASSUMP_DATA0: u32 = 3;
// BS cross-refs used by IS proj formulas (0-based rows).
const BS_DEBT_INT: u32 = 62; // Excel 63
const BS_CASH: u32 = 11; // Excel 12

/// Blue (raw-XBRL) historical keys — written as values, not formulas.
fn is_blue_hist(key: &str) -> bool {
    matches!(
        key,
        "revenue"
            | "cogs"
            | "sga"
            | "rd"
            | "da"
            | "ebit"
            | "ebita"
            | "net_income"
            | "interest_expense"
            | "interest_income"
            | "income_tax"
            | "nci_income_loss"
            | "eps_diluted"
            | "eps_basic"
            | "shares_diluted"
            | "shares_basic"
            | "utility_om"
            | "utility_taxes_other"
    )
}

fn av(is_d: &Statement, key: &str, i: usize) -> Option<f64> {
    is_d.get(key).and_then(|v| v.get(i)).copied().flatten()
}

/// `cell_ref` at IS body row `row0`, period index `ci`.
fn at(row0: u32, ci: usize) -> String {
    cell_ref(row0, col(ci))
}

fn rm_row(rm: &HashMap<String, u32>, key: &str, fallback: u32) -> u32 {
    rm.get(key).copied().unwrap_or(fallback)
}

/// True when the row-map has any key with `prefix` (segment / opex detail rows).
fn has_prefix(rm: &HashMap<String, u32>, prefix: &str) -> bool {
    rm.keys().any(|k| k.starts_with(prefix))
}

/// `=cell+cell+…` over the sorted rows whose keys start with `prefix`, at period `ci`.
fn sum_prefix(rm: &HashMap<String, u32>, prefix: &str, ci: usize) -> String {
    let mut rows: Vec<u32> = rm
        .iter()
        .filter(|(k, _)| k.starts_with(prefix))
        .map(|(_, v)| *v)
        .collect();
    rows.sort_unstable();
    let cells: Vec<String> = rows.iter().map(|r| at(*r, ci)).collect();
    format!("={}", cells.join("+"))
}

/// Historical formula for non-blue subtotals. Returns None → blank cell.
fn is_hist_formula(key: &str, j: usize, rm: &HashMap<String, u32>) -> Option<String> {
    let rev = rm_row(rm, "revenue", 10);
    match key {
        "revenue" if has_prefix(rm, "rev_seg_") => Some(sum_prefix(rm, "rev_seg_", j)),
        "cogs" if has_prefix(rm, "cogs_seg_") => Some(sum_prefix(rm, "cogs_seg_", j)),
        "gross_profit" => {
            let cogs = rm_row(rm, "cogs", 12);
            Some(format!("={}-{}", at(rev, j), at(cogs, j)))
        }
        "ebitda" => {
            let ebit = rm_row(rm, "ebit", 18);
            let da = rm_row(rm, "da", 24);
            Some(format!("={}+{}", at(ebit, j), at(da, j)))
        }
        "ebt" => {
            let ebit = rm_row(rm, "ebit", 18);
            let ie = rm_row(rm, "interest_expense", 25);
            let ii = rm_row(rm, "interest_income", 26);
            Some(format!("={}-{}+{}", at(ebit, j), at(ie, j), at(ii, j)))
        }
        "ni_common" => {
            let ni = rm_row(rm, "net_income", 31);
            let nci = rm_row(rm, "nci_income_loss", 33);
            Some(format!("={}-{}", at(ni, j), at(nci, j)))
        }
        "utility_total_opex" => {
            let om = rm_row(rm, "utility_om", 0);
            let da = rm_row(rm, "da", 0);
            let tax = rm_row(rm, "utility_taxes_other", 0);
            let oth = rm_row(rm, "utility_other", 0);
            Some(format!(
                "={}+{}+{}+{}",
                at(om, j),
                at(da, j),
                at(tax, j),
                at(oth, j)
            ))
        }
        "utility_other" => {
            let ebit = rm_row(rm, "ebit", 18);
            let om = rm_row(rm, "utility_om", 0);
            let da = rm_row(rm, "da", 0);
            let tax = rm_row(rm, "utility_taxes_other", 0);
            Some(format!(
                "={}-{}-{}-{}-{}",
                at(rev, j),
                at(ebit, j),
                at(om, j),
                at(da, j),
                at(tax, j)
            ))
        }
        _ => None,
    }
}

/// Projection formula for IS key at proj period `j`. Returns None → blank.
fn is_proj_formula(key: &str, j: usize, rm: &HashMap<String, u32>, n_h: usize) -> Option<String> {
    let ci = n_h + j;
    let rev = rm_row(rm, "revenue", 10);
    let drv = |dk: &str| at(rm_row(rm, &format!("__drv_{dk}"), 0), ci);

    let f = match key {
        "revenue" if has_prefix(rm, "rev_seg_") => sum_prefix(rm, "rev_seg_", ci),
        "revenue" => format!("={}*(1+{})", at(rev, ci - 1), drv("revenue_growth_pct")),
        k if k.starts_with("rev_seg_") => {
            format!(
                "={}*(1+{})",
                at(rm_row(rm, k, rev), ci - 1),
                drv("revenue_growth_pct")
            )
        }
        "cogs" if has_prefix(rm, "cogs_seg_") => sum_prefix(rm, "cogs_seg_", ci),
        "cogs" => {
            let gp = rm_row(rm, "gross_profit", 13);
            format!("={}-{}", at(rev, ci), at(gp, ci))
        }
        "gross_profit" => format!("={}*{}", at(rev, ci), drv("gross_margin_pct")),
        "sga" => format!("={}*{}", at(rev, ci), drv("sga_pct_rev")),
        "rd" => format!("={}*{}", at(rev, ci), drv("rd_pct_rev")),
        "da" => format!("={}*{}", at(rev, ci), drv("da_pct_rev")),
        "utility_om" => format!("={}*{}", at(rev, ci), drv("gross_margin_pct")),
        "utility_taxes_other" => format!("={}*{}", at(rev, ci), drv("sga_pct_rev")),
        "utility_other" => format!("={}*{}", at(rev, ci), drv("rd_pct_rev")),
        "utility_total_opex" => {
            let om = rm_row(rm, "utility_om", 0);
            let da = rm_row(rm, "da", 0);
            let tax = rm_row(rm, "utility_taxes_other", 0);
            let oth = rm_row(rm, "utility_other", 0);
            format!(
                "={}+{}+{}+{}",
                at(om, ci),
                at(da, ci),
                at(tax, ci),
                at(oth, ci)
            )
        }
        "ebit" => {
            if let Some(top) = rm.get("utility_total_opex") {
                format!("={}-{}", at(rev, ci), at(*top, ci))
            } else {
                let start = at(rm_row(rm, "gross_profit", rev), ci);
                let mut s = format!("={start}");
                for k in ["sga", "rd", "da"] {
                    if let Some(r) = rm.get(k) {
                        s.push_str(&format!("-{}", at(*r, ci)));
                    }
                }
                // Extra opex_ items subtract into EBIT (sorted by key).
                let mut extra: Vec<(&String, u32)> = rm
                    .iter()
                    .filter(|(k, _)| k.starts_with("opex_"))
                    .map(|(k, v)| (k, *v))
                    .collect();
                extra.sort_by(|a, b| a.0.cmp(b.0));
                for (_, r) in extra {
                    s.push_str(&format!("-{}", at(r, ci)));
                }
                s
            }
        }
        "ebita" => format!("={}", at(rm_row(rm, "ebit", 18), ci)),
        "ebitda" => {
            let ebit = rm_row(rm, "ebit", 18);
            let da = rm_row(rm, "da", 24);
            format!("={}+{}", at(ebit, ci), at(da, ci))
        }
        "interest_expense" => format!("=BS!{}", cell_ref(BS_DEBT_INT, col(ci))),
        "interest_income" => {
            format!("=BS!{}${}*0.02", col_name(col(ci - 1)), BS_CASH + 1)
        }
        "ebt" => {
            let ebit = rm_row(rm, "ebit", 18);
            let ie = rm_row(rm, "interest_expense", 25);
            let ii = rm_row(rm, "interest_income", 26);
            format!("={}-{}+{}", at(ebit, ci), at(ie, ci), at(ii, ci))
        }
        "income_tax" => format!(
            "=MAX(0,{}*{})",
            at(rm_row(rm, "ebt", 27), ci),
            drv("tax_rate_pct")
        ),
        "net_income" => {
            let ebt = rm_row(rm, "ebt", 27);
            let tax = rm_row(rm, "income_tax", 28);
            format!("={}-{}", at(ebt, ci), at(tax, ci))
        }
        "ni_common" => {
            let ni = rm_row(rm, "net_income", 31);
            let nci = rm_row(rm, "nci_income_loss", 33);
            format!("={}-{}", at(ni, ci), at(nci, ci))
        }
        "eps_diluted" => {
            let ni = rm_row(rm, "ni_common", 34);
            let sh = rm_row(rm, "shares_diluted", 39);
            format!(
                "=IF({sh}<>0,{ni}/{sh},\"\")",
                sh = at(sh, ci),
                ni = at(ni, ci)
            )
        }
        "eps_basic" => {
            let ni = rm_row(rm, "ni_common", 34);
            let sh = rm_row(rm, "shares_basic", 40);
            format!(
                "=IF({sh}<>0,{ni}/{sh},\"\")",
                sh = at(sh, ci),
                ni = at(ni, ci)
            )
        }
        k if k.starts_with("opex_") => format!("={}", at(rm_row(rm, k, 0), ci - 1)),
        _ => return None,
    };
    Some(f)
}

pub fn build(input: &WorkbookInput) -> Sheet {
    let m = &input.meta;
    let mut s = Sheet::new("IS");

    tab_header(&mut s, &m.company, "Income Statement", &m.currency);
    s.text(7, LABEL, "Circ Switch  (0 = off | 1 = on)");
    s.cell_mut(7, LABEL).italic = true;
    s.cell_mut(7, LABEL).font_hex = Some(crate::sheets::GRAY);
    s.number(7, DATA0, 0.0);
    period_headers(&mut s, 9, &input.model.periods);

    if input.is_structure.is_empty() {
        return s; // header-only (committed-snapshot config)
    }

    let is_d = &input.model.income_statement;
    let n_h = input.model.n_hist();
    let n_p = input.model.n_proj();
    let n = input.model.periods.len();
    let rm = compute_is_row_map(&input.is_structure);
    let proj_sh = input.assumptions.shares_diluted;

    for (idx, isr) in input.is_structure.iter().enumerate() {
        let r = IS_BODY_START + idx as u32;
        match isr.row_type {
            RowType::Spacer => {}
            RowType::SectionHeader => s.section(r, isr.label.clone()),
            RowType::LineItem | RowType::Subtotal => {
                s.text(r, LABEL, isr.label.clone());
                let key = &isr.key;
                // Historical: raw-XBRL keys (incl segment/opex detail) are values;
                // subtotals that aggregate detail rows are formulas.
                let blue = (is_blue_hist(key)
                    || key.starts_with("rev_seg_")
                    || key.starts_with("cogs_seg_")
                    || key.starts_with("opex_"))
                    && !(key == "revenue" && has_prefix(&rm, "rev_seg_"))
                    && !(key == "cogs" && has_prefix(&rm, "cogs_seg_"));
                if blue {
                    for j in 0..n_h {
                        if let Some(v) = av(is_d, key, j) {
                            s.number(r, col(j), v);
                        }
                    }
                } else {
                    for j in 0..n_h {
                        if let Some(f) = is_hist_formula(key, j, &rm) {
                            formula_maybe_cached(&mut s, r, col(j), f, av(is_d, key, j));
                        }
                    }
                }
                // Projection.
                if key == "shares_diluted" || key == "shares_basic" {
                    for j in 0..n_p {
                        let ci = n_h + j;
                        let v = av(is_d, key, ci).unwrap_or(proj_sh);
                        s.number(r, col(ci), v);
                    }
                } else if key == "nci_income_loss" {
                    let last = if n_h > 0 {
                        av(is_d, key, n_h - 1).unwrap_or(0.0)
                    } else {
                        0.0
                    };
                    for j in 0..n_p {
                        s.number(r, col(n_h + j), last);
                    }
                } else {
                    for j in 0..n_p {
                        if let Some(f) = is_proj_formula(key, j, &rm, n_h) {
                            formula_maybe_cached(
                                &mut s,
                                r,
                                col(n_h + j),
                                f,
                                av(is_d, key, n_h + j),
                            );
                        }
                    }
                }
                s.stamp_row(r, FMT_NUM);
                if isr.row_type == RowType::Subtotal {
                    s.stamp_bold_row(r);
                    s.stamp_top_border_row(r);
                } else if isr.bold {
                    s.cell_mut(r, LABEL).bold = true;
                }
            }
            RowType::Driver => {
                s.text(r, LABEL, isr.label.clone());
                let growth_ref =
                    if !isr.hist_denom_key.is_empty() && isr.hist_denom_key != "revenue" {
                        rm_row(&rm, &isr.hist_denom_key, rm_row(&rm, "revenue", 10))
                    } else {
                        rm_row(&rm, "revenue", 10)
                    };
                // Historical implied ratio / growth.
                for j in 0..n_h {
                    if isr.hist_numer_key == "__growth" {
                        if j > 0 {
                            let cur = at(growth_ref, j);
                            let prev = at(growth_ref, j - 1);
                            s.formula(r, col(j), format!("=IF({prev}<>0,{cur}/{prev}-1,\"\")"));
                        }
                    } else if !isr.hist_numer_key.is_empty() && !isr.hist_denom_key.is_empty() {
                        let num = at(rm_row(&rm, &isr.hist_numer_key, 0), j);
                        let den = at(rm_row(&rm, &isr.hist_denom_key, 0), j);
                        s.formula(r, col(j), format!("=IF({den}<>0,{num}/{den},\"\")"));
                    }
                }
                // Projection → link to Assumptions active block.
                if let Some(off) = driver_assump_offset(&isr.driver_key) {
                    let active_row = ASSUMP_ACTIVE_DRV0 + off;
                    for j in 0..n_p {
                        let c = cell_ref(active_row, ASSUMP_DATA0 + j as u32);
                        s.formula(r, col(n_h + j), format!("=Assumptions!{c}"));
                    }
                }
                s.stamp_row(
                    r,
                    if isr.driver_format == "num" {
                        FMT_NUM
                    } else {
                        FMT_PCT
                    },
                );
                s.stamp_italic_row(r);
                s.cell_mut(r, LABEL).font_hex = Some(crate::sheets::GRAY);
            }
            RowType::Memo => {
                s.text(r, LABEL, isr.label.clone());
                let num_r = rm_row(&rm, &isr.hist_numer_key, 0);
                let den_r = rm_row(&rm, &isr.hist_denom_key, 0);
                for j in 0..n {
                    let num = at(num_r, j);
                    let den = at(den_r, j);
                    s.formula(r, col(j), format!("=IF({den}<>0,{num}/{den},\"\")"));
                }
                s.stamp_row(r, FMT_PCT);
                s.stamp_italic_row(r);
                s.cell_mut(r, LABEL).font_hex = Some(crate::sheets::GRAY);
            }
        }
    }

    // Attach engine-projected caches to formula cells (LibreOffice offline).
    for (idx, isr) in input.is_structure.iter().enumerate() {
        if isr.key.is_empty() {
            continue;
        }
        let r = IS_BODY_START + idx as u32;
        if let Some(vals) = is_d.get(&isr.key) {
            for (j, v) in vals.iter().enumerate() {
                if let Some(n) = *v {
                    if let Some(cell) = s.cells.get_mut(&(r, col(j))) {
                        if cell.formula.is_some() && cell.cached.is_none() {
                            cell.cached = Some(n);
                        }
                    }
                }
            }
        }
    }

    // ── Revenue breakdown by segment (seg_* keys) — port of _write_is_segments.
    let mut seg_keys: Vec<&String> = is_d.keys().filter(|k| k.starts_with("seg_")).collect();
    if !seg_keys.is_empty() {
        seg_keys.sort();
        let start = IS_BODY_START + input.is_structure.len() as u32 + 2;
        s.section(start, "REVENUE BREAKDOWN BY SEGMENT  (from XBRL)");
        let rev_r = rm_row(&rm, "revenue", 10);
        let round2 = |x: f64| (x * 100.0).round() / 100.0;
        let mut row = start + 1;
        for seg_key in seg_keys {
            let base = &seg_key[4..];
            let cleaned = base.replace("Revenue", "").replace("Sales", "");
            let name = cleaned.trim();
            let name = if name.is_empty() { base } else { name };
            s.text(row, LABEL, format!("  {name}"));
            for j in 0..n_h {
                if let Some(v) = av(is_d, seg_key, j) {
                    s.number(row, col(j), v);
                }
            }
            for j in 0..n_p {
                let ci = n_h + j;
                let prev = av(is_d, seg_key, ci - 1);
                let proj_val = match prev {
                    Some(pv) if pv != 0.0 => {
                        let prev_total = av(is_d, "revenue", ci - 1)
                            .filter(|x| *x != 0.0)
                            .unwrap_or(1.0);
                        let cur_total = av(is_d, "revenue", ci)
                            .filter(|x| *x != 0.0)
                            .unwrap_or(prev_total);
                        round2(pv * (cur_total / prev_total))
                    }
                    other => other.unwrap_or(0.0),
                };
                s.number(row, col(ci), proj_val);
            }
            s.stamp_row(row, FMT_NUM);
            s.text(row + 1, LABEL, "    % of Total Revenue");
            for j in 0..n {
                let seg_c = cell_ref(row, col(j));
                let rev_c = cell_ref(rev_r, col(j));
                s.formula(
                    row + 1,
                    col(j),
                    format!("=IF({rev_c}<>0,{seg_c}/{rev_c},\"\")"),
                );
            }
            s.stamp_row(row + 1, FMT_PCT);
            row += 2;
        }
    }

    s
}
