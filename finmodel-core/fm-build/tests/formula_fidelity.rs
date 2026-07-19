//! Formula-fidelity audit (P0 #3): a workbook an analyst would trust is
//! formula-driven — projected periods derive from drivers via live Excel
//! formulas, not pasted numbers. This gate measures the REAL rendered xlsx:
//! it unzips the sheet XML and counts `<f>` formula cells against plain
//! numeric cells, per sheet, and fails if projections are value-dumps.

use std::collections::HashMap;
use std::io::Read;

use fm_extract::ExtractionResult;
use fm_types::StatementData;

fn fixture_extraction() -> ExtractionResult {
    let mut is = StatementData::new();
    for (k, v) in [
        ("revenue", vec![Some(100.0), Some(110.0), Some(121.0)]),
        ("cogs", vec![Some(40.0), Some(44.0), Some(48.4)]),
        ("net_income", vec![Some(10.0), Some(12.0), Some(14.0)]),
        ("income_tax", vec![Some(3.0), Some(4.0), Some(5.0)]),
        ("rd", vec![Some(5.0), Some(6.0), Some(7.0)]),
        ("sga", vec![Some(8.0), Some(9.0), Some(10.0)]),
    ] {
        is.insert(k.into(), v);
    }
    let mut bs = StatementData::new();
    for (k, v) in [
        ("cash", vec![Some(20.0), Some(25.0), Some(30.0)]),
        ("total_assets", vec![Some(200.0), Some(220.0), Some(242.0)]),
        ("total_equity", vec![Some(120.0), Some(130.0), Some(140.0)]),
    ] {
        bs.insert(k.into(), v);
    }
    let mut cf = StatementData::new();
    for (k, v) in [
        ("cfo", vec![Some(15.0), Some(17.0), Some(19.0)]),
        ("capex", vec![Some(-6.0), Some(-7.0), Some(-8.0)]),
    ] {
        cf.insert(k.into(), v);
    }
    ExtractionResult {
        currency: "USD".into(),
        years_found: vec!["2022".into(), "2023".into(), "2024".into()],
        income_statement: is,
        balance_sheet: bs,
        cash_flow_statement: cf,
        notes: HashMap::new(),
        confidence: 1.0,
        discrepancies: vec![],
    }
}

/// Per-sheet cell census from the raw xlsx XML: (formula_cells, number_only_cells).
fn census(path: &std::path::Path) -> Vec<(String, usize, usize)> {
    let bytes = std::fs::read(path).expect("read xlsx");
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("zip");
    // Sheet names in workbook order.
    let mut wb_xml = String::new();
    archive
        .by_name("xl/workbook.xml")
        .expect("workbook.xml")
        .read_to_string(&mut wb_xml)
        .unwrap();
    let names: Vec<String> = wb_xml
        .split("<sheet ")
        .skip(1)
        .filter_map(|s| {
            s.split("name=\"").nth(1).and_then(|r| r.split('"').next()).map(String::from)
        })
        .collect();
    let mut out = Vec::new();
    for (i, name) in names.iter().enumerate() {
        let mut xml = String::new();
        let member = format!("xl/worksheets/sheet{}.xml", i + 1);
        archive
            .by_name(&member)
            .unwrap_or_else(|_| panic!("{member}"))
            .read_to_string(&mut xml)
            .unwrap();
        // A formula cell contains <f>…</f>; a numeric value cell has <v> with
        // no <f> in the same <c> element.
        let mut formulas = 0usize;
        let mut numbers = 0usize;
        for cell in xml.split("<c ").skip(1) {
            let cell = cell.split("</c>").next().unwrap_or(cell);
            let has_f = cell.contains("<f>") || cell.contains("<f ");
            let has_v = cell.contains("<v>");
            // Skip shared-string (text) cells: t="s".
            let is_text = cell.starts_with("t=\"s\"")
                || cell.contains(" t=\"s\"")
                || cell.contains("t=\"str\"") && !has_f;
            if has_f {
                formulas += 1;
            } else if has_v && !is_text {
                numbers += 1;
            }
        }
        out.push((name.clone(), formulas, numbers));
    }
    out
}

#[test]
fn projected_periods_are_live_formulas_not_value_dumps() {
    let out = fm_build::build(&fixture_extraction(), "AUDIT", 5);
    let dir = std::env::temp_dir().join("fm-formula-audit");
    std::fs::create_dir_all(&dir).ok();
    let path = dir.join("audit_model.xlsx");
    fm_excel::render::render(&out.workbook, path.to_str().unwrap()).expect("render");

    let sheets = census(&path);
    let mut total_f = 0usize;
    let mut total_n = 0usize;
    for (name, f, n) in &sheets {
        println!("sheet {name}: {f} formula cells, {n} hardcoded numeric cells");
        total_f += f;
        total_n += n;
    }
    let _ = std::fs::remove_file(&path);

    // The workbook as a whole must be meaningfully formula-driven.
    assert!(total_f > 0, "workbook contains zero live formulas");
    let ratio = total_f as f64 / (total_f + total_n).max(1) as f64;
    println!(
        "TOTAL: {total_f} formulas vs {total_n} hardcoded numbers ({:.0}% formula-driven)",
        ratio * 100.0
    );
    // Historical actuals are legitimately hardcoded (they are reported facts);
    // a five-year projection on top of three actual years should still push
    // the overall formula share well above a token presence.
    assert!(
        ratio >= 0.30,
        "only {:.0}% of populated numeric cells are formulas — the projection \
         is a value-dump, not a model an analyst can audit",
        ratio * 100.0
    );
}
