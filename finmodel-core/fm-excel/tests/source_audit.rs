//! Phase 6.3 render gate — the Sources sheet's non-empty source-audit branch.
//! Default (empty) parity is covered by `snapshot_parity`; this exercises the
//! rows/headers/folded-metadata/verification-column/section-spacing path that
//! the snapshots can never reach.

use fm_excel::input::SourceAuditRow;
use fm_excel::model::{Value, LABEL};
use fm_excel::sheets::sources;
use fm_excel::snapshot::{load_snapshot, workbook_input_from_snapshot};

fn snapshot_path(name: &str) -> String {
    format!(
        "{}/../../tieout/excel_snapshots/{}_snapshot.json",
        env!("CARGO_MANIFEST_DIR"),
        name
    )
}

fn text_at(s: &fm_excel::model::Sheet, r: u32, c: u32) -> Option<String> {
    match s.cells.get(&(r, c)).and_then(|cell| cell.value.as_ref()) {
        Some(Value::Text(t)) => Some(t.clone()),
        _ => None,
    }
}

fn base_input() -> fm_excel::input::WorkbookInput {
    let snap = load_snapshot(&snapshot_path("SAND_ST")).expect("load snapshot");
    workbook_input_from_snapshot(&snap).expect("build input")
}

#[test]
fn empty_audit_keeps_five_column_header_and_snapshot_layout() {
    let input = base_input();
    assert!(input.source_audit.is_empty());
    let s = sources::build(&input);
    // No 6th "Verification" header column.
    assert_eq!(text_at(&s, 7, LABEL + 5), None);
    // Body empty → the verification section stays at the snapshot row 10.
    assert_eq!(
        text_at(&s, 10, LABEL).as_deref(),
        Some("VERIFICATION REPORT")
    );
    assert!(text_at(&s, 11, LABEL)
        .map(|t| t.starts_with("Status:"))
        .unwrap_or(false));
}

#[test]
fn populated_audit_renders_rows_metadata_and_verification_column() {
    let mut input = base_input();
    input.source_audit = vec![
        SourceAuditRow {
            line_item: "revenue_growth_pct".into(),
            period: "2025".into(),
            value: "0.2".into(),
            origin: "research".into(),
            detail: "https://example.com/10-K".into(),
            retrieved: "2026-01-01".into(),
            evidence: "S1, S2".into(),
            confidence: "0.9".into(),
            verification: "validated".into(),
        },
        SourceAuditRow {
            line_item: "gross_margin_pct".into(),
            period: "2026".into(),
            value: "0.35".into(),
            origin: "research".into(),
            detail: String::new(),
            retrieved: String::new(),
            evidence: "S3".into(),
            confidence: String::new(),
            verification: "unverified".into(),
        },
    ];
    let s = sources::build(&input);

    // 6th header column appears only because rows exist.
    assert_eq!(text_at(&s, 7, LABEL + 5).as_deref(), Some("Verification"));

    // Row 1 at body row 8: line item / period / value in their columns.
    assert_eq!(text_at(&s, 8, LABEL).as_deref(), Some("revenue_growth_pct"));
    assert_eq!(text_at(&s, 8, LABEL + 1).as_deref(), Some("2025"));
    assert_eq!(text_at(&s, 8, LABEL + 2).as_deref(), Some("0.2"));
    // Origin folds tag/URL + retrieval time into one cell.
    assert_eq!(
        text_at(&s, 8, LABEL + 3).as_deref(),
        Some("research: https://example.com/10-K @ 2026-01-01")
    );
    // Confidence column carries confidence · evidence.
    assert_eq!(text_at(&s, 8, LABEL + 4).as_deref(), Some("0.9 · S1, S2"));
    assert_eq!(text_at(&s, 8, LABEL + 5).as_deref(), Some("validated"));

    // Row 2 at body row 9: no detail/retrieved → bare origin; evidence-only conf.
    assert_eq!(text_at(&s, 9, LABEL + 3).as_deref(), Some("research"));
    assert_eq!(text_at(&s, 9, LABEL + 4).as_deref(), Some("S3"));
    assert_eq!(text_at(&s, 9, LABEL + 5).as_deref(), Some("unverified"));

    // Verification section is pushed below the 2 rows + a blank gap → row 12.
    assert_eq!(
        text_at(&s, 12, LABEL).as_deref(),
        Some("VERIFICATION REPORT")
    );
    assert!(text_at(&s, 13, LABEL)
        .map(|t| t.starts_with("Status:"))
        .unwrap_or(false));
}
