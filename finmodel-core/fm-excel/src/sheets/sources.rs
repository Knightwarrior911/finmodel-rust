//! Sources & Audit Trail tab. Mirrors writer.py `_write_sources`.

use crate::input::WorkbookInput;
use crate::model::{DATA0, LABEL, RED, Sheet};

pub fn build(input: &WorkbookInput) -> Sheet {
    let mut s = Sheet::new("Sources");
    let m = &input.meta;
    let v = &input.verification;

    // Header block (single cells at col C, per snapshot).
    s.title(2, m.company.clone());
    s.text(4, LABEL, "Sources & Audit Trail");
    s.cell_mut(4, LABEL).font_hex = Some(crate::sheets::NAVY);
    s.cell_mut(4, LABEL).bold = true;
    s.text(5, LABEL, format!("({} $ in millions)", m.currency));
    s.cell_mut(5, LABEL).font_hex = Some(crate::sheets::GRAY);
    s.cell_mut(5, LABEL).italic = true;

    // Column headers at row 7, starting col LABEL.
    let headers = [
        "Line Item",
        "Period",
        "Value ($M)",
        "Filing / XBRL Tag",
        "Confidence",
    ];
    for (j, h) in headers.iter().enumerate() {
        s.text(7, LABEL + j as u32, *h);
        s.cell_mut(7, LABEL + j as u32).bold = true;
    }
    // The audit's own per-row status is a 6th column, added ONLY when the body
    // carries rows — default (empty) builds keep the 5-column snapshot header.
    if !input.source_audit.is_empty() {
        s.text(7, LABEL + 5, "Verification");
        s.cell_mut(7, LABEL + 5).bold = true;
    }

    // Source-audit body (Phase 6.3). Empty by default → byte-identical to the
    // committed snapshots (Python starts r=8, writes no rows, then r+=2 → r=10).
    let mut r = 8u32;
    for row in &input.source_audit {
        s.text(r, LABEL, row.line_item.clone());
        s.text(r, LABEL + 1, row.period.clone());
        s.text(r, LABEL + 2, row.value.clone());
        // Origin + tag/URL + retrieval time fold into the "Filing / XBRL Tag"
        // column; evidence (research S#) and confidence share the Confidence
        // column; the per-row status is the 6th column.
        let mut origin = row.origin.clone();
        if !row.detail.is_empty() {
            origin = format!("{origin}: {}", row.detail);
        }
        if !row.retrieved.is_empty() {
            origin = format!("{origin} @ {}", row.retrieved);
        }
        s.text(r, LABEL + 3, origin);
        let conf = match (row.evidence.as_str(), row.confidence.as_str()) {
            ("", "") => String::new(),
            (e, "") => e.to_string(),
            ("", c) => c.to_string(),
            (e, c) => format!("{c} · {e}"),
        };
        s.text(r, LABEL + 4, conf);
        if !row.verification.is_empty() {
            s.text(r, LABEL + 5, row.verification.clone());
        }
        r += 1;
    }
    // Match the snapshot's blank-line gap before the verification section.
    let mut r = if input.source_audit.is_empty() {
        10
    } else {
        r + 2
    };
    s.section(r, "VERIFICATION REPORT");
    r += 1;
    let status = if v.passed { "PASSED ✓" } else { "FAILED ✗" };
    s.text(r, LABEL, format!("Status: {}", status));
    s.cell_mut(r, LABEL).bold = true;
    r += 1;

    if !v.critical_failures.is_empty() {
        s.text(r, LABEL, "Critical Failures:");
        r += 1;
        for cf in &v.critical_failures {
            s.text(r, DATA0, cf.clone());
            s.fill(r, DATA0, RED);
            r += 1;
        }
    }
    if !v.warnings.is_empty() {
        s.text(r, LABEL, "Warnings:");
        r += 1;
        for w in &v.warnings {
            s.text(r, DATA0, w.clone());
            r += 1;
        }
    }
    if !v.notes.is_empty() {
        s.text(r, LABEL, "Notes:");
        r += 1;
        for n in &v.notes {
            s.text(r, DATA0, n.clone());
            r += 1;
        }
    }
    // input.model.plug_used is always false here; skip the plug line.

    s
}
