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
    /// Diff a snapshot against the workbook generated from it (cell-by-cell)
    Compare {
        #[arg(short, long)]
        snapshot: PathBuf,
    },
    /// (Stub) Build a full model for a ticker
    Build { ticker: String },
    /// Verify all committed snapshots reproduce to zero diffs
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

/// Build the workbook from a snapshot and diff it cell-by-cell; returns the diff count.
fn diff_snapshot(path: &str) -> Result<usize, Box<dyn std::error::Error>> {
    use std::collections::BTreeMap;
    let snap = fm_excel::snapshot::load_snapshot(path)?;
    let input = fm_excel::snapshot::workbook_input_from_snapshot(&snap)?;
    let wb = fm_excel::sheets::build_workbook(&input);
    let diffs = fm_excel::snapshot::compare_workbook(&wb, &snap);
    if diffs.is_empty() {
        println!("  ✓ 0 diffs (all sheets match)");
    } else {
        let mut by: BTreeMap<String, usize> = BTreeMap::new();
        for d in &diffs {
            *by.entry(d.sheet.clone()).or_default() += 1;
        }
        for (s, n) in &by {
            println!("  ✗ {s}: {n} diffs");
        }
        for d in diffs.iter().take(20) {
            println!("      {d}");
        }
    }
    Ok(diffs.len())
}

fn cmd_compare(snapshot_path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    println!("Comparing {snapshot_path:?}");
    let n = diff_snapshot(&snapshot_path.to_string_lossy())?;
    if n > 0 {
        return Err(format!("{n} cell diff(s)").into());
    }
    Ok(())
}

/// Try to load a committed extraction fixture for a ticker (offline path).
fn load_fixture_extraction(ticker: &str) -> Option<fm_extract::ExtractionResult> {
    let stem = fm_build::ticker_to_stem(ticker);
    let candidates = [
        format!("fm-cli/tests/fixtures/{stem}_model.json"),
        format!("tests/fixtures/{stem}_model.json"),
        format!("finmodel-core/fm-cli/tests/fixtures/{stem}_model.json"),
        format!("../fm-cli/tests/fixtures/{stem}_model.json"),
    ];
    for path in &candidates {
        if let Ok(text) = std::fs::read_to_string(path) {
            if let Ok(mut val) = serde_json::from_str::<serde_json::Value>(&text) {
                if val.get("currency").is_none() {
                    val["currency"] = serde_json::json!(fm_build::currency_for_ticker(ticker));
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

    // Step 1: Obtain extraction — live-first when key, else committed fixture.
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

    // Step 2: reconcile + project + assemble sheets (SHARED fm-build core).
    println!("  reconcile -> project...");
    let out = fm_build::build(&extraction, ticker, 5);

    // Step 3: Write Excel — the actual deliverable.
    let stem = fm_build::ticker_to_stem(ticker);
    let xlsx_path = format!("{stem}_model.xlsx");
    fm_excel::render::render(&out.workbook, &xlsx_path)?;
    println!("  \u{2713} wrote Excel model -> {xlsx_path}");

    // Step 4: Save the projection JSON for inspection.
    let model_path = format!("{stem}_projection.json");
    let model_output = serde_json::json!({ "ticker": ticker, "projected": out.projected });
    std::fs::write(&model_path, serde_json::to_string_pretty(&model_output)?)?;
    println!("  \u{2713} wrote projection -> {model_path}");

    Ok(())
}

fn cmd_verify() -> Result<(), Box<dyn std::error::Error>> {
    let dir = ["tieout/excel_snapshots", "../tieout/excel_snapshots", "../../tieout/excel_snapshots"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.is_dir())
        .ok_or("could not locate tieout/excel_snapshots (run from the repo or finmodel-core)")?;
    println!("Verifying committed snapshots in {dir:?}");

    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|x| x == "json").unwrap_or(false))
        // The populated-IS oracles (`*_full_snapshot.json`) lack `model_output`
        // and are gated separately by the `full_is_parity` cargo test.
        .filter(|p| !p.file_name().and_then(|n| n.to_str()).map(|n| n.contains("_full_")).unwrap_or(false))
        .collect();
    files.sort();
    if files.is_empty() {
        return Err(format!("no snapshot .json files found in {dir:?}").into());
    }

    let mut total = 0usize;
    for f in &files {
        println!("{}", f.file_name().unwrap().to_string_lossy());
        total += diff_snapshot(&f.to_string_lossy())?;
    }
    if total > 0 {
        return Err(format!("{total} total cell diff(s) across {} snapshot(s)", files.len()).into());
    }
    println!("\n✓ All {} snapshot(s) reproduce to 0 diffs", files.len());
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
