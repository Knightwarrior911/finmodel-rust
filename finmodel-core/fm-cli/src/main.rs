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
    println!("Build pipeline for {ticker} (stub)");
    println!("  fetch -> extract -> reconcile -> project -> value -> write");
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
