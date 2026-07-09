//! R.6 Parity Gate — Rust engine vs Python reference implementation.
//!
//! Loads model cache + Excel snapshot for each baseline company, feeds
//! extraction data through the Rust projection engine, diffs projected
//! values against Python model_output. Asserts key intersection > 0.
//!
//! Skips gracefully when model cache files are absent (gitignored — only
//! present after a local tie-out run). Run with model cache to measure
//! the parity gap: `cargo test -p fm-cli --test parity -- --nocapture`

use std::collections::HashMap;
use std::path::Path;
use fm_engine::ModelEngine;
use fm_types::{CompanyConfig, ProjectedStatements, ReconciledData, StatementData};

const REPO_ROOT: &str = "../../";
const FP: &str = "4065a2c76ef95ca6";

const COMPANIES: &[(&str, &str, &str)] = &[
    ("ASML_AS",   "ASML.AS",  "EUR"),
    ("ATCO-B_ST", "ATCO-B.ST","SEK"),
    ("NESN_SW",   "NESN.SW",  "CHF"),
    ("NOVO-B_CO", "NOVO-B.CO","DKK"),
    ("SAND_ST",   "SAND.ST",  "SEK"),
];

fn model_cache_path(name: &str) -> String {
    format!("{REPO_ROOT}tieout/results/_modelcache/{FP}_{name}.json")
}

fn snapshot_path(name: &str) -> String {
    format!("{REPO_ROOT}tieout/excel_snapshots/{}_snapshot.json", name)
}

fn load_json(path: &str) -> serde_json::Value {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("Failed to read {path}: {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("Failed to parse {path}: {e}"))
}

fn cache_to_statement_data(cache_obj: &serde_json::Value) -> StatementData {
    let mut sd = StatementData::new();
    if let Some(obj) = cache_obj.as_object() {
        for (key, val) in obj {
            if let Some(arr) = val.as_array() {
                let vec: Vec<Option<f64>> = arr
                    .iter()
                    .map(|v| v.as_f64().or_else(|| v.as_i64().map(|i| i as f64)))
                    .collect();
                sd.insert(key.clone(), vec);
            }
        }
    }
    sd
}

fn build_reconciled_data(cache: &serde_json::Value, ccy: &str) -> ReconciledData {
    let years = cache["years_found"]
        .as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    ReconciledData {
        income_statement: cache_to_statement_data(&cache["income_statement"]),
        balance_sheet: cache_to_statement_data(&cache["balance_sheet"]),
        cash_flow_statement: cache_to_statement_data(&cache["cash_flow_statement"]),
        periods: years,
        currency: ccy.to_string(),
    }
}

/// Python model_output stores values as flat arrays indexed by snapshot periods.
fn parse_python_model_output(mo: &serde_json::Value, periods: &[String])
    -> HashMap<String, HashMap<String, f64>>
{
    let mut result = HashMap::new();
    for stmt in &["income_statement", "balance_sheet", "cash_flow_statement"] {
        let mut map = HashMap::new();
        if let Some(obj) = mo.get(*stmt).and_then(|v| v.as_object()) {
            for (key, val) in obj {
                if let Some(arr) = val.as_array() {
                    for (i, v) in arr.iter().enumerate() {
                        if let Some(n) = v.as_f64() {
                            map.insert(format!("{}.{}", key, periods.get(i).unwrap_or(&String::new())), n);
                        }
                    }
                }
            }
        }
        result.insert(stmt.to_string(), map);
    }
    result
}

fn flatten_projections(ps: &ProjectedStatements) -> HashMap<String, HashMap<String, f64>> {
    fn flatten(sd: &StatementData, periods: &[String]) -> HashMap<String, f64> {
        let mut map = HashMap::new();
        for (key, vals) in sd {
            for (i, v) in vals.iter().enumerate() {
                if let Some(val) = v {
                    map.insert(format!("{}.{}", key, periods.get(i).unwrap_or(&String::new())), *val);
                }
            }
        }
        map
    }
    let mut r = HashMap::new();
    r.insert("income_statement".into(), flatten(&ps.income_statement, &ps.periods));
    r.insert("balance_sheet".into(), flatten(&ps.balance_sheet, &ps.periods));
    r.insert("cash_flow_statement".into(), flatten(&ps.cash_flow, &ps.periods));
    r
}

/// Try matching a Rust key against Python's keys (which may have E/A suffix).
fn try_match<'a>(key: &str, py_map: &'a HashMap<String, f64>) -> Option<&'a f64> {
    if let Some(v) = py_map.get(key) { return Some(v); }
    if let Some(v) = py_map.get(&format!("{}E", key)) { return Some(v); }
    if let Some(v) = py_map.get(&format!("{}A", key)) { return Some(v); }
    None
}

