//! Assumptions tab: toggle + Active (CHOOSE) block + Base/Upside/Downside
//! hardcoded scenarios + shared valuation inputs. Layout mirrors `ASSUMP_R`.

use crate::input::{ScenarioInputs, WorkbookInput};
use crate::model::{DATA0, FMT_NUM, FMT_PCT, LABEL, Sheet, cell_ref};
use crate::sheets::{assumptions_proj_periods, col};

/// Per-driver number format, aligned with `DRIVERS` / writer.py ASSUMP_DRIVERS.
const DRIVER_FMT: [&str; 14] = [
    FMT_PCT, FMT_PCT, FMT_PCT, FMT_PCT, FMT_PCT, FMT_PCT, FMT_PCT, FMT_PCT, // 0-7 pct
    FMT_NUM, FMT_NUM, FMT_NUM, FMT_NUM, // 8-11 dso/dio/dpo/dividend
    FMT_PCT, FMT_NUM, // 12 terminal growth (pct), 13 exit multiple (num)
];
/// Per-shared-input number format, aligned with `SHARED` / writer.py ASSUMP_SHARED.
const SHARED_FMT: [&str; 6] = [FMT_PCT, FMT_PCT, FMT_NUM, FMT_PCT, FMT_NUM, FMT_NUM];

// Row anchors (0-based), from writer.py ASSUMP_R.
const TITLE: u32 = 2;
const SUBTITLE: u32 = 4;
const UNITS: u32 = 5;
const TOGGLE: u32 = 8;
const ACTIVE: u32 = 9;
const ACTIVE_HDR: u32 = 12;
const ACTIVE_PERIODS: u32 = 13;
const ACTIVE_DRV0: u32 = 14;
const BASE_HDR: u32 = 30;
const BASE_PERIODS: u32 = 31;
const BASE_DRV0: u32 = 32;
const UPSIDE_HDR: u32 = 48;
const UPSIDE_PERIODS: u32 = 49;
const UPSIDE_DRV0: u32 = 50;
const DOWNSIDE_HDR: u32 = 66;
const DOWNSIDE_PERIODS: u32 = 67;
const DOWNSIDE_DRV0: u32 = 68;
const SHARED_HDR: u32 = 84;
const SHARED_DRV0: u32 = 85;

/// (label, is_per_period). Order == ASSUMP_DRIVERS.
const DRIVERS: [(&str, bool); 14] = [
    ("Revenue Growth %", true),
    ("Gross Margin %", true),
    ("SG&A % of Revenue", true),
    ("R&D % of Revenue", true),
    ("D&A % of Revenue", true),
    ("CapEx % of Revenue", true),
    ("Tax Rate %", true),
    ("Interest Rate %", true),
    ("DSO (days)", true),
    ("DIO (days)", true),
    ("DPO (days)", true),
    ("Dividend per Share ($)", true),
    ("Terminal Growth Rate", false),
    ("Exit EBITDA Multiple", false),
];

const SHARED: [&str; 6] = [
    "Risk-Free Rate (10Y Treasury)",
    "Equity Risk Premium",
    "Target Debt/Equity Ratio",
    "Pre-Tax Cost of Debt",
    "Current Share Price ($)",
    "Diluted Shares Outstanding (M)",
];

/// Per-period value list for driver `i` (0-11); scalar drivers handled elsewhere.
fn per_period(scn: &ScenarioInputs, i: usize) -> &Vec<f64> {
    match i {
        0 => &scn.revenue_growth_pct,
        1 => &scn.gross_margin_pct,
        2 => &scn.sga_pct_rev,
        3 => &scn.rd_pct_rev,
        4 => &scn.da_pct_rev,
        5 => &scn.capex_pct_rev,
        6 => &scn.tax_rate_pct,
        7 => &scn.interest_rate_pct,
        8 => &scn.dso_days,
        9 => &scn.dio_days,
        10 => &scn.dpo_days,
        _ => &scn.dividend_per_share,
    }
}

/// Scalar value for driver 12/13.
fn scalar(scn: &ScenarioInputs, i: usize) -> f64 {
    if i == 12 {
        scn.terminal_growth_rate
    } else {
        scn.exit_ebitda_multiple
    }
}

fn periods_row(s: &mut Sheet, row: u32, proj: &[String]) {
    for (j, p) in proj.iter().enumerate() {
        s.text(row, col(j), p.clone());
        let c = s.cell_mut(row, col(j));
        c.font_hex = Some(crate::sheets::NAVY);
        c.bold = true;
        c.center = true;
        c.bottom_border = true;
    }
}

/// Driver row finish: gray italic label; italic data only for percent-format
/// rows (num rows — days / dividend / multiple / D-E / price / shares — stay
/// upright). Mirrors writer.py `lbl_drv` + `drv`/`hc` families.
fn style_driver_row(s: &mut Sheet, row: u32, is_pct: bool, ncols: usize) {
    let lc = s.cell_mut(row, LABEL);
    lc.italic = true;
    lc.font_hex = Some(crate::sheets::GRAY);
    if is_pct {
        for j in 0..ncols {
            s.cell_mut(row, col(j)).italic = true;
        }
    }
}

/// Driver label, sector-aware. Non-standard sectors relabel slots 1-3 (writer.py
/// `ASSUMP_DRIVERS_UTILITY`); everything else uses the standard label.
fn driver_label(sector: &str, i: usize) -> &'static str {
    let non_std = matches!(sector, "utility" | "bank" | "reit" | "insurance");
    match (non_std, i) {
        (true, 1) => "O&M % of Revenue",
        (true, 2) => "Taxes other than income % of Revenue",
        (true, 3) => "Other OpEx % of Revenue",
        _ => DRIVERS[i].0,
    }
}

