use std::collections::{HashMap, HashSet};

use crate::types::*;

// ---------------------------------------------------------------------------
// Canonical line-item universe — "industrial" sector (R.1)
// Mirrors CANONICAL_BY_SECTOR["industrial"] in tieout/config.py.
// ---------------------------------------------------------------------------

const CANONICAL_INDUSTRIAL: &[(&str, &[&str])] = &[
    (
        "income_statement",
        &[
            "revenue", "cogs", "gross_profit", "sga", "rd", "da", "ebit",
            "ebita", "interest_expense", "interest_income", "income_tax",
            "net_income",
        ],
    ),
    (
        "balance_sheet",
        &[
            "cash", "accounts_receivable", "inventory",
            "total_current_assets", "ppe_net", "goodwill",
            "intangibles_net", "total_assets", "accounts_payable",
            "long_term_debt", "total_liabilities", "total_equity",
        ],
    ),
    (
        "cash_flow_statement",
        &[
            "cfo", "capex", "cfi", "dividends_paid", "cff",
            "net_change_cash",
        ],
    ),
];

const ABS_KEYS_INDUSTRIAL: &[&str] = &[
    "cogs", "sga", "rd", "interest_expense", "income_tax", "capex",
    "dividends_paid",
];

const EXCLUDE_KEYS_INDUSTRIAL: &[&str] = &["shares_diluted"];

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Normalize a model value: apply `abs()` for known `abs_keys`, then
/// round to the nearest integer — mirrors Python's `_norm()`.
fn normalize_value(key: &str, value: f64, abs_keys: &HashSet<&str>) -> f64 {
    let v = if abs_keys.contains(key) {
        value.abs()
    } else {
        value
    };
    v.round()
}

fn get_gt_statement<'a>(gt: &'a GroundTruth, name: &str) -> &'a GtStatement {
    match name {
        "income_statement" => &gt.values.income_statement,
        "balance_sheet" => &gt.values.balance_sheet,
        "cash_flow_statement" => &gt.values.cash_flow_statement,
        _ => panic!("unknown ground-truth statement: {name}"),
    }
}

fn get_model_statement<'a>(model: &'a ModelOutput, name: &str) -> &'a ModelStatement {
    match name {
        "income_statement" => &model.income_statement,
        "balance_sheet" => &model.balance_sheet,
        "cash_flow_statement" => &model.cash_flow_statement,
        _ => panic!("unknown model statement: {name}"),
    }
}

// ---------------------------------------------------------------------------
// Main comparison
// ---------------------------------------------------------------------------

