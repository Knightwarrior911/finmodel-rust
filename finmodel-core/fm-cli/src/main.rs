//! fm-cli: integration CLI for the finmodel-core pipeline.
use std::path::PathBuf;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "fm", about = "Financial model engine CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Score a model JSON against ground truth (tie-out)
    Score {
        #[arg(short, long)]
        ground_truth: PathBuf,
        #[arg(short, long)]
        model: PathBuf,
    },
    /// Validate a snapshot JSON's structure
    Compare {
        #[arg(short, long)]
        snapshot: PathBuf,
    },
    /// (Stub) Build a full model for a ticker
    Build { ticker: String },
    /// Verify all committed snapshots (stub)
    Verify {},
}

fn cmd_score(gt_path: &PathBuf, model_path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let gt = fm_tieout::load_ground_truth(gt_path)?;
    let model = fm_tieout::load_model(model_path)?;
    let score = fm_tieout::score(&gt, &model);
    println!("Tie-out Score:");
    println!("  Trusted:  {}", score.trusted);
    println!("  Matched:  {}", score.matched);
    println!("  Pct:      {:.1}%", score.percentage);
    if score.mismatches.is_empty() {
        println!("  ✓ All cells match");
    } else {
        println!("  ✗ Mismatches:");
        for m in &score.mismatches {
            println!("    - {:?}", m);
        }
    }
    Ok(())
}

fn cmd_compare(snapshot_path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let snapshot = fm_excel::compare::load_snapshot(
        &snapshot_path.to_string_lossy())?;
    println!("Snapshot: {snapshot_path:?}");
    let sheets = snapshot.get("sheets").and_then(|s| s.as_object());
    match sheets {
        Some(s) => println!("  ✓ {} sheet(s) in snapshot", s.len()),
        None => println!("  ⚠ No 'sheets' key in snapshot"),
    }
    Ok(())
}

/// Sanitize a ticker to the fixture filename stem (e.g. "SAND.ST" -> "SAND_ST").
fn ticker_to_stem(ticker: &str) -> String {
    ticker.replace(['.', '/'], "_")
}

/// Map a ticker's exchange suffix to its reporting currency.
fn currency_for_ticker(ticker: &str) -> &'static str {
    let up = ticker.to_uppercase();
    if up.ends_with(".ST") { "SEK" }        // Stockholm
    else if up.ends_with(".CO") { "DKK" }   // Copenhagen
    else if up.ends_with(".SW") { "CHF" }   // SIX Swiss
    else if up.ends_with(".AS") { "EUR" }   // Euronext Amsterdam
    else if up.ends_with(".PA") { "EUR" }   // Euronext Paris
    else if up.ends_with(".DE") { "EUR" }   // Xetra
    else if up.ends_with(".L") { "GBP" }    // London
    else if up.ends_with(".TO") { "CAD" }   // Toronto
    else if up.ends_with(".T") { "JPY" }    // Tokyo
    else { "USD" }                          // US / default
}

/// Try to load a committed extraction fixture for a ticker (offline path).
/// Searches a few candidate locations relative to cwd and the exe.
fn load_fixture_extraction(ticker: &str) -> Option<fm_extract::ExtractionResult> {
    let stem = ticker_to_stem(ticker);
    let candidates = [
        format!("fm-cli/tests/fixtures/{stem}_model.json"),
        format!("tests/fixtures/{stem}_model.json"),
        format!("finmodel-core/fm-cli/tests/fixtures/{stem}_model.json"),
        format!("../fm-cli/tests/fixtures/{stem}_model.json"),
    ];
    for path in &candidates {
        if let Ok(text) = std::fs::read_to_string(path) {
            if let Ok(mut val) = serde_json::from_str::<serde_json::Value>(&text) {
                // Fixtures lack currency — derive from the ticker's exchange suffix.
                if val.get("currency").is_none() {
                    val["currency"] = serde_json::json!(currency_for_ticker(ticker));
                }
                if let Ok(result) = serde_json::from_value::<fm_extract::ExtractionResult>(val) {
                    println!("  [offline] loaded committed fixture: {path}");
                    return Some(result);
                }
            }
        }
    }
    None
}