/// Hardcoded driver-value block for one scenario.
fn scenario_inputs(s: &mut Sheet, drv0: u32, scn: &ScenarioInputs, n_proj: usize, sector: &str) {
    for (i, (_, is_pp)) in DRIVERS.iter().enumerate() {
        let r = drv0 + i as u32;
        s.text(r, LABEL, format!("  {}", driver_label(sector, i)));
        if *is_pp {
            let vals = per_period(scn, i);
            for j in 0..n_proj {
                let v = vals.get(j).copied().unwrap_or(0.0);
                s.number(r, col(j), v);
            }
        } else {
            s.number(r, DATA0, scalar(scn, i));
        }
        s.stamp_row(r, DRIVER_FMT[i]);
        style_driver_row(
            s,
            r,
            DRIVER_FMT[i] == FMT_PCT,
            if *is_pp { n_proj } else { 1 },
        );
    }
}

pub fn build(input: &WorkbookInput) -> Sheet {
    let m = &input.meta;
    let a = &input.assumptions;
    let n_proj = input.model.n_proj();
    let proj = assumptions_proj_periods(&m.fiscal_year_end, &m.as_of, n_proj);

    let mut s = Sheet::new("Assumptions");

    s.title(TITLE, format!("{} — Assumptions", m.company));
    s.text(SUBTITLE, LABEL, "Operating & Valuation Drivers");
    s.cell_mut(SUBTITLE, LABEL).font_hex = Some(crate::sheets::NAVY);
    s.cell_mut(SUBTITLE, LABEL).bold = true;
    s.text(
        UNITS,
        LABEL,
        "(per-period values in proj year columns; scalars in first column)",
    );
    s.cell_mut(UNITS, LABEL).font_hex = Some(crate::sheets::GRAY);
    s.cell_mut(UNITS, LABEL).italic = true;

    // Toggle + active-case display.
    s.text(
        TOGGLE,
        LABEL,
        "Case Toggle  (1 = Base  |  2 = Upside  |  3 = Downside)",
    );
    s.cell_mut(TOGGLE, LABEL).bold = true;
    s.number(TOGGLE, DATA0, a.active_case as f64);
    s.stamp_row(TOGGLE, FMT_NUM);
    s.text(ACTIVE, LABEL, "Active Case");
    s.cell_mut(ACTIVE, LABEL).bold = true;
    s.formula(
        ACTIVE,
        DATA0,
        "=CHOOSE($D$9,\"Base\",\"Upside\",\"Downside\")",
    );

    // Active block — CHOOSE formulas pulling from the three scenario blocks.
    s.section(
        ACTIVE_HDR,
        "ACTIVE CASE  (CHOOSE formulas — pulls from active scenario below)",
    );
    periods_row(&mut s, ACTIVE_PERIODS, &proj);
    for (i, (_, is_pp)) in DRIVERS.iter().enumerate() {
        let ar = ACTIVE_DRV0 + i as u32;
        let br = BASE_DRV0 + i as u32;
        let ur = UPSIDE_DRV0 + i as u32;
        let dr = DOWNSIDE_DRV0 + i as u32;
        s.text(ar, LABEL, format!("  {}", driver_label(&m.sector, i)));
        let ncols = if *is_pp { n_proj } else { 1 };
        for j in 0..ncols {
            let c = col(j);
            let f = format!(
                "=CHOOSE($D$9,{},{},{})",
                cell_ref(br, c),
                cell_ref(ur, c),
                cell_ref(dr, c)
            );
            s.formula(ar, c, f);
        }
        s.stamp_row(ar, DRIVER_FMT[i]);
        style_driver_row(&mut s, ar, DRIVER_FMT[i] == FMT_PCT, ncols);
    }

    // Hardcoded scenario blocks.
    s.section(BASE_HDR, "BASE CASE  (hardcoded inputs)");
    periods_row(&mut s, BASE_PERIODS, &proj);
    scenario_inputs(&mut s, BASE_DRV0, &a.base, n_proj, &m.sector);

    s.section(UPSIDE_HDR, "UPSIDE CASE  (hardcoded inputs)");
    periods_row(&mut s, UPSIDE_PERIODS, &proj);
    scenario_inputs(&mut s, UPSIDE_DRV0, &a.upside, n_proj, &m.sector);

    s.section(DOWNSIDE_HDR, "DOWNSIDE CASE  (hardcoded inputs)");
    periods_row(&mut s, DOWNSIDE_PERIODS, &proj);
    scenario_inputs(&mut s, DOWNSIDE_DRV0, &a.downside, n_proj, &m.sector);

    // Shared inputs.
    s.section(SHARED_HDR, "SHARED INPUTS  (non-scenario)");
    let shared_vals = [
        a.risk_free_rate,
        a.equity_risk_premium,
        a.target_de_ratio,
        a.cost_of_debt_pretax,
        a.current_share_price,
        a.shares_diluted,
    ];
    for (i, label) in SHARED.iter().enumerate() {
        let r = SHARED_DRV0 + i as u32;
        s.text(r, LABEL, format!("  {label}"));
        s.number(r, DATA0, shared_vals[i]);
        s.stamp_row(r, SHARED_FMT[i]);
        style_driver_row(&mut s, r, SHARED_FMT[i] == FMT_PCT, 1);
    }

    s
}
