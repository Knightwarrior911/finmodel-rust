//! Per-sheet builders. Each produces a [`Sheet`] cell-model from a
//! [`WorkbookInput`]; [`build_workbook`] assembles them in tab order.
//! Snapshot layout: Cover, Assumptions, IS, BS, CF, Sources.
//! When valuation is present: + DCF, WACC, Sensitivities before Sources.

use crate::input::WorkbookInput;
use crate::model::{Sheet, Workbook, DATA0};

pub mod assumptions;
pub mod bs;
pub mod cf;
pub mod comps_peers;
pub mod comps_summary;
pub mod cover;
pub mod dcf;
pub mod is_stmt;
pub mod sensitivities;
pub mod sources;
pub mod wacc;

/// 0-based worksheet column for period index `j` (period 0 → col D).
#[inline]
pub(crate) fn col(j: usize) -> u32 {
    DATA0 + j as u32
}


/// Write a formula, attaching a cached numeric result when available so
/// LibreOffice/Excel show a value before recalculation.
pub(crate) fn formula_maybe_cached(
    s: &mut Sheet,
    row: u32,
    col: u32,
    formula: impl AsRef<str>,
    cache: Option<f64>,
) {
    match cache {
        Some(v) => s.formula_cached(row, col, formula, v),
        None => s.formula(row, col, formula),
    }
}


/// Build every sheet. Valuation tabs (DCF/WACC/Sensitivities) are emitted only
/// when `input.dcf` / `input.wacc` are present — keeps the committed 6-sheet
/// snapshot gate green.
pub fn build_workbook(input: &WorkbookInput) -> Workbook {
    let mut wb = Workbook::new();
    wb.push(cover::build(input));
    wb.push(assumptions::build(input));
    wb.push(is_stmt::build(input));
    wb.push(bs::build(input));
    wb.push(cf::build(input));
    if input.dcf.is_some() {
        wb.push(dcf::build(input));
    }
    if input.wacc.is_some() {
        wb.push(wacc::build(input));
    }
    if input.dcf.is_some() {
        wb.push(sensitivities::build(input));
    }
    if input.public_comps.is_some() {
        wb.push(comps_peers::build(input));
        wb.push(comps_summary::build(input));
    }
    wb.push(sources::build(input));
    wb
}

/// Emphasis colors (writer.py `_Fmt`): navy brand + gray drivers/units.
pub(crate) const NAVY: &str = "255BE3";
pub(crate) const GRAY: &str = "595959";

/// Shared tab header used by IS/BS/CF: single-cell title (blue), subtitle,
/// units line, and the "Active Case:" link. Rows are 0-based and match the
/// snapshot layout (title row 2, subtitle 4, units 5, active-case 8).
pub(crate) fn tab_header(s: &mut Sheet, title: &str, subtitle: &str, currency: &str) {
    use crate::model::LABEL;
    s.title(2, title);
    s.text(4, LABEL, subtitle);
    s.cell_mut(4, LABEL).font_hex = Some(NAVY);
    s.cell_mut(4, LABEL).bold = true;
    s.text(5, LABEL, format!("({currency} in millions, unless noted)"));
    s.cell_mut(5, LABEL).font_hex = Some(GRAY);
    s.cell_mut(5, LABEL).italic = true;
    s.text(8, LABEL, "Active Case:");
    s.cell_mut(8, LABEL).bold = true;
    s.formula(8, DATA0, "=Assumptions!$D$10");
}

/// Column-header row of period labels (row `row`, cols D..): navy bold, centered,
/// with a hairline underline — the writer.py `hcol` family.
pub(crate) fn period_headers(s: &mut Sheet, row: u32, periods: &[String]) {
    for (j, p) in periods.iter().enumerate() {
        s.text(row, col(j), p.clone());
        let c = s.cell_mut(row, col(j));
        c.font_hex = Some(NAVY);
        c.bold = true;
        c.center = true;
        c.bottom_border = true;
    }
}

// ── Date arithmetic (port of src/utils.py) ──────────────────────────────────

fn month_num(fye: &str) -> u32 {
    match fye {
        "Jan" => 1, "Feb" => 2, "Mar" => 3, "Apr" => 4, "May" => 5, "Jun" => 6,
        "Jul" => 7, "Aug" => 8, "Sep" => 9, "Oct" => 10, "Nov" => 11, _ => 12,
    }
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn last_day_of_month(y: i64, m: u32) -> i64 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => if is_leap(y) { 29 } else { 28 },
    }
}

/// Days since 1970-01-01 (Howard Hinnant's `days_from_civil`).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

const FILING_LAG_DAYS: i64 = 90;

/// Parse an ISO `YYYY-MM-DD` date.
fn parse_iso(s: &str) -> Option<(i64, u32, u32)> {
    let mut it = s.splitn(3, '-');
    let y: i64 = it.next()?.parse().ok()?;
    let m: u32 = it.next()?.parse().ok()?;
    let d: u32 = it.next()?.parse().ok()?;
    Some((y, m, d))
}

/// Calendar year of the latest REPORTED fiscal year (90-day filing lag).
/// Port of `latest_reported_fy_year`.
fn latest_reported_fy_year(fye: &str, today: (i64, u32, u32)) -> i64 {
    let (ty, _tm, _td) = today;
    let today_ord = days_from_civil(today.0, today.1 as i64, today.2 as i64);
    let month = month_num(fye);

    let this_fye_day = last_day_of_month(ty, month);
    let this_fye_ord = days_from_civil(ty, month as i64, this_fye_day);

    if this_fye_ord < today_ord {
        if this_fye_ord + FILING_LAG_DAYS <= today_ord { ty } else { ty - 1 }
    } else {
        let prev_day = last_day_of_month(ty - 1, month);
        let prev_ord = days_from_civil(ty - 1, month as i64, prev_day);
        if prev_ord + FILING_LAG_DAYS <= today_ord { ty - 1 } else { ty - 2 }
    }
}

/// Projected-period labels for the Assumptions tab, date-derived exactly as the
/// Python snapshot generator does (latest reported FY + 1 .. + n_proj).
pub(crate) fn assumptions_proj_periods(fye: &str, as_of: &str, n_proj: usize) -> Vec<String> {
    let today = parse_iso(as_of).unwrap_or((1970, 1, 1));
    let latest = latest_reported_fy_year(fye, today);
    (0..n_proj).map(|i| format!("{}E", latest + 1 + i as i64)).collect()
}
