//! `build_model` — the core pipeline command: ticker -> model + Excel.
//!
//! Extraction source:
//! - OpenRouter key set + US ticker     -> live SEC EDGAR fetch
//! - OpenRouter key set + non-US ticker -> PDF discovery + LLM extraction
//! - otherwise                          -> embedded committed fixture (offline demo)
//!
//! Never fabricates data: a non-EDGAR ticker with no fixture and no key returns an error.
//!
//! Reconcile + project + Excel assembly are delegated to the shared `fm_build`
//! crate (same core the CLI uses — no drift).

use tauri::{Emitter, Manager};
use tauri_plugin_opener::OpenerExt;

/// The generated-workbook filename for a ticker — the single naming source
/// shared by the build command and the write-risk refinement (Task 4.3), so the
/// two never drift.
pub fn model_filename(ticker: &str) -> String {
    format!("{}_model.xlsx", fm_build::ticker_to_stem(ticker))
}

/// The default output root for generated workbooks when the analyst hasn't set an
/// explicit path: `{documents}/finmodel`. Single source of truth for
/// `build_model`'s target root + the write-risk refinement.
pub fn default_output_root(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    app.path().document_dir().ok().map(|d| d.join("finmodel"))
}

use crate::commands::settings::read_settings;
use crate::error::{AppError, AppResult};

/// Emit a `build_progress` event the UI listens on (4.1). Best-effort.
fn emit_progress(app: &tauri::AppHandle, stage: &str, detail: &str) {
    let _ = app.emit(
        "build_progress",
        serde_json::json!({ "stage": stage, "detail": detail }),
    );
}

