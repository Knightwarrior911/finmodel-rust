/// Input parameters for a Discounted Cash Flow (DCF) valuation.
#[derive(Debug, Clone)]
pub struct DCFInput {
    /// Projected free cash flows for each period.
    pub fcf: Vec<f64>,
    /// Perpetual growth rate for terminal value.
    pub terminal_growth: f64,
    /// Discount rate (WACC).
    pub wacc: f64,
    /// Number of projected periods.
    pub projected_periods: usize,
}

/// Calculate the terminal value using the Gordon Growth Model.
///
/// TV = last_fcf * (1 + g) / (wacc - g)
///
/// Returns NaN when the growth rate meets or exceeds the discount rate.
pub fn terminal_value(last_fcf: f64, growth: f64, wacc: f64) -> f64 {
    if wacc <= growth {
        f64::NAN
    } else {
        last_fcf * (1.0 + growth) / (wacc - growth)
    }
}

/// Calculate the enterprise value: sum of present value of projected FCFs
/// plus the present value of the terminal value.
pub fn enterprise_value(input: &DCFInput) -> f64 {
    if input.fcf.is_empty() || input.projected_periods == 0 {
        return 0.0;
    }

    let n = input.fcf.len().min(input.projected_periods);

    // Present value of projected FCFs
    let mut pv = 0.0;
    for (i, &fcf) in input.fcf[..n].iter().enumerate() {
        let period = (i + 1) as f64;
        pv += fcf / (1.0 + input.wacc).powf(period);
    }

    // Terminal value, discounted back to present
    if let Some(&last) = input.fcf[..n].last() {
        let tv = terminal_value(last, input.terminal_growth, input.wacc);
        if tv.is_finite() {
            pv += tv / (1.0 + input.wacc).powf(n as f64);
        }
    }

    pv
}

/// Calculate equity value from enterprise value.
///
/// Equity Value = EV - Net Debt - Preferred - Minorities
pub fn equity_value(ev: f64, net_debt: f64, preferred: f64, minorities: f64) -> f64 {
    ev - net_debt - preferred - minorities
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zero_growth_perpetuity() {
        // Flat FCF with zero growth -> perpetuity: TV = FCF / wacc
        let fcf = vec![100.0, 100.0, 100.0];
        let input = DCFInput {
            fcf: fcf.clone(),
            terminal_growth: 0.0,
            wacc: 0.10,
            projected_periods: 3,
        };
        let ev = enterprise_value(&input);

        // Sum PV of 3 years of 100 at 10%
        let expected_pv = 100.0 / 1.1
            + 100.0 / 1.1_f64.powi(2)
            + 100.0 / 1.1_f64.powi(3);
        // TV = 100 / 0.10 = 1000, discounted back 3 years
        let tv = 100.0 / 0.10;
        let expected = expected_pv + tv / 1.1_f64.powi(3);
        assert!((ev - expected).abs() < 1e-10);
    }

    #[test]
    fn test_terminal_value_gordon_growth() {
        let tv = terminal_value(100.0, 0.02, 0.10);
        let expected = 100.0 * 1.02 / (0.10 - 0.02);
        assert!((tv - expected).abs() < 1e-10);
    }

    #[test]
    fn test_terminal_value_growth_equals_discount() {
        let tv = terminal_value(100.0, 0.10, 0.10);
        assert!(tv.is_nan());
    }

    #[test]
    fn test_equity_value() {
        let eq = equity_value(1000.0, 300.0, 50.0, 20.0);
        assert!((eq - 630.0).abs() < 1e-10);
    }

    #[test]
    fn test_empty_fcf() {
        let input = DCFInput {
            fcf: vec![],
            terminal_growth: 0.02,
            wacc: 0.10,
            projected_periods: 5,
        };
        assert!((enterprise_value(&input) - 0.0).abs() < 1e-10);
    }

    #[test]
    fn test_growing_fcf() {
        let fcf = vec![100.0, 105.0, 110.25]; // ~5% growth
        let input = DCFInput {
            fcf: fcf.clone(),
            terminal_growth: 0.03,
            wacc: 0.10,
            projected_periods: 3,
        };
        let ev = enterprise_value(&input);
        // Just check it's positive and finite
        assert!(ev > 0.0);
        assert!(ev.is_finite());
    }
}
