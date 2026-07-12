//! Peer-benchmark bridge command — fetches SEC EDGAR XBRL for a set of tickers,
//! computes the filings benchmark, and writes an IB-grade comparison workbook to
//! Documents/finmodel/. Mirrors the CLI `fm benchmark`.

use tauri::Manager;

use crate::error::{AppError, AppResult};

/// Benchmark a comma-separated peer set. Returns a JSON summary the UI renders
/// plus the path to the generated Excel file.
#[tauri::command(rename_all = "snake_case")]
pub async fn benchmark_peers(app: tauri::AppHandle, tickers: String) -> AppResult<String> {
    // Live HTTP fetch + file I/O — run off the IPC thread.
    tauri::async_runtime::spawn_blocking(move || benchmark_blocking(&app, &tickers))
        .await
        .map_err(|e| AppError::Engine(format!("benchmark task failed: {e}")))?
}

fn benchmark_blocking(app: &tauri::AppHandle, tickers: &str) -> AppResult<String> {
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

    let title = format!("Peer Benchmark — {}", list.join(", "));
    let run = fm_research::benchmark_tickers(&list, &title).map_err(|e| {
        AppError::Engine(format!(
            "no usable filing data for any ticker ({e}) — US-listed tickers only (SEC EDGAR)"
        ))
    })?;

    // Write to Documents/finmodel/.
    let out_dir = app
        .path()
        .document_dir()
        .map_err(|e| AppError::Io(format!("no documents dir: {e}")))?
        .join("finmodel");
    std::fs::create_dir_all(&out_dir)?;
    let stem = stem_for(&list);
    let xlsx_path = out_dir.join(format!("{stem}.xlsx"));
    let csv_path = out_dir.join(format!("{stem}.csv"));

    let generated = fm_research::generated_stamp(&fm_research::today_iso());
    fm_research::render_benchmark(&run.table, &xlsx_path.to_string_lossy(), &generated)
        .map_err(|e| AppError::Engine(format!("Excel write failed: {e}")))?;
    std::fs::write(&csv_path, run.table.to_csv())?;

    // JSON summary for the UI: per-company headline metrics + any failures.
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

    Ok(serde_json::json!({
        "title": title,
        "count": run.metrics.len(),
        "requested": list.len(),
        "rows": rows,
        "failed": failed,
        "xlsx_path": xlsx_path.to_string_lossy(),
        "csv_path": csv_path.to_string_lossy(),
    })
    .to_string())
}

/// Filename stem from the peer set — first few sanitized tickers, capped.
fn stem_for(list: &[String]) -> String {
    let joined: String = list
        .iter()
        .take(4)
        .map(|t| t.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>())
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
