//! Comps Summary tab — port of `writer.py::_write_comps_summary`.

use crate::input::WorkbookInput;
use crate::model::{BLUE, DATA0, FMT_MULT, FMT_NUM, LABEL, Sheet};
use fm_value::PublicCompsOutput;

const STAT_KEYS: [&str; 7] = [
    "ev_rev_ltm",
    "ev_ebitda_ltm",
    "ev_ebit_ltm",
    "pe_ltm",
    "ev_rev_ntm",
    "ev_ebitda_ntm",
    "pe_ntm",
];

pub fn build(input: &WorkbookInput) -> Sheet {
    let pc = input
        .public_comps
        .as_ref()
        .expect("Comps Summary requires WorkbookInput.public_comps");
    build_from(pc)
}

pub fn build_from(pc: &PublicCompsOutput) -> Sheet {
    let mut s = Sheet::new("Comps Summary");
    s.title(
        2,
        format!("{} — Public Comps  (Summary Stats)", pc.target_company_name),
    );
    s.text(
        4,
        LABEL,
        "Peer-Set Trading Multiples + Implied Target Valuation",
    );
    s.text(
        5,
        LABEL,
        format!("As of {}  |  Source: {}", pc.as_of_date, pc.source),
    );

    s.section(7, "STATISTICS BY MULTIPLE");
    for (i, h) in ["Min", "P25", "Median", "Mean", "P75", "Max", "Count"]
        .iter()
        .enumerate()
    {
        s.text(8, DATA0 + i as u32, *h);
    }

    for (i, key) in STAT_KEYS.iter().enumerate() {
        let r = 9 + i as u32;
        if let Some(st) = pc.stats.get(*key) {
            s.text(r, LABEL, st.multiple_name.clone());
            if st.count > 0 {
                for (j, v) in [st.min, st.p25, st.median, st.mean, st.p75, st.max]
                    .iter()
                    .enumerate()
                {
                    s.number(r, DATA0 + j as u32, *v);
                    if let Some(c) = s.cells.get_mut(&(r, DATA0 + j as u32)) {
                        c.num_fmt = Some(FMT_MULT);
                    }
                }
                s.number(r, DATA0 + 6, st.count as f64);
                if let Some(c) = s.cells.get_mut(&(r, DATA0 + 6)) {
                    c.num_fmt = Some(FMT_NUM);
                }
            } else {
                for j in 0..7 {
                    s.text(r, DATA0 + j, "NM");
                }
            }
        } else {
            s.text(r, LABEL, *key);
            for j in 0..7 {
                s.text(r, DATA0 + j, "NM");
            }
        }
    }

    s.section(15, "IMPLIED TARGET VALUATION  (EV / EBITDA basis)");
    s.text(16, LABEL, "  Target LTM EBITDA ($M)");
    s.number(16, DATA0, pc.target_ebitda);
    if let Some(c) = s.cells.get_mut(&(16, DATA0)) {
        c.num_fmt = Some(FMT_NUM);
    }
    s.text(17, LABEL, "  Target Net Debt ($M)");
    s.number(17, DATA0, pc.target_total_debt - pc.target_cash);
    if let Some(c) = s.cells.get_mut(&(17, DATA0)) {
        c.num_fmt = Some(FMT_NUM);
    }
    s.text(18, LABEL, "  Target Diluted Shares (M)");
    s.number(18, DATA0, pc.target_shares_diluted);
    if let Some(c) = s.cells.get_mut(&(18, DATA0)) {
        c.num_fmt = Some(FMT_NUM);
    }

    s.text(20, LABEL, "Implied Per-Share Price (low / median / high)");
    s.number(20, DATA0, pc.implied_price_low);
    s.number(20, DATA0 + 1, pc.implied_price_median);
    s.number(20, DATA0 + 2, pc.implied_price_high);
    for j in 0..3 {
        if let Some(c) = s.cells.get_mut(&(20, DATA0 + j)) {
            c.num_fmt = Some(FMT_NUM);
            c.fill = Some(BLUE.to_string());
        }
    }
    s.text(21, DATA0, "p25 multiple");
    s.text(21, DATA0 + 1, "median");
    s.text(21, DATA0 + 2, "p75");

    if !pc.excluded.is_empty() {
        s.section(24, "EXCLUDED CANDIDATES");
        for (i, (tk, reason)) in pc.excluded.iter().take(15).enumerate() {
            let r = 25 + i as u32;
            s.text(r, LABEL, format!("  {tk}"));
            s.text(r, DATA0, reason.clone());
        }
    }

    s
}
