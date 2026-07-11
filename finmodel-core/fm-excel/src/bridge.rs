//! EV-bridge worksheet — port of `ResearchExcelWriter.write_ev_bridge`
//! (`src/research/output_writer.py`) onto the shared cell-model / render engine.
//!
//! Single-column analysis layout (label col C, data col D): Equity Value →
//! Enterprise Value Bridge (checklist of present add/subtract items) → Valuation
//! Multiples → Rules Applied. Monetary inputs are shown in millions; the market
//! cap and EV are live Excel formulas referencing the input rows. Blue inputs
//! carry source notes (provenance; ungated). Gated cell-for-cell
//! (value/formula/fill) against a Python oracle (`tieout/build_ev_bridge_oracle.py`
//! → `EV_BRIDGE_snapshot.json`, `tests/ev_bridge_parity.rs`).

use crate::adhoc::{ADHOC_TITLE, NF_DOLLAR, NF_MULT, NF_PRICE, NF_SHARES};
use crate::model::{cell_ref, Cell, Sheet, Value, DATA0, LABEL, TAN};
use fm_value::ev_bridge::EvBridgeInput;

const MILLIONS: f64 = 1_000_000.0;

/// Write a tan section header at column C (text carries the writer's leading space).
fn section(s: &mut Sheet, row: u32, label: &str) {
    s.merge(row, LABEL, Cell {
        value: Some(Value::Text(format!(" {label}"))),
        fill: Some(TAN.to_string()),
        ..Default::default()
    });
}

/// Write a blue hardcoded input (label at C, number at D) + optional source note.
fn input(s: &mut Sheet, row: u32, label: &str, value: f64, fmt: &'static str, note: &str) {
    s.text(row, LABEL, label.to_string());
    s.merge(row, DATA0, Cell {
        value: Some(Value::Number(value)),
        num_fmt: Some(fmt),
        comment: (!note.is_empty()).then(|| format!("Source: {note}")),
        ..Default::default()
    });
}

/// Write a black formula (label at C, formula at D).
fn formula(s: &mut Sheet, row: u32, label: &str, f: &str, fmt: &'static str) {
    s.text(row, LABEL, label.to_string());
    s.merge(row, DATA0, Cell {
        formula: Some(f.to_string()),
        num_fmt: Some(fmt),
        ..Default::default()
    });
}

fn line(s: &mut Sheet, row: u32, text: &str) {
    s.text(row, LABEL, text.to_string());
}

fn d(row: u32) -> String {
    cell_ref(row, DATA0)
}

