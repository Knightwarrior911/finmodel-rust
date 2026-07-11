//! Shared model-build orchestration used by BOTH the CLI and the desktop app.
//!
//! Keeps the demo-critical logic (currency mapping, projection wiring, Excel
//! sheet assembly) in ONE place so the two front-ends can't drift.

use std::collections::HashMap;

use fm_engine::ModelEngine;
use fm_excel::input::{AssumptionsBlock, Meta, ModelOutput, Verification, WorkbookInput};
use fm_excel::model::Workbook;
use fm_extract::ExtractionResult;
use fm_types::{CompanyConfig, ProjectedStatements, ReconciledData};

/// Map a ticker's exchange suffix to its reporting currency.
pub fn currency_for_ticker(ticker: &str) -> &'static str {
    let up = ticker.to_uppercase();
    if up.ends_with(".ST") {
        "SEK"
    } else if up.ends_with(".CO") {
        "DKK"
    } else if up.ends_with(".SW") {
        "CHF"
    } else if up.ends_with(".AS") || up.ends_with(".PA") || up.ends_with(".DE") {
        "EUR"
    } else if up.ends_with(".L") {
        "GBP"
    } else if up.ends_with(".TO") {
        "CAD"
    } else if up.ends_with(".T") {
        "JPY"
    } else {
        "USD"
    }
}

/// Sanitize a ticker to a filename stem (e.g. "SAND.ST" -> "SAND_ST").
pub fn ticker_to_stem(ticker: &str) -> String {
    ticker.replace(['.', '/'], "_")
}

/// The result of building a model: the forward projection plus the rich,
/// formula-driven Excel workbook (cell-model).
pub struct BuildOutput {
    pub projected: ProjectedStatements,
    pub workbook: Workbook,
}

/// Reconcile an extraction, project it forward, and build the rich workbook.
///
/// The single shared core both front-ends call — never duplicated.
pub fn build(extraction: &ExtractionResult, ticker: &str, proj_periods: usize) -> BuildOutput {
    let data = ReconciledData {
        income_statement: extraction.income_statement.clone(),
        balance_sheet: extraction.balance_sheet.clone(),
        cash_flow_statement: extraction.cash_flow_statement.clone(),
        periods: extraction.years_found.clone(),
        currency: extraction.currency.clone(),
    };
    let config = CompanyConfig {
        name: ticker.to_string(),
        currency: extraction.currency.clone(),
        hist_periods: extraction.years_found.len(),
        proj_periods,
        ..Default::default()
    };
    let engine = ModelEngine::new(data, config);
    let projected = engine.project(&HashMap::new());
    let input = build_workbook_input(extraction, &projected, ticker);
    let workbook = fm_excel::sheets::build_workbook(&input);
    BuildOutput { projected, workbook }
}

