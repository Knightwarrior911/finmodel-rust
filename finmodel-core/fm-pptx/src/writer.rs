//! 6.3 / 6.4 — Writer. Pure archetype-decision + formatting helpers (6.3) and
//! DrawingML deck emission (6.4), ported from `src/research/pptx_writer.py`
//! (with the `ResearchPPTXWriter` archetype surface from `pptx_output.py`).
//!
//! This top of the module is the 6.3 pure-function surface, snapshot-gated
//! against the Python originals over an input matrix. The deck writer (6.4)
//! lives in [`deck`].

use serde_json::{json, Value};

pub mod deck;
pub mod pkgbuild;

// ── archetype decision tree (pick_slide_archetype + _DENSITY_LIMITS) ──────────
pub const ARCH_COMPARISON: &str = "comparison_matrix";
pub const ARCH_SCORECARD: &str = "scorecard";
pub const ARCH_QUOTE_WALL: &str = "quote_wall";
pub const ARCH_TIMELINE: &str = "timeline";
pub const ARCH_PROCESS: &str = "process_diagram";
pub const ARCH_STRATEGY: &str = "strategy_framework";

/// `_DENSITY_LIMITS[arch] = (max_entities, max_metrics)`.
pub fn density_limits(arch: &str) -> Option<(usize, usize)> {
    Some(match arch {
        ARCH_COMPARISON => (8, 8),
        ARCH_SCORECARD => (9, 1),
        ARCH_QUOTE_WALL => (8, 1),
        ARCH_TIMELINE => (10, 1),
        ARCH_PROCESS => (8, 1),
        ARCH_STRATEGY => (5, 1),
        _ => return None,
    })
}

/// Output of [`pick_slide_archetype`].
#[derive(Debug, Clone, PartialEq)]
pub struct ArchetypeDecision {
    pub archetype: String,
    pub split_required: bool,
    pub n_slides: usize,
    pub rationale: Vec<String>,
}

impl ArchetypeDecision {
    pub fn to_json(&self) -> Value {
        json!({
            "archetype": self.archetype,
            "split_required": self.split_required,
            "n_slides": self.n_slides,
            "rationale": self.rationale,
        })
    }
}

/// Faithful port of `pick_slide_archetype`.
pub fn pick_slide_archetype(
    data_shape: &str,
    n_entities: usize,
    n_metrics: usize,
    has_quotes: bool,
    is_dated: bool,
) -> ArchetypeDecision {
    let mut rationale = Vec::new();
    let archetype = if has_quotes || data_shape == "quotes" {
        rationale.push("data_shape=quotes -> quote_wall".to_string());
        ARCH_QUOTE_WALL
    } else if is_dated || data_shape == "events" {
        rationale.push("is_dated/events -> timeline".to_string());
        ARCH_TIMELINE
    } else if matches!(data_shape, "framework" | "priorities" | "initiatives" | "strategy") {
        rationale.push(format!("data_shape={data_shape} -> strategy_framework"));
        ARCH_STRATEGY
    } else if matches!(data_shape, "process" | "structure") {
        rationale.push(format!("data_shape={data_shape} -> process_diagram"));
        ARCH_PROCESS
    } else if data_shape == "single_stat" || (n_metrics <= 9 && n_entities == 1) {
        rationale.push("single entity, multiple metrics -> scorecard".to_string());
        ARCH_SCORECARD
    } else {
        rationale.push(format!("data_shape={data_shape} -> comparison_matrix"));
        ARCH_COMPARISON
    };

    let (max_e, max_m) = density_limits(archetype).unwrap();
    let split_required = n_entities > max_e || n_metrics > max_m;
    let mut n_slides = 1;
    if split_required {
        n_slides = n_entities.div_ceil(max_e).max(1);
        rationale.push(format!(
            "density {n_entities}x{n_metrics} > limit {max_e}x{max_m} -> {n_slides} slides"
        ));
    }
    ArchetypeDecision {
        archetype: archetype.to_string(),
        split_required,
        n_slides,
        rationale,
    }
}

/// `split_into_chunks(items, archetype)`.
pub fn split_into_chunks(items: &[Value], archetype: &str) -> Result<Vec<Vec<Value>>, String> {
    let (max_per_slide, _) = density_limits(archetype).ok_or_else(|| format!("unknown archetype: {archetype}"))?;
    Ok(items.chunks(max_per_slide).map(|c| c.to_vec()).collect())
}

// ── title / value formatting ──────────────────────────────────────────────────

fn title_stopwords() -> &'static [&'static str] {
    &[
        "a", "an", "the", "and", "but", "or", "nor", "for", "yet", "so", "of", "in", "on", "at",
        "by", "to", "with", "from", "as", "vs", "via", "per", "into", "onto", "upon", "is", "are",
        "be", "been", "was", "were", "that", "than", "this", "these",
    ]
}

fn strip_punct(w: &str) -> &str {
    let pat: &[char] = &['.', ',', ';', ':', '!', '?', '"', '\'', '(', ')'];
    w.trim_matches(pat)
}

fn py_isupper(s: &str) -> bool {
    let mut has_cased = false;
    for c in s.chars() {
        if c.is_alphabetic() {
            has_cased = true;
            if !c.is_uppercase() {
                return false;
            }
        }
    }
    has_cased
}

