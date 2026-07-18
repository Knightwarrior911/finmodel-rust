//! Balance Sheet tab — main statement plus PP&E / Working-Capital / Debt /
//! Retained-Earnings supporting schedules. Ported from `writer.py::_write_bs`
//! and `_write_bs_schedules`; historicals are hardcoded numbers (skipped when
//! the model has no value), projections are Excel formulas that cross-link to
//! the schedules below and the IS/CF/Assumptions tabs.

use crate::input::{Statement, WorkbookInput};
use crate::model::{BLUE, FMT_NUM, FMT_PCT, LABEL, Sheet, cell_ref};
use crate::sheets::{col, formula_maybe_cached, period_headers, tab_header};

// ── BS main-section rows (0-based; Excel row = index + 1) ────────────────────
const ASSETS_HDR: u32 = 10;
const CASH: u32 = 11;
const AR: u32 = 12;
const INVENTORY: u32 = 13;
const TCA: u32 = 14;
const PPE_NET: u32 = 15;
const GOODWILL: u32 = 16;
const INTANG: u32 = 17;
const TOTAL_ASSETS: u32 = 18;
const LE_HDR: u32 = 20;
const AP: u32 = 21;
const TCL: u32 = 22;
const DEF_CUR: u32 = 23;
const LTD: u32 = 24;
const DEF_LT: u32 = 25;
const TOTAL_LIAB: u32 = 26;
const RNCI: u32 = 28;
const EQUITY_HDR: u32 = 30;
const RET_EARN: u32 = 31;
const TOTAL_EQ: u32 = 32;
const TOTAL_LE: u32 = 33;
const BS_CHECK: u32 = 34;

// ── Supporting-schedule rows ─────────────────────────────────────────────────
const SCHED_TITLE: u32 = 37;
const PPE_HDR: u32 = 39;
const PPE_BEG: u32 = 40;
const PPE_CAPEX: u32 = 41;
const PPE_DA: u32 = 42;
const PPE_OTHER: u32 = 43;
const PPE_END: u32 = 44;
const WC_HDR: u32 = 46;
const WC_AR_DAYS: u32 = 47;
const WC_AR: u32 = 48;
const WC_INV_DAYS: u32 = 49;
const WC_INV: u32 = 50;
const WC_AP_DAYS: u32 = 51;
const WC_AP: u32 = 52;
const WC_NET_CHG: u32 = 54;
const DEBT_HDR: u32 = 56;
const DEBT_RATE: u32 = 57;
const DEBT_BEG: u32 = 58;
const DEBT_NEW: u32 = 59;
const DEBT_REPAID: u32 = 60;
const DEBT_END: u32 = 61;
const DEBT_INT: u32 = 62;
const RE_HDR: u32 = 64;
const RE_BEG: u32 = 65;
const RE_NI: u32 = 66;
const RE_DIV: u32 = 67;
const RE_BB: u32 = 68;
const RE_END: u32 = 69;

// ── Cross-sheet rows referenced by BS formulas (0-based) ─────────────────────
const CF_ENDING_CASH: u32 = 37;
const CF_DIVIDENDS: u32 = 29;
const CF_BUYBACKS: u32 = 31;
const CF_CAPEX: u32 = 22;
/// IS cross-reference rows (0-based), resolved dynamically: the full-IS row-map
/// when an IS body is built, else the empty-IS fallback.
struct IsRows {
    revenue: u32,
    cogs: u32,
    da: u32,
    int_exp: u32,
    net_income: u32,
    ni_common: u32,
    circ: u32,
}
impl IsRows {
    fn new(input: &WorkbookInput) -> Self {
        Self {
            revenue: input.is_row("revenue"),
            cogs: input.is_row("cogs"),
            da: input.is_row("da"),
            int_exp: input.is_row("interest_expense"),
            net_income: input.is_row("net_income"),
            ni_common: input.is_row("ni_common"),
            circ: input.is_row("circ"),
        }
    }
}

// Assumptions active-driver rows and data column (0-based).
const ASMP_DSO: u32 = 22;
const ASMP_DIO: u32 = 23;
const ASMP_DPO: u32 = 24;
const ASMP_DATA0: u32 = 3;

