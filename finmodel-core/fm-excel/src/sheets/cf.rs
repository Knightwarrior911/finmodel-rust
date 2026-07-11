//! Cash Flow Statement tab — port of `writer.py::_write_cf`.

use crate::input::{Statement, WorkbookInput};
use crate::model::{cell_ref, Sheet, FMT_NUM, FMT_PCT, DATA0, LABEL};
use crate::sheets::{col, period_headers, tab_header};

/// First value under `key` at period `idx`, mirroring Python `_v` / `.get`.
fn g(stmt: &Statement, key: &str, idx: usize) -> Option<f64> {
    stmt.get(key).and_then(|v| v.get(idx)).copied().flatten()
}

/// Round to 2 decimals (matches Python `round(x, 2)` for non-tie values).
fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

pub fn build(input: &WorkbookInput) -> Sheet {
    let m = &input.meta;
    let mo = &input.model;
    let is = &mo.income_statement;
    let bs = &mo.balance_sheet;
    let cf = &mo.cash_flow_statement;
    let n_h = mo.n_hist();
    let n = mo.periods.len();
    let n_p = n - n_h;
    let (is_rev, is_da, is_ni, is_shares) = (
        input.is_row("revenue"),
        input.is_row("da"),
        input.is_row("net_income"),
        input.is_row("shares_diluted"),
    );

    let mut s = Sheet::new("CF");
    tab_header(&mut s, &m.company, "Cash Flow Statement", &m.currency);
    period_headers(&mut s, 9, &mo.periods);

    s.section(10, "OPERATING ACTIVITIES");
    s.section(21, "INVESTING ACTIVITIES");
    s.section(28, "FINANCING ACTIVITIES");

    // ── Historical residual values (writer.py lines 2181-2200) ──────────────
    // D&A add-back: CFS is authoritative; fall back to IS when the CFS tag was
    // unpopulated across all historicals.
    let cfs_da_has = (0..n_h).any(|j| g(cf, "da", j).is_some());
    let da_hist = |j: usize| if cfs_da_has { g(cf, "da", j) } else { g(is, "da", j) };

    let mut h_wc_ar = vec![0.0f64; n_h];
    let mut h_wc_inv = vec![0.0f64; n_h];
    let mut h_wc_ap = vec![0.0f64; n_h];
    let mut h_wc_defrev = vec![0.0f64; n_h];
    for j in 1..n_h {
        h_wc_ar[j] =
            round2(-(g(bs, "accounts_receivable", j).unwrap_or(0.0)
                - g(bs, "accounts_receivable", j - 1).unwrap_or(0.0)));
        h_wc_inv[j] =
            round2(-(g(bs, "inventory", j).unwrap_or(0.0) - g(bs, "inventory", j - 1).unwrap_or(0.0)));
        h_wc_ap[j] =
            round2(g(bs, "accounts_payable", j).unwrap_or(0.0)
                - g(bs, "accounts_payable", j - 1).unwrap_or(0.0));
        h_wc_defrev[j] =
            round2(g(bs, "deferred_revenue_current", j).unwrap_or(0.0)
                - g(bs, "deferred_revenue_current", j - 1).unwrap_or(0.0));
    }
    let h_wc_other: Vec<f64> = (0..n_h)
        .map(|j| {
            round2(
                g(cf, "cfo", j).unwrap_or(0.0)
                    - g(is, "net_income", j).unwrap_or(0.0)
                    - da_hist(j).unwrap_or(0.0)
                    - h_wc_ar[j]
                    - h_wc_inv[j]
                    - h_wc_ap[j]
                    - h_wc_defrev[j],
            )
        })
        .collect();
    let h_other_cfi: Vec<f64> = (0..n_h)
        .map(|j| {
            round2(
                g(cf, "cfi", j).unwrap_or(0.0)
                    + g(cf, "capex", j).unwrap_or(0.0)
                    + g(cf, "investments_net_cfi", j).unwrap_or(0.0),
            )
        })
        .collect();
    let h_other_cff: Vec<f64> = (0..n_h)
        .map(|j| {
            round2(
                g(cf, "cff", j).unwrap_or(0.0)
                    + g(cf, "dividends_paid", j).unwrap_or(0.0)
                    + g(cf, "buybacks", j).unwrap_or(0.0),
            )
        })
        .collect();
    let h_fx: Vec<f64> = (0..n_h)
        .map(|j| {
            round2(
                g(cf, "net_change_cash", j).unwrap_or(0.0)
                    - g(cf, "cfo", j).unwrap_or(0.0)
                    - g(cf, "cfi", j).unwrap_or(0.0)
                    - g(cf, "cff", j).unwrap_or(0.0),
            )
        })
        .collect();
    let h_beg0 = g(bs, "cash", 0)
        .map(|c| round2(c - g(cf, "net_change_cash", 0).unwrap_or(0.0)));

    // ── OPERATING ACTIVITIES ────────────────────────────────────────────────
    s.text(11, LABEL, "  Net Income");
    for j in 0..n {
        let c = col(j);
        s.formula(11, c, format!("=IS!{}", cell_ref(is_ni, c)));
    }
    s.text(12, LABEL, "  D&A (add-back)");
    for j in 0..n {
        let c = col(j);
        s.formula(12, c, format!("=IS!{}", cell_ref(is_da, c)));
    }
    s.text(13, LABEL, "  Δ Accounts Receivable");
    s.text(14, LABEL, "  Δ Inventory");
    s.text(15, LABEL, "  Δ Accounts Payable");
    s.text(16, LABEL, "  Δ Deferred Revenue");
    s.text(17, LABEL, "  Other working capital");
    for j in 0..n_h {
        let c = col(j);
        s.number(13, c, h_wc_ar[j]);
        s.number(14, c, h_wc_inv[j]);
        s.number(15, c, h_wc_ap[j]);
        s.number(16, c, h_wc_defrev[j]);
        s.number(17, c, h_wc_other[j]);
    }
    for j in n_h..n {
        let c = col(j);
        let p = col(j - 1);
        s.formula(13, c, format!("=-(BS!{}-BS!{})", cell_ref(12, c), cell_ref(12, p)));
        s.formula(14, c, format!("=-(BS!{}-BS!{})", cell_ref(13, c), cell_ref(13, p)));
        s.formula(15, c, format!("=BS!{}-BS!{}", cell_ref(21, c), cell_ref(21, p)));
        s.formula(16, c, format!("=BS!{}-BS!{}", cell_ref(23, c), cell_ref(23, p)));
        s.number(17, c, 0.0);
    }
    s.text(18, LABEL, "  Other operating");
    for j in 0..n {
        s.number(18, col(j), 0.0);
    }
    s.text(19, LABEL, "Cash from Operations");
    for j in 0..n {
        let c = col(j);
        s.formula(
            19,
            c,
            format!(
                "={}+{}+{}+{}+{}+{}+{}+{}",
                cell_ref(11, c),
                cell_ref(12, c),
                cell_ref(13, c),
                cell_ref(14, c),
                cell_ref(15, c),
                cell_ref(16, c),
                cell_ref(17, c),
                cell_ref(18, c),
            ),
        );
    }

    // ── INVESTING ACTIVITIES ────────────────────────────────────────────────
    s.text(22, LABEL, "  Capital Expenditures");
    for j in 0..n_h {
        if let Some(v) = g(cf, "capex", j) {
            s.number(22, col(j), -v);
        }
    }
    for j in n_h..n {
        let c = col(j);
        s.formula(22, c, format!("=-IS!{}*{}", cell_ref(is_rev, c), cell_ref(23, c)));
    }
    s.text(23, LABEL, "    CapEx % of Revenue");
    for j in 0..n_h {
        let c = col(j);
        s.formula(
            23,
            c,
            format!("=IF(IS!{r}<>0,ABS({cap})/IS!{r},0)", r = cell_ref(is_rev, c), cap = cell_ref(22, c)),
        );
    }
    for j in 0..n_p {
        s.formula(23, col(n_h + j), format!("=Assumptions!{}", cell_ref(19, DATA0 + j as u32)));
    }
    s.text(24, LABEL, "  Net Purchases of Investments");
    for j in 0..n_h {
        if let Some(v) = g(cf, "investments_net_cfi", j) {
            s.number(24, col(j), -v);
        }
    }
    for j in n_h..n {
        s.number(24, col(j), 0.0);
    }
    s.text(25, LABEL, "  Other investing");
    for j in 0..n_h {
        s.number(25, col(j), h_other_cfi[j]);
    }
    for j in n_h..n {
        s.number(25, col(j), 0.0);
    }
    s.text(26, LABEL, "Cash from Investing");
    for j in 0..n {
        let c = col(j);
        s.formula(
            26,
            c,
            format!("=ROUND({}+{}+{},2)", cell_ref(22, c), cell_ref(24, c), cell_ref(25, c)),
        );
    }

    // ── FINANCING ACTIVITIES ────────────────────────────────────────────────
    s.text(29, LABEL, "  Dividends Paid");
    for j in 0..n_h {
        if let Some(v) = g(cf, "dividends_paid", j) {
            s.number(29, col(j), v);
        }
    }
    s.text(30, LABEL, "    Dividend per Share ($)");
    for j in 0..n_h {
        let c = col(j);
        s.formula(
            30,
            c,
            format!("=IF(IS!{sh}<>0,ABS({div})/IS!{sh},0)", sh = cell_ref(is_shares, c), div = cell_ref(29, c)),
        );
    }
    for j in 0..n_p {
        let c = col(n_h + j);
        s.formula(30, c, format!("=Assumptions!{}", cell_ref(25, DATA0 + j as u32)));
        s.formula(29, c, format!("=IS!{}*{}", cell_ref(is_shares, c), cell_ref(30, c)));
    }
    s.text(31, LABEL, "  Share Buybacks");
    for j in 0..n {
        if let Some(v) = g(cf, "buybacks", j) {
            s.number(31, col(j), v);
        }
    }
    s.text(32, LABEL, "  Other financing  (debt ±, issuances)");
    for j in 0..n_h {
        s.number(32, col(j), h_other_cff[j]);
    }
    for j in n_h..n {
        s.number(32, col(j), 0.0);
    }
    s.text(33, LABEL, "Cash from Financing");
    for j in 0..n {
        let c = col(j);
        s.formula(
            33,
            c,
            format!("=ROUND(-{}-{}+{},2)", cell_ref(29, c), cell_ref(31, c), cell_ref(32, c)),
        );
    }
    s.text(34, LABEL, "  FX & Other Adjustments");
    for j in 0..n_h {
        s.number(34, col(j), h_fx[j]);
    }
    for j in n_h..n {
        s.number(34, col(j), 0.0);
    }

    // ── Net Change + Cash Balances ──────────────────────────────────────────
    s.text(35, LABEL, "Net Change in Cash");
    for j in 0..n {
        let c = col(j);
        s.formula(
            35,
            c,
            format!(
                "=ROUND({}+{}+{}+{},2)",
                cell_ref(19, c),
                cell_ref(26, c),
                cell_ref(33, c),
                cell_ref(34, c),
            ),
        );
    }
    s.text(36, LABEL, "Beginning Cash");
    if let Some(v) = h_beg0 {
        s.number(36, col(0), v);
    }
    for j in 1..n_h {
        if let Some(v) = g(bs, "cash", j - 1) {
            s.number(36, col(j), v);
        }
    }
    for j in 0..n_p {
        let ci = n_h + j;
        s.formula(36, col(ci), format!("={}", cell_ref(37, col(ci - 1))));
    }
    s.text(37, LABEL, "Ending Cash");
    for j in 0..n_h {
        if let Some(v) = g(bs, "cash", j) {
            s.number(37, col(j), v);
        }
    }
    for j in 0..n_p {
        let c = col(n_h + j);
        s.formula(37, c, format!("={}+{}", cell_ref(36, c), cell_ref(35, c)));
    }
    s.text(38, LABEL, "  Free Cash Flow  (CFO − CapEx)");
    for j in 0..n {
        let c = col(j);
        s.formula(38, c, format!("={}-ABS({})", cell_ref(19, c), cell_ref(22, c)));
    }

    // ── Validation Checks ───────────────────────────────────────────────────
    s.text(40, LABEL, "  Check: CF NI = IS NI  (should = 0)");
    for j in 0..n {
        let c = col(j);
        s.formula(40, c, format!("=ROUND({}-IS!{},2)", cell_ref(11, c), cell_ref(is_ni, c)));
    }
    s.text(41, LABEL, "  Check: CF Ending Cash = BS Cash  (should = 0)");
    for j in 0..n {
        let c = col(j);
        s.formula(41, c, format!("=ROUND({}-BS!{},2)", cell_ref(37, c), cell_ref(11, c)));
    }

    // Number formats (product polish; not gate-checked). Monetary cells default to
    // thousands-separated numbers; the CapEx%-of-revenue driver row is a percentage.

    // Attach engine-projected caches to formula cells (LibreOffice offline).
    for &(row, key) in &[
        (11u32, "net_income"), // from IS; may also be in CF statement
        (12, "da"),
        (19, "cfo"),
        (22, "capex"),
        (26, "cfi"),
        (29, "dividends_paid"),
        (33, "cff"),
        (35, "net_change_cash"),
        (37, "cash"), // ending cash ~ BS cash
        (38, "fcf"),
    ] {
        // Prefer CF statement; fall back to IS/BS for linked rows.
        let vals = cf.get(key)
            .or_else(|| is.get(key))
            .or_else(|| bs.get(key));
        if let Some(vals) = vals {
            for (j, v) in vals.iter().enumerate() {
                if let Some(n) = *v {
                    // CapEx displayed as outflow (negative) on CF.
                    let n = if row == 22 { -n.abs() } else { n };
                    if let Some(cell) = s.cells.get_mut(&(row, col(j))) {
                        if cell.formula.is_some() && cell.cached.is_none() {
                            cell.cached = Some(n);
                        }
                    }
                }
            }
        }
    }

    s.stamp_numeric_default(FMT_NUM);
    s.stamp_row(23, FMT_PCT);

    // Visual finish (render-only): bold subtotal rows + top border; italic drivers/checks.
    for row in [19u32, 26, 33, 35, 37] {
        s.stamp_bold_row(row);
        s.stamp_top_border_row(row);
    }
    // Drivers (capex% / dividend-per-share): whole-row italic, gray label.
    for row in [23u32, 30] {
        s.stamp_italic_row(row);
        s.cell_mut(row, LABEL).font_hex = Some(crate::sheets::GRAY);
    }
    // Checks: whole-row italic, ink label.
    for row in [40u32, 41] {
        s.stamp_italic_row(row);
    }
    // Free Cash Flow: italic label only (data cells stay upright).
    {
        let c = s.cell_mut(38, LABEL);
        c.italic = true;
        c.font_hex = Some(crate::sheets::GRAY);
    }
    s
}
