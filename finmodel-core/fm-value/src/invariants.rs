use crate::dcf::DCFInput;

/// Run all 11 sanity checks on DCF inputs, WACC, and market multiples.
///
/// Returns a list of invariant descriptions. Each non-empty string represents
/// a violated invariant that the caller should investigate. An empty `Vec`
/// means all checks passed.
pub fn check_all(dcf: &DCFInput, wacc: f64, pe: f64, pb: f64) -> Vec<String> {
    let mut issues: Vec<String> = Vec::new();

    // 1. Discount rate must exceed long-term terminal growth
    if wacc <= dcf.terminal_growth {
        issues.push(format!(
            "Discount rate ({:.4}) must exceed terminal growth rate ({:.4})",
            wacc, dcf.terminal_growth
        ));
    }

    // 2. WACC must be positive
    if wacc <= 0.0 {
        issues.push(format!("WACC must be positive (got {:.4})", wacc));
    }

    // 3. Terminal growth should be <= nominal GDP proxy (3.5%)
    if dcf.terminal_growth > 0.035 {
        issues.push(format!(
            "Terminal growth ({:.4}) exceeds nominal GDP proxy (0.035)",
            dcf.terminal_growth
        ));
    }

    // 4. Implied P/E must be positive
    if pe <= 0.0 {
        issues.push(format!("Implied P/E must be positive (got {:.4})", pe));
    }

    // 5. FCF must not be empty
    if dcf.fcf.is_empty() {
        issues.push("Free cash flow projections must not be empty".to_string());
    }

    // 6. Last projected FCF must be positive for terminal value
    if let Some(&last) = dcf.fcf.last() {
        if last <= 0.0 {
            issues.push(format!(
                "Last projected FCF must be positive for terminal value (got {:.4})",
                last
            ));
        }
    }

    // 7. WACC should not be unusually high (>50%)
    if wacc > 0.50 {
        issues.push(format!("WACC ({:.4}) is unusually high; check inputs", wacc));
    }

    // 8. Projected periods must be greater than zero
    if dcf.projected_periods == 0 {
        issues.push("Projected periods must be greater than zero".to_string());
    }

    // 9. P/B must be positive
    if pb <= 0.0 {
        issues.push(format!(
            "Price-to-book ratio must be positive (got {:.4})",
            pb
        ));
    }

    // 10. Terminal growth rate should not be negative for a going concern
    if dcf.terminal_growth < 0.0 {
        issues.push(format!(
            "Terminal growth rate should not be negative for going concern (got {:.4})",
            dcf.terminal_growth
        ));
    }

    // 11. FCF projections should have at least one period matching projected_periods
    if dcf.fcf.len() < dcf.projected_periods {
        issues.push(format!(
            "FCF projections length ({}) is less than projected periods ({})",
            dcf.fcf.len(),
            dcf.projected_periods
        ));
    }

    issues
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dcf::DCFInput;

    fn make_clean_dcf() -> DCFInput {
        DCFInput {
            fcf: vec![100.0, 110.0, 120.0],
            terminal_growth: 0.025,
            wacc: 0.10,
            projected_periods: 3,
        }
    }

    #[test]
    fn test_catches_growth_above_discount() {
        let dcf = DCFInput {
            fcf: vec![100.0],
            terminal_growth: 0.08,
            wacc: 0.05,
            projected_periods: 1,
        };
        let issues = check_all(&dcf, 0.05, 15.0, 1.5);
        assert!(issues
            .iter()
            .any(|s| s.contains("Discount rate") && s.contains("terminal growth")));
    }

    #[test]
    fn test_negative_wacc() {
        let dcf = make_clean_dcf();
        let issues = check_all(&dcf, -0.01, 10.0, 1.0);
        assert!(issues.iter().any(|s| s.contains("WACC must be positive")));
    }

    #[test]
    fn test_high_terminal_growth() {
        let dcf = DCFInput {
            fcf: vec![100.0],
            terminal_growth: 0.10,
            wacc: 0.12,
            projected_periods: 5,
        };
        let issues = check_all(&dcf, 0.12, 15.0, 2.0);
        assert!(issues.iter().any(|s| s.contains("Terminal growth") && s.contains("GDP")));
    }

    #[test]
    fn test_pe_must_be_positive() {
        let dcf = make_clean_dcf();
        let issues = check_all(&dcf, 0.10, -5.0, 1.5);
        assert!(issues.iter().any(|s| s.contains("P/E must be positive")));
    }

    #[test]
    fn test_pb_must_be_positive() {
        let dcf = make_clean_dcf();
        let issues = check_all(&dcf, 0.10, 15.0, -1.0);
        assert!(issues
            .iter()
            .any(|s| s.contains("Price-to-book ratio must be positive")));
    }

    #[test]
    fn test_empty_fcf() {
        let dcf = DCFInput {
            fcf: vec![],
            terminal_growth: 0.02,
            wacc: 0.10,
            projected_periods: 5,
        };
        let issues = check_all(&dcf, 0.10, 15.0, 2.0);
        assert!(issues
            .iter()
            .any(|s| s.contains("Free cash flow") && s.contains("empty")));
    }

    #[test]
    fn test_clean_input_passes() {
        let dcf = make_clean_dcf();
        let issues = check_all(&dcf, 0.10, 15.0, 2.0);
        assert!(issues.is_empty());
    }

    #[test]
    fn test_negative_terminal_growth() {
        let dcf = DCFInput {
            fcf: vec![100.0],
            terminal_growth: -0.01,
            wacc: 0.10,
            projected_periods: 1,
        };
        let issues = check_all(&dcf, 0.10, 15.0, 2.0);
        assert!(issues
            .iter()
            .any(|s| s.contains("Terminal growth") && s.contains("negative")));
    }

    #[test]
    fn test_wacc_too_high() {
        let dcf = make_clean_dcf();
        let issues = check_all(&dcf, 0.60, 15.0, 2.0);
        assert!(issues.iter().any(|s| s.contains("unusually high")));
    }

    #[test]
    fn test_zero_projected_periods() {
        let dcf = DCFInput {
            fcf: vec![100.0],
            terminal_growth: 0.02,
            wacc: 0.10,
            projected_periods: 0,
        };
        let issues = check_all(&dcf, 0.10, 15.0, 2.0);
        assert!(issues
            .iter()
            .any(|s| s.contains("Projected periods must be greater than zero")));
    }

    #[test]
    fn test_short_fcf_relative_to_periods() {
        let dcf = DCFInput {
            fcf: vec![100.0, 110.0],
            terminal_growth: 0.02,
            wacc: 0.10,
            projected_periods: 5,
        };
        let issues = check_all(&dcf, 0.10, 15.0, 2.0);
        assert!(issues
            .iter()
            .any(|s| s.contains("FCF projections length") && s.contains("less than")));
    }
}
