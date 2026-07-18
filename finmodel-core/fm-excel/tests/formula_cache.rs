//! Formula cached results — LibreOffice-friendly polish.
//!
//! Verifies that `Cell.cached` is written into the xlsx via
//! `Formula::set_result`, so offline opens show a number before recalculation.
//! Not part of the snapshot gate (openpyxl data_only=False only sees formulas).

use calamine::{Data, Reader, open_workbook_auto};
use fm_excel::model::{Sheet, Workbook};
use fm_excel::render::render;
use std::path::PathBuf;

#[test]
fn formula_cached_result_lands_in_xlsx() {
    let mut s = Sheet::new("T");
    s.formula_cached(0, 0, "=1+1", 2.0);
    s.formula(1, 0, "=2+2"); // no cache → default 0 in rust_xlsxwriter
    let mut wb = Workbook::new();
    wb.push(s);

    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/snapshots");
    std::fs::create_dir_all(&dir).ok();
    let path = dir.join("formula_cache_smoke.xlsx");
    render(&wb, path.to_str().unwrap()).expect("render");

    // calamine with formulas: we can only see formula strings, not cached results.
    // Inspect the raw sheet XML for the cached <v> node next to the formula.
    let zip = std::fs::read(&path).expect("read xlsx");
    // xlsx is a zip; extract xl/worksheets/sheet1.xml text cheaply.
    let cursor = std::io::Cursor::new(zip);
    let mut archive = zip::ZipArchive::new(cursor).expect("zip");
    let mut sheet_xml = String::new();
    {
        let mut f = archive.by_name("xl/worksheets/sheet1.xml").expect("sheet1");
        use std::io::Read;
        f.read_to_string(&mut sheet_xml).unwrap();
    }
    // Cached cell A1 should carry <f>1+1</f><v>2</v> (or similar).
    assert!(
        sheet_xml.contains("<f") && sheet_xml.contains(">2</v>"),
        "expected cached result 2 in sheet XML, got snippet: {}",
        &sheet_xml.chars().take(400).collect::<String>()
    );

    // Sanity: file still opens.
    let mut xl = open_workbook_auto(&path).expect("open");
    let range = xl.worksheet_range("T").expect("sheet");
    let _ = range.get_value((0, 0)).map(|c| match c {
        Data::Float(n) => *n,
        Data::Int(n) => *n as f64,
        _ => 0.0,
    });
}
