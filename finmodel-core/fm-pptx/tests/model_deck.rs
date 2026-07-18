//! Inspect-based tests for the sellable one-click decks (workstream C):
//! `write_model_deck` / `write_benchmark_deck` + the `add_table` archetype. No
//! Python oracle exists for these, so we build → save → inspect and assert on
//! slide count and the presence of cover/tile/table text runs.

use fm_pptx::writer::deck::{
    ModelDeckInput, PptxDeckWriter, write_benchmark_deck, write_model_deck,
};
use serde_json::Value;

const PINNED_DATE: &str = "January 2026";

fn inspect_saved(tag: &str, deck: PptxDeckWriter) -> Value {
    let out = std::env::temp_dir().join(format!("fmpptx_{tag}_{}.pptx", std::process::id()));
    let out = out.to_string_lossy().into_owned();
    deck.save(&out).expect("save deck");
    let got = fm_pptx::inspect::inspect_pptx(&out).expect("inspect deck");
    let _ = std::fs::remove_file(&out);
    got
}

fn all_text(v: &Value) -> Vec<String> {
    let mut texts = Vec::new();
    if let Some(slides) = v.get("slides").and_then(|s| s.as_array()) {
        for slide in slides {
            if let Some(els) = slide.get("elements").and_then(|e| e.as_array()) {
                for el in els {
                    if let Some(t) = el
                        .get("text")
                        .and_then(|t| t.get("text"))
                        .and_then(|t| t.as_str())
                    {
                        texts.push(t.to_string());
                    }
                }
            }
        }
    }
    texts
}

fn model_input() -> ModelDeckInput {
    ModelDeckInput {
        ticker: "AAPL".into(),
        company: "Apple Inc.".into(),
        currency: "USD".into(),
        periods: vec![
            "2022".into(),
            "2023".into(),
            "2024".into(),
            "2025E".into(),
            "2026E".into(),
        ],
        revenue: vec![300.0, 320.0, 383.0, 400.0, 420.0],
        ebitda: vec![100.0, 110.0, 120.0, 130.0, 140.0],
        hist_n: 3,
        implied_price: 210.0,
        current_price: 190.0,
        upside_pct: 10.5,
        wacc: 0.089,
        ev: 3_000_000_000_000.0,
        tv_method: "EBITDA exit multiple".into(),
        comps_headers: vec!["Ticker".into(), "EV/EBITDA".into(), "P/E".into()],
        comps_rows: vec![
            vec!["MSFT".into(), "22.0x".into(), "33.0x".into()],
            vec!["GOOGL".into(), "15.0x".into(), "25.0x".into()],
        ],
    }
}

#[test]
fn model_deck_has_expected_slides_and_text() {
    let deck = write_model_deck(&model_input(), PINNED_DATE).expect("model deck");
    let got = inspect_saved("model", deck);
    // cover + scorecard + revenue chart + ebitda chart + comps table = 5
    assert_eq!(got.get("slideCount").and_then(|c| c.as_i64()), Some(5));
    let texts = all_text(&got);
    let joined = texts.join("\n");
    assert!(
        joined.contains("AAPL — Financial model summary"),
        "cover title missing:\n{joined}"
    );
    assert!(
        joined.contains("Implied price"),
        "scorecard tile metric missing"
    );
    assert!(
        texts.iter().any(|t| t.contains("Apple Inc.")),
        "company subtitle missing"
    );
    assert!(texts.iter().any(|t| t == "MSFT"), "comps peer cell missing");
    assert!(
        texts.iter().any(|t| t == "22.0x"),
        "comps multiple cell missing"
    );
}

#[test]
fn model_deck_without_comps_skips_table() {
    let mut inp = model_input();
    inp.comps_rows.clear();
    inp.comps_headers.clear();
    let deck = write_model_deck(&inp, PINNED_DATE).expect("model deck");
    let got = inspect_saved("model_nocomps", deck);
    // cover + scorecard + 2 charts = 4 (no comps table)
    assert_eq!(got.get("slideCount").and_then(|c| c.as_i64()), Some(4));
}

#[test]
fn benchmark_deck_cover_table_and_margin_chart() {
    let headers = vec!["Company".into(), "Revenue".into(), "EBITDA margin".into()];
    let rows = vec![
        vec!["AAPL".into(), "383".into(), "32.1%".into()],
        vec!["MSFT".into(), "245".into(), "48.5%".into()],
    ];
    let deck = write_benchmark_deck("Tech peer benchmark", &headers, &rows, PINNED_DATE)
        .expect("bench deck");
    let got = inspect_saved("bench", deck);
    // cover + table + EBITDA-margin chart = 3
    assert_eq!(got.get("slideCount").and_then(|c| c.as_i64()), Some(3));
    let texts = all_text(&got);
    assert!(texts.iter().any(|t| t == "AAPL"), "peer table cell missing");
    assert!(
        texts.iter().any(|t| t.contains("EBITDA margin dispersion")),
        "margin chart title missing"
    );
}

#[test]
fn add_table_rejects_oversized_grids() {
    let mut deck = PptxDeckWriter::new(
        &fm_pptx::writer::deck::BrandProfile::default(),
        "CONFIDENTIAL",
        PINNED_DATE,
    );
    let headers: Vec<String> = (0..9).map(|i| format!("C{i}")).collect();
    assert!(
        deck.add_table("Too many columns here now", &headers, &[], "src")
            .is_err()
    );
}
