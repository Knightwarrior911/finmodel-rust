//! Dev utility: render the SAND_ST full-IS workbook to a fixed path so the
//! Python format-diff (`tieout/diff_formats.py`) can compare visual finish
//! against the writer.py oracle. Not a gate — it just materializes the file.

use fm_excel::is_structure::build_is_structure;
use fm_excel::render::render;
use fm_excel::sheets::build_workbook;
use fm_excel::snapshot::{load_snapshot, workbook_input_from_snapshot};

fn snap_path(name: &str) -> String {
    format!(
        "{}/../../tieout/excel_snapshots/{}_snapshot.json",
        env!("CARGO_MANIFEST_DIR"),
        name
    )
}

fn nonzero(mo: &serde_json::Value, key: &str) -> bool {
    mo["income_statement"][key]
        .as_array()
        .map(|a| a.iter().any(|v| v.as_f64().map(|n| n != 0.0).unwrap_or(false)))
        .unwrap_or(false)
}

#[test]
fn dump_sand_full_xlsx() {
    let snap = load_snapshot(&snap_path("SAND_ST")).expect("snapshot");
    let mut input = workbook_input_from_snapshot(&snap).expect("input");
    let mo = &snap["model_output"];
    input.is_structure =
        build_is_structure("standard", nonzero(mo, "cogs"), nonzero(mo, "rd"), nonzero(mo, "sga"));
    let wb = build_workbook(&input);

    let out = format!("{}/../../tests/snapshots/SAND_ST_rust.xlsx", env!("CARGO_MANIFEST_DIR"));
    std::fs::create_dir_all(format!("{}/../../tests/snapshots", env!("CARGO_MANIFEST_DIR"))).ok();
    render(&wb, &out).expect("render");
    assert!(std::path::Path::new(&out).exists());
}
