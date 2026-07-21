//! Offline answer-quality sweep CLI: grade produced answer artifacts against
//! gold and print a ranked report.
//!
//! Usage:
//!   quality_sweep <artifacts.json> <gold.json> [min_mean_overall]
//!
//! `artifacts.json` is a JSON array of `AnswerArtifact` objects
//! (`{model, prompt_variant, case_id, answer}`) — a live model×prompt producer
//! writes one per cell. `gold.json` is the versioned wrapper committed as
//! `gold_answers.json` (`{ "schema_version": .., "gold": [ … ] }`). The winning
//! variant's mean must be ≥ the optional floor (default 0.0 = report only), else
//! the process exits non-zero — so this doubles as a CI quality gate over real
//! model output. Grading is identical to the in-process `run_sweep`.

use std::process::ExitCode;

use fm_research::quality_eval::{report_sweep, run_sweep_from_json};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 || args.len() > 4 {
        eprintln!(
            "usage: {} <artifacts.json> <gold.json> [min_mean_overall]",
            args.first().map(String::as_str).unwrap_or("quality_sweep")
        );
        return ExitCode::from(2);
    }

    let artifacts = match std::fs::read_to_string(&args[1]) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cannot read {}: {e}", args[1]);
            return ExitCode::from(2);
        }
    };
    let gold = match std::fs::read_to_string(&args[2]) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cannot read {}: {e}", args[2]);
            return ExitCode::from(2);
        }
    };
    // Floor is a mean overall in 0.0..=1.0; reject junk / out-of-range so a
    // typo like "1.5" or "NaN" can never make the gate trivially pass or fail.
    let floor: f64 = match args.get(3) {
        None => 0.0,
        Some(s) => match s.parse::<f64>() {
            Ok(f) if f.is_finite() && (0.0..=1.0).contains(&f) => f,
            _ => {
                eprintln!("invalid min_mean_overall {s:?}: expected a number in 0.0..=1.0");
                return ExitCode::from(2);
            }
        },
    };

    let report = match run_sweep_from_json(&artifacts, &gold) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("sweep failed: {e}");
            return ExitCode::from(2);
        }
    };
    print!("{}", report_sweep(&report));

    match report.best() {
        Some(best) if best.mean_overall < floor => {
            eprintln!(
                "FAIL: best {}::{} mean {:.3} < floor {floor:.3}",
                best.model, best.prompt_variant, best.mean_overall
            );
            ExitCode::FAILURE
        }
        Some(_) => ExitCode::SUCCESS,
        None => {
            eprintln!("no gradable variant (no artifact matched a gold case)");
            ExitCode::FAILURE
        }
    }
}