/// Cell-by-cell comparison of a `ModelOutput` against `GroundTruth`.
///
/// Returns a `Score` with per-statement breakdowns and a list of mismatches.
/// The logic matches Python's `_compare()` in `tieout/run_tieout.py`.
pub fn compare(gt: &GroundTruth, model: &ModelOutput) -> Score {
    let abs_keys: HashSet<&str> = ABS_KEYS_INDUSTRIAL.iter().copied().collect();
    let exclude_keys: HashSet<&str> = EXCLUDE_KEYS_INDUSTRIAL.iter().copied().collect();

    let mut total_trusted: usize = 0;
    let mut total_matched: usize = 0;
    let mut per_stmt: HashMap<String, PerStatementScore> = HashMap::new();
    let mut mismatches: Vec<CellScore> = Vec::new();

    for &(stmt_name, keys) in CANONICAL_INDUSTRIAL {
        let mut s_trusted: usize = 0;
        let mut s_matched: usize = 0;

        let gt_stmt = get_gt_statement(gt, stmt_name);
        let model_stmt = get_model_statement(model, stmt_name);

        for &key in keys {
            // Skip keys explicitly excluded for this sector.
            if exclude_keys.contains(key) {
                continue;
            }

            // Ground-truth year-values for this key.
            let Some(gt_year_map) = gt_stmt.get(key) else {
                continue;
            };
            if gt_year_map.is_empty() {
                continue;
            }

            // Model value list for this key (may be absent).
            let model_vals: Option<&Vec<Option<f64>>> = model_stmt.get(key);

            for &year in &gt.years {
                let year_str = year.to_string();

                // GT value for this year — None means "not on the face
                // statement", so we do NOT count it as a trusted cell.
                let Some(gt_val) = gt_year_map.get(&year_str).copied().flatten() else {
                    continue;
                };

                // This cell IS on the face statement: count it.
                total_trusted += 1;
                s_trusted += 1;

                // Look up the model value by year index.
                let model_val: Option<f64> = model_vals.and_then(|vals| {
                    let idx = gt.years.iter().position(|&y| y == year)?;
                    vals.get(idx).copied()?
                });

                // Normalize the model value (abs + round) to match Python's
                // `_norm()`.
                let model_normalized = model_val
                    .map(|v| normalize_value(key, v, &abs_keys));

                // A match requires the model value to be present AND to equal
                // the ground-truth value after both are rounded.
                let gt_rounded = gt_val.round();
                let is_match = model_normalized
                    .map_or(false, |mv| (mv - gt_rounded).abs() < 0.001);

                if is_match {
                    total_matched += 1;
                    s_matched += 1;
                } else {
                    mismatches.push(CellScore {
                        statement: stmt_name.to_string(),
                        key: key.to_string(),
                        year,
                        ground_truth: Some(gt_val),
                        model: model_val.map(|v| {
                            if abs_keys.contains(key) {
                                v.abs()
                            } else {
                                v
                            }
                        }),
                    });
                }
            }
        }

        let pct = if s_trusted > 0 {
            Some((s_matched as f64 / s_trusted as f64) * 100.0)
        } else {
            None
        };
        per_stmt.insert(
            stmt_name.to_string(),
            PerStatementScore {
                trusted: s_trusted,
                matched: s_matched,
                percentage: pct,
            },
        );
    }

    let overall_pct = if total_trusted > 0 {
        (total_matched as f64 / total_trusted as f64) * 100.0
    } else {
        100.0
    };

    Score {
        trusted: total_trusted,
        matched: total_matched,
        percentage: overall_pct,
        per_statement: per_stmt,
        mismatches,
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::types::*;

    fn repo_root() -> &'static Path {
        // CARGO_MANIFEST_DIR = finmodel-core/fm-tieout/
        let dir = env!("CARGO_MANIFEST_DIR");
        Path::new(dir)
            .parent()
            .expect("fm-tieout has no parent — unexpected layout")
            .parent()
            .expect("finmodel-core has no parent — unexpected layout")
    }

    fn load_ground_truth() -> GroundTruth {
        let path = repo_root()
            .join("tieout")
            .join("groundtruth")
            .join("ATCO-B_ST.json");
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read ground truth at {}: {e}", path.display()));
        serde_json::from_str(&text).expect("ground truth JSON parse")
    }

    fn load_model() -> ModelOutput {
        // Committed fixture: the Python modelcache under tieout/results/ is
        // gitignored, so a fresh clone / CI cannot read it. This is a frozen
        // 48/48 ATCO extraction — a compare()-logic unit test, not the live
        // parity gate (R.6 exercises the full pipeline).
        let text = include_str!("../tests/fixtures/atco_model.json");
        serde_json::from_str(text).expect("model cache JSON parse")
    }

    #[test]
    fn atco_b_st_scores_48_of_48() {
        let gt = load_ground_truth();
        let model = load_model();
        let score = super::compare(&gt, &model);

        assert_eq!(
            score.trusted, 48,
            "expected 48 trusted cells for ATCO-B_ST, got {}",
            score.trusted
        );
        assert_eq!(
            score.matched, 48,
            "expected 48/48 matched cells for ATCO-B_ST, got {}/{}",
            score.matched, score.trusted
        );
        assert!(
            (score.percentage - 100.0).abs() < 0.001,
            "expected 100% match, got {}%",
            score.percentage
        );
        assert!(
            score.mismatches.is_empty(),
            "expected no mismatches, got {}: {:?}",
            score.mismatches.len(),
            score.mismatches
        );
    }

    #[test]
    fn corrupted_value_detected_as_mismatch() {
        let gt = load_ground_truth();
        let mut model = load_model();

        // Corrupt the first revenue value
        if let Some(first) = model.income_statement.get_mut("revenue") {
            if let Some(v) = first.first_mut() {
                *v = Some(999999.0);
            }
        }

        let score = super::compare(&gt, &model);

        assert_eq!(
            score.matched,
            47,
            "expected 47/48 after corruption, got {}/{}",
            score.matched,
            score.trusted
        );
        assert_eq!(score.mismatches.len(), 1, "expected exactly 1 mismatch");

        let mm = &score.mismatches[0];
        assert_eq!(mm.statement, "income_statement");
        assert_eq!(mm.key, "revenue");
        assert_eq!(mm.year, 2022);
    }
}
