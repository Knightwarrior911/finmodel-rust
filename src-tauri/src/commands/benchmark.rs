//! Peer-benchmark bridge command — fetches SEC EDGAR XBRL for a set of tickers,
//! computes the filings benchmark, and writes an IB-grade comparison workbook to
//! Documents/finmodel/. Mirrors the CLI `fm benchmark`.

use tauri::{Emitter, Manager};

use crate::commands::settings::read_settings;
use crate::error::{AppError, AppResult};

/// Options for a benchmark run (from the UI Benchmark card).
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(default)]
pub struct BenchOpts {
    /// Reporting basis: "annual" | "ltm" | "quarter" | "semi".
    pub period: String,
    /// Add trading multiples (live prices).
    pub multiples: bool,
    /// Convert monetary metrics to USD at spot FX.
    pub usd: bool,
    pub title: Option<String>,
    /// Explicit output .xlsx path (Save-As); `None` → Documents/finmodel/.
    pub out_path: Option<String>,
    /// Also write a `<stem>_deck.pptx` peer-comparison deck.
    pub deck: bool,
}

impl Default for BenchOpts {
    fn default() -> Self {
        Self {
            period: "annual".into(),
            multiples: false,
            usd: false,
            title: None,
            out_path: None,
            deck: false,
        }
    }
}

/// Benchmark a comma-separated peer set. Returns a JSON summary the UI renders
/// plus the path to the generated Excel file.
#[tauri::command(rename_all = "snake_case")]
pub async fn benchmark_peers(
    app: tauri::AppHandle,
    tickers: String,
    opts: Option<BenchOpts>,
) -> AppResult<String> {
    let opts = opts.unwrap_or_default();
    tauri::async_runtime::spawn_blocking(move || benchmark_blocking(&app, &tickers, opts))
        .await
        .map_err(|e| AppError::Engine(format!("benchmark task failed: {e}")))?
}

