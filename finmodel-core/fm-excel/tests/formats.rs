//! Number-format assignment (product polish; not covered by the snapshot gate,
//! which is blind to number formats). Verifies percent vs number formats land on
//! the right cells, mirroring writer.py's `_Fmt` per-row assignment.

use fm_excel::model::{Cell, FMT_NUM, FMT_PCT, Sheet, fmt_per_share, parse_ref};
use fm_excel::sheets::build_workbook;
use fm_excel::snapshot::{load_snapshot, workbook_input_from_snapshot};

fn snapshot_path(name: &str) -> String {
    format!(
        "{}/../../tieout/excel_snapshots/{}_snapshot.json",
        env!("CARGO_MANIFEST_DIR"),
        name
    )
}

fn cell<'a>(s: &'a Sheet, reference: &str) -> &'a Cell {
    let (r, c) = parse_ref(reference).expect("ref");
    s.cells
        .get(&(r, c))
        .unwrap_or_else(|| panic!("no cell {reference}"))
}

#[test]
fn number_formats_are_assigned() {
    let snap = load_snapshot(&snapshot_path("SAND_ST")).expect("load");
    let input = workbook_input_from_snapshot(&snap).expect("input");
    let wb = build_workbook(&input);
    let ps = fmt_per_share(&input.meta.currency);

    let asmp = wb.sheet("Assumptions").expect("Assumptions");
    // Percent drivers.
    assert_eq!(
        cell(asmp, "D33").num_fmt,
        Some(FMT_PCT),
        "base rev-growth % "
    );
    assert_eq!(
        cell(asmp, "D34").num_fmt,
        Some(FMT_PCT),
        "base gross-margin %"
    );
    assert_eq!(
        cell(asmp, "D15").num_fmt,
        Some(FMT_PCT),
        "active rev-growth CHOOSE %"
    );
    // Number drivers (days / dividend / exit multiple).
    assert_eq!(cell(asmp, "D41").num_fmt, Some(FMT_NUM), "DSO days");
    assert_eq!(cell(asmp, "D46").num_fmt, Some(FMT_NUM), "exit multiple");
    assert_eq!(
        cell(asmp, "D26").num_fmt,
        Some(ps),
        "active dividend/share (per-share, cents)"
    );
    // Shared inputs: rf/erp/kd pct; de/price/shares num.
    assert_eq!(cell(asmp, "D86").num_fmt, Some(FMT_PCT), "risk-free %");
    assert_eq!(cell(asmp, "D88").num_fmt, Some(FMT_NUM), "target D/E (num)");
    assert_eq!(
        cell(asmp, "D90").num_fmt,
        Some(ps),
        "current share price (per-share, cents)"
    );

    // BS: monetary default, interest-rate schedule row is percent.
    let bs = wb.sheet("BS").expect("BS");
    assert_eq!(cell(bs, "D12").num_fmt, Some(FMT_NUM), "cash");
    assert_eq!(
        cell(bs, "F19").num_fmt,
        Some(FMT_NUM),
        "total assets formula"
    );
    assert_eq!(
        cell(bs, "F58").num_fmt,
        Some(FMT_PCT),
        "debt interest-rate %"
    );

    // CF: monetary default, CapEx%-of-revenue driver row is percent.
    let cf = wb.sheet("CF").expect("CF");
    assert_eq!(cell(cf, "F24").num_fmt, Some(FMT_PCT), "capex % driver");
    assert_eq!(cell(cf, "D12").num_fmt, Some(FMT_NUM), "net income link");
}

