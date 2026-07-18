//! Comps Peers tab — port of `writer.py::_write_comps_peers`.

use crate::input::WorkbookInput;
use crate::model::{FMT_MULT, FMT_NUM, FMT_PCT, LABEL, Sheet};
use fm_value::PublicCompsOutput;

pub fn build(input: &WorkbookInput) -> Sheet {
    let pc = input
        .public_comps
        .as_ref()
        .expect("Comps Peers requires WorkbookInput.public_comps");
    build_from(pc)
}

fn write_num(s: &mut Sheet, row: u32, col: u32, n: f64, fmt: &'static str) {
    s.number(row, col, n);
    if let Some(c) = s.cells.get_mut(&(row, col)) {
        c.num_fmt = Some(fmt);
    }
}

fn write_opt(s: &mut Sheet, row: u32, col: u32, v: Option<f64>, fmt: &'static str) {
    match v {
        Some(n) => write_num(s, row, col, n, fmt),
        None => s.text(row, col, "NM"),
    }
}

pub fn build_from(pc: &PublicCompsOutput) -> Sheet {
    let mut s = Sheet::new("Comps Peers");
    s.title(
        2,
        format!("{} — Public Comps  (Peer Detail)", pc.target_company_name),
    );
    s.text(
        4,
        LABEL,
        format!("As of {}  |  Source: {}", pc.as_of_date, pc.source),
    );
    s.text(5, LABEL, "(USD $ in millions, multiples per spec)");

    let cols = [
        "Ticker",
        "Tier",
        "Price",
        "52w High",
        "52w Low",
        "Shares (M)",
        "Mkt Cap ($M)",
        "Debt",
        "Cash",
        "EV ($M)",
        "LTM Rev",
        "LTM EBITDA",
        "LTM EBIT",
        "LTM NI",
        "LTM EPS",
        "EV/Rev",
        "EV/EBITDA",
        "EV/EBIT",
        "P/E",
        "% off High",
        "NTM Rev",
        "FY+1 Rev",
        "FY+2 Rev",
        "EV/Rev NTM",
        "EV/EBITDA NTM",
        "EV/EBITDA FY+1",
        "P/E NTM",
    ];
    for (i, c) in cols.iter().enumerate() {
        s.text(7, LABEL + i as u32, *c);
    }

    for (i, p) in pc.peers.iter().enumerate() {
        let r = 8 + i as u32;
        let pct_off_hi = if p.week52_high > 0.0 {
            1.0 - p.share_price / p.week52_high
        } else {
            0.0
        };
        s.text(r, LABEL, p.ticker.clone());
        write_num(&mut s, r, LABEL + 1, p.tier as f64, FMT_NUM);
        write_num(&mut s, r, LABEL + 2, p.share_price, FMT_NUM);
        write_num(&mut s, r, LABEL + 3, p.week52_high, FMT_NUM);
        write_num(&mut s, r, LABEL + 4, p.week52_low, FMT_NUM);
        write_num(&mut s, r, LABEL + 5, p.shares_diluted, FMT_NUM);
        write_num(&mut s, r, LABEL + 6, p.market_cap, FMT_NUM);
        write_num(&mut s, r, LABEL + 7, p.total_debt, FMT_NUM);
        write_num(&mut s, r, LABEL + 8, p.cash, FMT_NUM);
        write_num(&mut s, r, LABEL + 9, p.enterprise_value, FMT_NUM);
        write_num(&mut s, r, LABEL + 10, p.ltm_revenue, FMT_NUM);
        write_num(&mut s, r, LABEL + 11, p.ltm_ebitda, FMT_NUM);
        write_num(&mut s, r, LABEL + 12, p.ltm_ebit, FMT_NUM);
        write_num(&mut s, r, LABEL + 13, p.ltm_net_income, FMT_NUM);
        write_num(&mut s, r, LABEL + 14, p.ltm_eps_diluted, FMT_NUM);
        write_opt(&mut s, r, LABEL + 15, p.ev_rev_ltm, FMT_MULT);
        write_opt(&mut s, r, LABEL + 16, p.ev_ebitda_ltm, FMT_MULT);
        write_opt(&mut s, r, LABEL + 17, p.ev_ebit_ltm, FMT_MULT);
        write_opt(&mut s, r, LABEL + 18, p.pe_ltm, FMT_MULT);
        write_num(&mut s, r, LABEL + 19, pct_off_hi, FMT_PCT);
        write_num(&mut s, r, LABEL + 20, p.ntm_revenue, FMT_NUM);
        write_num(&mut s, r, LABEL + 21, p.fy1_revenue, FMT_NUM);
        write_num(&mut s, r, LABEL + 22, p.fy2_revenue, FMT_NUM);
        write_opt(&mut s, r, LABEL + 23, p.ev_rev_ntm, FMT_MULT);
        write_opt(&mut s, r, LABEL + 24, p.ev_ebitda_ntm, FMT_MULT);
        write_opt(&mut s, r, LABEL + 25, p.ev_ebitda_fy1, FMT_MULT);
        write_opt(&mut s, r, LABEL + 26, p.pe_ntm, FMT_MULT);
    }

    s
}
