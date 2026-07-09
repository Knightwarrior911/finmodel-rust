/// Input parameters for the Weighted Average Cost of Capital calculation.
#[derive(Debug, Clone)]
pub struct WACCInput {
    pub risk_free_rate: f64,
    pub equity_risk_premium: f64,
    pub beta: f64,
    pub cost_of_debt: f64,
    pub tax_rate: f64,
    pub debt_pct: f64,
    pub equity_pct: f64,
}

/// Calculate the cost of equity using the Capital Asset Pricing Model (CAPM).
///
/// Re = Rf + beta * ERP
pub fn cost_of_equity(rf: f64, erp: f64, beta: f64) -> f64 {
    rf + beta * erp
}

/// Calculate the Weighted Average Cost of Capital (WACC).
///
/// WACC = E/V * Re + D/V * Rd * (1 - tax_rate)
pub fn calculate(input: &WACCInput) -> f64 {
    let re = cost_of_equity(input.risk_free_rate, input.equity_risk_premium, input.beta);
    input.equity_pct * re + input.debt_pct * input.cost_of_debt * (1.0 - input.tax_rate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wacc_known_value() {
        let input = WACCInput {
            risk_free_rate: 0.03,
            equity_risk_premium: 0.05,
            beta: 1.2,
            cost_of_debt: 0.04,
            tax_rate: 0.25,
            debt_pct: 0.4,
            equity_pct: 0.6,
        };
        // Re = 0.03 + 1.2 * 0.05 = 0.09
        // WACC = 0.6 * 0.09 + 0.4 * 0.04 * (1 - 0.25)
        //       = 0.054 + 0.012 = 0.066
        let re = cost_of_equity(input.risk_free_rate, input.equity_risk_premium, input.beta);
        assert!((re - 0.09).abs() < 1e-12);

        let wacc = calculate(&input);
        assert!((wacc - 0.066).abs() < 1e-12);
    }

    #[test]
    fn test_cost_of_equity_zero_beta() {
        let re = cost_of_equity(0.03, 0.05, 0.0);
        assert!((re - 0.03).abs() < 1e-12);
    }

    #[test]
    fn test_wacc_all_equity() {
        let input = WACCInput {
            risk_free_rate: 0.03,
            equity_risk_premium: 0.06,
            beta: 1.0,
            cost_of_debt: 0.05,
            tax_rate: 0.20,
            debt_pct: 0.0,
            equity_pct: 1.0,
        };
        // Re = 0.03 + 1.0 * 0.06 = 0.09
        // WACC = 1.0 * 0.09 + 0 = 0.09
        let wacc = calculate(&input);
        assert!((wacc - 0.09).abs() < 1e-12);
    }
}
