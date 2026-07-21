//! DCF tab — port of `writer.py::_write_dcf` (core body + inline sensitivities).

use crate::input::WorkbookInput;
use crate::model::{
    BLUE, DATA0, FMT_MULT, FMT_NUM, FMT_PCT, LABEL, LIGHT_BLUE, Sheet, cell_ref, col_name, fmt_per_share,
};
use crate::sheets::wacc::rows as wr;

// DCF_R (0-based)
const TITLE: u32 = 2;
const SUBTITLE: u32 = 4;
const UNITS: u32 = 5;
const WACC_HDR: u32 = 8;
const BETA: u32 = 10;
const RF: u32 = 11;
const ERP: u32 = 12;
const KE: u32 = 13;
const KD_PRE: u32 = 15;
const TAX_SHIELD: u32 = 16;
const KD: u32 = 17;
const EQ_WT: u32 = 19;
const D_WT: u32 = 20;
const WACC: u32 = 21;
const FCF_HDR: u32 = 24;
const FCF_HEADERS: u32 = 25;
const FCF_EBIT: u32 = 26;
const FCF_NOPAT: u32 = 27;
const FCF_DA: u32 = 28;
const FCF_CAPEX: u32 = 29;
const FCF_DWC: u32 = 30;
const FCF_FCFF: u32 = 31;
const FCF_T: u32 = 32;
const FCF_FACTOR: u32 = 33;
const FCF_PV: u32 = 34;
const PV_FCFS: u32 = 37;
const TV_HDR: u32 = 40;
const TV_METHOD: u32 = 41;
const TV1_LBL: u32 = 42;
const TV1_MULT: u32 = 43;
const TV1_EBITDA: u32 = 44;
const TV1_TV: u32 = 45;
const TV2_LBL: u32 = 46;
const TV2_G: u32 = 47;
const TV2_FCF: u32 = 48;
const TV2_TV: u32 = 49;
const TV_SELECTED: u32 = 51;
const TV_PV: u32 = 52;
const EV_HDR: u32 = 54;
const EV_PVFCFS: u32 = 55;
const EV_PVTV: u32 = 56;
const EV_TOTAL: u32 = 57;
const EV_DEBT: u32 = 59;
const EV_CASH: u32 = 60;
const EV_NET_DEBT: u32 = 61;
const EV_EQUITY: u32 = 62;
const EV_SHARES: u32 = 63;
const EV_PRICE: u32 = 64;
const SENS_HDR: u32 = 67;
const SENS1_LBL: u32 = 68;
const SENS1_COL_HDR: u32 = 69;
const SENS2_LBL: u32 = 76;
const SENS2_COL_HDR: u32 = 77;
const XC_HDR: u32 = 84;
const XC_TV_PCT: u32 = 85;
const XC_WACC_G: u32 = 86;
const XC_IMP_MULT: u32 = 87;
const XC_IMP_G: u32 = 88;
const XC_UPSIDE: u32 = 89;
const XC_CURRENT: u32 = 90;

// BS / CF fixed rows
const BS_AR: u32 = 12;
const BS_INV: u32 = 13;
const BS_AP: u32 = 21;
const BS_LTD: u32 = 24;
const BS_CASH: u32 = 11;
const CF_CAPEX: u32 = 22;

