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

use tauri::{Emitter, Manager};
use tauri_plugin_opener::OpenerExt;

use crate::commands::settings::read_settings;
use crate::error::{AppError, AppResult};

/// Emit a `build_progress` event the UI listens on (4.1). Best-effort.
fn emit_progress(app: &tauri::AppHandle, stage: &str, detail: &str) {
    let _ = app.emit("build_progress", serde_json::json!({ "stage": stage, "detail": detail }));
}

// Canonical statement row order for the UI preview — mirrors the order the
// fm-excel sheet builders emit. Keys are canonical (sector-agnostic: bank /
// insurer / REIT tags map onto these same keys), so one list serves all
// sectors. Keys present in the data but absent here sort alphabetically after.
const IS_ORDER: &[&str] = &[
    "revenue", "cogs", "gross_profit", "sga", "rd",
    "utility_om", "utility_fuel", "utility_taxes_other", "utility_other", "utility_total_opex",
    "da", "ebit", "ebita", "ebitda",
    "interest_expense", "interest_income", "ebt", "income_tax",
    "net_income", "nci_income_loss", "ni_common",
    "eps_basic", "eps_diluted", "shares_basic", "shares_diluted",
];
const BS_ORDER: &[&str] = &[
    "cash", "accounts_receivable", "inventory", "total_current_assets",
    "ppe_net", "goodwill", "intangibles_net", "total_assets",
    "accounts_payable", "deferred_revenue_current", "short_term_debt", "total_current_liabilities",
    "long_term_debt", "deferred_revenue_lt", "total_liabilities",
    "retained_earnings", "total_equity", "redeemable_nci",
];
const CF_ORDER: &[&str] = &[
    "cfo", "capex", "investments_net_cfi", "cfi",
    "dividends_paid", "buybacks", "cff",
    "fx_effect_on_cash", "net_change_cash",
];

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
pub async fn build_model(
    app: tauri::AppHandle,
    ticker: String,
    options: Option<fm_build::BuildOptions>,
) -> AppResult<String> {
    // Run the (blocking: HTTP fetch, PDF, LLM, file I/O) pipeline off the IPC
    // thread so the window stays responsive during live extraction.
    let opts = options.unwrap_or_default();
    tauri::async_runtime::spawn_blocking(move || build_model_blocking(&app, &ticker, opts))
        .await
        .map_err(|e| AppError::Engine(format!("build task failed: {e}")))?
}

pub(crate) fn build_model_blocking(
    app: &tauri::AppHandle,
    ticker: &str,
    mut opts: fm_build::BuildOptions,
) -> AppResult<String> {
    let ticker = ticker.trim().to_string();
    if ticker.is_empty() {
        return Err(AppError::Config("Enter a ticker (e.g. SAND.ST).".into()));
    }

    let s = read_settings(app);
    // Default output folder from settings (unless the caller passed an explicit path).
    if opts.out_path.is_none() && !s.out_dir.trim().is_empty() {
        let stem = fm_build::ticker_to_stem(ticker.trim());
        opts.out_path = Some(
            std::path::Path::new(s.out_dir.trim())
                .join(format!("{stem}_model.xlsx"))
                .to_string_lossy()
                .to_string(),
        );
    }

    // 1. Obtain extraction (live-first when key; fixture fallback; never fabricate).
    let (extraction, source) = obtain_extraction(app, &ticker)?;

    render_build(app, &extraction, source, &ticker, opts)
}

