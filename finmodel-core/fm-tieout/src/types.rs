use std::collections::HashMap;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Ground Truth — the immutable answer key
// ---------------------------------------------------------------------------

/// Values inside a ground-truth statement block.
/// key → year (as string) → optional numeric value.
pub type GtStatement = HashMap<String, HashMap<String, Option<f64>>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroundTruthValues {
    pub income_statement: GtStatement,
    pub balance_sheet: GtStatement,
    pub cash_flow_statement: GtStatement,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroundTruth {
    pub ticker: String,
    pub company: String,
    #[serde(default = "default_sector")]
    pub sector: String,
    pub years: Vec<i32>,
    pub values: GroundTruthValues,
    #[serde(default)]
    pub citations: HashMap<String, u32>,
}

fn default_sector() -> String {
    "industrial".to_string()
}

// ---------------------------------------------------------------------------
// Model Output  — what the engine extracted
// ---------------------------------------------------------------------------

/// Values inside a model-output statement block.
/// key → array of optional numbers, aligned with `years_found`.
pub type ModelStatement = HashMap<String, Vec<Option<f64>>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelOutput {
    pub income_statement: ModelStatement,
    pub balance_sheet: ModelStatement,
    pub cash_flow_statement: ModelStatement,
    pub years_found: Vec<String>,
}

// ---------------------------------------------------------------------------
// Scoring types
// ---------------------------------------------------------------------------

/// A single cell-level mismatch.
#[derive(Debug, Clone, Serialize)]
pub struct CellScore {
    pub statement: String,
    pub key: String,
    pub year: i32,
    pub ground_truth: Option<f64>,
    pub model: Option<f64>,
}

/// Per-statement scoring breakdown.
#[derive(Debug, Clone, Serialize)]
pub struct PerStatementScore {
    pub trusted: usize,
    pub matched: usize,
    pub percentage: Option<f64>,
}

/// Aggregate score across all statements.
#[derive(Debug, Clone, Serialize)]
pub struct Score {
    pub trusted: usize,
    pub matched: usize,
    pub percentage: f64,
    pub per_statement: HashMap<String, PerStatementScore>,
    pub mismatches: Vec<CellScore>,
}

// ---------------------------------------------------------------------------
// Simple error type
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum Error {
    JsonParse(String),
    Io(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::JsonParse(msg) => write!(f, "JSON parse error: {msg}"),
            Error::Io(msg) => write!(f, "IO error: {msg}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::JsonParse(e.to_string())
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e.to_string())
    }
}
