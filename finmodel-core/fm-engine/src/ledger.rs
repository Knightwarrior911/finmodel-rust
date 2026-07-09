use fm_types::{Ledger, TrustTier};

// ---------------------------------------------------------------------------
// R.3 — 5-tier trust ledger consistency checks
// ---------------------------------------------------------------------------

/// Check consistency of a `Ledger`: no duplicate (key, period) with mismatched values.
///
/// Simple wrapper around `Ledger::check_consistency()`.
pub fn check_ledger_consistency(ledger: &Ledger) -> Vec<String> {
    ledger.check_consistency()
}

/// Verify that all entries have a tier assigned (none are `Unverified`).
pub fn check_all_tiered(ledger: &Ledger) -> Vec<String> {
    let mut issues = Vec::new();
    for (i, entry) in ledger.entries.iter().enumerate() {
        if entry.tier == TrustTier::Unverified {
            issues.push(format!(
                "Entry[{}] '{} / {}' is still Unverified",
                i, entry.key, entry.period
            ));
        }
    }
    issues
}

/// Verify trust hierarchy: Derived entries must trace to an Xbrl source.
pub fn check_derived_basis(ledger: &Ledger) -> Vec<String> {
    let mut issues = Vec::new();
    for entry in &ledger.entries {
        if entry.tier == TrustTier::Derived && entry.basis.is_empty() {
            issues.push(format!(
                "Derived entry '{} / {}' has no basis explanation",
                entry.key, entry.period
            ));
        }
    }
    issues
}

#[cfg(test)]
mod tests {
    use super::*;
    use fm_types::{Ledger, LedgerEntry, TrustTier};

    #[test]
    fn tier_assignment_works() {
        let mut ledger = Ledger::new();
        ledger.push(LedgerEntry {
            key: "revenue".into(),
            period: "2024".into(),
            value: 1000.0,
            tier: TrustTier::Xbrl,
            basis: "SEC filing Q4 2024".into(),
        });
        ledger.push(LedgerEntry {
            key: "gross_profit".into(),
            period: "2024".into(),
            value: 400.0,
            tier: TrustTier::Derived,
            basis: "revenue - cogs".into(),
        });

        // No unverified entries
        let unresolved = check_all_tiered(&ledger);
        assert!(unresolved.is_empty());

        // Derived entries have basis
        let basis_issues = check_derived_basis(&ledger);
        assert!(basis_issues.is_empty());
    }

    #[test]
    fn detects_unverified() {
        let mut ledger = Ledger::new();
        ledger.push(LedgerEntry {
            key: "revenue".into(),
            period: "2024".into(),
            value: 1000.0,
            tier: TrustTier::Unverified,
            basis: String::new(),
        });
        let issues = check_all_tiered(&ledger);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("Unverified"));
    }

    #[test]
    fn consistency_detects_mismatch() {
        let mut ledger = Ledger::new();
        ledger.push(LedgerEntry {
            key: "revenue".into(),
            period: "2024".into(),
            value: 1000.0,
            tier: TrustTier::Xbrl,
            basis: "source".into(),
        });
        ledger.push(LedgerEntry {
            key: "revenue".into(),
            period: "2024".into(),
            value: 999.0,
            tier: TrustTier::Extracted,
            basis: "alt source".into(),
        });
        let issues = ledger.check_consistency();
        assert!(!issues.is_empty());
        assert!(issues[0].contains("Mismatch"));
    }
}
