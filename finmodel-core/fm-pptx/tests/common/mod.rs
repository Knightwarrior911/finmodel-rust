//! Shared parity helpers for the fm-pptx gates.
#![allow(dead_code)]

use serde_json::Value;

/// Repo `tieout/excel_snapshots` directory.
pub fn snap_dir() -> String {
    format!("{}/../../tieout/excel_snapshots", env!("CARGO_MANIFEST_DIR"))
}

/// Repo `tests/fixtures/pptx` directory.
pub fn fixture_dir() -> String {
    format!("{}/../../tests/fixtures/pptx", env!("CARGO_MANIFEST_DIR"))
}

pub fn load_json(path: &str) -> Value {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {path}: {e}"))
}

/// Recursive structural diff between `got` and `want`.
///
/// - numbers compare with absolute tolerance `1e-3` (both sides are rounded to
///   2–4 decimals by the inspector, so this masks only last-digit rounding);
/// - object keys in `ignore` (dotted path from root) are skipped;
/// - returns human-readable diff lines (empty == identical).
pub fn diff_json(got: &Value, want: &Value, ignore: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    walk(got, want, "$", ignore, &mut out);
    out
}

fn walk(got: &Value, want: &Value, path: &str, ignore: &[&str], out: &mut Vec<String>) {
    if ignore.iter().any(|p| *p == path) {
        return;
    }
    match (got, want) {
        (Value::Object(g), Value::Object(w)) => {
            for (k, wv) in w {
                let cp = format!("{path}.{k}");
                match g.get(k) {
                    Some(gv) => walk(gv, wv, &cp, ignore, out),
                    None => {
                        if !ignore.iter().any(|p| *p == cp) {
                            out.push(format!("{cp}: missing in got (want {})", short(wv)));
                        }
                    }
                }
            }
            for k in g.keys() {
                let cp = format!("{path}.{k}");
                if !w.contains_key(k) && !ignore.iter().any(|p| *p == cp) {
                    out.push(format!("{cp}: extra in got ({})", short(&g[k])));
                }
            }
        }
        (Value::Array(g), Value::Array(w)) => {
            if g.len() != w.len() {
                out.push(format!("{path}: array len {} != {}", g.len(), w.len()));
            }
            for (i, wv) in w.iter().enumerate() {
                match g.get(i) {
                    Some(gv) => walk(gv, wv, &format!("{path}[{i}]"), ignore, out),
                    None => out.push(format!("{path}[{i}]: missing in got")),
                }
            }
        }
        (Value::Number(g), Value::Number(w)) => {
            let (gf, wf) = (g.as_f64().unwrap_or(f64::NAN), w.as_f64().unwrap_or(f64::NAN));
            if (gf - wf).abs() > 1e-3 {
                out.push(format!("{path}: number {gf} != {wf}"));
            }
        }
        _ => {
            if got != want {
                out.push(format!("{path}: {} != {}", short(got), short(want)));
            }
        }
    }
}

fn short(v: &Value) -> String {
    let s = v.to_string();
    if s.len() > 80 {
        format!("{}…", &s[..80])
    } else {
        s
    }
}
