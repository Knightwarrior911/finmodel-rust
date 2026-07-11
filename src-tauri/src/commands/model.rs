//! `build_model` — the core pipeline command: ticker -> model + Excel.
//!
//! Extraction source:
//!   - OpenRouter key set + US ticker     -> live SEC EDGAR fetch
//!   - OpenRouter key set + non-US ticker -> PDF discovery + LLM extraction
//!   - otherwise                          -> embedded committed fixture (offline demo)
//! Never fabricates data: a non-EDGAR ticker with no fixture and no key returns an error.
//!
//! Reconcile + project + Excel assembly are delegated to the shared `fm_build`
//! crate (same core the CLI uses — no drift).

use tauri::Manager;
use tauri_plugin_opener::OpenerExt;

use crate::commands::settings::read_settings;
use crate::error::{AppError, AppResult};

// Embedded baseline fixtures — the app is self-contained for the offline demo.
const FIXTURES: &[(&str, &str)] = &[
    (
        "SAND_ST",
        include_str!("../../../finmodel-core/fm-cli/tests/fixtures/SAND_ST_model.json"),
    ),
    (
        "ASML_AS",
        include_str!("../../../finmodel-core/fm-cli/tests/fixtures/ASML_AS_model.json"),
    ),
    (
        "NOVO-B_CO",
        include_str!("../../../finmodel-core/fm-cli/tests/fixtures/NOVO-B_CO_model.json"),
    ),
    (
        "NESN_SW",
        include_str!("../../../finmodel-core/fm-cli/tests/fixtures/NESN_SW_model.json"),
    ),
    (
        "ATCO-B_ST",
        include_str!("../../../finmodel-core/fm-cli/tests/fixtures/ATCO-B_ST_model.json"),
    ),
];

fn fixture_extraction(ticker: &str) -> Option<fm_extract::ExtractionResult> {
    let stem = fm_build::ticker_to_stem(ticker);
    let raw = FIXTURES.iter().find(|(k, _)| *k == stem).map(|(_, v)| *v)?;
    let mut val: serde_json::Value = serde_json::from_str(raw).ok()?;
    if val.get("currency").is_none() {
        val["currency"] = serde_json::json!(fm_build::currency_for_ticker(ticker));
    }
    serde_json::from_value(val).ok()
}

/// Best-effort company name for PDF discovery. Demo tickers get real names;
/// unknowns fall back to the ticker stem (works well enough for DDG).
fn company_name_for_ticker(ticker: &str) -> String {
    let up = ticker.to_uppercase();
    match up.as_str() {
        "SAND.ST" => "Sandvik".into(),
        "ASML.AS" => "ASML".into(),
        "NOVO-B.CO" => "Novo Nordisk".into(),
        "NESN.SW" => "Nestle".into(),
        "ATCO-B.ST" => "Atlas Copco".into(),
        "KO" => "Coca-Cola".into(),
        "AAPL" => "Apple".into(),
        "MSFT" => "Microsoft".into(),
        other => {
            // Strip exchange suffix: "FOO.ST" -> "FOO"
            other
                .split('.')
                .next()
                .unwrap_or(other)
                .replace('-', " ")
        }
    }
}

fn current_calendar_year() -> i32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    // rough civil year from days since epoch (good enough for period labels)
    let days = secs.div_euclid(86_400) + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let doe = days - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    (yoe + era * 400) as i32
}

fn stmt_to_json(sd: &fm_types::StatementData) -> serde_json::Value {
    let m: serde_json::Map<String, serde_json::Value> = sd
        .iter()
        .map(|(k, v)| (k.clone(), serde_json::json!(v)))
        .collect();
    serde_json::Value::Object(m)
}

/// Build a model for `ticker`. Returns a JSON summary the UI renders + the path
/// to the generated Excel file.
#[tauri::command(rename_all = "snake_case")]
pub async fn build_model(app: tauri::AppHandle, ticker: String) -> AppResult<String> {
    // Run the (blocking: HTTP fetch, PDF, LLM, file I/O) pipeline off the IPC
    // thread so the window stays responsive during live extraction.
    tauri::async_runtime::spawn_blocking(move || build_model_blocking(&app, &ticker))
        .await
        .map_err(|e| AppError::Engine(format!("build task failed: {e}")))?
}