/// Assemble the rich-writer input (model output + derived assumptions + meta)
/// from an extraction and its forward projection.
///
/// Historical statement columns come straight from the extraction; projected
/// columns in the workbook are Excel formulas, so only the historicals need to
/// be materialized here. The two external market inputs (risk-free rate, share
/// price) are not fetched by the app yet — they default until wired.
pub fn build_workbook_input(
    extraction: &ExtractionResult,
    projected: &ProjectedStatements,
    ticker: &str,
) -> WorkbookInput {
    use fm_excel::is_structure::{apply_filing_labels, build_is_structure, build_standard_is_detailed, CogsDetail, OpexItem, Segment};

    let hist: Vec<String> = extraction.years_found.iter().map(|y| format!("{y}A")).collect();
    let proj: Vec<String> = projected.periods.iter().map(|y| format!("{y}E")).collect();
    let mut periods = hist;
    periods.extend(proj);

    let sector = "standard".to_string();

    // XBRL detail from footnotes (US filings): revenue segments, opex line items,
    // detailed COGS. Parse + (for standard sector) remap cogs/rd/sga into their
    // canonical slots, mirroring cli.py.
    let items = |k: &str| -> Vec<(String, String, String)> {
        extraction.notes.get(k).and_then(|v| v.as_array()).map(|a| {
            a.iter().filter_map(|o| {
                let label = o.get("label")?.as_str()?.to_string();
                let key = o.get("key")?.as_str()?.to_string();
                let cat = o.get("category").and_then(|c| c.as_str()).unwrap_or("").to_string();
                Some((label, key, cat))
            }).collect()
        }).unwrap_or_default()
    };
    let seg_raw = items("revenue_segments");
    let opex_raw = items("opex_items");
    let cogs_raw = items("cogs_detail");

    let mut is = extraction.income_statement.clone();
    let detailed = sector == "standard" && (!seg_raw.is_empty() || !opex_raw.is_empty());
    if sector == "standard" && !opex_raw.is_empty() {
        let first = |cat: &str| opex_raw.iter().find(|(_, _, c)| c == cat).map(|(_, k, _)| k.clone());
        for (slot, cat) in [("cogs", "cogs"), ("rd", "opex_rd"), ("sga", "opex")] {
            if let Some(src) = first(cat) {
                if let Some(vals) = is.get(&src).cloned() {
                    is.insert(slot.to_string(), vals);
                }
            }
        }
    }

    let model = ModelOutput {
        periods,
        income_statement: is.clone(),
        balance_sheet: extraction.balance_sheet.clone(),
        cash_flow_statement: extraction.cash_flow_statement.clone(),
        plug_used: false,
    };

    let meta = Meta {
        company: ticker.to_string(),
        ticker: ticker.to_string(),
        currency: extraction.currency.clone(),
        fiscal_year_end: "Dec".to_string(),
        sector: sector.clone(),
        as_of: today_iso(),
    };

    // App has no live market feed yet; defaults keep the workbook well-formed.
    let assumptions: AssumptionsBlock =
        fm_excel::derive::build_assumptions_block(&model, &meta.sector, 0.045, 0.0);
    let verification = Verification { passed: true, ..Default::default() };

    let nonzero = |k: &str| {
        is.get(k).map(|v| v.iter().any(|x| x.map(|n| n != 0.0).unwrap_or(false))).unwrap_or(false)
    };
    let mut is_structure = if detailed {
        let segments: Vec<Segment> = seg_raw.iter().map(|(l, k, _)| Segment { label: l.clone(), key: k.clone() }).collect();
        let opex_items: Vec<OpexItem> = opex_raw.iter().map(|(l, k, c)| OpexItem { label: l.clone(), key: k.clone(), category: c.clone() }).collect();
        let cogs_detail: Vec<CogsDetail> = cogs_raw.iter().map(|(l, k, _)| CogsDetail { label: l.clone(), key: k.clone() }).collect();
        build_standard_is_detailed(nonzero("cogs"), nonzero("rd"), nonzero("sga"), &segments, &opex_items, &cogs_detail)
    } else {
        build_is_structure(&meta.sector, nonzero("cogs"), nonzero("rd"), nonzero("sga"))
    };

    // Override IS labels with actual XBRL concept labels when the filing provides them.
    if let Some(fl) = extraction.notes.get("filing_labels").and_then(|v| v.as_object()) {
        let labels: std::collections::HashMap<String, String> = fl
            .iter()
            .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
            .collect();
        apply_filing_labels(&mut is_structure, &labels);
    }

    // Valuation: fallback peer-set WACC + DCF so Cover/DCF/WACC/Sens tabs ship
    // with every workbook. Market inputs still default until a live feed lands.
    // Offline fallback beta = 1.0 (cli.py unlevers a fetched own-beta when peers
    // are empty; we have no market beta feed yet).
    let last = |stmt: &fm_excel::input::Statement, key: &str| -> f64 {
        stmt.get(key)
            .and_then(|v| v.iter().rev().find_map(|x| *x))
            .unwrap_or(0.0)
    };
    let shares = if assumptions.shares_diluted != 0.0 {
        assumptions.shares_diluted
    } else {
        last(&model.income_statement, "shares_diluted")
    };
    let mkt_cap = assumptions.current_share_price * shares;
    let debt = last(&model.balance_sheet, "long_term_debt");
    // cli.py uses base.tax_rate_pct[0] (first projected year) for beta unlever tax.
    let tax = assumptions
        .base
        .tax_rate_pct
        .first()
        .copied()
        .unwrap_or(0.21);
    let peer_set = fm_value::fallback_peer_set(&meta.ticker, mkt_cap, assumptions.target_de_ratio);
    let wacc = fm_value::compute_wacc(
        &peer_set,
        mkt_cap,
        debt,
        assumptions.risk_free_rate,
        assumptions.equity_risk_premium,
        assumptions.cost_of_debt_pretax,
        tax,
        Some(assumptions.target_de_ratio),
        1.0, // no own-beta feed yet
    );
    let active = match assumptions.active_case {
        2 => &assumptions.upside,
        3 => &assumptions.downside,
        _ => &assumptions.base,
    };
    let dcf_asmp = fm_value::DCFAssumptions {
        mid_year_convention: assumptions.mid_year_convention,
        current_share_price: assumptions.current_share_price,
        shares_diluted: shares,
        active: fm_value::DCFScenario {
            terminal_growth_rate: active.terminal_growth_rate,
            exit_ebitda_multiple: active.exit_ebitda_multiple,
        },
    };
    let dcf = fm_value::compute_dcf(
        &model.periods,
        &model.income_statement,
        &model.balance_sheet,
        &model.cash_flow_statement,
        &meta.ticker,
        &wacc,
        &dcf_asmp,
        1,
    );

    WorkbookInput {
        meta,
        model,
        assumptions,
        verification,
        is_structure,
        wacc: Some(wacc),
        peer_source: peer_set.source,
        dcf: Some(dcf),
    }
}

