//! WACC builder — port of `src/wacc.py`.

use crate::types::{PeerSet, WACCOutput};

/// CAPM cost of equity: Re = Rf + beta × ERP.
pub fn cost_of_equity(rf: f64, erp: f64, beta: f64) -> f64 {
    rf + beta * erp
}

/// Bu = Be / (1 + (1 − t) × D/E)
pub fn unlever_beta(levered_beta: f64, de_ratio: f64, tax_rate: f64) -> f64 {
    let denom = 1.0 + (1.0 - tax_rate) * de_ratio;
    if denom == 0.0 {
        levered_beta
    } else {
        levered_beta / denom
    }
}

/// Be_target = Bu × (1 + (1 − t) × D/E_target)
pub fn relever_beta(unlevered_beta: f64, target_de: f64, target_tax: f64) -> f64 {
    unlevered_beta * (1.0 + (1.0 - target_tax) * target_de)
}

fn median(mut xs: Vec<f64>) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = xs.len();
    if n % 2 == 1 {
        xs[n / 2]
    } else {
        (xs[n / 2 - 1] + xs[n / 2]) / 2.0
    }
}

/// Lightweight WACC from explicit capital-structure inputs (unit-test helper).
pub fn calculate(input: &WACCInput) -> f64 {
    let re = cost_of_equity(input.risk_free_rate, input.equity_risk_premium, input.beta);
    input.equity_pct * re + input.debt_pct * input.cost_of_debt * (1.0 - input.tax_rate)
}

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

/// Full peer-set unlever/relever + CAPM WACC. Mirrors `compute_wacc`.
pub fn compute_wacc(
    peer_set: &PeerSet,
    target_market_cap: f64,
    target_debt: f64,
    risk_free_rate: f64,
    equity_risk_premium: f64,
    cost_of_debt_pretax: f64,
    target_tax_rate: f64,
    target_de_ratio: Option<f64>,
    fallback_beta: f64,
) -> WACCOutput {
    let median_bu = if peer_set.peers.is_empty() {
        fallback_beta
    } else {
        median(
            peer_set
                .peers
                .iter()
                .map(|p| unlever_beta(p.levered_beta, p.de_ratio, p.tax_rate))
                .collect(),
        )
    };
    let de = target_de_ratio.unwrap_or(peer_set.target_de_ratio);
    let target_levered_beta = relever_beta(median_bu, de, target_tax_rate);
    let ke = cost_of_equity(risk_free_rate, equity_risk_premium, target_levered_beta);
    let after_tax_kd = cost_of_debt_pretax * (1.0 - target_tax_rate);
    let total_capital = target_market_cap + target_debt;
    let (equity_weight, debt_weight) = if total_capital <= 0.0 {
        (1.0, 0.0)
    } else {
        (target_market_cap / total_capital, target_debt / total_capital)
    };
    let mut wacc = equity_weight * ke + debt_weight * after_tax_kd;
    wacc = wacc.clamp(0.05, 0.30);

    WACCOutput {
        peers: peer_set.peers.clone(),
        median_unlevered_beta: round4(median_bu),
        target_levered_beta: round4(target_levered_beta),
        target_de_ratio: de,
        risk_free_rate,
        equity_risk_premium,
        cost_of_equity: round4(ke),
        cost_of_debt_pretax,
        tax_rate: target_tax_rate,
        after_tax_cost_of_debt: round4(after_tax_kd),
        target_market_cap,
        target_debt,
        target_total_capital: total_capital,
        equity_weight: round4(equity_weight),
        debt_weight: round4(debt_weight),
        wacc: round4(wacc),
    }
}

/// Empty-peer fallback PeerSet used when no LLM/market peers are available.
pub fn fallback_peer_set(ticker: &str, market_cap: f64, de: f64) -> PeerSet {
    PeerSet {
        target_ticker: ticker.to_string(),
        target_market_cap: market_cap,
        target_de_ratio: de,
        peers: Vec::new(),
        excluded: Vec::new(),
        source: "fallback".into(),
    }
}

fn round4(x: f64) -> f64 {
    (x * 10_000.0).round() / 10_000.0
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
        let re = cost_of_equity(input.risk_free_rate, input.equity_risk_premium, input.beta);
        assert!((re - 0.09).abs() < 1e-12);
        let wacc = calculate(&input);
        assert!((wacc - 0.066).abs() < 1e-12);
    }

    #[test]
    fn test_compute_wacc_no_peers_clamped() {
        let ps = fallback_peer_set("TEST", 1000.0, 0.3);
        let out = compute_wacc(&ps, 1000.0, 300.0, 0.04, 0.05, 0.04, 0.21, Some(0.3), 1.0);
        assert!(out.wacc >= 0.05 && out.wacc <= 0.30);
        assert!((out.median_unlevered_beta - 1.0).abs() < 1e-9);
        assert!(out.equity_weight > 0.0);
    }

    #[test]
    fn test_unlever_relever_roundtrip() {
        let bl = 1.2;
        let de = 0.5;
        let t = 0.25;
        let bu = unlever_beta(bl, de, t);
        let back = relever_beta(bu, de, t);
        assert!((back - bl).abs() < 1e-12);
    }
}
