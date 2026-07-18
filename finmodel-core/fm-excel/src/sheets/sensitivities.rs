//! Sensitivities tab — port of `writer.py::_write_sensitivities`.

use crate::input::WorkbookInput;
use crate::model::{BLUE, DATA0, FMT_MULT, FMT_NUM, FMT_PCT, LABEL, LIGHT_BLUE, Sheet, col_name};
use crate::sheets::dcf::rows as dr;

// SENS_R
const TITLE: u32 = 2;
const SUBTITLE: u32 = 4;
const UNITS: u32 = 5;
const TBL1_HDR: u32 = 8;
const TBL1_AXIS: u32 = 9;
const TBL1_COLS: u32 = 10;
const TBL1_START: u32 = 11;
const TBL2_HDR: u32 = 18;
const TBL2_AXIS: u32 = 19;
const TBL2_COLS: u32 = 20;
const TBL2_START: u32 = 21;

pub fn build(input: &WorkbookInput) -> Sheet {
    let dcf = input.dcf.as_ref().expect("sensitivities requires dcf");
    let m = &input.meta;
    let n_proj = dcf.proj_periods.len();
    let offset = if dcf.mid_year_convention { -0.5 } else { 0.0 };
    let exps: Vec<f64> = (0..n_proj).map(|k| (k as f64 + 1.0) + offset).collect();
    let mut s = Sheet::new("Sensitivities");

    s.title(TITLE, format!("{} — Sensitivity Analysis", m.company));
    s.text(SUBTITLE, LABEL, "Implied Share Price Sensitivities");
    // Python hardcodes USD in the units line even for non-USD models.
    s.text(UNITS, LABEL, "(USD $ per share)");

    let lc = col_name(LABEL);
    let dcol = col_name(DATA0);
    let fe = dr::FCF_FCFF + 1;
    let t1e = dr::TV1_EBITDA + 1;
    let debt_er = dr::EV_DEBT + 1;
    let cash_er = dr::EV_CASH + 1;
    let shrs_er = dr::EV_SHARES + 1;
    let last_ufcf_col = col_name(DATA0 + n_proj as u32 - 1);

    let ufcf_sum = |wacc_ref: &str| -> String {
        (0..n_proj)
            .map(|k| {
                format!(
                    "DCF!${}${fe}/(1+{wacc_ref})^{exp}",
                    col_name(DATA0 + k as u32),
                    exp = exps[k]
                )
            })
            .collect::<Vec<_>>()
            .join(" + ")
    };

    // Table 1: WACC × Terminal Growth (Gordon)
    s.section(TBL1_HDR, "WACC × Terminal Growth (Gordon)");
    s.text(TBL1_AXIS, LABEL, "WACC ↓  /  Terminal g →");
    for (j, g) in dcf.gordon_growth_range.iter().enumerate() {
        s.number(TBL1_COLS, DATA0 + j as u32, *g);
        if let Some(c) = s.cells.get_mut(&(TBL1_COLS, DATA0 + j as u32)) {
            c.num_fmt = Some(FMT_PCT);
        }
    }
    let g_hdr_er = TBL1_COLS + 1;
    let n_wacc = dcf.wacc_range.len();
    let mid = n_wacc / 2;
    for (i, w) in dcf.wacc_range.iter().enumerate() {
        let r = TBL1_START + i as u32;
        let r_excel = r + 1;
        s.number(r, LABEL, *w);
        if let Some(c) = s.cells.get_mut(&(r, LABEL)) {
            c.num_fmt = Some(FMT_PCT);
        }
        let wacc_ref = format!("${lc}${r_excel}");
        for j in 0..dcf.gordon_growth_range.len() {
            let col = DATA0 + j as u32;
            let g_ref = format!("{}${}", col_name(col), g_hdr_er);
            let tv_pv = format!(
                "IF({wacc_ref}>{g_ref},DCF!${last_ufcf_col}${fe}*(1+{g_ref})/(({wacc_ref}-{g_ref})*(1+{wacc_ref})^{n_proj}),0)"
            );
            let bridge = format!("-DCF!${dcol}${debt_er}+DCF!${dcol}${cash_er}");
            let shares = format!("DCF!${dcol}${shrs_er}");
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
        s.stamp_row(r, FMT_NUM);
    }

    // Table 2: WACC × Exit Multiple
    s.section(TBL2_HDR, "WACC × Exit EBITDA Multiple");
    s.text(TBL2_AXIS, LABEL, "WACC ↓  /  Exit Mult →");
    for (j, mult) in dcf.ebitda_multiple_range.iter().enumerate() {
        s.number(TBL2_COLS, DATA0 + j as u32, *mult);
        if let Some(c) = s.cells.get_mut(&(TBL2_COLS, DATA0 + j as u32)) {
            c.num_fmt = Some(FMT_MULT);
        }
    }
    let mult_hdr_er = TBL2_COLS + 1;
    for (i, w) in dcf.wacc_range.iter().enumerate() {
        let r = TBL2_START + i as u32;
        let r_excel = r + 1;
        s.number(r, LABEL, *w);
        if let Some(c) = s.cells.get_mut(&(r, LABEL)) {
            c.num_fmt = Some(FMT_PCT);
        }
        let wacc_ref = format!("${lc}${r_excel}");
        for j in 0..dcf.ebitda_multiple_range.len() {
            let col = DATA0 + j as u32;
            let mult_ref = format!("{}${}", col_name(col), mult_hdr_er);
            let tv_pv = format!("DCF!${dcol}${t1e}*{mult_ref}/(1+{wacc_ref})^{n_proj}");
            let bridge = format!("-DCF!${dcol}${debt_er}+DCF!${dcol}${cash_er}");
            let shares = format!("DCF!${dcol}${shrs_er}");
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
        s.stamp_row(r, FMT_NUM);
    }

    s
}
