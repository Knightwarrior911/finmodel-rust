//! Cover tab. When DCF/WACC are present, valuation summary pulls live formulas
//! from those tabs; otherwise shows the snapshot placeholder.

use crate::input::WorkbookInput;
use crate::model::{cell_ref, Sheet, DATA0, LABEL};
use crate::sheets::dcf::rows as dr;
use crate::sheets::wacc::rows as wr;

// Cover valuation rows (0-based)
const VAL_HDR: u32 = 17;
const CURRENT_PX: u32 = 18;
const IMPLIED_PX: u32 = 19;
const UPSIDE: u32 = 20;
const WACC: u32 = 21;
const TERMINAL_G: u32 = 22;
const EXIT_MULT: u32 = 23;
const EV: u32 = 24;
const EQUITY_VALUE: u32 = 25;

// Assumptions anchors
const ASSUMP_ACTIVE_DRV0: u32 = 14;
const ASSUMP_SHARED0: u32 = 85;

pub fn build(input: &WorkbookInput) -> Sheet {
    let m = &input.meta;
    let periods = &input.model.periods;
    let hist: Vec<&String> = periods.iter().filter(|p| p.ends_with('A')).collect();
    let proj: Vec<&String> = periods.iter().filter(|p| p.ends_with('E')).collect();

    let mut s = Sheet::new("Cover");

    s.title(2, format!("{} — Valuation Model", m.company));
    s.text(4, LABEL, "3-Statement + DCF Valuation");
    s.cell_mut(4, LABEL).font_hex = Some(crate::sheets::NAVY);
    s.cell_mut(4, LABEL).bold = true;
    s.text(
        5,
        LABEL,
        format!("As of {}  |  ({} in millions)", m.as_of, m.currency),
    );
    s.cell_mut(5, LABEL).font_hex = Some(crate::sheets::GRAY);
    s.cell_mut(5, LABEL).italic = true;

    s.section(8, "MODEL OVERVIEW");
    s.text(9, LABEL, "Company");
    s.text(9, DATA0, m.company.clone());
    s.text(10, LABEL, "Ticker");
    s.text(
        10,
        DATA0,
        if m.ticker.is_empty() {
            "—".to_string()
        } else {
            m.ticker.clone()
        },
    );
    s.cell_mut(10, DATA0).bold = true;
    s.text(11, LABEL, "Active Case");
    s.formula(11, DATA0, "=Assumptions!D10");
    s.text(12, LABEL, "Currency");
    s.text(12, DATA0, m.currency.clone());
    s.text(13, LABEL, "Fiscal Year End");
    s.text(13, DATA0, m.fiscal_year_end.clone());
    s.text(14, LABEL, "Periods");
    let periods_str = if let (Some(h0), Some(hn), Some(p0), Some(pn)) =
        (hist.first(), hist.last(), proj.first(), proj.last())
    {
        format!("Hist: {h0}–{hn}  |  Proj: {p0}–{pn}")
    } else {
        "—".to_string()
    };
    s.text(14, DATA0, periods_str);

    s.section(VAL_HDR, "VALUATION SUMMARY");
    if input.dcf.is_some() && input.wacc.is_some() {
        let cur_px = cell_ref(ASSUMP_SHARED0 + 4, DATA0);
        let term_g = cell_ref(ASSUMP_ACTIVE_DRV0 + 12, DATA0);
        let exit_mt = cell_ref(ASSUMP_ACTIVE_DRV0 + 13, DATA0);

        s.text(CURRENT_PX, LABEL, "Current Share Price");
        s.formula(CURRENT_PX, DATA0, format!("=Assumptions!{cur_px}"));
        s.text(IMPLIED_PX, LABEL, "DCF Implied Price");
        s.formula(
            IMPLIED_PX,
            DATA0,
            format!("=DCF!{}", cell_ref(dr::EV_PRICE, DATA0)),
        );
        s.text(UPSIDE, LABEL, "Upside / (Downside) %");
        let cur_c = cell_ref(CURRENT_PX, DATA0);
        let imp_c = cell_ref(IMPLIED_PX, DATA0);
        s.formula(
            UPSIDE,
            DATA0,
            format!("=IF({cur_c}>0,{imp_c}/{cur_c}-1,\"—\")"),
        );
        s.text(WACC, LABEL, "WACC");
        s.formula(WACC, DATA0, format!("=WACC!{}", cell_ref(wr::WACC, DATA0)));
        s.text(TERMINAL_G, LABEL, "Terminal Growth Rate");
        s.formula(TERMINAL_G, DATA0, format!("=Assumptions!{term_g}"));
        s.text(EXIT_MULT, LABEL, "Exit EBITDA Multiple");
        s.formula(EXIT_MULT, DATA0, format!("=Assumptions!{exit_mt}"));
        s.text(EV, LABEL, "Enterprise Value");
        s.formula(EV, DATA0, format!("=DCF!{}", cell_ref(dr::EV_TOTAL, DATA0)));
        s.text(EQUITY_VALUE, LABEL, "Equity Value");
        s.formula(
            EQUITY_VALUE,
            DATA0,
            format!("=DCF!{}", cell_ref(dr::EV_EQUITY, DATA0)),
        );
    } else {
        s.text(CURRENT_PX, LABEL, "(DCF / Assumptions not yet built)");
        s.cell_mut(CURRENT_PX, LABEL).font_hex = Some(crate::sheets::GRAY);
        s.cell_mut(CURRENT_PX, LABEL).italic = true;
    }

    s
}