/// Live share price + build + Excel render + JSON summary. Shared by
/// [`build_model`] and [`finalize_model`].
fn render_build(
    app: &tauri::AppHandle,
    extraction: &fm_extract::ExtractionResult,
    source: &str,
    ticker: &str,
    mut opts: fm_build::BuildOptions,
) -> AppResult<String> {
    // Live share price (the real DCF-upside input) unless the analyst set one.
    // Only for LIVE extractions — the offline/fixture demo path stays instant.
    let mut warnings: Vec<String> = Vec::new();
    if opts.share_price.is_none() && source.starts_with("live") {
        match fm_fetch::fetch_quote(ticker) {
            Ok(q) if q.currency == extraction.currency => opts.share_price = Some(q.price),
            Ok(q) => warnings.push(format!(
                "quote currency {} ≠ filing currency {} — live share price not applied; \
                 enter one in Advanced options",
                q.currency, extraction.currency
            )),
            Err(_) => warnings.push(
                "live quote unavailable — enter a share price in Advanced options for DCF upside"
                    .into(),
            ),
        }
    }

    // Shared core: reconcile + project + assemble sheets (honoring options).
    emit_progress(app, "project", "Projecting the forecast…");
    let out = fm_build::build_with(extraction, ticker, &opts);
    warnings.extend(out.warnings.iter().cloned());

    // Write Excel — to the analyst's chosen path, else Documents/finmodel/.
    let xlsx_path = if let Some(p) = opts.out_path.as_ref().filter(|p| !p.trim().is_empty()) {
        std::path::PathBuf::from(p)
    } else {
        let out_dir = app
            .path()
            .document_dir()
            .map_err(|e| AppError::Io(format!("no documents dir: {e}")))?
            .join("finmodel");
        std::fs::create_dir_all(&out_dir)?;
        out_dir.join(format!("{}_model.xlsx", fm_build::ticker_to_stem(ticker)))
    };
    if let Some(parent) = xlsx_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    emit_progress(app, "render", "Writing the Excel workbook…");
    fm_excel::render::render(&out.workbook, &xlsx_path.to_string_lossy())
        .map_err(|e| AppError::Engine(format!("Excel write failed: {e}")))?;
    // Record in Recent files (4.2).
    push_recent(app, &xlsx_path.to_string_lossy(), &format!("{ticker} model"));

    let val_method = out
        .dcf
        .as_ref()
        .map(|d| if d.tv_method == 1 { "EBITDA exit multiple" } else { "Gordon growth" });
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
        "order": {
            "income_statement": IS_ORDER,
            "balance_sheet": BS_ORDER,
            "cash_flow_statement": CF_ORDER,
        },
        "warnings": warnings,
        "xlsx_path": xlsx_path.to_string_lossy(),
        "valuation": {
            "has_dcf": out.workbook.sheet("DCF").is_some(),
            "has_wacc": out.workbook.sheet("WACC").is_some(),
            "sheets": out.workbook.sheets.iter().map(|s| s.name.clone()).collect::<Vec<_>>(),
            "price_per_share": out.dcf.as_ref().map(|d| d.implied_price),
            "current_price": out.dcf.as_ref().map(|d| d.current_share_price),
            "upside_pct": out.dcf.as_ref().map(|d| d.upside_downside_pct),
            "ev": out.dcf.as_ref().map(|d| d.enterprise_value),
            "wacc": out.wacc_out.as_ref().map(|w| w.wacc),
            "method": val_method,
        },
    })
    .to_string())
}

/// Acquire an extraction for `ticker` (live-first when a key is set; committed
/// fixture fallback; never fabricates). Also applies the EDGAR contact from
/// settings. Shared by build / prepare.
fn obtain_extraction(
    app: &tauri::AppHandle,
    ticker: &str,
) -> AppResult<(fm_extract::ExtractionResult, &'static str)> {
    let s = read_settings(app);
    emit_progress(app, "fetch", "Fetching the SEC filing…");
    if !s.edgar_contact.trim().is_empty() {
        fm_fetch::edgar::set_edgar_contact(s.edgar_contact.trim().to_string());
    }
    let has_key = !s.openrouter_api_key.trim().is_empty();
    let llm_cfg = fm_extract::LlmConfig {
        api_key: s.openrouter_api_key.trim().to_string(),
        model: s.model.trim().to_string(),
    };
    if has_key {
        match fm_extract::fetch_xbrl(ticker) {
            Ok(e) => Ok((e, "live (SEC EDGAR)")),
            Err(edgar_err) => match fixture_extraction(ticker) {
                Some(e) => Ok((e, "committed fixture (fallback)")),
                None => {
                    if !ticker.contains('.') {
                        return Err(AppError::Engine(format!(
                            "{ticker}: not found on SEC EDGAR ({edgar_err}). For non-US \
                             companies use the exchange suffix (e.g. SAND.ST); check the spelling."
                        )));
                    }
                    let company = company_name_for_ticker(ticker);
                    let year = fm_extract::current_year() - 1;
                    let periods = [(year - 2).to_string(), (year - 1).to_string(), year.to_string()];
                    match fm_extract::fetch_non_us_filing(&company, ticker, &periods, Some(year), Some(&llm_cfg)) {
                        Ok(e) => Ok((e, "live (PDF + LLM)")),
                        Err(pdf_err) => Err(AppError::Engine(format!(
                            "{ticker}: EDGAR failed ({edgar_err}); PDF/LLM path failed ({pdf_err})"
                        ))),
                    }
                }
            },
        }
    } else {
        match fixture_extraction(ticker) {
            Some(e) => Ok((e, "committed fixture (offline)")),
            None => Err(AppError::Config(format!(
                "{ticker}: no offline data. Demo tickers: SAND.ST, ASML.AS, NOVO-B.CO, \
                 NESN.SW, ATCO-B.ST. Or add an OpenRouter key for live extraction."
            ))),
        }
    }
}

