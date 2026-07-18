//! 6.5 shape-primitive gate — zip+XML move/resize/restyle/add/delete/table ops,
//! verified behaviourally via the inspector (apply -> inspect -> assert), the
//! 6.2 round-trip pattern.

mod common;

use serde_json::Value;

fn temp_from_fixture(tag: &str) -> String {
    let src = format!("{}/deck.pptx", common::fixture_dir());
    let dst = std::env::temp_dir().join(format!("fmpptx_sp_{tag}_{}.pptx", std::process::id()));
    std::fs::copy(&src, &dst).expect("copy fixture");
    dst.to_string_lossy().into_owned()
}

fn slide0_elem(deck: &str, name: &str) -> Value {
    let js = fm_pptx::inspect::inspect_pptx(deck).expect("inspect");
    let els = js["slides"][0]["elements"].as_array().unwrap().clone();
    els.into_iter()
        .find(|e| e["name"] == name)
        .unwrap_or(Value::Null)
}

fn cleanup(deck: &str) {
    let _ = std::fs::remove_file(deck);
    let _ = std::fs::remove_file(format!("{deck}.edit_log.jsonl"));
}

#[test]
fn move_shape_updates_position() {
    let deck = temp_from_fixture("move");
    // deck slide 0 has textbox "Title" (id 2) at (1,1).
    fm_pptx::edit::move_shape(
        &deck,
        0,
        Some(2),
        None,
        Some(2.5),
        Some(3.0),
        None,
        None,
        None,
    )
    .unwrap();
    let el = slide0_elem(&deck, "Title");
    assert!(
        (el["pos"]["left"].as_f64().unwrap() - 2.5).abs() < 1e-3,
        "left {}",
        el["pos"]["left"]
    );
    assert!(
        (el["pos"]["top"].as_f64().unwrap() - 3.0).abs() < 1e-3,
        "top {}",
        el["pos"]["top"]
    );
    cleanup(&deck);
}

#[test]
fn resize_shape_updates_extent() {
    let deck = temp_from_fixture("resize");
    fm_pptx::edit::resize_shape(&deck, 0, None, Some("Title"), Some(4.0), Some(2.0), None).unwrap();
    let el = slide0_elem(&deck, "Title");
    assert!((el["pos"]["width"].as_f64().unwrap() - 4.0).abs() < 1e-3);
    assert!((el["pos"]["height"].as_f64().unwrap() - 2.0).abs() < 1e-3);
    cleanup(&deck);
}

#[test]
fn set_shape_fill_sets_solid_color() {
    let deck = temp_from_fixture("fill");
    fm_pptx::edit::set_shape_fill(&deck, 0, Some(2), None, Some("#255BE3"), false, None).unwrap();
    let el = slide0_elem(&deck, "Title");
    assert_eq!(el["fill"]["type"], "solid");
    assert_eq!(el["fill"]["rgb"], "#255BE3");
    cleanup(&deck);
}

#[test]
fn delete_shape_removes_it() {
    let deck = temp_from_fixture("delete");
    let before = fm_pptx::inspect::inspect_pptx(&deck).unwrap()["slides"][0]["elementCount"]
        .as_u64()
        .unwrap();
    fm_pptx::edit::delete_shape(&deck, 0, Some(2), None, None).unwrap();
    let after = fm_pptx::inspect::inspect_pptx(&deck).unwrap()["slides"][0]["elementCount"]
        .as_u64()
        .unwrap();
    assert_eq!(after, before - 1);
    cleanup(&deck);
}

#[test]
fn add_textbox_appends_shape() {
    let deck = temp_from_fixture("add");
    fm_pptx::edit::add_textbox(
        &deck,
        0,
        1.0,
        5.0,
        3.0,
        0.5,
        "Injected note",
        Some("Note"),
        None,
    )
    .unwrap();
    let el = slide0_elem(&deck, "Note");
    assert_eq!(el["type"], "TEXT_BOX (17)");
    assert_eq!(el["text"]["text"], "Injected note");
    cleanup(&deck);
}

// ── table ops on a synthetic table deck ───────────────────────────────────────

const TABLE_SHAPE: &str = "<p:graphicFrame xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\">\
<p:nvGraphicFramePr><p:cNvPr id=\"2\" name=\"Table 1\"/><p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr>\
<p:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"3000000\" cy=\"1000000\"/></p:xfrm>\
<a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/table\">\
<a:tbl><a:tblPr/><a:tblGrid><a:gridCol w=\"1500000\"/><a:gridCol w=\"1500000\"/></a:tblGrid>\
<a:tr h=\"500000\"><a:tc><a:txBody><a:bodyPr/><a:p><a:r><a:t>A1</a:t></a:r></a:p></a:txBody><a:tcPr/></a:tc>\
<a:tc><a:txBody><a:bodyPr/><a:p><a:r><a:t>B1</a:t></a:r></a:p></a:txBody><a:tcPr/></a:tc></a:tr>\
<a:tr h=\"500000\"><a:tc><a:txBody><a:bodyPr/><a:p><a:r><a:t>A2</a:t></a:r></a:p></a:txBody><a:tcPr/></a:tc>\
<a:tc><a:txBody><a:bodyPr/><a:p><a:r><a:t>B2</a:t></a:r></a:p></a:txBody><a:tcPr/></a:tc></a:tr>\
</a:tbl></a:graphicData></a:graphic></p:graphicFrame>";

fn table_deck(tag: &str) -> String {
    let pkg =
        fm_pptx::writer::pkgbuild::build_package(13.333, 7.5, &[vec![TABLE_SHAPE.to_string()]]);
    let dst = std::env::temp_dir().join(format!("fmpptx_tbl_{tag}_{}.pptx", std::process::id()));
    let dst = dst.to_string_lossy().into_owned();
    pkg.write(&dst).unwrap();
    dst
}

fn cell_texts(deck: &str) -> Vec<Vec<String>> {
    let js = fm_pptx::inspect::inspect_pptx(deck).unwrap();
    let tbl = js["slides"][0]["elements"][0]["table"].clone();
    tbl["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            r["cells"]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| c["text"]["text"].as_str().unwrap_or("").to_string())
                .collect()
        })
        .collect()
}

#[test]
fn swap_table_columns_reorders_cells() {
    let deck = table_deck("cols");
    assert_eq!(cell_texts(&deck), vec![vec!["A1", "B1"], vec!["A2", "B2"]]);
    fm_pptx::edit::swap_table_columns(&deck, 0, Some(2), None, 0, 1, None).unwrap();
    assert_eq!(cell_texts(&deck), vec![vec!["B1", "A1"], vec!["B2", "A2"]]);
    let _ = std::fs::remove_file(&deck);
    let _ = std::fs::remove_file(format!("{deck}.edit_log.jsonl"));
}

#[test]
fn swap_table_rows_reorders_rows() {
    let deck = table_deck("rows");
    fm_pptx::edit::swap_table_rows(&deck, 0, None, Some("Table 1"), 0, 1, None).unwrap();
    assert_eq!(cell_texts(&deck), vec![vec!["A2", "B2"], vec!["A1", "B1"]]);
    let _ = std::fs::remove_file(&deck);
    let _ = std::fs::remove_file(format!("{deck}.edit_log.jsonl"));
}