fn cmd_build(ticker: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("Build pipeline for {ticker}");

    // Step 1: Obtain extraction.
    //   - Key present: try LIVE fetch first (real product), fall back to fixture on failure.
    //   - No key: use committed fixture (offline demo). Error if neither works.
    let has_key = std::env::var("OPENROUTER_API_KEY").map(|k| !k.trim().is_empty()).unwrap_or(false);
    let extraction = if has_key {
        println!("  OPENROUTER_API_KEY set — attempting live fetch...");
        match fm_extract::fetch_xbrl(ticker) {
            Ok(e) => e,
            Err(live_err) => {
                println!("  live fetch failed ({live_err}); falling back to committed fixture");
                load_fixture_extraction(ticker).ok_or_else(|| format!(
                    "live fetch failed and no committed fixture for {ticker}"
                ))?
            }
        }
    } else {
        load_fixture_extraction(ticker).ok_or_else(|| format!(
            "No committed fixture for {ticker} and no OPENROUTER_API_KEY set.\n\
             Offline demo tickers: SAND.ST, ASML.AS, NOVO-B.CO, NESN.SW, ATCO-B.ST.\n\
             Live run: set OPENROUTER_API_KEY (see docs/DEMO.md)."
        ))?
    };

    let n_is = extraction.income_statement.len();
    let n_bs = extraction.balance_sheet.len();
    let n_cfs = extraction.cash_flow_statement.len();
    let ny = extraction.years_found.len();
    println!("  extracted: IS({n_is}) BS({n_bs}) CFS({n_cfs}) across {ny} years in {ccy}",
        ccy = extraction.currency);

    // Step 2: Reconcile for the engine.
    let years = extraction.years_found.clone();
    let data = fm_types::ReconciledData {
        income_statement: extraction.income_statement.clone(),
        balance_sheet: extraction.balance_sheet.clone(),
        cash_flow_statement: extraction.cash_flow_statement.clone(),
        periods: years,
        currency: extraction.currency.clone(),
    };

    // Step 3: Project.
    println!("  reconcile -> project...");
    let config = fm_types::CompanyConfig {
        name: ticker.to_string(),
        currency: extraction.currency.clone(),
        hist_periods: extraction.years_found.len(),
        proj_periods: 5,
        ..Default::default()
    };
    let engine = fm_engine::ModelEngine::new(data, config);
    let assumptions = engine.derive_assumptions();
    let projected = engine.project(&std::collections::HashMap::new());
    println!("  assumptions: {} drivers", assumptions.len());

    // Step 4: Write Excel — the actual deliverable.
    let stem = ticker_to_stem(ticker);
    let xlsx_path = format!("{stem}_model.xlsx");
    let sheets = build_excel_sheets(&extraction, &projected);
    fm_excel::writer::write_workbook(&xlsx_path, &sheets)?;
    println!("  \u{2713} wrote Excel model -> {xlsx_path}");

    // Step 5: Also save the projection JSON for inspection.
    let model_path = format!("{stem}_projection.json");
    let model_output = serde_json::json!({
        "ticker": ticker,
        "projected": projected,
        "assumptions": assumptions,
    });
    std::fs::write(&model_path, serde_json::to_string_pretty(&model_output)?)?;
    println!("  \u{2713} wrote projection -> {model_path}");

    Ok(())
}

/// Build Excel sheets (IS/BS/CFS) combining historical + projected columns.
fn build_excel_sheets(
    extraction: &fm_extract::ExtractionResult,
    projected: &fm_types::ProjectedStatements,
) -> Vec<fm_excel::writer::SheetData> {
    use fm_excel::writer::{CellValue, SheetData};

    let hist_years = &extraction.years_found;
    let proj_periods = &projected.periods;
    let mut headers = vec!["Item".to_string()];
    for y in hist_years { headers.push(y.clone()); }
    for p in proj_periods { headers.push(p.clone()); }

    let make_sheet = |name: &str,
                      hist: &std::collections::HashMap<String, Vec<Option<f64>>>,
                      proj: &std::collections::HashMap<String, Vec<Option<f64>>>| -> SheetData {
        // Union of keys, hist first ordering preserved by sorting for determinism.
        let mut keys: Vec<String> = hist.keys().chain(proj.keys()).cloned().collect();
        keys.sort();
        keys.dedup();
        let mut rows = Vec::new();
        for key in keys {
            let mut cells = Vec::new();
            if let Some(hv) = hist.get(&key) {
                for v in hv { cells.push(v.map(CellValue::Value).unwrap_or(CellValue::Empty)); }
            } else {
                for _ in hist_years { cells.push(CellValue::Empty); }
            }
            if let Some(pv) = proj.get(&key) {
                for v in pv { cells.push(v.map(CellValue::Value).unwrap_or(CellValue::Empty)); }
            }
            rows.push((key, cells));
        }
        SheetData { name: name.to_string(), headers: headers.clone(), rows }
    };

    vec![
        make_sheet("Income Statement", &extraction.income_statement, &projected.income_statement),
        make_sheet("Balance Sheet", &extraction.balance_sheet, &projected.balance_sheet),
        make_sheet("Cash Flow", &extraction.cash_flow_statement, &projected.cash_flow),
    ]
}

fn cmd_verify() -> Result<(), Box<dyn std::error::Error>> {
    println!("Verifying all snapshots...");
    println!("  All snapshots verified (stub)");
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    match cli.command {
        Command::Score { ground_truth, model } => cmd_score(&ground_truth, &model),
        Command::Compare { snapshot } => cmd_compare(&snapshot),
        Command::Build { ticker } => cmd_build(&ticker),
        Command::Verify {} => cmd_verify(),
    }
}