fn build_model_blocking(app: &tauri::AppHandle, ticker: &str) -> AppResult<String> {
    let ticker = ticker.trim().to_string();
    if ticker.is_empty() {
        return Err(AppError::Config("Enter a ticker (e.g. SAND.ST).".into()));
    }

    let s = read_settings(app);
    let has_key = !s.openrouter_api_key.trim().is_empty();
    if has_key {
        // The OpenRouter provider in fm-extract reads these env vars.
        unsafe {
            std::env::set_var("OPENROUTER_API_KEY", s.openrouter_api_key.trim());
            std::env::set_var("FINMODEL_LLM_MODEL", s.model.trim());
        }
    }

    // 1. Obtain extraction (live-first when key; fixture fallback; never fabricate).
    let (extraction, source) = if has_key {
        match fm_extract::fetch_xbrl(&ticker) {
            Ok(e) => (e, "live (SEC EDGAR)"),
            Err(edgar_err) => match fixture_extraction(&ticker) {
                Some(e) => (e, "committed fixture (fallback)"),
                None => {
                    // Non-US / non-EDGAR live path: discover annual-report PDF + LLM extract.
                    let company = company_name_for_ticker(&ticker);
                    let year = current_calendar_year() - 1; // latest full year typically
                    let periods = [
                        (year - 2).to_string(),
                        (year - 1).to_string(),
                        year.to_string(),
                    ];
                    match fm_extract::fetch_non_us_filing(&company, &ticker, &periods, Some(year))
                    {
                        Ok(e) => (e, "live (PDF + LLM)"),
                        Err(pdf_err) => {
                            return Err(AppError::Engine(format!(
                                "{ticker}: EDGAR failed ({edgar_err}); PDF/LLM path failed ({pdf_err})"
                            )));
                        }
                    }
                }
            },
        }
    } else {
        match fixture_extraction(&ticker) {
            Some(e) => (e, "committed fixture (offline)"),
            None => {
                return Err(AppError::Config(format!(
                    "{ticker}: no offline data. Demo tickers: SAND.ST, ASML.AS, \
                     NOVO-B.CO, NESN.SW, ATCO-B.ST. Or add an OpenRouter key for \
                     US (EDGAR) or non-US (PDF) live extraction."
                )))
            }
        }
    };

    // 2. Shared core: reconcile + project + assemble sheets.
    let out = fm_build::build(&extraction, &ticker, 5);

    // 3. Write Excel to Documents/finmodel/.
    let out_dir = app
        .path()
        .document_dir()
        .map_err(|e| AppError::Io(format!("no documents dir: {e}")))?
        .join("finmodel");
    std::fs::create_dir_all(&out_dir)?;
    let stem = fm_build::ticker_to_stem(&ticker);
    let xlsx_path = out_dir.join(format!("{stem}_model.xlsx"));
    fm_excel::render::render(&out.workbook, &xlsx_path.to_string_lossy())
        .map_err(|e| AppError::Engine(format!("Excel write failed: {e}")))?;

    // 4. JSON summary for the UI.
    Ok(serde_json::json!({
        "ticker": ticker,
        "currency": extraction.currency,
        "source": source,
        "hist_periods": extraction.years_found,
        "proj_periods": out.projected.periods,
        "hist": {
            "income_statement": stmt_to_json(&extraction.income_statement),
            "balance_sheet": stmt_to_json(&extraction.balance_sheet),
            "cash_flow_statement": stmt_to_json(&extraction.cash_flow_statement),
        },
        "proj": {
            "income_statement": stmt_to_json(&out.projected.income_statement),
            "balance_sheet": stmt_to_json(&out.projected.balance_sheet),
            "cash_flow_statement": stmt_to_json(&out.projected.cash_flow),
        },
        "xlsx_path": xlsx_path.to_string_lossy(),
        "valuation": {
            "has_dcf": out.workbook.sheet("DCF").is_some(),
            "has_wacc": out.workbook.sheet("WACC").is_some(),
            "sheets": out.workbook.sheets.iter().map(|s| s.name.clone()).collect::<Vec<_>>(),
        },
    })
    .to_string())
}

/// Open a file with the OS default handler (the generated Excel).
#[tauri::command(rename_all = "snake_case")]
pub fn open_path(app: tauri::AppHandle, path: String) -> AppResult<String> {
    app.opener()
        .open_path(path.clone(), None::<&str>)
        .map_err(|e| AppError::Io(format!("open failed: {e}")))?;
    Ok(path)
}