/// Read a per-period statement value (None when missing/null).
fn g(st: &Statement, key: &str, i: usize) -> Option<f64> {
    st.get(key).and_then(|v| v.get(i)).copied().flatten()
}

/// Write hardcoded historical numbers from statement `key` (skip missing).
fn hist_nums(s: &mut Sheet, st: &Statement, key: &str, row: u32, n_h: usize) {
    for j in 0..n_h {
        if let Some(v) = g(st, key, j) {
            s.number(row, col(j), v);
        }
    }
}

pub fn build(input: &WorkbookInput) -> Sheet {
    let m = &input.meta;
    let mo = &input.model;
    let bs = &mo.balance_sheet;
    let is = &mo.income_statement;
    let cf = &mo.cash_flow_statement;
    let isr = IsRows::new(input);
    let n_h = mo.n_hist();
    let n = mo.periods.len();
    let last = n_h - 1;

    let mut s = Sheet::new("BS");
    tab_header(&mut s, &m.company, "Balance Sheet", &m.currency);
    period_headers(&mut s, 9, &mo.periods);

    // "Other" plug constants — unmodeled items held flat at last historical.
    let gl = |key: &str| g(bs, key, last).unwrap_or(0.0);
    let other_ca =
        gl("total_current_assets") - gl("cash") - gl("accounts_receivable") - gl("inventory");
    let other_cl = gl("total_current_liabilities") - gl("accounts_payable");
    let other_ltl =
        gl("total_liabilities") - gl("total_current_liabilities") - gl("long_term_debt");
    let other_ltl_adj = other_ltl - gl("deferred_revenue_lt");

    // ── ASSETS ───────────────────────────────────────────────────────────────
    s.section(ASSETS_HDR, "ASSETS");

    // Cash: hist number; proj link to CF ending cash.
    s.text(CASH, LABEL, "  Cash & Equivalents");
    hist_nums(&mut s, bs, "cash", CASH, n_h);
    for j in n_h..n {
        let c = col(j);
        formula_maybe_cached(
            &mut s,
            CASH,
            c,
            format!("=CF!{}", cell_ref(CF_ENDING_CASH, c)),
            g(bs, "cash", j),
        );
    }

    // AR / Inventory: hist number; proj link to WC schedule.
    s.text(AR, LABEL, "  Accounts Receivable");
    hist_nums(&mut s, bs, "accounts_receivable", AR, n_h);
    for j in n_h..n {
        let c = col(j);
        formula_maybe_cached(
            &mut s,
            AR,
            c,
            format!("={}", cell_ref(WC_AR, c)),
            g(bs, "accounts_receivable", j),
        );
    }
    s.text(INVENTORY, LABEL, "  Inventory");
    hist_nums(&mut s, bs, "inventory", INVENTORY, n_h);
    for j in n_h..n {
        let c = col(j);
        formula_maybe_cached(
            &mut s,
            INVENTORY,
            c,
            format!("={}", cell_ref(WC_INV, c)),
            g(bs, "inventory", j),
        );
    }

    // Total Current Assets.
    s.text(TCA, LABEL, "Total Current Assets");
    hist_nums(&mut s, bs, "total_current_assets", TCA, n_h);
    for j in n_h..n {
        let c = col(j);
        s.formula(
            TCA,
            c,
            format!(
                "=ROUND({}+{}+{}+{:.2},2)",
                cell_ref(CASH, c),
                cell_ref(AR, c),
                cell_ref(INVENTORY, c),
                other_ca
            ),
        );
    }

    // PP&E, net: hist number; proj link to PP&E schedule ending.
    s.text(PPE_NET, LABEL, "  PP&E, net");
    hist_nums(&mut s, bs, "ppe_net", PPE_NET, n_h);
    for j in n_h..n {
        let c = col(j);
        formula_maybe_cached(
            &mut s,
            PPE_NET,
            c,
            format!("={}", cell_ref(PPE_END, c)),
            g(bs, "ppe_net", j),
        );
    }

    // Goodwill / Intangibles: no formulas; proj = model value or hold flat.
    for (row, label, key) in [
        (GOODWILL, "  Goodwill", "goodwill"),
        (INTANG, "  Intangibles, net", "intangibles_net"),
    ] {
        s.text(row, LABEL, label);
        hist_nums(&mut s, bs, key, row, n_h);
        for j in n_h..n {
            if let Some(v) = g(bs, key, j).or_else(|| g(bs, key, last)) {
                s.number(row, col(j), v);
            }
        }
    }

    // Total Assets: blue fill on all periods; proj derived from L+M+E side.
    s.text(TOTAL_ASSETS, LABEL, "Total Assets");
    for j in 0..n_h {
        if let Some(v) = g(bs, "total_assets", j) {
            s.number(TOTAL_ASSETS, col(j), v);
        }
    }
    for j in n_h..n {
        let c = col(j);
        s.formula(
            TOTAL_ASSETS,
            c,
            format!("=ROUND({},2)", cell_ref(TOTAL_LE, c)),
        );
    }
    for j in 0..n {
        s.fill(TOTAL_ASSETS, col(j), BLUE);
    }

    // ── LIABILITIES & EQUITY ───────────────────────────────────────────────
    s.section(LE_HDR, "LIABILITIES & EQUITY");

    // Accounts Payable: hist number; proj link to WC schedule.
    s.text(AP, LABEL, "  Accounts Payable");
    hist_nums(&mut s, bs, "accounts_payable", AP, n_h);
    for j in n_h..n {
        let c = col(j);
        formula_maybe_cached(
            &mut s,
            AP,
            c,
            format!("={}", cell_ref(WC_AP, c)),
            g(bs, "accounts_payable", j),
        );
    }

    // Total Current Liabilities.
    s.text(TCL, LABEL, "Total Current Liabilities");
    hist_nums(&mut s, bs, "total_current_liabilities", TCL, n_h);
    for j in n_h..n {
        let c = col(j);
        s.formula(
            TCL,
            c,
            format!("=ROUND({}+{:.2},2)", cell_ref(AP, c), other_cl),
        );
    }

    // Deferred Revenue (current): hist number; proj held flat.
    s.text(DEF_CUR, LABEL, "  Deferred Revenue (current)");
    hist_nums(&mut s, bs, "deferred_revenue_current", DEF_CUR, n_h);
    let last_def_cur = g(bs, "deferred_revenue_current", last).unwrap_or(0.0);
    for j in n_h..n {
        s.number(DEF_CUR, col(j), last_def_cur);
    }

    // Long-Term Debt: hist number; proj link to debt schedule ending.
    s.text(LTD, LABEL, "  Long-Term Debt");
    hist_nums(&mut s, bs, "long_term_debt", LTD, n_h);
    for j in n_h..n {
        let c = col(j);
        formula_maybe_cached(
            &mut s,
            LTD,
            c,
            format!("={}", cell_ref(DEBT_END, c)),
            g(bs, "long_term_debt", j),
        );
    }

    // Deferred Revenue (non-current): hist number; proj held flat.
    s.text(DEF_LT, LABEL, "  Deferred Revenue (non-current)");
    hist_nums(&mut s, bs, "deferred_revenue_lt", DEF_LT, n_h);
    let last_def_lt = g(bs, "deferred_revenue_lt", last).unwrap_or(0.0);
    for j in n_h..n {
        s.number(DEF_LT, col(j), last_def_lt);
    }

    // Total Liabilities.
    s.text(TOTAL_LIAB, LABEL, "Total Liabilities");
    hist_nums(&mut s, bs, "total_liabilities", TOTAL_LIAB, n_h);
    for j in n_h..n {
        let c = col(j);
        s.formula(
            TOTAL_LIAB,
            c,
            format!(
                "={}+{}+{}+{:.2}",
                cell_ref(TCL, c),
                cell_ref(LTD, c),
                cell_ref(DEF_LT, c),
                other_ltl_adj
            ),
        );
    }

    // Redeemable NCI (Mezzanine): hist number; proj held flat.
    s.text(RNCI, LABEL, "  Redeemable NCI (Mezzanine)");
    hist_nums(&mut s, bs, "redeemable_nci", RNCI, n_h);
    let last_rnci = g(bs, "redeemable_nci", last).unwrap_or(0.0);
    for j in n_h..n {
        s.number(RNCI, col(j), last_rnci);
    }

    // ── EQUITY ─────────────────────────────────────────────────────────────
    s.section(EQUITY_HDR, "EQUITY");

    // Retained Earnings: hist number; proj link to RE rollforward.
    s.text(RET_EARN, LABEL, "  Retained Earnings");
    hist_nums(&mut s, bs, "retained_earnings", RET_EARN, n_h);
    for j in n_h..n {
        let c = col(j);
        formula_maybe_cached(
            &mut s,
            RET_EARN,
            c,
            format!("={}", cell_ref(RE_END, c)),
            g(bs, "retained_earnings", j),
        );
    }

    // Total Equity: hist number; proj rollforward (prev + NI − Divs − Buybacks).
    s.text(TOTAL_EQ, LABEL, "Total Equity");
    hist_nums(&mut s, bs, "total_equity", TOTAL_EQ, n_h);
    for j in n_h..n {
        let c = col(j);
        s.formula(
            TOTAL_EQ,
            c,
            format!(
                "=ROUND({}+IS!{}-CF!{}-CF!{},2)",
                cell_ref(TOTAL_EQ, col(j - 1)),
                cell_ref(isr.net_income, c),
                cell_ref(CF_DIVIDENDS, c),
                cell_ref(CF_BUYBACKS, c)
            ),
        );
    }

    // Total Liab + Mezzanine + Equity: blue fill on all periods.
    s.text(TOTAL_LE, LABEL, "Total Liab + Mezzanine + Equity");
    for j in 0..n {
        let c = col(j);
        s.formula(
            TOTAL_LE,
            c,
            format!(
                "=ROUND({}+{}+{},2)",
                cell_ref(TOTAL_LIAB, c),
                cell_ref(RNCI, c),
                cell_ref(TOTAL_EQ, c)
            ),
        );
        s.fill(TOTAL_LE, c, BLUE);
    }

    // BS Check (Assets − L+M+E): no fill.
    s.text(BS_CHECK, LABEL, "  BS Check  (Assets \u{2212} L+M+E)");
    for j in 0..n {
        let c = col(j);
        s.formula(
            BS_CHECK,
            c,
            format!(
                "=ROUND({}-{},2)",
                cell_ref(TOTAL_ASSETS, c),
                cell_ref(TOTAL_LE, c)
            ),
        );
    }

    // ── Supporting schedules ────────────────────────────────────────────────
    build_schedules(&mut s, bs, is, &isr, cf, n_h, n, last);

    // Number formats (product polish; not gate-checked). Monetary cells default to
    // thousands-separated numbers; the interest-rate schedule row is a percentage.
    // Attach engine-projected caches to formula cells (LibreOffice offline).
    for &(row, key) in &[
        (CASH, "cash"),
        (AR, "accounts_receivable"),
        (INVENTORY, "inventory"),
        (TCA, "total_current_assets"),
        (PPE_NET, "ppe_net"),
        (GOODWILL, "goodwill"),
        (INTANG, "intangibles"),
        (TOTAL_ASSETS, "total_assets"),
        (AP, "accounts_payable"),
        (TCL, "total_current_liabilities"),
        (DEF_CUR, "deferred_revenue_current"),
        (LTD, "long_term_debt"),
        (DEF_LT, "deferred_revenue_lt"),
        (TOTAL_LIAB, "total_liabilities"),
        (RNCI, "redeemable_nci"),
        (RET_EARN, "retained_earnings"),
        (TOTAL_EQ, "total_equity"),
        (PPE_END, "ppe_net"),
        (WC_AR, "accounts_receivable"),
        (WC_INV, "inventory"),
        (WC_AP, "accounts_payable"),
        (DEBT_END, "long_term_debt"),
        (RE_END, "retained_earnings"),
    ] {
        if let Some(vals) = bs.get(key) {
            for (j, v) in vals.iter().enumerate() {
                if let Some(n) = *v {
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
    s.stamp_row(DEBT_RATE, FMT_PCT);

    // Visual finish (render-only): mirrors writer.py `_Fmt` families.
    for row in [
        TCA, TCL, TOTAL_LIAB, TOTAL_EQ, TOTAL_LE, PPE_END, DEBT_END, RE_END,
    ] {
        s.stamp_bold_row(row);
        s.stamp_top_border_row(row);
    }
    // Total Assets + schedule sub-headers: bold label (data already navy where filled).
    for row in [TOTAL_ASSETS, SCHED_TITLE, PPE_HDR, WC_HDR, DEBT_HDR, RE_HDR] {
        s.cell_mut(row, LABEL).bold = true;
    }
    // BS Check: whole-row italic, ink label (writer.py `lbl_chk`).
    s.stamp_italic_row(BS_CHECK);
    // Memo rows: gray italic label only.
    for row in [WC_NET_CHG, DEBT_INT] {
        let c = s.cell_mut(row, LABEL);
        c.italic = true;
        c.font_hex = Some(crate::sheets::GRAY);
    }
    // Driver rows: gray italic label + italic on the historical (implied) cells;
    // projected cells link to Assumptions (green, upright), so leave them.
    for row in [WC_AR_DAYS, WC_INV_DAYS, WC_AP_DAYS, DEBT_RATE] {
        let lc = s.cell_mut(row, LABEL);
        lc.italic = true;
        lc.font_hex = Some(crate::sheets::GRAY);
        for j in 0..n_h {
            s.cell_mut(row, col(j)).italic = true;
        }
    }
    s
}

fn build_schedules(
    s: &mut Sheet,
    bs: &Statement,
    is: &Statement,
    isr: &IsRows,
    cf: &Statement,
    n_h: usize,
    n: usize,
    last: usize,
) {
    s.section(SCHED_TITLE, "SUPPORTING SCHEDULES");

    // ── PP&E Schedule ─────────────────────────────────────────────────────
    s.text(PPE_HDR, LABEL, "PP&E Schedule");
    s.text(PPE_BEG, LABEL, "  Beginning PP&E");
    s.text(PPE_CAPEX, LABEL, "  + Capital Expenditures");
    s.text(PPE_DA, LABEL, "  \u{2212} Depreciation & Amort.");
    s.text(PPE_OTHER, LABEL, "  + Other / Acquisitions");
    s.text(PPE_END, LABEL, "Ending PP&E");

    for j in 0..n_h {
        if j > 0 {
            if let Some(v) = g(bs, "ppe_net", j - 1) {
                s.number(PPE_BEG, col(j), v);
            }
        }
        if let Some(v) = g(cf, "capex", j) {
            s.number(PPE_CAPEX, col(j), v);
        }
        if let Some(v) = g(is, "da", j) {
            s.number(PPE_DA, col(j), v);
        }
        s.number(PPE_OTHER, col(j), 0.0);
        if let Some(v) = g(bs, "ppe_net", j) {
            s.number(PPE_END, col(j), v);
        }
    }
    for j in n_h..n {
        let c = col(j);
        let prev_end = if j == n_h {
            cell_ref(PPE_NET, col(last))
        } else {
            cell_ref(PPE_END, col(j - 1))
        };
        s.formula(PPE_BEG, c, format!("={prev_end}"));
        s.formula(PPE_CAPEX, c, format!("=-CF!{}", cell_ref(CF_CAPEX, c)));
        s.formula(PPE_DA, c, format!("=IS!{}", cell_ref(isr.da, c)));
        s.number(PPE_OTHER, c, 0.0);
        s.formula(
            PPE_END,
            c,
            format!(
                "={}+{}-{}+{}",
                cell_ref(PPE_BEG, c),
                cell_ref(PPE_CAPEX, c),
                cell_ref(PPE_DA, c),
                cell_ref(PPE_OTHER, c)
            ),
        );
    }

    // ── Working Capital Schedule ──────────────────────────────────────────
    s.text(WC_HDR, LABEL, "Working Capital Schedule");
    s.text(WC_AR_DAYS, LABEL, "  AR Days");
    s.text(WC_AR, LABEL, "  Accounts Receivable");
    s.text(WC_INV_DAYS, LABEL, "  Inventory Days");
    s.text(WC_INV, LABEL, "  Inventory");
    s.text(WC_AP_DAYS, LABEL, "  AP Days");
    s.text(WC_AP, LABEL, "  Accounts Payable");
    s.text(WC_NET_CHG, LABEL, "  Net WC Change (CFO add-back)");

    for j in 0..n_h {
        let c = col(j);
        s.formula(
            WC_AR_DAYS,
            c,
            format!(
                "=IF(IS!{rev}<>0,{ar}/IS!{rev}*365,\"\")",
                rev = cell_ref(isr.revenue, c),
                ar = cell_ref(AR, c)
            ),
        );
        if let Some(v) = g(bs, "accounts_receivable", j) {
            s.number(WC_AR, c, v);
        }
        s.formula(
            WC_INV_DAYS,
            c,
            format!(
                "=IF(IS!{cogs}<>0,{inv}/IS!{cogs}*365,\"\")",
                cogs = cell_ref(isr.cogs, c),
                inv = cell_ref(INVENTORY, c)
            ),
        );
        if let Some(v) = g(bs, "inventory", j) {
            s.number(WC_INV, c, v);
        }
        s.formula(
            WC_AP_DAYS,
            c,
            format!(
                "=IF(IS!{cogs}<>0,{ap}/IS!{cogs}*365,\"\")",
                cogs = cell_ref(isr.cogs, c),
                ap = cell_ref(AP, c)
            ),
        );
        if let Some(v) = g(bs, "accounts_payable", j) {
            s.number(WC_AP, c, v);
        }
        // wc_net_chg historical: write_blank → skip.
    }
    for j in n_h..n {
        let c = col(j);
        let pj = (j - n_h) as u32;
        s.formula(
            WC_AR_DAYS,
            c,
            format!("=Assumptions!{}", cell_ref(ASMP_DSO, ASMP_DATA0 + pj)),
        );
        s.formula(
            WC_INV_DAYS,
            c,
            format!("=Assumptions!{}", cell_ref(ASMP_DIO, ASMP_DATA0 + pj)),
        );
        s.formula(
            WC_AP_DAYS,
            c,
            format!("=Assumptions!{}", cell_ref(ASMP_DPO, ASMP_DATA0 + pj)),
        );
        s.formula(
            WC_AR,
            c,
            format!(
                "=IF(IS!{rev}<>0,IS!{rev}*{d}/365,0)",
                rev = cell_ref(isr.revenue, c),
                d = cell_ref(WC_AR_DAYS, c)
            ),
        );
        s.formula(
            WC_INV,
            c,
            format!(
                "=IF(IS!{cogs}<>0,IS!{cogs}*{d}/365,0)",
                cogs = cell_ref(isr.cogs, c),
                d = cell_ref(WC_INV_DAYS, c)
            ),
        );
        s.formula(
            WC_AP,
            c,
            format!(
                "=IF(IS!{cogs}<>0,IS!{cogs}*{d}/365,0)",
                cogs = cell_ref(isr.cogs, c),
                d = cell_ref(WC_AP_DAYS, c)
            ),
        );
        let (prev_ar, prev_inv, prev_ap) = if j == n_h {
            (
                cell_ref(AR, col(last)),
                cell_ref(INVENTORY, col(last)),
                cell_ref(AP, col(last)),
            )
        } else {
            (
                cell_ref(WC_AR, col(j - 1)),
                cell_ref(WC_INV, col(j - 1)),
                cell_ref(WC_AP, col(j - 1)),
            )
        };
        s.formula(
            WC_NET_CHG,
            c,
            format!(
                "=-({}-{})-({}-{})+({}-{})",
                cell_ref(WC_AR, c),
                prev_ar,
                cell_ref(WC_INV, c),
                prev_inv,
                cell_ref(WC_AP, c),
                prev_ap
            ),
        );
    }

    // ── Debt Schedule ─────────────────────────────────────────────────────
    s.text(DEBT_HDR, LABEL, "Debt Schedule");
    s.text(DEBT_RATE, LABEL, "  Interest Rate %");
    s.text(DEBT_BEG, LABEL, "  Beginning LTD");
    s.text(DEBT_NEW, LABEL, "  + New Issuances");
    s.text(DEBT_REPAID, LABEL, "  \u{2212} Repayments");
    s.text(DEBT_END, LABEL, "Ending LTD");
    s.text(DEBT_INT, LABEL, "  Interest Expense (to IS)");

    for j in 0..n_h {
        let c = col(j);
        let end_c = cell_ref(DEBT_END, c);
        let ie_c = cell_ref(isr.int_exp, c);
        let rate_fmla = if j == 0 {
            format!("=IF({end_c}<>0,IS!{ie_c}/{end_c},\"\")")
        } else {
            let prev_end_c = cell_ref(DEBT_END, col(j - 1));
            format!(
                "=IF(AVERAGE({prev_end_c},{end_c})<>0,IS!{ie_c}/AVERAGE({prev_end_c},{end_c}),\"\")"
            )
        };
        s.formula(DEBT_RATE, c, rate_fmla);
        if j > 0 {
            if let Some(v) = g(bs, "long_term_debt", j - 1) {
                s.number(DEBT_BEG, c, v);
            }
        }
        s.number(DEBT_NEW, c, 0.0);
        s.number(DEBT_REPAID, c, 0.0);
        if let Some(v) = g(bs, "long_term_debt", j) {
            s.number(DEBT_END, c, v);
        }
        if let Some(v) = g(is, "interest_expense", j) {
            s.number(DEBT_INT, c, v);
        }
    }
    for j in n_h..n {
        let c = col(j);
        s.formula(DEBT_RATE, c, "=Assumptions!$D$22");
        let prev_end = if j == n_h {
            cell_ref(LTD, col(last))
        } else {
            cell_ref(DEBT_END, col(j - 1))
        };
        s.formula(DEBT_BEG, c, format!("={prev_end}"));
        s.number(DEBT_NEW, c, 0.0);
        s.number(DEBT_REPAID, c, 0.0);
        s.formula(
            DEBT_END,
            c,
            format!(
                "={}+{}-{}",
                cell_ref(DEBT_BEG, c),
                cell_ref(DEBT_NEW, c),
                cell_ref(DEBT_REPAID, c)
            ),
        );
        s.formula(
            DEBT_INT,
            c,
            format!(
                "=IF(IS!{circ},AVERAGE({beg},{end}),{beg})*{rate}",
                circ = cell_ref(isr.circ, ASMP_DATA0),
                beg = cell_ref(DEBT_BEG, c),
                end = cell_ref(DEBT_END, c),
                rate = cell_ref(DEBT_RATE, c)
            ),
        );
    }

    // ── Retained Earnings Rollforward ─────────────────────────────────────
    s.text(RE_HDR, LABEL, "Retained Earnings Rollforward");
    s.text(RE_BEG, LABEL, "  Beginning Retained Earnings");
    s.text(RE_NI, LABEL, "  + Net Income to Common");
    s.text(RE_DIV, LABEL, "  \u{2212} Dividends Paid");
    s.text(RE_BB, LABEL, "  \u{2212} Share Buybacks");
    s.text(RE_END, LABEL, "Ending Retained Earnings");

    for j in 0..n {
        let c = col(j);
        s.formula(RE_NI, c, format!("=IS!{}", cell_ref(isr.ni_common, c)));
        s.formula(RE_DIV, c, format!("=CF!{}", cell_ref(CF_DIVIDENDS, c)));
        s.formula(RE_BB, c, format!("=CF!{}", cell_ref(CF_BUYBACKS, c)));
    }
    for j in 0..n_h {
        if j > 0 {
            if let Some(v) = g(bs, "retained_earnings", j - 1) {
                s.number(RE_BEG, col(j), v);
            }
        }
        if let Some(v) = g(bs, "retained_earnings", j) {
            s.number(RE_END, col(j), v);
        }
    }
    for j in n_h..n {
        let c = col(j);
        let prev_end = if j == n_h {
            cell_ref(RET_EARN, col(last))
        } else {
            cell_ref(RE_END, col(j - 1))
        };
        s.formula(RE_BEG, c, format!("={prev_end}"));
        s.formula(
            RE_END,
            c,
            format!(
                "={}+{}-{}-{}",
                cell_ref(RE_BEG, c),
                cell_ref(RE_NI, c),
                cell_ref(RE_DIV, c),
                cell_ref(RE_BB, c)
            ),
        );
    }
}