/// Build the EV-bridge worksheet from a fully-specified [`EvBridgeInput`].
/// Only present, positive add/subtract items are rendered (BIWS checklist).
pub fn build_ev_bridge_sheet(inp: &EvBridgeInput, generated: &str) -> Sheet {
    let mut s = Sheet::new("EV Bridge");
    let name = if inp.company.is_empty() { "Company" } else { &inp.company };

    // Title (row 2) + units (row 5), mirroring `_setup`.
    s.merge(2, LABEL, Cell {
        value: Some(Value::Text(format!("{name} – Enterprise Value Bridge"))),
        fill: Some(ADHOC_TITLE.to_string()),
        ..Default::default()
    });
    let curr = if inp.currency.is_empty() { "USD" } else { &inp.currency };
    s.text(5, LABEL, format!("({curr} in millions unless noted)"));

    let mut row = 7u32;
    let m = |v: f64| v / MILLIONS;
    let present = |v: Option<f64>| v.filter(|&x| x > 0.0);

    // ── Equity Value ──────────────────────────────────────────────────
    section(&mut s, row, "Equity Value");
    row += 1;
    let r_price = row;
    input(&mut s, row, "Share Price", inp.share_price.unwrap_or(0.0), NF_PRICE, "Primary exchange");
    row += 1;
    let r_shares = row;
    input(&mut s, row, "Shares Outstanding (wtd avg basic)",
        inp.shares_outstanding.unwrap_or(0.0), NF_SHARES,
        "Latest filing — weighted average basic shares (F-001)");
    row += 1;
    row += 1; // divider (borders only; invisible to the gate)
    let r_mc = row;
    formula(&mut s, row, "Market Cap (Equity Value)",
        &format!("={}*{}/1000000", d(r_price), d(r_shares)), NF_DOLLAR);
    row += 1;
    row += 1; // spacer

    // ── Enterprise Value Bridge ───────────────────────────────────────
    section(&mut s, row, "Enterprise Value Bridge");
    row += 1;
    formula(&mut s, row, "Market Cap", &format!("={}", d(r_mc)), NF_DOLLAR);
    row += 1;

    let add_map: [(Option<f64>, &str, &str); 6] = [
        (inp.total_debt, "Total Debt", "Balance Sheet"),
        (inp.finance_leases, "Finance/Capital Lease Liabilities", "ASC 842 / IFRS 16 note"),
        (inp.operating_leases, "Operating Lease Liabilities (R-016)", "ASC 842 / IFRS 16 lease footnote (R-016)"),
        (inp.underfunded_pension, "Underfunded Pension (R-015)", "Pension footnote ONLY — NOT balance sheet (R-015)"),
        (inp.minority_interest, "Minority Interest (NCI)", "Balance Sheet"),
        (inp.preferred_stock, "Preferred Stock", "Balance Sheet"),
    ];
    let mut add_rows: Vec<u32> = Vec::new();
    for (v, label, note) in add_map {
        if let Some(val) = present(v) {
            input(&mut s, row, &format!("+  {label}"), m(val), NF_DOLLAR, note);
            add_rows.push(row);
            row += 1;
        }
    }

    let sub_map: [(Option<f64>, &str, &str); 7] = [
        (inp.cash, "Cash & Cash Equivalents", "Balance Sheet"),
        (inp.short_term_investments, "Short-term Investments", "Balance Sheet"),
        (inp.equity_investments, "Equity Method Investments (R-014)", "Balance Sheet — non-operating (R-014)"),
        (inp.financial_investments, "Financial Investments (non-operating)", "Balance Sheet"),
        (inp.assets_held_for_sale, "Assets Held for Sale", "Balance Sheet"),
        (inp.discontinued_ops_assets, "Discontinued Ops Assets", "Balance Sheet"),
        (inp.nol_dta, "NOL Deferred Tax Assets", "Balance Sheet"),
    ];
    let mut sub_rows: Vec<u32> = Vec::new();
    for (v, label, note) in sub_map {
        if let Some(val) = present(v) {
            input(&mut s, row, &format!("-  {label}"), m(val), NF_DOLLAR, note);
            sub_rows.push(row);
            row += 1;
        }
    }

    row += 1; // divider
    let r_ev = row;
    let mut ev_formula = format!("={}", d(r_mc));
    for r in &add_rows {
        ev_formula.push_str(&format!("+{}", d(*r)));
    }
    for r in &sub_rows {
        ev_formula.push_str(&format!("-{}", d(*r)));
    }
    formula(&mut s, row, "Enterprise Value", &ev_formula, NF_DOLLAR);
    s.stamp_bold_row(row);
    row += 1;
    row += 1; // spacer

    // ── Valuation Multiples ───────────────────────────────────────────
    let has_multiples =
        inp.ltm_revenue.is_some() || inp.ltm_ebitda.is_some() || inp.ltm_ebit.is_some();
    if has_multiples {
        section(&mut s, row, "Valuation Multiples");
        row += 1;
        let mut r_rev: Option<u32> = None;
        if let Some(rev) = inp.ltm_revenue {
            r_rev = Some(row);
            input(&mut s, row, "LTM Revenue", m(rev), NF_DOLLAR, "SEC EDGAR / Annual Report");
            row += 1;
        }
        if let Some(ebitda) = inp.ltm_ebitda {
            input(&mut s, row, "LTM EBITDA", m(ebitda), NF_DOLLAR, "yfinance / Company filing");
            row += 1;
        }
        row += 1; // spacer
        if let Some(rr) = r_rev {
            formula(&mut s, row, "EV / LTM Revenue", &format!("={}/{}", d(r_ev), d(rr)), NF_MULT);
            row += 1;
            if inp.computed_market_cap().is_some() {
                formula(&mut s, row, "Market Cap / LTM Revenue",
                    &format!("={}/{}", d(r_mc), d(rr)), NF_MULT);
                row += 1;
            }
        }
        if inp.ltm_ebitda.is_some() {
            // Faithful bug-for-bug port of write_ev_bridge (output_writer.py:596):
            // `r_eb = r_rev + 1 if r_rev else self._row - 1`. With revenue present
            // this correctly points at the LTM EBITDA row; WITHOUT revenue the
            // Python references the spacer row (an inherited latent quirk — fix on
            // the Python side + regenerate the oracle if it ever needs correcting).
            let r_eb = match r_rev {
                Some(rr) => rr + 1,
                None => row - 1,
            };
            formula(&mut s, row, "EV / LTM EBITDA", &format!("={}/{}", d(r_ev), d(r_eb)), NF_MULT);
            row += 1;
        }
    }
    row += 1; // spacer

    // ── Rules Applied ─────────────────────────────────────────────────
    section(&mut s, row, "Rules Applied");
    row += 1;
    for rule in [
        "R-009  EV Bridge — checklist, not template",
        "R-014  Goodwill NOT subtracted from EV",
        "R-015  Pension sourced from NOTES section only (not BS XBRL tag)",
        "R-016  Operating leases from ASC 842 / IFRS 16 footnote",
        "F-001  Shares = latest filing weighted average basic",
    ] {
        line(&mut s, row, &format!("  {rule}"));
        row += 1;
    }

    row += 1; // footer spacer
    line(&mut s, row, generated);

    s
}