/// In-memory prepare→finalize session cache (lost on app restart — acceptable;
/// finalize after restart errors, telling the user to rebuild).
#[derive(Default)]
pub struct SessionCache(
    pub std::sync::Mutex<std::collections::HashMap<String, (fm_extract::ExtractionResult, String, String)>>,
);

/// Session id from ticker + wall-clock nanos (no new dep).
fn session_id(ticker: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    ticker.hash(&mut h);
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
        .hash(&mut h);
    format!("{:x}", h.finish())
}

/// Friendly labels for the assumption-driver grid.
const DRIVER_LABELS: &[(&str, &str)] = &[
    ("revenue_growth_pct", "Revenue growth %"),
    ("gross_margin_pct", "Gross margin %"),
    ("sga_pct_rev", "SG&A % of revenue"),
    ("rd_pct_rev", "R&D % of revenue"),
    ("da_pct_rev", "D&A % of revenue"),
    ("capex_pct_rev", "Capex % of revenue"),
    ("tax_rate_pct", "Tax rate %"),
    ("interest_rate_pct", "Interest rate %"),
    ("dso_days", "DSO (days)"),
    ("dio_days", "DIO (days)"),
    ("dpo_days", "DPO (days)"),
    ("dividend_per_share", "Dividend / share"),
];

/// Blocking core for step 1 of the two-step build: extract + derive drivers (no
/// Excel). Caches the extraction under a returned `session_id` so
/// [`finalize_model_core`] can reuse it. Shared by the [`prepare_model`] command
/// and the chat `build_model` tool (review path).
pub(crate) fn prepare_model_core(
    app: &tauri::AppHandle,
    ticker: &str,
    opts: fm_build::BuildOptions,
) -> AppResult<String> {
    let t = ticker.trim().to_string();
    if t.is_empty() {
        return Err(AppError::Config("Enter a ticker (e.g. SAND.ST).".into()));
    }
    let (extraction, source) = obtain_extraction(app, &t)?;
    let (block, hist, proj) = fm_build::prepare_assumptions(&extraction, &t, &opts);

    let base = &block.base;
    let field = |k: &str| -> &Vec<f64> {
        match k {
            "revenue_growth_pct" => &base.revenue_growth_pct,
            "gross_margin_pct" => &base.gross_margin_pct,
            "sga_pct_rev" => &base.sga_pct_rev,
            "rd_pct_rev" => &base.rd_pct_rev,
            "da_pct_rev" => &base.da_pct_rev,
            "capex_pct_rev" => &base.capex_pct_rev,
            "tax_rate_pct" => &base.tax_rate_pct,
            "interest_rate_pct" => &base.interest_rate_pct,
            "dso_days" => &base.dso_days,
            "dio_days" => &base.dio_days,
            "dpo_days" => &base.dpo_days,
            _ => &base.dividend_per_share,
        }
    };
    let mut drivers = serde_json::Map::new();
    let mut labels = serde_json::Map::new();
    for (k, label) in DRIVER_LABELS {
        drivers.insert((*k).to_string(), serde_json::json!(field(k)));
        labels.insert((*k).to_string(), serde_json::json!(label));
    }
    let sid = session_id(&t);
    app.state::<SessionCache>()
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(sid.clone(), (extraction.clone(), t.clone(), source.to_string()));
    Ok(serde_json::json!({
        "session_id": sid,
        "ticker": t,
        "currency": extraction.currency,
        "source": source,
        "hist_periods": hist,
        "proj_periods": proj,
        "drivers": drivers,
        "labels": labels,
    })
    .to_string())
}

