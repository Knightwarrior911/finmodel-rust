//! 6.4 parity gate — `fm_pptx::writer::deck` reproduces the Python
//! `ResearchPPTXWriter.write_ev_bridge_deck` / `write_ifrs_bridge_deck`
//! reference decks at the slide-shape-tree level (geometry, text runs, fills).
//!
//! The Rust deck is built, inspected with the Rust inspector, and its `slides`
//! are diffed against the committed inspection of the Python reference deck
//! (`PPTX_deck_{ev,ifrs}.json`). Cosmetic `id`/`name` are stripped (out of the
//! stated "geometry, text runs, fills" scope); master/layout/theme (template
//! differences) are not compared.

mod common;

use fm_pptx::writer::deck::{
    EvBridgeInput, IfrsInput, write_ev_bridge_deck, write_ifrs_bridge_deck,
};
use serde_json::Value;

const PINNED_DATE: &str = "January 2026";

fn strip_slides(v: &Value) -> Value {
    // Return the slides array with id/name removed from every element.
    let mut slides = v.get("slides").cloned().unwrap_or(Value::Null);
    if let Some(arr) = slides.as_array_mut() {
        for slide in arr.iter_mut() {
            if let Some(els) = slide.get_mut("elements").and_then(|e| e.as_array_mut()) {
                for el in els.iter_mut() {
                    if let Some(obj) = el.as_object_mut() {
                        obj.remove("id");
                        obj.remove("name");
                    }
                }
            }
        }
    }
    slides
}

fn ev_input() -> EvBridgeInput {
    EvBridgeInput {
        company: "DemoCo".into(),
        period: "LTM Sep-25".into(),
        currency: "USD".into(),
        share_price: Some(150.0),
        shares_outstanding: Some(1_000_000_000.0),
        total_debt: Some(50_000_000_000.0),
        finance_leases: Some(5_000_000_000.0),
        operating_leases: Some(8_000_000_000.0),
        underfunded_pension: Some(2_000_000_000.0),
        minority_interest: Some(1_000_000_000.0),
        preferred_stock: Some(500_000_000.0),
        cash: Some(20_000_000_000.0),
        short_term_investments: Some(10_000_000_000.0),
        equity_investments: Some(3_000_000_000.0),
        ltm_revenue: Some(100_000_000_000.0),
        ltm_ebitda: Some(30_000_000_000.0),
        ..Default::default()
    }
}

fn ifrs_input() -> IfrsInput {
    IfrsInput {
        accounting_standard: "IFRS".into(),
        reported_ebitda: 30_000_000_000.0,
        rou_depreciation: 4_000_000_000.0,
        lease_interest: 1_200_000_000.0,
        short_term_rent: 300_000_000.0,
        adjusted_ebitda: Some(24_500_000_000.0),
    }
}

fn run_deck(tag: &str, deck: fm_pptx::writer::deck::PptxDeckWriter) {
    let out = std::env::temp_dir().join(format!("fmpptx_deck_{tag}_{}.pptx", std::process::id()));
    let out = out.to_string_lossy().into_owned();
    deck.save(&out).expect("save deck");
    let got = fm_pptx::inspect::inspect_pptx(&out).expect("inspect rust deck");
    let want = common::load_json(&format!("{}/PPTX_deck_{tag}.json", common::snap_dir()));
    let diffs = common::diff_json(&strip_slides(&got), &strip_slides(&want), &[]);
    let _ = std::fs::remove_file(&out);
    if !diffs.is_empty() {
        let shown: Vec<String> = diffs.iter().take(40).cloned().collect();
        panic!(
            "{} shape-tree diff(s) vs PPTX_deck_{tag} oracle:\n{}",
            diffs.len(),
            shown.join("\n")
        );
    }
}

#[test]
fn ev_bridge_deck_matches_oracle() {
    let deck = write_ev_bridge_deck(&ev_input(), PINNED_DATE).expect("ev deck");
    run_deck("ev", deck);
}

#[test]
fn ifrs_bridge_deck_matches_oracle() {
    let deck = write_ifrs_bridge_deck(
        &ifrs_input(),
        "DemoCo",
        "FY2025",
        100_000_000_000.0,
        PINNED_DATE,
    )
    .expect("ifrs deck");
    run_deck("ifrs", deck);
}