pub(crate) fn benchmark_blocking(
    app: &tauri::AppHandle,
    tickers: &str,
    opts: BenchOpts,
) -> AppResult<String> {
    let list: Vec<String> = tickers
        .split(',')
        .map(|t| t.trim().to_uppercase())
        .filter(|t| !t.is_empty())
        .collect();
    if list.is_empty() {
        return Err(AppError::Config(
            "Enter comma-separated tickers, e.g. AAPL, MSFT, GOOGL.".into(),
        ));
    }

    let settings = read_settings(app);
    if !settings.edgar_contact.trim().is_empty() {
        fm_fetch::edgar::set_edgar_contact(settings.edgar_contact.trim().to_string());
    }

    let basis = match opts.period.to_lowercase().as_str() {
        "ltm" => fm_research::PeriodBasis::Ltm,
        "quarter" => fm_research::PeriodBasis::Quarter,
        "semi" => fm_research::PeriodBasis::SemiAnnual,
        _ => fm_research::PeriodBasis::AnnualFy,
    };
    let title = opts
        .title
        .clone()
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| format!("Peer Benchmark — {}", list.join(", ")));
    let run = fm_research::benchmark_tickers_opts_progress(
        &list,
        &title,
        fm_research::BenchmarkOpts { basis, multiples: opts.multiples, to_usd: opts.usd },
        |i, n, t| {
            let _ = app.emit(
                "build_progress",
                serde_json::json!({ "stage": "fetch", "detail": format!("Fetching {t} ({i} of {n})…") }),
            );
        },
    )
    .map_err(|e| {
        AppError::Engine(format!(
            "no usable filing data for any ticker ({e}) — US-listed tickers only (SEC EDGAR)"
        ))
    })?;

    // Write to the chosen path, else Documents/finmodel/.
    let (xlsx_path, csv_path) =
        if let Some(p) = opts.out_path.as_ref().filter(|p| !p.trim().is_empty()) {
            let pb = std::path::PathBuf::from(p);
            let csv = pb.with_extension("csv");
            (pb, csv)
        } else {
            let out_dir = if !settings.out_dir.trim().is_empty() {
                std::path::PathBuf::from(settings.out_dir.trim())
            } else {
                app.path()
                    .document_dir()
                    .map_err(|e| AppError::Io(format!("no documents dir: {e}")))?
                    .join("finmodel")
            };
            std::fs::create_dir_all(&out_dir)?;
            let stem = stem_for(&list);
            (
                out_dir.join(format!("{stem}.xlsx")),
                out_dir.join(format!("{stem}.csv")),
            )
        };
    if let Some(parent) = xlsx_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let generated = fm_research::generated_stamp(&fm_research::today_iso());
    fm_research::render_benchmark(&run.table, &xlsx_path.to_string_lossy(), &generated)
        .map_err(|e| AppError::Engine(format!("Excel write failed: {e}")))?;
    std::fs::write(&csv_path, run.table.to_csv())?;

    // Record in Recent files (4.2).
    {
        use crate::commands::settings::{write_settings, RecentEntry};
        let mut sset = read_settings(app);
        let p = xlsx_path.to_string_lossy().to_string();
        sset.recent.retain(|r| r.path != p);
        sset.recent.insert(
            0,
            RecentEntry {
                path: p,
                label: format!("Benchmark — {}", list.join(", ")),
                when: fm_extract::today_iso(),
            },
        );
        sset.recent.truncate(10);
        let _ = write_settings(app, &sset);
    }

    // JSON summary for the UI: per-company headline metrics + failures + warnings.
    let rows: Vec<serde_json::Value> = run
        .metrics
        .iter()
        .map(|m| {
            serde_json::json!({
                "ticker": m.ticker,
                "currency": m.currency,
                "sector": m.sector,
                "fiscal_year": m.fiscal_year,
                "revenue_m": m.revenue.map(|v| v / 1_000_000.0),
                "ebitda_m": m.ebitda.map(|v| v / 1_000_000.0),
                "net_income_m": m.net_income.map(|v| v / 1_000_000.0),
                "rev_growth": m.revenue_growth,
                "ebitda_margin": m.ebitda_margin,
                "net_margin": m.net_margin,
                "roe": m.roe,
                "net_debt_to_ebitda": m.net_debt_to_ebitda,
            })
        })
        .collect();
    let failed: Vec<serde_json::Value> = run
        .failed
        .iter()
        .map(|(t, why)| serde_json::json!({ "ticker": t, "why": why }))
        .collect();

    // Optional one-click peer-comparison PPTX deck.
    let mut pptx_path = serde_json::Value::Null;
    if opts.deck {
        let deck_out = xlsx_path.with_file_name(format!("{}_deck.pptx", stem_for(&list)));
        let pct = |v: Option<f64>| {
            v.map(|x| format!("{:.1}%", x * 100.0))
                .unwrap_or_else(|| "—".into())
        };
        let headers = vec![
            "Ticker".to_string(),
            "Revenue (M)".to_string(),
            "EBITDA margin".to_string(),
            "Net margin".to_string(),
            "ROE".to_string(),
        ];
        let drows: Vec<Vec<String>> = run
            .metrics
            .iter()
            .take(14)
            .map(|m| {
                vec![
                    m.ticker.clone(),
                    m.revenue
                        .map(|v| format!("{:.0}", v / 1_000_000.0))
                        .unwrap_or_else(|| "—".into()),
                    pct(m.ebitda_margin),
                    pct(m.net_margin),
                    pct(m.roe),
                ]
            })
            .collect();
        match fm_pptx::writer::deck::write_benchmark_deck(
            &title,
            &headers,
            &drows,
            &fm_extract::today_iso(),
        )
        .and_then(|d| d.save(&deck_out.to_string_lossy()))
        {
            Ok(p) => pptx_path = serde_json::Value::String(p),
            Err(e) => eprintln!("warning: benchmark deck not written ({e})"),
        }
    }

    Ok(serde_json::json!({
        "title": title,
        "count": run.metrics.len(),
        "requested": list.len(),
        "rows": rows,
        "failed": failed,
        "data_warnings": run.data_warnings,
        "xlsx_path": xlsx_path.to_string_lossy(),
        "csv_path": csv_path.to_string_lossy(),
        "pptx_path": pptx_path,
    })
    .to_string())
}

/// Filename stem from the peer set — first few sanitized tickers, capped.
fn stem_for(list: &[String]) -> String {
    let joined: String = list
        .iter()
        .take(4)
        .map(|t| {
            t.chars()
                .filter(|c| c.is_ascii_alphanumeric())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("_");
    if joined.is_empty() {
        "peer_benchmark".to_string()
    } else if list.len() > 4 {
        format!("benchmark_{joined}_plus{}", list.len() - 4)
    } else {
        format!("benchmark_{joined}")
    }
}