/// Today's date as `YYYY-MM-DD` (UTC) for the Cover "As of …" line.
fn today_iso() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
    format!("{y:04}-{m:02}-{d:02}")
}

/// Inverse of Howard Hinnant's `days_from_civil`: days-since-epoch → (y, m, d).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (y + if m <= 2 { 1 } else { 0 }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_currency_for_ticker() {
        assert_eq!(currency_for_ticker("SAND.ST"), "SEK");
        assert_eq!(currency_for_ticker("NOVO-B.CO"), "DKK");
        assert_eq!(currency_for_ticker("NESN.SW"), "CHF");
        assert_eq!(currency_for_ticker("ASML.AS"), "EUR");
        assert_eq!(currency_for_ticker("MC.PA"), "EUR");
        assert_eq!(currency_for_ticker("AAPL"), "USD");
    }

    #[test]
    fn test_ticker_to_stem() {
        assert_eq!(ticker_to_stem("SAND.ST"), "SAND_ST");
        assert_eq!(ticker_to_stem("NOVO-B.CO"), "NOVO-B_CO");
    }

    #[test]
    fn test_build_produces_workbook_and_projection() {
        use fm_types::StatementData;
        // Minimal 2-year extraction
        let mut is = StatementData::new();
        is.insert("revenue".into(), vec![Some(100.0), Some(110.0)]);
        is.insert("net_income".into(), vec![Some(10.0), Some(12.0)]);
        is.insert("income_tax".into(), vec![Some(3.0), Some(4.0)]);
        let extraction = ExtractionResult {
            currency: "USD".into(),
            years_found: vec!["2023".into(), "2024".into()],
            income_statement: is,
            balance_sheet: StatementData::new(),
            cash_flow_statement: StatementData::new(),
            notes: HashMap::new(),
            confidence: 1.0,
            discrepancies: vec![],
        };
        let out = build(&extraction, "TEST", 5);
        assert_eq!(out.projected.periods.len(), 5);
        // Rich workbook: 3-statement + valuation tabs + Sources.
        let names: Vec<&str> = out.workbook.sheets.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(
            names,
            ["Cover", "Assumptions", "IS", "BS", "CF", "DCF", "WACC", "Sensitivities", "Sources"]
        );
        // Cover carries the ticker and periods (Hist: 2023A–2024A | Proj: 2025E–2029E).
        let cover = out.workbook.sheet("Cover").expect("cover");
        assert!(cover.cells.values().any(|c| matches!(&c.value,
            Some(fm_excel::model::Value::Text(t)) if t.contains("2025E"))));
    }

    #[test]
    fn test_build_detailed_is_from_notes() {
        use fm_types::StatementData;
        let mut is = StatementData::new();
        for (k, v) in [
            ("revenue", vec![Some(100.0), Some(110.0)]),
            ("rev_seg_a", vec![Some(60.0), Some(66.0)]),
            ("rev_seg_b", vec![Some(40.0), Some(44.0)]),
            ("cogs_seg_a", vec![Some(30.0), Some(33.0)]),
            ("net_income", vec![Some(10.0), Some(12.0)]),
            ("income_tax", vec![Some(3.0), Some(4.0)]),
            ("rd", vec![Some(5.0), Some(6.0)]),
            ("sga", vec![Some(8.0), Some(9.0)]),
        ] {
            is.insert(k.into(), v);
        }
        let mut notes = HashMap::new();
        notes.insert("revenue_segments".into(), serde_json::json!([
            {"label": "Products", "key": "rev_seg_a"}, {"label": "Services", "key": "rev_seg_b"}]));
        notes.insert("opex_items".into(), serde_json::json!([
            {"label": "Cost of products", "key": "cogs_seg_a", "category": "cogs"},
            {"label": "R&D", "key": "rd", "category": "opex_rd"},
            {"label": "SG&A", "key": "sga", "category": "opex"}]));
        let extraction = ExtractionResult {
            currency: "USD".into(),
            years_found: vec!["2023".into(), "2024".into()],
            income_statement: is,
            balance_sheet: StatementData::new(),
            cash_flow_statement: StatementData::new(),
            notes,
            confidence: 1.0,
            discrepancies: vec![],
        };
        let out = build(&extraction, "TEST", 5);
        // Detailed IS: segment rows render (label "  Products" / "  Services").
        let is_sheet = out.workbook.sheet("IS").expect("IS");
        let has = |t: &str| is_sheet.cells.values().any(|c| matches!(&c.value,
            Some(fm_excel::model::Value::Text(s)) if s == t));
        assert!(has("  Products"), "segment row missing");
        assert!(has("Total Revenue"), "Total Revenue subtotal missing");
    }
}