#[test]
fn per_share_and_price_cells_show_cents_across_valuation_sheets() {
    use fm_excel::input::{AssumptionsBlock, Meta, ModelOutput, Verification, WorkbookInput};
    use fm_excel::model::{DATA0, LABEL, Value, fmt_per_share};
    use fm_excel::sheets::{comps_peers, comps_summary, cover, dcf, sensitivities};
    use fm_value::{DCFOutput, PublicCompPeer, PublicCompsOutput, WACCOutput};

    let ps = fmt_per_share("USD"); // $#,##0.00 …
    assert_ne!(ps, FMT_NUM, "per-share must differ from the integer default");

    // A row's first stamped number-format, located by a LABEL-cell substring.
    fn row_fmt(s: &Sheet, needle: &str) -> &'static str {
        let row = s
            .cells
            .iter()
            .filter_map(|((r, c), cell)| match &cell.value {
                Some(Value::Text(t)) if *c == LABEL && t.contains(needle) => Some(*r),
                _ => None,
            })
            .min()
            .unwrap_or_else(|| panic!("row '{needle}' not found"));
        s.cells
            .iter()
            .filter_map(|((r, _), cell)| if *r == row { cell.num_fmt } else { None })
            .next()
            .unwrap_or_else(|| panic!("row '{needle}' has no stamped format"))
    }
    let at = |s: &Sheet, r: u32, c: u32| {
        s.cells
            .get(&(r, c))
            .and_then(|x| x.num_fmt)
            .unwrap_or_else(|| panic!("no stamped cell at ({r},{c})"))
    };

    // ── DCF + Sensitivities + Cover: USD valuation fixture. ──
    let dcf_out = DCFOutput {
        proj_periods: vec!["2025E".into(), "2026E".into()],
        wacc_range: vec![0.09, 0.10, 0.11],
        ebitda_multiple_range: vec![9.0, 10.0, 11.0],
        gordon_growth_range: vec![0.02, 0.025, 0.03],
        sensitivity_ebitda: vec![vec![150.0; 3]; 3],
        sensitivity_gordon: vec![vec![150.0; 3]; 3],
        implied_price: 187.42,
        current_share_price: 150.10,
        enterprise_value: 1000.0,
        equity_value: 900.0,
        shares_diluted: 5.0,
        ..Default::default()
    };
    let input = WorkbookInput {
        meta: Meta {
            company: "TestCo".into(),
            ticker: "T".into(),
            currency: "USD".into(),
            fiscal_year_end: "Dec".into(),
            sector: String::new(),
            as_of: "2026-07-21".into(),
        },
        model: ModelOutput {
            periods: vec!["2023A".into(), "2024A".into(), "2025E".into(), "2026E".into()],
            ..Default::default()
        },
        assumptions: AssumptionsBlock::default(),
        verification: Verification::default(),
        is_structure: vec![],
        wacc: Some(WACCOutput::default()),
        peer_source: String::new(),
        dcf: Some(dcf_out),
        public_comps: None,
        source_audit: vec![],
    };

    let d = dcf::build(&input);
    assert_eq!(row_fmt(&d, "Implied Share Price"), ps, "DCF implied price → cents");
    assert_eq!(row_fmt(&d, "Current Share Price"), ps, "DCF current price → cents");
    // Both DCF inline matrices: first result cell of the Exit-Multiple table
    // (row SENS1_COL_HDR+1=70) and the Terminal-Growth table (SENS2_COL_HDR+1=78).
    assert_eq!(at(&d, 70, DATA0), ps, "DCF exit-multiple matrix → cents");
    assert_eq!(at(&d, 78, DATA0), ps, "DCF terminal-growth matrix → cents");

    // Both standalone Sensitivities tables: WACC×growth (TBL1_START=11) and
    // WACC×exit-multiple (TBL2_START=21).
    let sens = sensitivities::build(&input);
    assert_eq!(at(&sens, 11, DATA0), ps, "sensitivity table 1 → cents");
    assert_eq!(at(&sens, 21, DATA0), ps, "sensitivity table 2 → cents");

    let cov = cover::build(&input);
    assert_eq!(row_fmt(&cov, "Current Share Price"), ps, "Cover current price → cents");
    assert_eq!(row_fmt(&cov, "DCF Implied Price"), ps, "Cover implied price → cents");

    // ── Comps Peers + Summary: USD-normalized market data. ──
    let pc = PublicCompsOutput {
        target_company_name: "TestCo".into(),
        target_ebitda: 500.0,
        implied_price_low: 120.25,
        implied_price_median: 150.50,
        implied_price_high: 180.75,
        peers: vec![PublicCompPeer {
            ticker: "PEER".into(),
            share_price: 42.37,
            week52_high: 55.10,
            week52_low: 30.05,
            market_cap: 8000.0,
            ltm_eps_diluted: 3.14,
            ..Default::default()
        }],
        ..Default::default()
    };
    // Peer row 8: LABEL+2 price, LABEL+14 LTM EPS → cents; LABEL+6 market cap ($M) stays integer.
    let peers = comps_peers::build_from(&pc);
    assert_eq!(at(&peers, 8, LABEL + 2), ps, "peer price → cents");
    assert_eq!(at(&peers, 8, LABEL + 14), ps, "peer LTM EPS → cents");
    assert_eq!(at(&peers, 8, LABEL + 6), FMT_NUM, "peer market cap ($M) stays integer");

    let summ = comps_summary::build_from(&pc);
    assert_eq!(at(&summ, 20, DATA0), ps, "implied per-share price → cents");
    assert_eq!(at(&summ, 16, DATA0), FMT_NUM, "target EBITDA ($M) stays integer");
}
