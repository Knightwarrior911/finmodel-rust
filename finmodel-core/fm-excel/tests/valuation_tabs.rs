//! Valuation tabs smoke gate — proves WACC/DCF/Sensitivities emit when
//! WorkbookInput carries valuation outputs, and stay absent for the snapshot
//! path (dcf/wacc = None).

use fm_excel::is_structure::build_is_structure;
use fm_excel::model::Value;
use fm_excel::sheets::build_workbook;
use fm_excel::snapshot::{load_snapshot, workbook_input_from_snapshot};
use fm_value::{compute_dcf, compute_wacc, fallback_peer_set, DCFAssumptions, DCFScenario};

fn path(name: &str) -> String {
    format!(
        "{}/../../tieout/excel_snapshots/{}_snapshot.json",
        env!("CARGO_MANIFEST_DIR"),
        name
    )
}

#[test]
fn snapshot_path_has_no_valuation_tabs() {
    let snap = load_snapshot(&path("SAND_ST")).expect("snap");
    let input = workbook_input_from_snapshot(&snap).expect("input");
    assert!(input.dcf.is_none() && input.wacc.is_none());
    let wb = build_workbook(&input);
    let names: Vec<&str> = wb.sheets.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        ["Cover", "Assumptions", "IS", "BS", "CF", "Sources"]
    );
}

#[test]
fn valuation_tabs_emit_with_cross_links() {
    let snap = load_snapshot(&path("SAND_ST")).expect("snap");
    let mut input = workbook_input_from_snapshot(&snap).expect("input");
    input.is_structure = build_is_structure("standard", true, true, true);

    let shares = input.assumptions.shares_diluted.max(1.0);
    let mkt_cap = input.assumptions.current_share_price.max(1.0) * shares;
    let debt = input
        .model
        .balance_sheet
        .get("long_term_debt")
        .and_then(|v| v.iter().rev().find_map(|x| *x))
        .unwrap_or(0.0);
    let tax = input
        .assumptions
        .base
        .tax_rate_pct
        .first()
        .copied()
        .unwrap_or(0.21);
    let peer_set = fallback_peer_set(&input.meta.ticker, mkt_cap, input.assumptions.target_de_ratio);
    let wacc = compute_wacc(
        &peer_set,
        mkt_cap,
        debt,
        input.assumptions.risk_free_rate,
        input.assumptions.equity_risk_premium,
        input.assumptions.cost_of_debt_pretax,
        tax,
        Some(input.assumptions.target_de_ratio),
        1.0,
    );
    let dcf_asmp = DCFAssumptions {
        mid_year_convention: input.assumptions.mid_year_convention,
        current_share_price: input.assumptions.current_share_price,
        shares_diluted: shares,
        active: DCFScenario {
            terminal_growth_rate: input.assumptions.base.terminal_growth_rate,
            exit_ebitda_multiple: input.assumptions.base.exit_ebitda_multiple,
        },
    };
    let dcf = compute_dcf(
        &input.model.periods,
        &input.model.income_statement,
        &input.model.balance_sheet,
        &input.model.cash_flow_statement,
        &input.meta.ticker,
        &wacc,
        &dcf_asmp,
        1,
    );
    input.wacc = Some(wacc);
    input.peer_source = peer_set.source;
    input.dcf = Some(dcf);

    let wb = build_workbook(&input);
    let names: Vec<&str> = wb.sheets.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Cover",
            "Assumptions",
            "IS",
            "BS",
            "CF",
            "DCF",
            "WACC",
            "Sensitivities",
            "Sources"
        ]
    );

    // Cover valuation summary is live (not the placeholder).
    let cover = wb.sheet("Cover").unwrap();
    let has_placeholder = cover.cells.values().any(|c| {
        matches!(&c.value, Some(Value::Text(t)) if t.contains("not yet built"))
    });
    assert!(!has_placeholder, "Cover still shows DCF placeholder");
    let has_implied = cover.cells.values().any(|c| {
        matches!(&c.value, Some(Value::Text(t)) if t == "DCF Implied Price")
    });
    assert!(has_implied, "Cover missing DCF Implied Price label");

    // DCF links to WACC single source of truth.
    let dcf_sheet = wb.sheet("DCF").unwrap();
    let has_wacc_link = dcf_sheet.cells.values().any(|c| {
        c.formula
            .as_deref()
            .map(|f| f.contains("WACC!"))
            .unwrap_or(false)
    });
    assert!(has_wacc_link, "DCF missing WACC! cross-links");

    // WACC links to Assumptions shared inputs.
    let wacc_sheet = wb.sheet("WACC").unwrap();
    let has_asmp_link = wacc_sheet.cells.values().any(|c| {
        c.formula
            .as_deref()
            .map(|f| f.contains("Assumptions!"))
            .unwrap_or(false)
    });
    assert!(has_asmp_link, "WACC missing Assumptions! cross-links");

    // Sensitivities reference DCF cells.
    let sens = wb.sheet("Sensitivities").unwrap();
    let has_dcf_link = sens.cells.values().any(|c| {
        c.formula
            .as_deref()
            .map(|f| f.contains("DCF!"))
            .unwrap_or(false)
    });
    assert!(has_dcf_link, "Sensitivities missing DCF! cross-links");
}