/// `_normalize_heading` — page-heading capitalization.
pub fn normalize_heading(t: &str) -> String {
    if t.is_empty() {
        return t.to_string();
    }
    let words: Vec<&str> = t.split_whitespace().collect();
    let mut out: Vec<String> = Vec::new();
    for (i, w) in words.iter().enumerate() {
        let stripped = strip_punct(w);
        if stripped.is_empty() {
            out.push((*w).to_string());
            continue;
        }
        if py_isupper(stripped) && stripped.chars().count() >= 2 {
            out.push((*w).to_string());
            continue;
        }
        if stripped.chars().any(|c| c.is_ascii_digit()) {
            out.push((*w).to_string());
            continue;
        }
        let inner: String = stripped.chars().skip(1).collect();
        if !inner.is_empty() && inner.chars().any(|c| c.is_uppercase()) {
            out.push((*w).to_string());
            continue;
        }
        if i == 0 {
            let mut chars = w.chars();
            let first = chars.next().unwrap();
            let rest: String = chars.collect();
            if w.chars().count() > 1 {
                out.push(format!("{}{}", first.to_uppercase(), rest.to_lowercase()));
            } else {
                out.push(first.to_uppercase().to_string());
            }
            continue;
        }
        if title_stopwords().contains(&stripped.to_lowercase().as_str()) {
            out.push(w.to_lowercase());
        } else {
            out.push((*w).to_string());
        }
    }
    out.join(" ")
}

/// `_fmt_to_numfmt` — Python format string -> Excel number_format.
pub fn fmt_to_numfmt(fmt: &str) -> String {
    let f = fmt.trim();
    let has = |needle: &str| f.contains(needle);
    if has(":.1%") {
        return "0.0%".into();
    }
    if has(":.2%") {
        return "0.00%".into();
    }
    if has(":.0%") {
        return "0%".into();
    }
    if f.ends_with("%}") || f.ends_with('%') {
        if has(":.1f") {
            return "0.0\"%\"".into();
        }
        if has(":.2f") {
            return "0.00\"%\"".into();
        }
        return "0\"%\"".into();
    }
    if f.ends_with("x}") || f.ends_with('x') {
        if has(":.1f") {
            return "0.0\"x\"".into();
        }
        if has(":.2f") {
            return "0.00\"x\"".into();
        }
        return "0\"x\"".into();
    }
    if has(":+,.0f") {
        return "+#,##0;-#,##0".into();
    }
    if has(":,.0f") {
        return "#,##0".into();
    }
    if has(":,.1f") {
        return "#,##0.0".into();
    }
    if has(":,.2f") {
        return "#,##0.00".into();
    }
    if has(":.0f") {
        return "0".into();
    }
    if has(":.1f") {
        return "0.0".into();
    }
    if has(":.2f") {
        return "0.00".into();
    }
    "General".into()
}

/// A typed value for [`format_value`] (int vs float matters).
#[derive(Debug, Clone)]
pub enum FmtValue {
    Null,
    Str(String),
    Float(f64),
    Int(i64),
}

/// `_format_value(v)`.
pub fn format_value(v: &FmtValue) -> String {
    match v {
        FmtValue::Null => "n/a".to_string(),
        FmtValue::Str(s) => s.clone(),
        FmtValue::Float(f) => {
            if f.abs() < 1.0 && *f != 0.0 {
                // abs<1 implies abs<5 -> percent, 1 decimal.
                fmt_percent(*f, 1)
            } else {
                fmt_grouped(*f, 1, false)
            }
        }
        FmtValue::Int(n) => fmt_int_grouped(*n),
    }
}

// ── Python numeric-format emulation ───────────────────────────────────────────

fn group_int(digits: &str) -> String {
    let bytes = digits.as_bytes();
    let mut out = String::new();
    let len = bytes.len();
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && (len - i) % 3 == 0 {
            out.push(',');
        }
        out.push(*b as char);
    }
    out
}

/// `{:,.<dec>f}` (optionally forcing a leading `+`).
pub fn fmt_grouped(v: f64, dec: usize, force_sign: bool) -> String {
    let neg = v.is_sign_negative() && v != 0.0;
    let fixed = format!("{:.*}", dec, v.abs());
    let (int_part, frac_part) = match fixed.split_once('.') {
        Some((i, f)) => (i.to_string(), Some(f.to_string())),
        None => (fixed.clone(), None),
    };
    let mut s = group_int(&int_part);
    if let Some(f) = frac_part {
        s.push('.');
        s.push_str(&f);
    }
    if neg {
        format!("-{s}")
    } else if force_sign {
        format!("+{s}")
    } else {
        s
    }
}

/// `{:,}` for an integer.
pub fn fmt_int_grouped(n: i64) -> String {
    let neg = n < 0;
    let digits = n.unsigned_abs().to_string();
    let s = group_int(&digits);
    if neg {
        format!("-{s}")
    } else {
        s
    }
}

/// `{:.<dec>%}` (value scaled by 100 with a `%` suffix).
pub fn fmt_percent(v: f64, dec: usize) -> String {
    format!("{:.*}%", dec, v * 100.0)
}
