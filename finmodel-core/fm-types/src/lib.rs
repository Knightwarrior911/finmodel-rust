use std::collections::HashMap;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Statement data — line items keyed to an array of period values
// ---------------------------------------------------------------------------

/// A mapping from line-item name → array of values (one per period).
/// Period alignment lives in `ReconciledData.periods`.
pub type StatementData = HashMap<String, Vec<Option<f64>>>;

// ---------------------------------------------------------------------------
// Reconciled / validated financial data
// ---------------------------------------------------------------------------

/// Company-level configuration / metadata for projection assumptions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompanyConfig {
    /// Name or ticker.
    pub name: String,
    /// Reporting currency (ISO 4217).
    pub currency: String,
    /// Number of historical periods available.
    pub hist_periods: usize,
    /// Number of forward periods to project.
    pub proj_periods: usize,
    /// Optional growth cap used to clamp extreme extrapolations.
    pub growth_cap: Option<f64>,
}

impl Default for CompanyConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            currency: "USD".into(),
            hist_periods: 5,
            proj_periods: 5,
            growth_cap: None,
        }
    }
}

/// Cleaned, reconciled financial data ready for projection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconciledData {
    pub income_statement: StatementData,
    pub balance_sheet: StatementData,
    pub cash_flow_statement: StatementData,
    /// Period labels (e.g. "2021", "2022", …).
    pub periods: Vec<String>,
    pub currency: String,
}

impl ReconciledData {
    pub fn num_periods(&self) -> usize {
        self.periods.len()
    }
}

// ---------------------------------------------------------------------------
// Projected statements
// ---------------------------------------------------------------------------

/// Projected financial statements for forward periods.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectedStatements {
    pub periods: Vec<String>,
    pub income_statement: StatementData,
    pub balance_sheet: StatementData,
    pub cash_flow: StatementData,
}

// ---------------------------------------------------------------------------
// Trust ledger — 5-tier cascade
// ---------------------------------------------------------------------------

/// Trust level for a single data point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrustTier {
    /// Directly from XBRL filings.
    Xbrl,
    /// Derived via deterministic calculation from XBRL items.
    Derived,
    /// Extracted via LLM from unstructured documents.
    Extracted,
    /// Set by analyst assumption.
    Assumption,
    /// Not yet validated.
    Unverified,
}

impl TrustTier {
    pub fn priority(&self) -> u8 {
        match self {
            TrustTier::Xbrl => 5,
            TrustTier::Derived => 4,
            TrustTier::Extracted => 3,
            TrustTier::Assumption => 2,
            TrustTier::Unverified => 1,
        }
    }
}

/// A single entry in the trust ledger.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerEntry {
    pub key: String,
    pub period: String,
    pub value: f64,
    pub tier: TrustTier,
    /// Human-readable explanation of provenance.
    pub basis: String,
}

/// Trust ledger covering all data points.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ledger {
    pub entries: Vec<LedgerEntry>,
}

impl Ledger {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn push(&mut self, entry: LedgerEntry) {
        self.entries.push(entry);
    }

    /// Check consistency of the ledger: no duplicate (key, period) with mismatched values.
    pub fn check_consistency(&self) -> Vec<String> {
        let mut issues = Vec::new();
        let mut seen: HashMap<(&str, &str), f64> = HashMap::new();
        for entry in &self.entries {
            let k = (entry.key.as_str(), entry.period.as_str());
            if let Some(&prev) = seen.get(&k) {
                if (prev - entry.value).abs() > 1e-9 {
                    issues.push(format!(
                        "Mismatch for {} / {}: {} vs {} (tier {:?})",
                        entry.key, entry.period, prev, entry.value, entry.tier
                    ));
                }
            } else {
                seen.insert(k, entry.value);
            }
        }
        issues
    }
}

impl Default for Ledger {
    fn default() -> Self {
        Self::new()
    }
}
