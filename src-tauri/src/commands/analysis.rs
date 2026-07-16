//! Post-build analyst actions (Phase 6.5): the enterprise-value bridge, the
//! IFRS↔US-GAAP lease bridge, and the ground-truth tie-out score — the same
//! `fm-value` / `fm-ifrs` / `fm-tieout` builders the CLI drives, now reachable
//! from the desktop app.
//!
//! These are deliberately **UI-invoked commands, not chat tools**: each is a
//! deterministic calculator over explicit, structured financial inputs (the CLI
//! exposes them as arg-heavy subcommands, never free text). Adding them to the
//! per-turn LLM tool list would reintroduce exactly the flat, auto-`tool_choice`
//! surface Phase 1 removed. The intent router therefore never registers them;
//! the analyst triggers each action directly on a built model.

use crate::error::{AppError, AppResult};

/// Enterprise-value bridge: Equity Value + non-equity claims − non-operating
/// assets (BIWS R-001..R-018). Only present/material items are applied; goodwill
/// is never subtracted. Returns the full bridge (additions/subtractions + EV) as
/// a JSON string (per the desktop IPC convention — the frontend `call()` parses).
#[tauri::command(rename_all = "snake_case")]
pub fn ev_bridge(input: fm_value::ev_bridge::EvBridgeInput) -> AppResult<String> {
    let bridge = fm_value::ev_bridge::build_ev_bridge(&input);
    serde_json::to_string(&bridge)
        .map_err(|e| AppError::Engine(format!("ev-bridge serialize failed: {e}")))
}

/// IFRS-16 ↔ US-GAAP lease bridge: restates EBIT/EBITDA/EBITA and margins for
/// the direction implied by `input.accounting_standard`, using the extracted
/// lease-note figures. `revenue` drives the margin lines (0 → margins omitted).
#[tauri::command(rename_all = "snake_case")]
pub fn ifrs_bridge(input: fm_ifrs::IfrsAdjustmentInput, revenue: f64) -> AppResult<String> {
    let out = fm_ifrs::auto_convert(&input, revenue);
    serde_json::to_string(&out)
        .map_err(|e| AppError::Engine(format!("ifrs-bridge serialize failed: {e}")))
}

/// Tie-out score: match a built model's line items against an immutable
/// ground-truth workbook and report trusted/matched counts, percentage, and the
/// specific mismatches. Both arguments are JSON documents (as the CLI loads).
#[tauri::command(rename_all = "snake_case")]
pub fn tie_out(ground_truth_json: String, model_json: String) -> AppResult<String> {
    let score = fm_tieout::score_from_json(&ground_truth_json, &model_json)
        .map_err(|e| AppError::Engine(format!("tie-out failed: {e}")))?;
    serde_json::to_string(&score)
        .map_err(|e| AppError::Engine(format!("tie-out serialize failed: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ev_bridge_deserializes_input_and_serializes_bridge() {
        // Market cap 1000; +200 debt, +50 finance leases; −100 cash → EV 1150.
        let input: fm_value::ev_bridge::EvBridgeInput = serde_json::from_value(serde_json::json!({
            "company": "ACME",
            "market_cap": 1000.0,
            "total_debt": 200.0,
            "finance_leases": 50.0,
            "cash": 100.0
        }))
        .unwrap();
        let json = ev_bridge(input).unwrap();
        let b: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(b["total_ev"], 1150.0);
        assert!(b["additions"].as_array().unwrap().len() >= 2);
    }

    #[test]
    fn ifrs_bridge_restates_ebit_from_lease_note() {
        // US GAAP → IFRS: adjusted EBIT adds back ROU depreciation portion.
        let input: fm_ifrs::IfrsAdjustmentInput = serde_json::from_value(serde_json::json!({
            "rou_depreciation": 80.0,
            "lease_interest": 20.0,
            "short_term_rent": 0.0,
            "reported_ebit": 500.0,
            "reported_ebitda": 600.0,
            "reported_ebita": 550.0,
            "standard_depreciation": 0.0,
            "standard_amortization": 0.0,
            "accounting_standard": "US GAAP",
            "weighted_discount_rate": null,
            "weighted_lease_term": null
        }))
        .unwrap();
        let json = ifrs_bridge(input, 1000.0).unwrap();
        let o: serde_json::Value = serde_json::from_str(&json).unwrap();
        // A conversion occurred and margins were computed off the 1000 revenue.
        assert!(o["adjusted_ebit"].as_f64().unwrap() != 0.0);
        assert!((o["reported_ebit_margin"].as_f64().unwrap() - 50.0).abs() < 1e-6);
    }

    #[test]
    fn tie_out_rejects_malformed_json() {
        let err = tie_out("not json".into(), "{}".into()).unwrap_err();
        assert!(matches!(err, AppError::Engine(_)));
    }
}
