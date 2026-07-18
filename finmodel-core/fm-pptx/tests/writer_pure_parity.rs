//! 6.3 parity gate — `fm_pptx::writer` pure functions reproduce the Python
//! `pptx_writer` originals over the oracle's input matrix (`PPTX_pure.json`).

mod common;

use fm_pptx::writer::{
    FmtValue, fmt_to_numfmt, format_value, normalize_heading, pick_slide_archetype,
    split_into_chunks,
};
use serde_json::Value;

const MD_SAMPLE: &str = "type: cover\ntitle: Sandvik AB Investment Memo\nsubtitle: Industrials | Long\ndate: April 2026\n---\ntype: bar_chart\naction_title: Sandvik trades at a discount to peer median\nlabels: [SAND.ST, CAT, KMT, ITW]\nvalues: [10.5, 11.0, 9.2, 11.8]\ntarget_label: SAND.ST\nvalue_format: \"{:.1f}x\"\nx_label: EV / LTM EBITDA\nsource: Bloomberg, Apr 30 2026\n";

fn snap() -> Value {
    common::load_json(&format!("{}/PPTX_pure.json", common::snap_dir()))
}

#[test]
fn pick_slide_archetype_matches() {
    let s = snap();
    for case in s["pick_slide_archetype"].as_array().unwrap() {
        let inp = case["in"].as_array().unwrap();
        let ds = inp[0].as_str().unwrap();
        let ne = inp[1].as_u64().unwrap() as usize;
        let nm = inp[2].as_u64().unwrap() as usize;
        let hq = inp[3].as_bool().unwrap();
        let isd = inp[4].as_bool().unwrap();
        let got = pick_slide_archetype(ds, ne, nm, hq, isd).to_json();
        let diffs = common::diff_json(&got, &case["out"], &[]);
        assert!(diffs.is_empty(), "archetype {inp:?}: {diffs:?}");
    }
}

#[test]
fn split_into_chunks_matches() {
    let s = snap();
    for case in s["split_into_chunks"].as_array().unwrap() {
        let inp = case["in"].as_array().unwrap();
        let items = inp[0].as_array().unwrap().clone();
        let arch = inp[1].as_str().unwrap();
        let got: Vec<Vec<Value>> = split_into_chunks(&items, arch).unwrap();
        let got_json = Value::Array(got.into_iter().map(Value::Array).collect());
        let diffs = common::diff_json(&got_json, &case["out"], &[]);
        assert!(diffs.is_empty(), "chunks {arch}: {diffs:?}");
    }
}

#[test]
fn normalize_heading_matches() {
    let s = snap();
    for case in s["normalize_heading"].as_array().unwrap() {
        let inp = case["in"].as_str().unwrap();
        let want = case["out"].as_str().unwrap();
        assert_eq!(normalize_heading(inp), want, "heading {inp:?}");
    }
}

#[test]
fn fmt_to_numfmt_matches() {
    let s = snap();
    for case in s["fmt_to_numfmt"].as_array().unwrap() {
        let inp = case["in"].as_str().unwrap();
        let want = case["out"].as_str().unwrap();
        assert_eq!(fmt_to_numfmt(inp), want, "fmt {inp:?}");
    }
}

#[test]
fn format_value_matches() {
    let s = snap();
    for case in s["format_value"].as_array().unwrap() {
        let kind = case["kind"].as_str().unwrap();
        let inv = &case["in"];
        let v = match kind {
            "null" => FmtValue::Null,
            "str" => FmtValue::Str(inv.as_str().unwrap().to_string()),
            "int" => FmtValue::Int(inv.as_i64().unwrap()),
            "float" => FmtValue::Float(inv.as_f64().unwrap()),
            other => panic!("unknown kind {other}"),
        };
        assert_eq!(
            format_value(&v),
            case["out"].as_str().unwrap(),
            "format_value {kind} {inv}"
        );
    }
}

#[test]
fn parse_deck_markdown_matches() {
    let s = snap();
    let got = fm_pptx::writer::deck::parse_deck_markdown(MD_SAMPLE).expect("parse md");
    let got_json = Value::Array(got);
    let diffs = common::diff_json(&got_json, &s["parse_deck_markdown"], &[]);
    assert!(diffs.is_empty(), "parse_deck_markdown: {diffs:?}");
}
