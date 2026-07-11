//! WACC tab — port of `writer.py::_write_wacc`.

use crate::input::WorkbookInput;
use crate::model::{cell_ref, Sheet, BLUE, FMT_NUM, FMT_PCT, DATA0, LABEL};
use fm_value::unlever_beta;

// Row map (0-based) — WACC_R
const TITLE: u32 = 2;
const SUBTITLE: u32 = 4;
const UNITS: u32 = 5;
const PEER_HDR: u32 = 8;
const PEER_COLS: u32 = 9;
const PEER_START: u32 = 10;
const PEER_MEDIAN: u32 = 21;
const CAPM_HDR: u32 = 24;
const RF: u32 = 25;
const ERP: u32 = 26;
const BE_TARGET: u32 = 27;
const KE: u32 = 28;
const DE_RESTATE: u32 = 29;
const KD_HDR: u32 = 30;
const KD_PRE: u32 = 31;
const TAX: u32 = 32;
const KD_AFTER: u32 = 33;
const CAP_HDR: u32 = 35;
const MKT_CAP: u32 = 36;
const DEBT: u32 = 37;
const TOTAL_CAP: u32 = 38;
const WE: u32 = 39;
const WD: u32 = 40;
const WACC: u32 = 42;

// Assumptions shared rows (0-based)
const ASSUMP_SHARED0: u32 = 85;

