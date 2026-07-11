//! Sources & Audit Trail tab. Mirrors writer.py `_write_sources`.

use crate::input::WorkbookInput;
use crate::model::{Sheet, DATA0, LABEL, RED};

pub fn build(input: &WorkbookInput) -> Sheet {
    let mut s = Sheet::new("Sources");
    let m = &input.meta;
    let v = &input.verification;

    // Header block (single cells at col C, per snapshot).
    s.title(2, m.company.clone());
    s.text(4, LABEL, "Sources & Audit Trail");
    s.text(5, LABEL, format!("({} $ in millions)", m.currency));

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
    }

    // Verification report. Sources table body is empty in the snapshot config;
    // Python starts r=8, writes no line-item rows, then r+=2 -> r=10.
    let mut r = 10u32;
    s.section(r, "VERIFICATION REPORT");
    r += 1;
    let status = if v.passed { "PASSED ✓" } else { "FAILED ✗" };
    s.text(r, LABEL, format!("Status: {}", status));
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
