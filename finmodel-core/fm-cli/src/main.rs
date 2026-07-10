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

fn cmd_build(ticker: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("Build pipeline for {ticker}");

    // Step 1: Fetch + extract
    println!("  fetch -> extract...");
    let extraction = fm_extract::fetch_xbrl(ticker)
        .map_err(|e| format!("extraction failed: {e}"))?;

    // Print summary
    let n_is = extraction.income_statement.len();
    let n_bs = extraction.balance_sheet.len();
    let n_cfs = extraction.cash_flow_statement.len();
    let ny = extraction.years_found.len();
    println!("  extracted: IS({n_is}) BS({n_bs}) CFS({n_cfs}) across {ny} years in {ccy}",
        ccy = extraction.currency);

    // Step 2: Save extraction result
    let extract_path = format!("{ticker}_extraction.json");
    let extract_json = serde_json::to_string_pretty(&extraction)?;
    std::fs::write(&extract_path, &extract_json)?;
    println!("  saved -> {extract_path}");

    // Step 3: Reconcile data for engine
    let years = extraction.years_found.clone();
    let data = fm_types::ReconciledData {
        income_statement: extraction.income_statement,
        balance_sheet: extraction.balance_sheet,
        cash_flow_statement: extraction.cash_flow_statement,
        periods: years,
        currency: extraction.currency.clone(),
    };

    // Step 4: Project model
    println!("  reconcile -> project...");
    let config = fm_types::CompanyConfig {
        name: ticker.to_string(),
        currency: extraction.currency.clone(),
        hist_periods: extraction.years_found.len(),
        proj_periods: 3,
        ..Default::default()
    };
    let engine = fm_engine::ModelEngine::new(data, config);
    let assumptions = engine.derive_assumptions();
    let projected = engine.project(&std::collections::HashMap::new());

    println!("  assumptions: {} drivers", assumptions.len());
    let np = projected.periods.len();
    let mut total_keys = 0;
    for stmt in [&projected.income_statement, &projected.balance_sheet, &projected.cash_flow] {
        total_keys += stmt.len();
    }
    println!("  projected: {total_keys} keys across {np} periods");

    // Step 5: Save projection
    let model_path = format!("{ticker}_model.json");
    let model_output = serde_json::json!({
        "ticker": ticker,
        "projected": projected,
        "assumptions": assumptions,
    });
    let model_json = serde_json::to_string_pretty(&model_output)?;
    std::fs::write(&model_path, &model_json)?;
    println!("  saved -> {model_path}");

    Ok(())
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
