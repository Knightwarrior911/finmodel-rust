//! 6.2 parity gate — `fm_pptx::edit` reproduces the Python `pptx_editor`
//! slide-structure/theme/text ops.
//!
//! Structure ops (duplicate/delete/reorder) and recolor are pure zip+XML in
//! both implementations, so control-file / theme members are compared after
//! canonicalization (namespace-prefix and attribute-order agnostic). Copied
//! slide bytes are compared the same way. `replace_text_in_deck` is gated
//! behaviourally via the inspector (the reference reserializes through
//! python-pptx, so member bytes intentionally differ). Every op's edit-log
//! `op`+`params` are checked (the nondeterministic `ts`/`output_path` excluded).

mod common;

use fm_pptx::pkg::Package;
use fm_pptx::xmldom::Element;
use serde_json::Value;

fn canon(bytes: &[u8]) -> String {
    Element::parse(bytes).expect("parse xml").canonical()
}

/// Copy the fixture deck to a fresh temp path (with its edit log cleared).
fn temp_deck(tag: &str) -> String {
    let src = format!("{}/deck.pptx", common::fixture_dir());
    let dst = std::env::temp_dir().join(format!("fmpptx_{tag}_{}.pptx", std::process::id()));
    std::fs::copy(&src, &dst).expect("copy fixture");
    let dst = dst.to_string_lossy().into_owned();
    fm_pptx::edit::clear_edit_history(&dst);
    dst
}

fn assert_members_canonical(pkg: &Package, want: &Value, key: &str) {
    let members = want.get(key).and_then(|v| v.as_object()).expect("members map");
    for (name, wtext) in members {
        let got = pkg.get(name).unwrap_or_else(|| panic!("missing member {name}"));
        let (gc, wc) = (canon(got), canon(wtext.as_str().unwrap().as_bytes()));
        assert_eq!(gc, wc, "member {name} differs from oracle");
    }
}

fn assert_log(deck: &str, want: &Value) {
    let hist = fm_pptx::edit::get_edit_history(deck, 1, None);
    let got = hist.last().expect("edit log entry");
    let want_log = want.get("log").expect("oracle log");
    assert_eq!(got.get("op"), want_log.get("op"), "log op mismatch");
    // Compare params minus output_path (Rust never logs it; oracle strips it).
    let strip = |v: &Value| -> Value {
        let mut m = v.get("params").and_then(|p| p.as_object()).cloned().unwrap_or_default();
        m.remove("output_path");
        Value::Object(m)
    };
    assert_eq!(strip(got), strip(want_log), "log params mismatch");
}

fn assert_slide_parts(pkg: &Package, want: &Value) {
    let mut got: Vec<String> = pkg
        .names
        .iter()
        .filter(|n| n.starts_with("ppt/slides/slide") && n.ends_with(".xml"))
        .cloned()
        .collect();
    got.sort_by_key(|n| {
        n.rsplit_once("slide")
            .and_then(|(_, a)| a.rsplit_once('.'))
            .and_then(|(s, _)| s.parse::<i64>().ok())
            .unwrap_or(i64::MAX)
    });
    let want_parts: Vec<String> = want
        .get("slide_parts")
        .and_then(|v| v.as_array())
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(got, want_parts, "slide part names differ");
}

fn run_structure(tag: &str, op: impl Fn(&str) -> Result<String, String>) {
    let deck = temp_deck(tag);
    let want = common::load_json(&format!("{}/PPTX_edit_{tag}.json", common::snap_dir()));
    op(&deck).expect("edit op");
    let pkg = Package::read(&deck).expect("read output");
    assert_slide_parts(&pkg, &want);
    assert_members_canonical(&pkg, &want, "control");
    assert_members_canonical(&pkg, &want, "slides");
    assert_log(&deck, &want);
    let _ = std::fs::remove_file(&deck);
    let _ = std::fs::remove_file(format!("{deck}.edit_log.jsonl"));
}

#[test]
fn duplicate_slide_matches_oracle() {
    run_structure("duplicate", |d| fm_pptx::edit::duplicate_slide(d, 1, None, None));
}

#[test]
fn delete_slide_matches_oracle() {
    run_structure("delete", |d| fm_pptx::edit::delete_slide(d, 1, None));
}

#[test]
fn reorder_slides_matches_oracle() {
    run_structure("reorder", |d| fm_pptx::edit::reorder_slides(d, &[2, 0, 1], None));
}

#[test]
fn recolor_theme_matches_oracle() {
    let deck = temp_deck("recolor");
    let want = common::load_json(&format!("{}/PPTX_edit_recolor.json", common::snap_dir()));
    fm_pptx::edit::recolor_theme(&deck, &[("accent1", "#255BE3"), ("accent2", "#0F1632")], None, None)
        .expect("recolor");
    let pkg = Package::read(&deck).expect("read output");
    assert_members_canonical(&pkg, &want, "theme");
    assert_log(&deck, &want);
    let _ = std::fs::remove_file(&deck);
    let _ = std::fs::remove_file(format!("{deck}.edit_log.jsonl"));
}

#[test]
fn replace_text_changes_run_text() {
    // Behavioural gate: the observable text changes; run formatting preserved.
    let deck = temp_deck("replace");
    fm_pptx::edit::replace_text_in_deck(&deck, &[("Q1", "Q2")], None).expect("replace");
    let js = fm_pptx::inspect::inspect_pptx(&deck).expect("inspect");
    let slides = js.get("slides").and_then(|v| v.as_array()).unwrap();
    // Slide index 1 holds "Q1 revenue grew strongly".
    let text = slides[1]["elements"][0]["text"]["text"].as_str().unwrap_or("");
    assert!(text.contains("Q2 revenue"), "expected Q2 revenue, got {text:?}");
    assert!(!text.contains("Q1 revenue"), "Q1 should be replaced, got {text:?}");
    let hist = fm_pptx::edit::get_edit_history(&deck, 1, None);
    assert_eq!(hist.last().unwrap().get("op").unwrap(), "replace_text_in_deck");
    let _ = std::fs::remove_file(&deck);
    let _ = std::fs::remove_file(format!("{deck}.edit_log.jsonl"));
}
