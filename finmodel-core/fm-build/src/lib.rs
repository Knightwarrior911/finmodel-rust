//! Shared model-build orchestration used by BOTH the CLI and the desktop app.
//!
//! Keeps the demo-critical logic (currency mapping, projection wiring, Excel
//! sheet assembly) in ONE place so the two front-ends can't drift.

use std::collections::HashMap;

use fm_engine::ModelEngine;
use fm_excel::writer::{CellValue, SheetData};
use fm_extract::ExtractionResult;
use fm_types::{CompanyConfig, ProjectedStatements, ReconciledData, StatementData};

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

/// The result of building a model: the forward projection plus assembled sheets.
pub struct BuildOutput {
    pub projected: ProjectedStatements,
    pub sheets: Vec<SheetData>,
}

/// Reconcile an extraction, project it forward, and assemble Excel sheets.
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
    let sheets = build_excel_sheets(extraction, &projected);
    BuildOutput { projected, sheets }
}

/// Assemble IS/BS/CFS sheets combining historical + projected columns.
pub fn build_excel_sheets(
    extraction: &ExtractionResult,
    projected: &ProjectedStatements,
) -> Vec<SheetData> {
    let hist_years = &extraction.years_found;
    let mut headers = vec!["Item".to_string()];
    for y in hist_years {
        headers.push(y.clone());
    }
    for p in &projected.periods {
        headers.push(p.clone());
    }

    let make = |name: &str, hist: &StatementData, proj: &StatementData| -> SheetData {
        let mut keys: Vec<String> = hist.keys().chain(proj.keys()).cloned().collect();
        keys.sort();
        keys.dedup();
        let mut rows = Vec::new();
        for key in keys {
            let mut cells = Vec::new();
            match hist.get(&key) {
                Some(hv) => {
                    for v in hv {
                        cells.push(v.map(CellValue::Value).unwrap_or(CellValue::Empty));
                    }
                }
                None => {
                    for _ in hist_years {
                        cells.push(CellValue::Empty);
                    }
                }
            }
            if let Some(pv) = proj.get(&key) {
                for v in pv {
                    cells.push(v.map(CellValue::Value).unwrap_or(CellValue::Empty));
                }
            }
            rows.push((key, cells));
        }
        SheetData { name: name.to_string(), headers: headers.clone(), rows }
    };

    vec![
        make("Income Statement", &extraction.income_statement, &projected.income_statement),
        make("Balance Sheet", &extraction.balance_sheet, &projected.balance_sheet),
        make("Cash Flow", &extraction.cash_flow_statement, &projected.cash_flow),
    ]
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
    fn test_build_produces_sheets_and_projection() {
        // Minimal 2-year extraction
        let mut is = StatementData::new();
        is.insert("revenue".into(), vec![Some(100.0), Some(110.0)]);
        is.insert("net_income".into(), vec![Some(10.0), Some(12.0)]);
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
        assert_eq!(out.sheets.len(), 3);
        // IS sheet has header Item + 2 hist + 5 proj = 8 columns
        assert_eq!(out.sheets[0].headers.len(), 8);
        assert!(out.sheets[0].rows.iter().any(|(k, _)| k == "revenue"));
    }
}