// Canonical statement row order for the UI preview — mirrors the order the
// fm-excel sheet builders emit. Keys are canonical (sector-agnostic: bank /
// insurer / REIT tags map onto these same keys), so one list serves all
// sectors. Keys present in the data but absent here sort alphabetically after.
const IS_ORDER: &[&str] = &[
    "revenue",
    "cogs",
    "gross_profit",
    "sga",
    "rd",
    "utility_om",
    "utility_fuel",
    "utility_taxes_other",
    "utility_other",
    "utility_total_opex",
    "da",
    "ebit",
    "ebita",
    "ebitda",
    "interest_expense",
    "interest_income",
    "ebt",
    "income_tax",
    "net_income",
    "nci_income_loss",
    "ni_common",
    "eps_basic",
    "eps_diluted",
    "shares_basic",
    "shares_diluted",
];
const BS_ORDER: &[&str] = &[
    "cash",
    "accounts_receivable",
    "inventory",
    "total_current_assets",
    "ppe_net",
    "goodwill",
    "intangibles_net",
    "total_assets",
    "accounts_payable",
    "deferred_revenue_current",
    "short_term_debt",
    "total_current_liabilities",
    "long_term_debt",
    "deferred_revenue_lt",
    "total_liabilities",
    "retained_earnings",
    "total_equity",
    "redeemable_nci",
];
const CF_ORDER: &[&str] = &[
    "cfo",
    "capex",
    "investments_net_cfi",
    "cfi",
    "dividends_paid",
    "buybacks",
    "cff",
    "fx_effect_on_cash",
    "net_change_cash",
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
            other.split('.').next().unwrap_or(other).replace('-', " ")
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

/// A per-period series for `key` from a statement (missing → 0.0).
fn stmt_series(sd: &fm_types::StatementData, key: &str) -> Vec<f64> {
    sd.get(key)
        .map(|v| v.iter().map(|x| x.unwrap_or(0.0)).collect())
        .unwrap_or_default()
}

/// EBITDA series: prefer the `ebitda` line, else `ebit + da`.
fn ebitda_series(sd: &fm_types::StatementData, n: usize) -> Vec<f64> {
    let direct = stmt_series(sd, "ebitda");
    if direct.iter().any(|v| *v != 0.0) {
        return direct;
    }
    let ebit = stmt_series(sd, "ebit");
    let da = stmt_series(sd, "da");
    (0..n)
        .map(|i| ebit.get(i).copied().unwrap_or(0.0) + da.get(i).copied().unwrap_or(0.0))
        .collect()
}

/// Trading-comps table (headers + rows) for the deck, from assembled comps.
fn comps_deck_table(pc: &fm_value::PublicCompsOutput) -> (Vec<String>, Vec<Vec<String>>) {
    let headers = vec![
        "Ticker".to_string(),
        "EV/Rev".to_string(),
        "EV/EBITDA".to_string(),
        "P/E".to_string(),
    ];
    let mult = |m: Option<f64>| m.map(|v| format!("{v:.1}x")).unwrap_or_else(|| "—".into());
    let rows: Vec<Vec<String>> = pc
        .peers
        .iter()
        .take(14)
        .map(|p| {
            vec![
                p.ticker.clone(),
                mult(p.ev_rev_ltm),
                mult(p.ev_ebitda_ltm),
                mult(p.pe_ltm),
            ]
        })
        .collect();
    (headers, rows)
}

/// Assemble a [`fm_pptx::writer::deck::ModelDeckInput`] from a completed build.
fn model_deck_input(
    ticker: &str,
    currency: &str,
    extraction: &fm_extract::ExtractionResult,
    opts: &fm_build::BuildOptions,
    out: &fm_build::BuildOutput,
) -> fm_pptx::writer::deck::ModelDeckInput {
    let hist_n = extraction.years_found.len();
    let mut periods = extraction.years_found.clone();
    periods.extend(out.projected.periods.iter().cloned());
    let mut revenue = stmt_series(&extraction.income_statement, "revenue");
    revenue.extend(stmt_series(&out.projected.income_statement, "revenue"));
    let proj_n = out.projected.periods.len();
    let mut ebitda = ebitda_series(&extraction.income_statement, hist_n);
    ebitda.extend(ebitda_series(&out.projected.income_statement, proj_n));
    let dcf = out.dcf.as_ref();
    let (comps_headers, comps_rows) = match &opts.public_comps {
        Some(pc) if !pc.peers.is_empty() => comps_deck_table(pc),
        _ => (Vec::new(), Vec::new()),
    };
    let tv_method = dcf
        .map(|d| {
            if d.tv_method == 1 {
                "EBITDA exit multiple"
            } else {
                "Gordon growth"
            }
        })
        .unwrap_or("—")
        .to_string();
    fm_pptx::writer::deck::ModelDeckInput {
        ticker: ticker.to_string(),
        company: ticker.to_string(),
        currency: currency.to_string(),
        periods,
        revenue,
        ebitda,
        hist_n,
        implied_price: dcf.map(|d| d.implied_price).unwrap_or(0.0),
        current_price: dcf.map(|d| d.current_share_price).unwrap_or(0.0),
        upside_pct: dcf.map(|d| d.upside_downside_pct).unwrap_or(0.0),
        wacc: out.wacc_out.as_ref().map(|w| w.wacc).unwrap_or(0.0),
        ev: dcf.map(|d| d.enterprise_value).unwrap_or(0.0),
        tv_method,
        comps_headers,
        comps_rows,
    }
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

    // Live WACC inputs (real risk-free + regression beta) — only for LIVE
    // extractions and only when the caller left the defaults (an explicit user
    // value always wins). Never fail the build over a market fetch.
    if source.starts_with("live") {
        if opts.risk_free_rate == 0.045 {
            match fm_fetch::market::fetch_risk_free_rate() {
                Ok(rf) => {
                    opts.risk_free_rate = rf;
                    warnings.push(format!(
                        "Risk-free rate {:.2}% from ^TNX (live)",
                        rf * 100.0
                    ));
                }
                Err(_) => {
                    warnings.push("Risk-free rate defaulted to 4.5% (live 10Y fetch failed)".into())
                }
            }
        }
        if opts.beta == 1.0 {
            match fm_fetch::market::fetch_beta(ticker) {
                Ok(beta) => {
                    opts.beta = beta;
                    warnings.push(format!(
                        "Beta {beta:.2} from 2y weekly regression vs S&P 500"
                    ));
                }
                Err(_) => warnings.push("Beta defaulted to 1.0 (history fetch failed)".into()),
            }
        }
    }

    // Trading-comps peer assembly (network stays out of fm-build). Each peer
    // failure lands in `excluded`, never fatal. Peers require EDGAR.
    let mut comps_summary = serde_json::Value::Null;
    if !opts.peers.is_empty() {
        let mut peers: Vec<fm_value::PublicCompPeer> = Vec::new();
        let mut excluded: Vec<(String, String)> = Vec::new();
        for t in &opts.peers {
            emit_progress(app, "comps", &format!("Fetching peer {t}"));
            match fm_extract::fetch_xbrl(t) {
                Ok(ex) => {
                    let m = fm_research::metrics_from_extraction(t, &ex);
                    let quote = fm_fetch::fetch_quote(t).ok();
                    peers.push(fm_research::comps::peer_from_metrics(&m, quote.as_ref()));
                }
                Err(e) => excluded.push((t.clone(), e.to_string())),
            }
        }
        let target_metrics = fm_research::metrics_from_extraction(ticker, extraction);
        let count = peers.len();
        let excluded_names: Vec<String> = excluded.iter().map(|(t, _)| t.clone()).collect();
        opts.public_comps = Some(fm_research::comps::build_public_comps(
            &target_metrics,
            &peers,
            excluded,
            &fm_extract::today_iso(),
        ));
        comps_summary = serde_json::json!({ "count": count, "excluded": excluded_names });
    }

    // Two-outcome extraction gate (Phase 6.1): unsafe inputs (non-finite,
    // inconsistent vectors, empty periods, invalid currency) BLOCK file creation
    // — no workbook. A finite accounting imbalance is NOT blocking; it surfaces
    // as a failed Verification in the built workbook (BuildAllowedWithFailures).
    let block_reasons = fm_build::validate_extraction(extraction);
    if !block_reasons.is_empty() {
        return Err(AppError::Config(format!(
            "extraction failed validation — no model written: {}",
            block_reasons.join("; ")
        )));
    }

    // Shared core: reconcile + project + assemble sheets (honoring options).
    emit_progress(app, "project", "Projecting the forecast…");
    let out = fm_build::build_with(extraction, ticker, &opts);
    warnings.extend(out.warnings.iter().cloned());

    // Write Excel — to the analyst's chosen path, else Documents/finmodel/.
    let xlsx_path = if let Some(p) = opts.out_path.as_ref().filter(|p| !p.trim().is_empty()) {
        std::path::PathBuf::from(p)
    } else {
        let out_dir =
            default_output_root(app).ok_or_else(|| AppError::Io("no documents dir".into()))?;
        std::fs::create_dir_all(&out_dir)?;
        out_dir.join(model_filename(ticker))
    };
    if let Some(parent) = xlsx_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    emit_progress(app, "render", "Writing the Excel workbook…");
    fm_excel::render::render(&out.workbook, &xlsx_path.to_string_lossy())
        .map_err(|e| AppError::Engine(format!("Excel write failed: {e}")))?;
    // Record in Recent files (4.2).
    push_recent(
        app,
        &xlsx_path.to_string_lossy(),
        &format!("{ticker} model"),
    );

    // Optional one-click PPTX deck beside the workbook (pure OOXML write, no
    // LibreOffice). Deck failure is a warning, never fatal.
    let mut pptx_path = serde_json::Value::Null;
    if opts.deck {
        emit_progress(app, "deck", "Writing the summary deck…");
        let deck_out =
            xlsx_path.with_file_name(format!("{}_deck.pptx", fm_build::ticker_to_stem(ticker)));
        let input = model_deck_input(ticker, &extraction.currency, extraction, &opts, &out);
        match fm_pptx::writer::deck::write_model_deck(&input, &fm_extract::today_iso())
            .and_then(|d| d.save(&deck_out.to_string_lossy()))
        {
            Ok(p) => {
                push_recent(app, &p, &format!("{ticker} deck"));
                pptx_path = serde_json::Value::String(p);
            }
            Err(e) => warnings.push(format!("Deck not written ({e})")),
        }
    }

    let val_method = out.dcf.as_ref().map(|d| {
        if d.tv_method == 1 {
            "EBITDA exit multiple"
        } else {
            "Gordon growth"
        }
    });
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
        "verification": {
            "passed": out.verification.passed,
            "critical_failures": out.verification.critical_failures,
        },
        "xlsx_path": xlsx_path.to_string_lossy(),
        "comps": comps_summary,
        "pptx_path": pptx_path,
        "case": match opts.active_case { 2 => "upside", 3 => "downside", _ => "base" },
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
    let has_key = crate::commands::settings::has_effective_credentials(&s);
    if let Err(e) = crate::commands::settings::ensure_provider_ready(&s) {
        return Err(AppError::Engine(e));
    }
    let llm_cfg = fm_extract::LlmConfig {
        api_key: crate::commands::settings::effective_api_key(&s),
        model: crate::commands::settings::effective_model(&s),
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
                    let periods = [
                        (year - 2).to_string(),
                        (year - 1).to_string(),
                        year.to_string(),
                    ];
                    match fm_extract::fetch_non_us_filing(
                        &company,
                        ticker,
                        &periods,
                        Some(year),
                        Some(&llm_cfg),
                    ) {
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

/// Analyze a local annual-report PDF into a model (workstream F). Reuses the
/// non-US PDF + LLM extraction path for a file the analyst supplies directly.
/// Requires an OpenRouter key; `source = "pdf"` so no live quote/beta fetch runs.
pub(crate) fn analyze_pdf_blocking(
    app: &tauri::AppHandle,
    path: &str,
    label: &str,
    opts: fm_build::BuildOptions,
) -> AppResult<String> {
    let p = std::path::Path::new(path);
    if !p.is_file() {
        return Err(AppError::Config(format!("PDF not found: {path}")));
    }
    let is_pdf = p
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("pdf"))
        .unwrap_or(false);
    if !is_pdf {
        return Err(AppError::Config(format!("Not a .pdf file: {path}")));
    }
    let label = label.trim();
    let label = if label.is_empty() { "PDF" } else { label };

    let s = read_settings(app);
    if !crate::commands::settings::has_effective_credentials(&s) {
        return Err(AppError::Config(
            "PDF analysis needs an OpenRouter API key (Settings)".into(),
        ));
    }
    let llm_cfg = fm_extract::LlmConfig {
        api_key: crate::commands::settings::effective_api_key(&s),
        model: crate::commands::settings::effective_model(&s),
    };
    // Mirror fetch_non_us_filing's periods: the last full year and the two prior.
    let year = fm_extract::current_year() - 1;
    let periods = [
        (year - 2).to_string(),
        (year - 1).to_string(),
        year.to_string(),
    ];
    emit_progress(app, "extract", "Extracting financials from the PDF…");
    let extraction = fm_extract::extract_financials_from_pdf(path, &periods, label, Some(&llm_cfg))
        .map_err(|e| AppError::Engine(format!("PDF extraction failed: {e}")))?;
    render_build(app, &extraction, "pdf", label, opts)
}

/// Analyze a registered PDF artifact into a model.
/// Accepts only an opaque `artifact_id` (from pick/claim/user-path mint) —
/// never a raw filesystem path from the webview.
#[tauri::command(rename_all = "snake_case")]
pub async fn analyze_pdf(
    app: tauri::AppHandle,
    artifact_id: String,
    conversation_id: String,
    label: Option<String>,
    options: Option<fm_build::BuildOptions>,
) -> AppResult<String> {
    use crate::commands::artifacts::{ArtifactKind, ArtifactRegistry};
    let reg = app
        .try_state::<ArtifactRegistry>()
        .ok_or_else(|| AppError::Config("artifact registry unavailable".into()))?;
    let (path, kind, reg_label) = reg
        .resolve(artifact_id.trim(), Some(conversation_id.trim()))
        .map_err(AppError::Config)?;
    if kind != ArtifactKind::UserPdf {
        return Err(AppError::Config(
            "analyze_pdf requires a UserPdf artifact handle".into(),
        ));
    }
    if !path.is_file()
        || !path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("pdf"))
            .unwrap_or(false)
    {
        return Err(AppError::Config(
            "artifact no longer points at a readable PDF".into(),
        ));
    }
    let label = label.filter(|s| !s.trim().is_empty()).unwrap_or(reg_label);
    let path_str = path.to_string_lossy().to_string();
    let opts = options.unwrap_or_default();
    tauri::async_runtime::spawn_blocking(move || {
        analyze_pdf_blocking(&app, &path_str, &label, opts)
    })
    .await
    .map_err(|e| AppError::Engine(format!("analyze_pdf task failed: {e}")))?
}

/// In-memory prepare→finalize session cache (lost on app restart — acceptable;
/// finalize after restart errors, telling the user to rebuild).
/// Bounded, TTL'd cache of prepared extractions keyed by session id — 16
/// entries / 30-minute TTL with LRU eviction (Phase 3.6). Removed on a
/// successful finalize.
pub struct SessionCache(
    pub  parking_lot::Mutex<
        crate::commands::cache::BoundedCache<
            String,
            (fm_extract::ExtractionResult, String, String),
        >,
    >,
);

impl Default for SessionCache {
    fn default() -> Self {
        Self(parking_lot::Mutex::new(
            crate::commands::cache::BoundedCache::new(16, 30 * 60),
        ))
    }
}

/// Unix seconds — the session cache's injected clock.
pub(crate) fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

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
    app.state::<SessionCache>().0.lock().insert(
        sid.clone(),
        (extraction.clone(), t.clone(), source.to_string()),
        now_secs(),
    );
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
    let cache = app.state::<SessionCache>();
    let entry = cache
        .0
        .lock()
        .get(&session_id.to_string(), now_secs())
        .cloned();
    let (extraction, ticker, source) = entry.ok_or_else(|| {
        AppError::Config("session expired (app restarted?) — rebuild the model".into())
    })?;
    let result = render_build(app, &extraction, &source, &ticker, opts)?;
    // Remove after a successful finalize (bounded state; Phase 3.6).
    cache.0.lock().remove(&session_id.to_string());
    Ok(result)
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
/// Also mints an opaque artifact handle so open_path can allowlist it.
pub(crate) fn push_recent(app: &tauri::AppHandle, path: &str, label: &str) {
    use crate::commands::artifacts::ArtifactRegistry;
    use crate::commands::settings::{read_settings, write_settings, RecentEntry};
    if let Some(reg) = app.try_state::<ArtifactRegistry>() {
        let _ = reg.ensure_generated(std::path::PathBuf::from(path), label);
        // Also allowlist the containing folder so "Show in folder" (which
        // opens the parent dir through open_path's exact-path gate) resolves.
        if let Some(parent) = std::path::Path::new(path).parent() {
            let _ = reg.ensure_generated(parent.to_path_buf(), "folder");
        }
    }
    let mut s = read_settings(app);
    s.recent.retain(|r| r.path != path);
    s.recent.insert(
        0,
        RecentEntry {
            path: path.to_string(),
            label: label.to_string(),
            when: today_iso_local(),
        },
    );
    s.recent.truncate(10);
    let _ = write_settings(app, &s);
}

/// Local date stamp for Recent entries.
fn today_iso_local() -> String {
    fm_extract::today_iso()
}

/// Re-register every persisted Recent path (and its containing folder) into
/// the in-memory artifact registry, so `open_path` still allowlists them after
/// a restart (handles are process-scoped). Called at startup AND by
/// `list_recent` — without a startup call, Open / Show-in-folder buttons on
/// reloaded memo/model cards break on relaunch.
pub(crate) fn rehydrate_recent(app: &tauri::AppHandle) {
    use crate::commands::artifacts::ArtifactRegistry;
    use crate::commands::settings::read_settings;
    let s = read_settings(app);
    if let Some(reg) = app.try_state::<ArtifactRegistry>() {
        for r in &s.recent {
            let _ = reg.ensure_generated(std::path::PathBuf::from(&r.path), &r.label);
            if let Some(parent) = std::path::Path::new(&r.path).parent() {
                let _ = reg.ensure_generated(parent.to_path_buf(), "folder");
            }
        }
    }
}

/// Recent generated files (4.2), most-recent-first.
#[tauri::command(rename_all = "snake_case")]
pub fn list_recent(app: tauri::AppHandle) -> AppResult<String> {
    rehydrate_recent(&app);
    let s = crate::commands::settings::read_settings(&app);
    serde_json::to_string(&s.recent).map_err(|e| AppError::Engine(e.to_string()))
}
/// Open a file with the OS default handler. Accepts either an `artifact_id`
/// (preferred) or a path that is currently registered as a generated artifact.
/// Arbitrary model/webview paths are rejected.
#[tauri::command(rename_all = "snake_case")]
pub fn open_path(app: tauri::AppHandle, path: String) -> AppResult<String> {
    use crate::commands::artifacts::ArtifactRegistry;
    let reg = app
        .try_state::<ArtifactRegistry>()
        .ok_or_else(|| AppError::Config("artifact registry unavailable".into()))?;
    let resolved = if path.starts_with("art-") {
        // Generated outputs are session-wide; user PDFs need conversation scope
        // which open_path does not carry — only generated handles are openable here.
        let (p, kind, _) = reg.resolve(&path, None).map_err(AppError::Config)?;
        if matches!(
            kind,
            crate::commands::artifacts::ArtifactKind::UserPdf
                | crate::commands::artifacts::ArtifactKind::UserFile
        ) {
            // Allow if registered without conversation (legacy) or if resolve
            // with None succeeded above for unscoped.
        }
        p
    } else {
        let p = std::path::PathBuf::from(&path);
        if !reg.contains_path(&p) {
            return Err(AppError::Config(
                "open_path requires a registered artifact handle".into(),
            ));
        }
        p
    };
    let display = resolved.to_string_lossy().to_string();
    app.opener()
        .open_path(display.clone(), None::<&str>)
        .map_err(|e| AppError::Io(format!("open failed: {e}")))?;
    Ok(display)
}

/// Open a URL in the default browser (news headlines, external links).
#[tauri::command(rename_all = "snake_case")]
pub fn open_url(app: tauri::AppHandle, url: String) -> AppResult<String> {
    app.opener()
        .open_url(url.clone(), None::<&str>)
        .map_err(|e| AppError::Io(format!("open url failed: {e}")))?;
    Ok(url)
}