pub fn build(input: &WorkbookInput) -> Sheet {
    let dcf = input
        .dcf
        .as_ref()
        .expect("dcf sheet requires WorkbookInput.dcf");
    let m = &input.meta;
    let n_proj = dcf.proj_periods.len();
    let n_h = input.model.n_hist();
    let vc = DATA0;
    let mut s = Sheet::new("DCF");

    s.title(TITLE, format!("{} — DCF Valuation", m.company));
    s.text(SUBTITLE, LABEL, "Discounted Cash Flow Analysis");
    s.text(
        UNITS,
        LABEL,
        format!("({} in millions, unless noted)", m.currency),
    );

    // ── WACC build-up (links to WACC tab) ───────────────────────────────────
    s.section(WACC_HDR, "WACC BUILD-UP");
    s.text(BETA, LABEL, "  Beta (3–5Y)");
    s.formula_cached(
        BETA,
        vc,
        format!("=WACC!{}", cell_ref(wr::BE_TARGET, DATA0)),
        dcf.beta,
    );
    s.stamp_row(BETA, FMT_NUM);
    s.text(RF, LABEL, "  Risk-Free Rate (10Y Treasury)");
    s.formula_cached(
        RF,
        vc,
        format!("=WACC!{}", cell_ref(wr::RF, DATA0)),
        dcf.risk_free_rate,
    );
    s.stamp_row(RF, FMT_PCT);
    s.text(ERP, LABEL, "  Equity Risk Premium");
    s.formula_cached(
        ERP,
        vc,
        format!("=WACC!{}", cell_ref(wr::ERP, DATA0)),
        dcf.equity_risk_premium,
    );
    s.stamp_row(ERP, FMT_PCT);

    let beta_c = cell_ref(BETA, vc);
    let rf_c = cell_ref(RF, vc);
    let erp_c = cell_ref(ERP, vc);
    s.text(KE, LABEL, "  Cost of Equity  (CAPM = rf + β × ERP)");
    s.formula_cached(
        KE,
        vc,
        format!("={rf_c}+{beta_c}*{erp_c}"),
        dcf.cost_of_equity,
    );
    s.stamp_row(KE, FMT_PCT);

    s.text(KD_PRE, LABEL, "  Pre-Tax Cost of Debt");
    s.formula_cached(
        KD_PRE,
        vc,
        format!("=WACC!{}", cell_ref(wr::KD_PRE, DATA0)),
        dcf.cost_of_debt_pretax,
    );
    s.stamp_row(KD_PRE, FMT_PCT);
    s.text(TAX_SHIELD, LABEL, "  Effective Tax Rate");
    s.formula_cached(
        TAX_SHIELD,
        vc,
        format!("=WACC!{}", cell_ref(wr::TAX, DATA0)),
        dcf.tax_rate,
    );
    s.stamp_row(TAX_SHIELD, FMT_PCT);
    let kd_pre_c = cell_ref(KD_PRE, vc);
    let tax_c = cell_ref(TAX_SHIELD, vc);
    s.text(KD, LABEL, "  After-Tax Cost of Debt  [kd × (1 − t)]");
    s.formula_cached(
        KD,
        vc,
        format!("={kd_pre_c}*(1-{tax_c})"),
        dcf.after_tax_cost_of_debt,
    );
    s.stamp_row(KD, FMT_PCT);

    s.text(EQ_WT, LABEL, "  Equity Weight  (% of Total Capital)");
    s.formula_cached(
        EQ_WT,
        vc,
        format!("=WACC!{}", cell_ref(wr::WE, DATA0)),
        dcf.equity_weight,
    );
    s.stamp_row(EQ_WT, FMT_PCT);
    s.text(D_WT, LABEL, "  Debt Weight  (% of Total Capital)");
    s.formula_cached(
        D_WT,
        vc,
        format!("=WACC!{}", cell_ref(wr::WD, DATA0)),
        dcf.debt_weight,
    );
    s.stamp_row(D_WT, FMT_PCT);
    s.text(WACC, LABEL, "  WACC  (single source of truth: WACC tab)");
    s.formula_cached(
        WACC,
        vc,
        format!("=WACC!{}", cell_ref(wr::WACC, DATA0)),
        dcf.wacc,
    );
    s.stamp_row(WACC, FMT_PCT);
    let wacc_c = cell_ref(WACC, vc);

    // ── FCF projection ──────────────────────────────────────────────────────
    s.section(FCF_HDR, "FREE CASH FLOW PROJECTION  (Unlevered FCFF)");
    for (i, period) in dcf.proj_periods.iter().enumerate() {
        s.text(FCF_HEADERS, vc + i as u32, period.clone());
    }

    let is_ebit = input.is_row("ebit");
    let is_da = input.is_row("da");
    let is_ebitda = input.is_row("ebitda");
    let is_shares = input.is_row("shares_diluted");

    s.text(FCF_EBIT, LABEL, "  EBIT");
    for i in 0..n_proj {
        s.formula(
            FCF_EBIT,
            vc + i as u32,
            format!("=IS!{}", cell_ref(is_ebit, DATA0 + n_h as u32 + i as u32)),
        );
    }
    s.stamp_row(FCF_EBIT, FMT_NUM);

    s.text(FCF_NOPAT, LABEL, "  NOPAT  [EBIT × (1 − t)]");
    for i in 0..n_proj {
        let ebit_ci = cell_ref(FCF_EBIT, vc + i as u32);
        s.formula(FCF_NOPAT, vc + i as u32, format!("={ebit_ci}*(1-{tax_c})"));
    }
    s.stamp_row(FCF_NOPAT, FMT_NUM);

    s.text(FCF_DA, LABEL, "  Plus: D&A");
    for i in 0..n_proj {
        s.formula(
            FCF_DA,
            vc + i as u32,
            format!("=IS!{}", cell_ref(is_da, DATA0 + n_h as u32 + i as u32)),
        );
    }
    s.stamp_row(FCF_DA, FMT_NUM);

    s.text(FCF_CAPEX, LABEL, "  Less: Capital Expenditures");
    for i in 0..n_proj {
        s.formula(
            FCF_CAPEX,
            vc + i as u32,
            format!("=CF!{}", cell_ref(CF_CAPEX, DATA0 + n_h as u32 + i as u32)),
        );
    }
    s.stamp_row(FCF_CAPEX, FMT_NUM);

    s.text(FCF_DWC, LABEL, "  Less: Change in Net Working Capital");
    for i in 0..n_proj {
        let t = n_h + i;
        let t_prv = t - 1;
        let ar_t = format!("BS!{}", cell_ref(BS_AR, DATA0 + t as u32));
        let inv_t = format!("BS!{}", cell_ref(BS_INV, DATA0 + t as u32));
        let ap_t = format!("BS!{}", cell_ref(BS_AP, DATA0 + t as u32));
        let ar_p = format!("BS!{}", cell_ref(BS_AR, DATA0 + t_prv as u32));
        let inv_p = format!("BS!{}", cell_ref(BS_INV, DATA0 + t_prv as u32));
        let ap_p = format!("BS!{}", cell_ref(BS_AP, DATA0 + t_prv as u32));
        s.formula(
            FCF_DWC,
            vc + i as u32,
            format!("=({ar_t}+{inv_t}-{ap_t})-({ar_p}+{inv_p}-{ap_p})"),
        );
    }
    s.stamp_row(FCF_DWC, FMT_NUM);

    s.text(FCF_FCFF, LABEL, "  Unlevered Free Cash Flow  (FCFF)");
    for i in 0..n_proj {
        let nop_c = cell_ref(FCF_NOPAT, vc + i as u32);
        let da_ci = cell_ref(FCF_DA, vc + i as u32);
        let cap_c = cell_ref(FCF_CAPEX, vc + i as u32);
        let dwc_c = cell_ref(FCF_DWC, vc + i as u32);
        // CapEx on CF is stored as negative outflow → +cap_c subtracts.
        let cache = dcf.fcff_proj.get(i).copied().unwrap_or(0.0);
        s.formula_cached(
            FCF_FCFF,
            vc + i as u32,
            format!("={nop_c}+{da_ci}+{cap_c}-{dwc_c}"),
            cache,
        );
    }
    s.stamp_row(FCF_FCFF, FMT_NUM);

    let offset = if dcf.mid_year_convention { -0.5 } else { 0.0 };
    let period_label = if dcf.mid_year_convention {
        "  Discount Period (t − 0.5, mid-year)"
    } else {
        "  Discount Period (t, year-end)"
    };
    s.text(FCF_T, LABEL, period_label);
    for i in 0..n_proj {
        s.number(FCF_T, vc + i as u32, (i as f64 + 1.0) + offset);
    }
    s.stamp_row(FCF_T, FMT_NUM);

    s.text(FCF_FACTOR, LABEL, "  Discount Factor  [1 ÷ (1 + WACC)^t]");
    for i in 0..n_proj {
        let t_ci = cell_ref(FCF_T, vc + i as u32);
        let cache = dcf.discount_factors.get(i).copied().unwrap_or(1.0);
        s.formula_cached(
            FCF_FACTOR,
            vc + i as u32,
            format!("=1/(1+{wacc_c})^{t_ci}"),
            cache,
        );
    }
    s.stamp_row(FCF_FACTOR, FMT_NUM);

    s.text(FCF_PV, LABEL, "  PV of FCF");
    for i in 0..n_proj {
        let fcff_ci = cell_ref(FCF_FCFF, vc + i as u32);
        let fac_ci = cell_ref(FCF_FACTOR, vc + i as u32);
        let cache = dcf.pv_fcfs_per_period.get(i).copied().unwrap_or(0.0);
        s.formula_cached(FCF_PV, vc + i as u32, format!("={fcff_ci}*{fac_ci}"), cache);
    }
    s.stamp_row(FCF_PV, FMT_NUM);

    let pv_first = cell_ref(FCF_PV, vc);
    let pv_last = cell_ref(FCF_PV, vc + n_proj as u32 - 1);
    s.text(PV_FCFS, LABEL, "Sum of PV(FCFs)");
    s.formula_cached(
        PV_FCFS,
        vc,
        format!("=SUM({pv_first}:{pv_last})"),
        dcf.pv_fcfs,
    );
    s.stamp_row(PV_FCFS, FMT_NUM);
    let pv_fcfs_c = cell_ref(PV_FCFS, vc);

    // ── Terminal value ──────────────────────────────────────────────────────
    s.section(TV_HDR, "TERMINAL VALUE");
    s.text(
        TV_METHOD,
        LABEL,
        "  TV Method  (1 = EBITDA Multiple  |  2 = Gordon Growth)",
    );
    s.number(TV_METHOD, vc, dcf.tv_method as f64);
    let tv_meth_c = cell_ref(TV_METHOD, vc);

    s.section(TV1_LBL, "  Method 1 — Exit EBITDA Multiple");
    s.text(TV1_MULT, LABEL, "    Exit EBITDA Multiple (×)");
    s.number(TV1_MULT, vc, dcf.tv_ebitda_multiple);
    s.stamp_row(TV1_MULT, FMT_MULT);
    let tv_mult_c = cell_ref(TV1_MULT, vc);

    let last_proj_is_col = DATA0 + n_h as u32 + n_proj as u32 - 1;
    s.text(TV1_EBITDA, LABEL, "    Terminal Year EBITDA");
    s.formula_cached(
        TV1_EBITDA,
        vc,
        format!("=IS!{}", cell_ref(is_ebitda, last_proj_is_col)),
        dcf.terminal_ebitda,
    );
    s.stamp_row(TV1_EBITDA, FMT_NUM);
    let tv_ebitda_c = cell_ref(TV1_EBITDA, vc);

    s.text(TV1_TV, LABEL, "    Terminal Value (EBITDA Multiple)");
    s.formula_cached(
        TV1_TV,
        vc,
        format!("={tv_ebitda_c}*{tv_mult_c}"),
        dcf.tv_ebitda,
    );
    s.stamp_row(TV1_TV, FMT_NUM);
    let tv1_c = cell_ref(TV1_TV, vc);

    s.section(TV2_LBL, "  Method 2 — Gordon Growth Model");
    s.text(TV2_G, LABEL, "    Long-Term Growth Rate");
    s.number(TV2_G, vc, dcf.tv_growth_rate);
    s.stamp_row(TV2_G, FMT_PCT);
    let tv_g_c = cell_ref(TV2_G, vc);

    let last_fcff_c = cell_ref(FCF_FCFF, vc + n_proj as u32 - 1);
    s.text(TV2_FCF, LABEL, "    Terminal Year FCF");
    s.formula_cached(
        TV2_FCF,
        vc,
        format!("={last_fcff_c}"),
        dcf.fcff_proj.last().copied().unwrap_or(0.0),
    );
    s.stamp_row(TV2_FCF, FMT_NUM);

    s.text(TV2_TV, LABEL, "    Terminal Value (Gordon Growth)");
    s.formula_cached(
        TV2_TV,
        vc,
        format!("=IF({wacc_c}>{tv_g_c},{last_fcff_c}*(1+{tv_g_c})/({wacc_c}-{tv_g_c}),0)"),
        dcf.tv_gordon,
    );
    s.stamp_row(TV2_TV, FMT_NUM);
    let tv2_c = cell_ref(TV2_TV, vc);

    s.text(TV_SELECTED, LABEL, "  Selected Terminal Value");
    s.formula_cached(
        TV_SELECTED,
        vc,
        format!("=CHOOSE({tv_meth_c},{tv1_c},{tv2_c})"),
        dcf.tv_selected,
    );
    s.stamp_row(TV_SELECTED, FMT_NUM);
    let tv_sel_c = cell_ref(TV_SELECTED, vc);

    s.text(TV_PV, LABEL, "  PV of Terminal Value");
    // Writer uses n_proj (not mid-year adjusted) for TV PV exponent in the sheet formula.
    s.formula_cached(
        TV_PV,
        vc,
        format!("={tv_sel_c}/(1+{wacc_c})^{n_proj}"),
        dcf.pv_tv,
    );
    s.stamp_row(TV_PV, FMT_NUM);
    let pv_tv_c = cell_ref(TV_PV, vc);

    // ── EV bridge ───────────────────────────────────────────────────────────
    s.section(EV_HDR, "ENTERPRISE VALUE BRIDGE");
    s.text(EV_PVFCFS, LABEL, "  PV of Free Cash Flows");
    s.formula_cached(EV_PVFCFS, vc, format!("={pv_fcfs_c}"), dcf.pv_fcfs);
    s.stamp_row(EV_PVFCFS, FMT_NUM);
    let ev_pvf_c = cell_ref(EV_PVFCFS, vc);

    s.text(EV_PVTV, LABEL, "  PV of Terminal Value");
    s.formula_cached(EV_PVTV, vc, format!("={pv_tv_c}"), dcf.pv_tv);
    s.stamp_row(EV_PVTV, FMT_NUM);
    let ev_pvt_c = cell_ref(EV_PVTV, vc);

    s.text(EV_TOTAL, LABEL, "  Total Enterprise Value");
    s.formula_cached(
        EV_TOTAL,
        vc,
        format!("={ev_pvf_c}+{ev_pvt_c}"),
        dcf.enterprise_value,
    );
    s.stamp_row(EV_TOTAL, FMT_NUM);
    let ev_c = cell_ref(EV_TOTAL, vc);

    let last_proj_bs_col = DATA0 + n_h as u32 + n_proj as u32 - 1;
    s.text(EV_DEBT, LABEL, "  Less: Total Debt");
    s.formula_cached(
        EV_DEBT,
        vc,
        format!("=BS!{}", cell_ref(BS_LTD, last_proj_bs_col)),
        dcf.total_debt,
    );
    s.stamp_row(EV_DEBT, FMT_NUM);
    let debt_c = cell_ref(EV_DEBT, vc);

    s.text(EV_CASH, LABEL, "  Plus: Cash & Equivalents");
    s.formula_cached(
        EV_CASH,
        vc,
        format!("=BS!{}", cell_ref(BS_CASH, last_proj_bs_col)),
        dcf.cash,
    );
    s.stamp_row(EV_CASH, FMT_NUM);
    let cash_c = cell_ref(EV_CASH, vc);

    s.text(EV_NET_DEBT, LABEL, "  Net Debt  (Debt − Cash)");
    // Writer net_debt display is debt-cash (bridge); engine net_debt may include pref/nci.
    s.formula_cached(
        EV_NET_DEBT,
        vc,
        format!("={debt_c}-{cash_c}"),
        dcf.total_debt - dcf.cash,
    );
    s.stamp_row(EV_NET_DEBT, FMT_NUM);
    let nd_c = cell_ref(EV_NET_DEBT, vc);

    s.text(EV_EQUITY, LABEL, "  Equity Value");
    s.formula_cached(EV_EQUITY, vc, format!("={ev_c}-{nd_c}"), dcf.equity_value);
    s.stamp_row(EV_EQUITY, FMT_NUM);
    let eq_val_c = cell_ref(EV_EQUITY, vc);

    s.text(EV_SHARES, LABEL, "  Diluted Shares Outstanding (M)");
    s.formula_cached(
        EV_SHARES,
        vc,
        format!("=IS!{}", cell_ref(is_shares, last_proj_is_col)),
        dcf.shares_diluted,
    );
    s.stamp_row(EV_SHARES, FMT_NUM);
    let sh_c = cell_ref(EV_SHARES, vc);

    s.text(EV_PRICE, LABEL, "  Implied Share Price");
    s.formula_cached(
        EV_PRICE,
        vc,
        format!("=IF({sh_c}<>0,{eq_val_c}/{sh_c},0)"),
        dcf.implied_price,
    );
    s.stamp_row(EV_PRICE, fmt_per_share(&m.currency));

    // ── Sensitivity (inline) ────────────────────────────────────────────────
    s.section(SENS_HDR, "SENSITIVITY ANALYSIS  (Implied Share Price)");
    let vc0 = col_name(vc);
    let lc = col_name(LABEL);
    let fcff_er = FCF_FCFF + 1;
    let tv1e_er = TV1_EBITDA + 1;
    let debt_er = EV_DEBT + 1;
    let cash_er = EV_CASH + 1;
    let shrs_er = EV_SHARES + 1;
    let exps: Vec<f64> = (0..n_proj).map(|k| (k as f64 + 1.0) + offset).collect();

    let ufcf_sum = |wacc_ref: &str| -> String {
        (0..n_proj)
            .map(|k| {
                format!(
                    "${}${fcff_er}/(1+{wacc_ref})^{exp}",
                    col_name(vc + k as u32),
                    exp = exps[k]
                )
            })
            .collect::<Vec<_>>()
            .join(" + ")
    };

    s.text(SENS1_LBL, LABEL, "WACC  ↓  /  Exit Multiple  →");
    for (j, mult) in dcf.ebitda_multiple_range.iter().enumerate() {
        s.number(SENS1_COL_HDR, vc + j as u32, *mult);
        if let Some(c) = s.cells.get_mut(&(SENS1_COL_HDR, vc + j as u32)) {
            c.num_fmt = Some(FMT_MULT);
        }
    }
    let mult_hdr_er = SENS1_COL_HDR + 1;
    let n_wacc = dcf.wacc_range.len();
    let mid = n_wacc / 2;
    for (i, w) in dcf.wacc_range.iter().enumerate() {
        let r = SENS1_COL_HDR + 1 + i as u32;
        let r_excel = r + 1;
        s.number(r, LABEL, *w);
        if let Some(c) = s.cells.get_mut(&(r, LABEL)) {
            c.num_fmt = Some(FMT_PCT);
        }
        let wacc_ref = format!("${lc}${r_excel}");
        for j in 0..dcf.ebitda_multiple_range.len() {
            let col = vc + j as u32;
            let mult_ref = format!("{}${}", col_name(col), mult_hdr_er);
            let tv_pv = format!("${vc0}${tv1e_er}*{mult_ref}/(1+{wacc_ref})^{n_proj}");
            let bridge = format!("-${vc0}${debt_er}+${vc0}${cash_er}");
            let shares = format!("${vc0}${shrs_er}");
            let formula = format!(
                "=IF({shares}<>0,({sum}+{tv_pv}{bridge})/{shares},0)",
                sum = ufcf_sum(&wacc_ref)
            );
            let cache = dcf
                .sensitivity_ebitda
                .get(i)
                .and_then(|row| row.get(j))
                .copied()
                .unwrap_or(0.0);
            s.formula_cached(r, col, formula, cache);
            if i == mid {
                let fill = if j == dcf.ebitda_multiple_range.len() / 2 {
                    BLUE
                } else {
                    LIGHT_BLUE
                };
                s.fill(r, col, fill);
            }
        }
        s.stamp_row(r, fmt_per_share(&m.currency));
    }

    s.text(SENS2_LBL, LABEL, "WACC  ↓  /  Terminal Growth Rate  →");
    for (j, g) in dcf.gordon_growth_range.iter().enumerate() {
        s.number(SENS2_COL_HDR, vc + j as u32, *g);
        if let Some(c) = s.cells.get_mut(&(SENS2_COL_HDR, vc + j as u32)) {
            c.num_fmt = Some(FMT_PCT);
        }
    }
    let g_hdr_er = SENS2_COL_HDR + 1;
    for (i, w) in dcf.wacc_range.iter().enumerate() {
        let r = SENS2_COL_HDR + 1 + i as u32;
        let r_excel = r + 1;
        s.number(r, LABEL, *w);
        if let Some(c) = s.cells.get_mut(&(r, LABEL)) {
            c.num_fmt = Some(FMT_PCT);
        }
        let wacc_ref = format!("${lc}${r_excel}");
        for j in 0..dcf.gordon_growth_range.len() {
            let col = vc + j as u32;
            let g_ref = format!("{}${}", col_name(col), g_hdr_er);
            let last_ufcf_col = col_name(vc + n_proj as u32 - 1);
            let tv_pv = format!(
                "IF({wacc_ref}>{g_ref},${last_ufcf_col}${fcff_er}*(1+{g_ref})/(({wacc_ref}-{g_ref})*(1+{wacc_ref})^{n_proj}),0)"
            );
            let bridge = format!("-${vc0}${debt_er}+${vc0}${cash_er}");
            let shares = format!("${vc0}${shrs_er}");
            let formula = format!(
                "=IF({shares}<>0,({sum}+{tv_pv}{bridge})/{shares},0)",
                sum = ufcf_sum(&wacc_ref)
            );
            let cache = dcf
                .sensitivity_gordon
                .get(i)
                .and_then(|row| row.get(j))
                .copied()
                .unwrap_or(0.0);
            s.formula_cached(r, col, formula, cache);
            if i == mid {
                let fill = if j == dcf.gordon_growth_range.len() / 2 {
                    BLUE
                } else {
                    LIGHT_BLUE
                };
                s.fill(r, col, fill);
            }
        }
        s.stamp_row(r, fmt_per_share(&m.currency));
    }

    // Cross-checks (hardcoded numbers from engine, matching Python)
    s.section(XC_HDR, "CROSS-CHECKS");
    s.text(XC_TV_PCT, LABEL, "  TV / EV %  (target: 60-80%)");
    s.number(XC_TV_PCT, vc, dcf.tv_pct_of_ev);
    s.stamp_row(XC_TV_PCT, FMT_PCT);
    s.text(XC_WACC_G, LABEL, "  WACC − Terminal g  (target: > 2%)");
    s.number(XC_WACC_G, vc, dcf.wacc_minus_g);
    s.stamp_row(XC_WACC_G, FMT_PCT);
    s.text(
        XC_IMP_MULT,
        LABEL,
        "  Implied Exit Multiple  (Gordon TV ÷ Terminal EBITDA)",
    );
    s.number(XC_IMP_MULT, vc, dcf.implied_exit_mult_from_gordon);
    s.stamp_row(XC_IMP_MULT, FMT_NUM);
    s.text(
        XC_IMP_G,
        LABEL,
        "  Implied Perpetuity g  (from Exit Multiple)",
    );
    s.number(XC_IMP_G, vc, dcf.implied_g_from_exit_mult);
    s.stamp_row(XC_IMP_G, FMT_PCT);
    s.text(XC_CURRENT, LABEL, "  Current Share Price");
    s.number(XC_CURRENT, vc, dcf.current_share_price);
    s.stamp_row(XC_CURRENT, fmt_per_share(&m.currency));
    s.text(XC_UPSIDE, LABEL, "  Implied Upside / (Downside) vs Current");
    s.number(XC_UPSIDE, vc, dcf.upside_downside_pct);
    s.stamp_row(XC_UPSIDE, FMT_PCT);

    s
}

pub mod rows {
    pub const EV_TOTAL: u32 = super::EV_TOTAL;
    pub const EV_EQUITY: u32 = super::EV_EQUITY;
    pub const EV_PRICE: u32 = super::EV_PRICE;
    pub const EV_DEBT: u32 = super::EV_DEBT;
    pub const EV_CASH: u32 = super::EV_CASH;
    pub const EV_SHARES: u32 = super::EV_SHARES;
    pub const FCF_FCFF: u32 = super::FCF_FCFF;
    pub const TV1_EBITDA: u32 = super::TV1_EBITDA;
}
