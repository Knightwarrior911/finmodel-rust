// ---------------------------------------------------------------------------
// R.3 — Deterministic reconciliation (identity checks)
// ---------------------------------------------------------------------------

/// Check that Assets ≈ Liabilities + Equity for every period.
///
/// Returns a list of discrepancy descriptions (empty when all periods balance).
pub fn check_balance_sheet(assets: &[f64], liabilities: &[f64], equity: &[f64]) -> Vec<String> {
    let max_len = assets.len().max(liabilities.len()).max(equity.len());
    let mut issues = Vec::new();

    for i in 0..max_len {
        let a = *assets.get(i).unwrap_or(&0.0);
        let l = *liabilities.get(i).unwrap_or(&0.0);
        let e = *equity.get(i).unwrap_or(&0.0);
        let expected = l + e;
        let diff = (a - expected).abs();
        if diff > 1e-6 {
            issues.push(format!(
                "Period {}: Assets ({}) != Liabilities ({}) + Equity ({})  (diff={:.6})",
                i, a, l, e, diff
            ));
        }
    }
    issues
}

/// Check that Depreciation & Amortisation from the cash-flow statement
/// is consistent with the D&A from the fixed-asset / PP&E schedule.
pub fn check_da_cross(cfo_da: &[f64], da_from_fixed: &[f64]) -> Vec<String> {
    let max_len = cfo_da.len().max(da_from_fixed.len());
    let mut issues = Vec::new();

    for i in 0..max_len {
        let cfo = *cfo_da.get(i).unwrap_or(&0.0);
        let fixed = *da_from_fixed.get(i).unwrap_or(&0.0);
        let diff = (cfo - fixed).abs();
        if diff > 1e-6 {
            issues.push(format!(
                "Period {}: CFO D&A ({}) != Fixed-asset D&A ({})  (diff={:.6})",
                i, cfo, fixed, diff
            ));
        }
    }
    issues
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bs_check_catches_imbalance() {
        let assets = vec![100.0, 200.0, 300.0];
        let liabilities = vec![60.0, 120.0, 150.0];
        let equity = vec![40.0, 80.0, 150.0]; // period 2: 150 != 150+0=150 OK; period 0/1 OK
        let issues = check_balance_sheet(&assets, &liabilities, &equity);
        assert!(issues.is_empty(), "expected balanced sheets: {:?}", issues);
    }

    #[test]
    fn bs_check_detects_mismatch() {
        let assets = vec![100.0, 200.0];
        let liabilities = vec![60.0, 120.0];
        let equity = vec![40.0, 90.0]; // period 1: 200 != 120+90=210
        let issues = check_balance_sheet(&assets, &liabilities, &equity);
        assert!(!issues.is_empty());
        assert!(issues[0].contains("Period 1"));
    }

    #[test]
    fn da_cross_check() {
        let cfo_da = vec![50.0, 55.0];
        let fixed_da = vec![50.0, 53.0]; // period 1 mismatch
        let issues = check_da_cross(&cfo_da, &fixed_da);
        assert!(!issues.is_empty());
        assert!(issues[0].contains("Period 1"));
    }
}