fn run_parity(name: &str, ccy: &str) {
    let cpath = model_cache_path(name);
    if !Path::new(&cpath).exists() {
        println!("  SKIP: model cache not found (run tie-out to generate)");
        return;
    }

    let cache = load_json(&cpath);
    let snapshot = load_json(&snapshot_path(name));
    let reconciled = build_reconciled_data(&cache, ccy);

    let snap_periods: Vec<String> = snapshot["periods"]
        .as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();

    let config = CompanyConfig {
        name: name.to_string(),
        currency: ccy.to_string(),
        hist_periods: reconciled.periods.len(),
        proj_periods: 5,
        ..Default::default()
    };

    let engine = ModelEngine::new(reconciled, config);
    let scalar = engine.derive_assumptions();
    let n = 5usize;

    let mut assumptions: HashMap<String, Vec<f64>> = HashMap::new();
    for (k, v) in &scalar { assumptions.insert(k.clone(), vec![*v; n]); }
    assumptions.entry("revenue_growth".into()).or_insert(vec![0.03; n]);
    assumptions.entry("gross_margin".into()).or_insert(vec![0.30; n]);
    assumptions.entry("tax_rate".into()).or_insert(vec![0.21; n]);

    let projected = engine.project(&assumptions);
    let rust_out = flatten_projections(&projected);
    let mo = snapshot.get("model_output").expect("snapshot has model_output");
    let py_out = parse_python_model_output(mo, &snap_periods);

    let rg = scalar.get("revenue_growth").copied().unwrap_or(0.0);
    let gm = scalar.get("gross_margin").copied().unwrap_or(0.0);
    println!("  Derived: rev_growth={:.1}%, gross_margin={:.1}%", rg*100.0, gm*100.0);

    let mut grand_compared = 0u32;
    let mut grand_diffs = 0u32;

    for stmt in &["income_statement", "balance_sheet", "cash_flow_statement"] {
        let rust_map = &rust_out[*stmt];
        let py_map = &py_out[*stmt];
        let mut compared = 0u32;
        let mut val_diffs = 0u32;

        for (rkey, rv) in rust_map {
            if let Some(pv) = try_match(rkey, py_map) {
                compared += 1;
                let diff = (rv - pv).abs();
                let rel = if pv.abs() > 1e-9 { diff / pv.abs() } else { diff };
                if rel > 0.15 && diff > 1.0 {
                    val_diffs += 1;
                    if val_diffs <= 20 {
                        println!("  [{}] {}: Rust={:.1}, Python={:.1} ({:.1}%)",
                                 stmt, rkey, rv, pv, rel * 100.0);
                    }
                }
            }
        }

        assert!(compared > 0,
                "[{}] Zero keys matched for {} — period label mismatch",
                name, stmt);
        println!("  {}: compared={}, >15% diffs={}", stmt, compared, val_diffs);
        grand_compared += compared;
        grand_diffs += val_diffs;
    }

    println!("  TOTAL: {} keys compared, {} >15% diff(s)", grand_compared, grand_diffs);
    println!("  STATUS: {} gap(s) to resolve before parity gate", grand_diffs);
}

#[test]
fn parity_all_companies() {
    for (name, _ticker, ccy) in COMPANIES {
        println!("\n=== {} ===", name);
        run_parity(name, ccy);
    }
}

#[test]
fn parity_atco_snapshot_exists() {
    let snap = load_json(&snapshot_path("ATCO-B_ST"));
    assert!(snap.get("model_output").is_some());
    assert!(snap.get("sheets").is_some());
    assert!(snap.get("periods").and_then(|p| p.as_array()).map(|a| a.len() > 0).unwrap_or(false));
}