/// Step 1 of the two-step build: extract + derive drivers (no Excel). Caches the
/// extraction under a returned `session_id` so [`finalize_model`] reuses it.
#[tauri::command(rename_all = "snake_case")]
pub async fn prepare_model(
    app: tauri::AppHandle,
    ticker: String,
    options: Option<fm_build::BuildOptions>,
) -> AppResult<String> {
    let opts = options.unwrap_or_default();
    tauri::async_runtime::spawn_blocking(move || prepare_model_core(&app, &ticker, opts))
        .await
        .map_err(|e| AppError::Engine(format!("prepare task failed: {e}")))?
}

/// Blocking core for step 2: pull the cached extraction, apply the grid
/// overrides in `opts`, build + render. Shared by the [`finalize_model`] command
/// and the chat assumptions card.
pub(crate) fn finalize_model_core(
    app: &tauri::AppHandle,
    session_id: &str,
    opts: fm_build::BuildOptions,
) -> AppResult<String> {
    let entry = app
        .state::<SessionCache>()
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(session_id)
        .cloned();
    let (extraction, ticker, source) = entry.ok_or_else(|| {
        AppError::Config("session expired (app restarted?) — rebuild the model".into())
    })?;
    render_build(app, &extraction, &source, &ticker, opts)
}

/// Step 2: pull the cached extraction, apply the grid overrides in `options`,
/// build + render, and return the same summary as [`build_model`].
#[tauri::command(rename_all = "snake_case")]
pub async fn finalize_model(
    app: tauri::AppHandle,
    session_id: String,
    options: Option<fm_build::BuildOptions>,
) -> AppResult<String> {
    let opts = options.unwrap_or_default();
    tauri::async_runtime::spawn_blocking(move || finalize_model_core(&app, &session_id, opts))
        .await
        .map_err(|e| AppError::Engine(format!("finalize task failed: {e}")))?
}

/// Append a generated file to the Recent list (4.2), capped at 10, dedup by path.
fn push_recent(app: &tauri::AppHandle, path: &str, label: &str) {
    use crate::commands::settings::{read_settings, write_settings, RecentEntry};
    let mut s = read_settings(app);
    s.recent.retain(|r| r.path != path);
    s.recent.insert(
        0,
        RecentEntry { path: path.to_string(), label: label.to_string(), when: today_iso_local() },
    );
    s.recent.truncate(10);
    let _ = write_settings(app, &s);
}

/// Local date stamp for Recent entries.
fn today_iso_local() -> String {
    fm_extract::today_iso()
}

/// Recent generated files (4.2), most-recent-first. `{ path, label, when }`.
#[tauri::command(rename_all = "snake_case")]
pub fn list_recent(app: tauri::AppHandle) -> AppResult<String> {
    let s = read_settings(&app);
    serde_json::to_string(&s.recent).map_err(|e| AppError::Engine(e.to_string()))
}

/// Open a file with the OS default handler (the generated Excel).
#[tauri::command(rename_all = "snake_case")]
pub fn open_path(app: tauri::AppHandle, path: String) -> AppResult<String> {
    app.opener()
        .open_path(path.clone(), None::<&str>)
        .map_err(|e| AppError::Io(format!("open failed: {e}")))?;
    Ok(path)
}

/// Open a URL in the default browser (news headlines, external links).
#[tauri::command(rename_all = "snake_case")]
pub fn open_url(app: tauri::AppHandle, url: String) -> AppResult<String> {
    app.opener()
        .open_url(url.clone(), None::<&str>)
        .map_err(|e| AppError::Io(format!("open url failed: {e}")))?;
    Ok(url)
}
