//! 6.1 parity gate — `fm_pptx::inspect::inspect_pptx` reproduces the Python
//! `pptx_inspector.inspect_pptx_json` oracle over the committed fixture deck.
//!
//! Oracle: `py tieout/build_pptx_oracle.py` writes
//! `tests/fixtures/pptx/deck.pptx` (the exact deck built by
//! `tests/test_pptx_editor.py`) and `tieout/excel_snapshots/PPTX_inspect_deck.json`.
//! `path`/`fileSizeBytes` are environment-dependent and excluded.

mod common;

#[test]
fn inspect_reproduces_oracle_deck() {
    let deck = format!("{}/deck.pptx", common::fixture_dir());
    let got = fm_pptx::inspect::inspect_pptx(&deck).expect("inspect deck");
    let want = common::load_json(&format!("{}/PPTX_inspect_deck.json", common::snap_dir()));

    let diffs = common::diff_json(&got, &want, &["$.path", "$.fileSizeBytes"]);
    if !diffs.is_empty() {
        let shown: Vec<String> = diffs.iter().take(50).cloned().collect();
        panic!(
            "{} structural diff(s) vs PPTX_inspect_deck oracle:\n{}",
            diffs.len(),
            shown.join("\n")
        );
    }
}