pub fn build(input: &WorkbookInput) -> Sheet {
    let w = input.wacc.as_ref().expect("wacc sheet requires WorkbookInput.wacc");
    let m = &input.meta;
    let mut s = Sheet::new("WACC");

    s.title(TITLE, format!("{} — WACC Build-Up", m.company));
    s.text(SUBTITLE, LABEL, "Peer-Set Beta Unlever / Relever + CAPM");
    s.text(UNITS, LABEL, format!("(peer source: {})", input.peer_source));

    // Peer set
    s.section(PEER_HDR, "PEER SET");
    let headers = ["Ticker", "Levered β", "D/E", "Tax", "Unlevered β", "Mkt Cap ($M)"];
    for (i, lbl) in headers.iter().enumerate() {
        let c = if i == 0 { LABEL } else { DATA0 + (i as u32) - 1 };
        s.text(PEER_COLS, c, *lbl);
    }

    for (j, p) in w.peers.iter().take(10).enumerate() {
        let r = PEER_START + j as u32;
        s.text(r, LABEL, p.ticker.clone());
        s.number(r, DATA0, p.levered_beta);
        s.number(r, DATA0 + 1, p.de_ratio);
        s.stamp_row(r, FMT_PCT); // will stamp de/tax later carefully
        // reset: write numbers then stamp specific cols via formula path
        s.number(r, DATA0 + 2, p.tax_rate);
        let bu = unlever_beta(p.levered_beta, p.de_ratio, p.tax_rate);
        let bl_c = cell_ref(r, DATA0);
        let de_p = cell_ref(r, DATA0 + 1);
        let t_p = cell_ref(r, DATA0 + 2);
        s.formula(r, DATA0 + 3, format!("={bl_c}/(1+(1-{t_p})*{de_p})"));
        // hardcode mkt cap (no Comps Peers tab yet)
        s.number(r, DATA0 + 4, p.market_cap);
        // formats
        if let Some(c) = s.cells.get_mut(&(r, DATA0 + 1)) {
            c.num_fmt = Some(FMT_PCT);
        }
        if let Some(c) = s.cells.get_mut(&(r, DATA0 + 2)) {
            c.num_fmt = Some(FMT_PCT);
        }
        if let Some(c) = s.cells.get_mut(&(r, DATA0 + 3)) {
            c.num_fmt = Some(FMT_NUM);
        }
        if let Some(c) = s.cells.get_mut(&(r, DATA0 + 4)) {
            c.num_fmt = Some(FMT_NUM);
        }
        let _ = bu;
    }

    s.text(PEER_MEDIAN, LABEL, "Median Unlevered β");
    let bu_col = crate::model::col_name(DATA0 + 3);
    let ps_er = PEER_START + 1;
    let pe_er = PEER_START + 10;
    s.formula(
        PEER_MEDIAN,
        DATA0 + 3,
        format!("=MEDIAN({bu_col}{ps_er}:{bu_col}{pe_er})"),
    );
    if let Some(c) = s.cells.get_mut(&(PEER_MEDIAN, DATA0 + 3)) {
        c.num_fmt = Some(FMT_NUM);
    }

    // CAPM
    s.section(CAPM_HDR, "CAPM COST OF EQUITY");
    s.text(RF, LABEL, "  Risk-Free Rate (10Y Treasury)");
    s.formula(RF, DATA0, format!("=Assumptions!{}", cell_ref(ASSUMP_SHARED0, DATA0)));
    s.stamp_row(RF, FMT_PCT);
    s.text(ERP, LABEL, "  Equity Risk Premium");
    s.formula(ERP, DATA0, format!("=Assumptions!{}", cell_ref(ASSUMP_SHARED0 + 1, DATA0)));
    s.stamp_row(ERP, FMT_PCT);

    s.text(DE_RESTATE, LABEL, "  Target D/E Ratio");
    s.formula(DE_RESTATE, DATA0, format!("=Assumptions!{}", cell_ref(ASSUMP_SHARED0 + 2, DATA0)));
    s.stamp_row(DE_RESTATE, FMT_PCT);

    s.text(BE_TARGET, LABEL, "  Target Levered β  (re-levered to target D/E)");
    let median_c = cell_ref(PEER_MEDIAN, DATA0 + 3);
    let de_c = cell_ref(DE_RESTATE, DATA0);
    let tax_c = cell_ref(TAX, DATA0);
    s.formula(BE_TARGET, DATA0, format!("={median_c}*(1+(1-{tax_c})*{de_c})"));
    s.stamp_row(BE_TARGET, FMT_NUM);

    s.text(KE, LABEL, "  Cost of Equity  (Ke = Rf + β × ERP)");
    let rf_c = cell_ref(RF, DATA0);
    let erp_c = cell_ref(ERP, DATA0);
    let be_c = cell_ref(BE_TARGET, DATA0);
    s.formula(KE, DATA0, format!("={rf_c}+{be_c}*{erp_c}"));
    s.stamp_row(KE, FMT_PCT);

    // Cost of debt
    s.section(KD_HDR, "COST OF DEBT");
    s.text(KD_PRE, LABEL, "  Pre-Tax Cost of Debt");
    s.formula(KD_PRE, DATA0, format!("=Assumptions!{}", cell_ref(ASSUMP_SHARED0 + 3, DATA0)));
    s.stamp_row(KD_PRE, FMT_PCT);
    s.text(TAX, LABEL, "  Effective Tax Rate");
    s.number(TAX, DATA0, w.tax_rate);
    s.stamp_row(TAX, FMT_PCT);
    s.text(KD_AFTER, LABEL, "  After-Tax Cost of Debt  [Kd × (1 − t)]");
    let kd_c = cell_ref(KD_PRE, DATA0);
    s.formula(KD_AFTER, DATA0, format!("={kd_c}*(1-{tax_c})"));
    s.stamp_row(KD_AFTER, FMT_PCT);

    // Capital structure
    s.section(CAP_HDR, "CAPITAL STRUCTURE WEIGHTS");
    s.text(MKT_CAP, LABEL, "  Target Market Cap ($M)");
    s.number(MKT_CAP, DATA0, w.target_market_cap);
    s.stamp_row(MKT_CAP, FMT_NUM);
    s.text(DEBT, LABEL, "  Total Debt ($M)");
    s.number(DEBT, DATA0, w.target_debt);
    s.stamp_row(DEBT, FMT_NUM);
    s.text(TOTAL_CAP, LABEL, "  Total Capital  (Equity + Debt)");
    let mc_c = cell_ref(MKT_CAP, DATA0);
    let d_c = cell_ref(DEBT, DATA0);
    s.formula(TOTAL_CAP, DATA0, format!("={mc_c}+{d_c}"));
    s.stamp_row(TOTAL_CAP, FMT_NUM);
    let tc_c = cell_ref(TOTAL_CAP, DATA0);
    s.text(WE, LABEL, "  Equity Weight  (E / V)");
    s.formula(WE, DATA0, format!("=IF({tc_c}<>0,{mc_c}/{tc_c},0)"));
    s.stamp_row(WE, FMT_PCT);
    s.text(WD, LABEL, "  Debt Weight  (D / V)");
    s.formula(WD, DATA0, format!("=IF({tc_c}<>0,{d_c}/{tc_c},0)"));
    s.stamp_row(WD, FMT_PCT);

    // Final WACC
    s.section(WACC, "WACC  (We × Ke + Wd × Kd_after_tax)");
    let we_c = cell_ref(WE, DATA0);
    let ke_c = cell_ref(KE, DATA0);
    let wd_c = cell_ref(WD, DATA0);
    let kdat_c = cell_ref(KD_AFTER, DATA0);
    s.formula(WACC, DATA0, format!("={we_c}*{ke_c}+{wd_c}*{kdat_c}"));
    s.fill(WACC, DATA0, BLUE);
    s.stamp_row(WACC, FMT_PCT);

    s
}

/// Public row anchors used by DCF / Cover cross-links.
pub mod rows {
    pub const RF: u32 = super::RF;
    pub const ERP: u32 = super::ERP;
    pub const BE_TARGET: u32 = super::BE_TARGET;
    pub const KD_PRE: u32 = super::KD_PRE;
    pub const TAX: u32 = super::TAX;
    pub const WE: u32 = super::WE;
    pub const WD: u32 = super::WD;
    pub const WACC: u32 = super::WACC;
}
