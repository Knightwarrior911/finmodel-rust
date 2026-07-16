//! Public trading-comps assembly — turn [`BenchmarkMetrics`] (filing figures ×
//! live prices) into the `fm_value` comps contract that the gated Comps Peers /
//! Comps Summary sheets consume. No estimates feed exists, so every NTM/FY field
//! is left at its zero/None default — never fabricated.

use std::collections::HashMap;

use fm_value::{CompMultipleStats, PublicCompPeer, PublicCompsOutput};

use crate::BenchmarkMetrics;

/// Canonical stat map keys (must match what the Comps Summary sheet reads).
const K_EV_REV: &str = "EV/Revenue (LTM)";
const K_EV_EBITDA: &str = "EV/EBITDA (LTM)";
const K_PE: &str = "P/E (LTM)";

/// Map one peer's benchmark metrics (+ optional live quote) into a
/// [`PublicCompPeer`]. LTM fields map 1:1 from the filing; multiples come from
/// the already-derived `m.ev_revenue/ev_ebitda/pe`; NTM/FY fields stay default.
pub fn peer_from_metrics(m: &BenchmarkMetrics, quote: Option<&fm_fetch::Quote>) -> PublicCompPeer {
    let share_price = m
        .share_price
        .or_else(|| quote.map(|q| q.price))
        .unwrap_or(0.0);
    let shares_diluted = m.shares_diluted.unwrap_or(0.0);
    let total_debt = m.total_debt.unwrap_or(0.0);
    let cash = m.cash.unwrap_or(0.0);
    let market_cap = m.market_cap.unwrap_or(share_price * shares_diluted);
    let enterprise_value = m.enterprise_value.unwrap_or(market_cap + total_debt - cash);
    // EV/EBIT is not carried on BenchmarkMetrics; derive it from the filing EV
    // and EBIT when both are present (same EV basis as the other multiples).
    let ev_ebit_ltm = match (m.enterprise_value, m.ebit) {
        (Some(ev), Some(ebit)) if ebit != 0.0 => Some(ev / ebit),
        _ => None,
    };
    PublicCompPeer {
        ticker: m.ticker.clone(),
        name: m.ticker.clone(),
        country: String::new(),
        currency: m.currency.clone(),
        tier: 1,
        share_price,
        shares_diluted,
        market_cap,
        total_debt,
        cash,
        enterprise_value,
        week52_high: quote.and_then(|q| q.week52_high).unwrap_or(0.0),
        week52_low: quote.and_then(|q| q.week52_low).unwrap_or(0.0),
        ltm_revenue: m.revenue.unwrap_or(0.0),
        ltm_ebitda: m.ebitda.unwrap_or(0.0),
        ltm_ebit: m.ebit.unwrap_or(0.0),
        ltm_net_income: m.net_income.unwrap_or(0.0),
        ltm_eps_diluted: m.eps_diluted.unwrap_or(0.0),
        // No estimates feed — leave every forward figure at its default.
        ntm_revenue: 0.0,
        ntm_ebitda: 0.0,
        fy1_revenue: 0.0,
        fy1_ebitda: 0.0,
        fy2_revenue: 0.0,
        fy2_ebitda: 0.0,
        ntm_eps: 0.0,
        fy1_eps: 0.0,
        ev_rev_ltm: m.ev_revenue,
        ev_ebitda_ltm: m.ev_ebitda,
        ev_ebit_ltm,
        pe_ltm: m.pe,
        ev_rev_ntm: None,
        ev_ebitda_ntm: None,
        ev_rev_fy1: None,
        ev_ebitda_fy1: None,
        ev_rev_fy2: None,
        ev_ebitda_fy2: None,
        pe_ntm: None,
        pe_fy1: None,
        rationale: m.sector.clone().unwrap_or_default(),
    }
}

/// Percentile by linear interpolation over a sorted slice (numpy convention:
/// position = (n-1)·q). `sorted` must be non-empty and ascending.
fn percentile(sorted: &[f64], q: f64) -> f64 {
    let n = sorted.len();
    if n == 1 {
        return sorted[0];
    }
    let pos = (n - 1) as f64 * q;
    let lo = pos.floor() as usize;
    let hi = pos.ceil() as usize;
    let frac = pos - lo as f64;
    sorted[lo] + (sorted[hi] - sorted[lo]) * frac
}

/// Build [`CompMultipleStats`] for one multiple over the present peer values.
/// Returns `None` when no peer carries the multiple (caller skips the entry).
fn stats_for(name: &str, values: Vec<f64>) -> Option<CompMultipleStats> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let count = sorted.len();
    let mean = sorted.iter().sum::<f64>() / count as f64;
    Some(CompMultipleStats {
        multiple_name: name.to_string(),
        values,
        min: sorted[0],
        p25: percentile(&sorted, 0.25),
        median: percentile(&sorted, 0.50),
        mean,
        p75: percentile(&sorted, 0.75),
        max: sorted[count - 1],
        count: count as i32,
    })
}

/// Assemble the full [`PublicCompsOutput`]: target figures, stat blocks over the
/// peer multiples, and EV/EBITDA-implied prices for the target.
pub fn build_public_comps(
    target: &BenchmarkMetrics,
    peers: &[PublicCompPeer],
    excluded: Vec<(String, String)>,
    as_of: &str,
) -> PublicCompsOutput {
    let target_ebitda = target.ebitda.unwrap_or(0.0);
    let target_total_debt = target.total_debt.unwrap_or(0.0);
    let target_cash = target.cash.unwrap_or(0.0);
    let target_shares_diluted = target.shares_diluted.unwrap_or(0.0);

    let mut stats: HashMap<String, CompMultipleStats> = HashMap::new();
    let ev_rev: Vec<f64> = peers.iter().filter_map(|p| p.ev_rev_ltm).collect();
    let ev_ebitda: Vec<f64> = peers.iter().filter_map(|p| p.ev_ebitda_ltm).collect();
    let pe: Vec<f64> = peers.iter().filter_map(|p| p.pe_ltm).collect();
    if let Some(s) = stats_for(K_EV_REV, ev_rev) {
        stats.insert(K_EV_REV.to_string(), s);
    }
    let ebitda_stats = stats_for(K_EV_EBITDA, ev_ebitda);
    if let Some(s) = &ebitda_stats {
        stats.insert(K_EV_EBITDA.to_string(), s.clone());
    }
    if let Some(s) = stats_for(K_PE, pe) {
        stats.insert(K_PE.to_string(), s);
    }

    // Implied prices from the EV/EBITDA stats: EV_q = stat_q × target_ebitda;
    // equity = EV_q − net_debt; price_q = equity / diluted shares. Degenerate
    // (no ebitda / no shares) ⇒ all three 0.0.
    let (mut implied_low, mut implied_median, mut implied_high) = (0.0, 0.0, 0.0);
    if let Some(s) = &ebitda_stats {
        if target_ebitda > 0.0 && target_shares_diluted > 0.0 {
            let net_debt = target_total_debt - target_cash;
            let price = |mult: f64| ((mult * target_ebitda) - net_debt) / target_shares_diluted;
            implied_low = price(s.p25);
            implied_median = price(s.median);
            implied_high = price(s.p75);
        }
    }

    PublicCompsOutput {
        target_ticker: target.ticker.clone(),
        target_company_name: target.ticker.clone(),
        as_of_date: as_of.to_string(),
        target_revenue: target.revenue.unwrap_or(0.0),
        target_ebitda,
        target_ebit: target.ebit.unwrap_or(0.0),
        target_net_income: target.net_income.unwrap_or(0.0),
        target_total_debt,
        target_cash,
        target_shares_diluted,
        peers: peers.to_vec(),
        excluded,
        stats,
        implied_price_low: implied_low,
        implied_price_median: implied_median,
        implied_price_high: implied_high,
        source: "EDGAR filings × Yahoo Finance prices".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peer(ev_ebitda: f64, ev_rev: f64, pe: f64) -> PublicCompPeer {
        PublicCompPeer {
            ev_ebitda_ltm: Some(ev_ebitda),
            ev_rev_ltm: Some(ev_rev),
            pe_ltm: Some(pe),
            ..Default::default()
        }
    }

    #[test]
    fn stats_percentiles_fixed_vector() {
        // EV/EBITDA values {6,8,10,12}; numpy linear interp on (n-1)*q.
        let peers = vec![
            peer(6.0, 1.0, 10.0),
            peer(8.0, 2.0, 12.0),
            peer(10.0, 3.0, 14.0),
            peer(12.0, 4.0, 16.0),
        ];
        let target = BenchmarkMetrics {
            ticker: "T".into(),
            ebitda: Some(100.0),
            total_debt: Some(20.0),
            cash: Some(5.0),
            shares_diluted: Some(10.0),
            ..Default::default()
        };
        let out = build_public_comps(&target, &peers, vec![], "2024-01-01");
        let s = out.stats.get(K_EV_EBITDA).expect("ebitda stats");
        assert_eq!(s.count, 4);
        assert!((s.min - 6.0).abs() < 1e-9);
        assert!((s.max - 12.0).abs() < 1e-9);
        assert!((s.median - 9.0).abs() < 1e-9); // (n-1)*0.5 = 1.5 -> 8 + 0.5*(10-8)
        assert!((s.p25 - 7.5).abs() < 1e-9); // 1.5*0.25=0.75 -> 6+0.75*2
        assert!((s.p75 - 10.5).abs() < 1e-9); // 1.5*3=... pos=2.25 -> 10+0.25*2
        assert!((s.mean - 9.0).abs() < 1e-9);
        // Implied median = (9.0*100 - (20-5))/10 = (900-15)/10 = 88.5
        assert!((out.implied_price_median - 88.5).abs() < 1e-9);
        assert!((out.implied_price_low - (7.5 * 100.0 - 15.0) / 10.0).abs() < 1e-9);
        assert!((out.implied_price_high - (10.5 * 100.0 - 15.0) / 10.0).abs() < 1e-9);
    }

    #[test]
    fn implied_price_zero_without_ebitda() {
        let peers = vec![peer(6.0, 1.0, 10.0), peer(8.0, 2.0, 12.0)];
        let target = BenchmarkMetrics {
            ticker: "T".into(),
            ebitda: Some(0.0),
            shares_diluted: Some(10.0),
            ..Default::default()
        };
        let out = build_public_comps(&target, &peers, vec![], "2024-01-01");
        assert_eq!(out.implied_price_low, 0.0);
        assert_eq!(out.implied_price_median, 0.0);
        assert_eq!(out.implied_price_high, 0.0);
    }

    #[test]
    fn implied_price_zero_without_shares() {
        let peers = vec![peer(6.0, 1.0, 10.0), peer(8.0, 2.0, 12.0)];
        let target = BenchmarkMetrics {
            ticker: "T".into(),
            ebitda: Some(100.0),
            shares_diluted: Some(0.0),
            ..Default::default()
        };
        let out = build_public_comps(&target, &peers, vec![], "2024-01-01");
        assert_eq!(out.implied_price_median, 0.0);
    }

    #[test]
    fn empty_multiple_skipped() {
        // Peers carry EV/EBITDA but no P/E -> P/E entry absent.
        let mut p = peer(6.0, 1.0, 0.0);
        p.pe_ltm = None;
        let target = BenchmarkMetrics {
            ticker: "T".into(),
            ..Default::default()
        };
        let out = build_public_comps(&target, &[p], vec![], "2024-01-01");
        assert!(out.stats.contains_key(K_EV_EBITDA));
        assert!(!out.stats.contains_key(K_PE));
    }

    #[test]
    fn peer_from_metrics_maps_ltm() {
        let m = BenchmarkMetrics {
            ticker: "MSFT".into(),
            currency: "USD".into(),
            revenue: Some(200.0),
            ebit: Some(80.0),
            ebitda: Some(90.0),
            net_income: Some(60.0),
            eps_diluted: Some(8.0),
            shares_diluted: Some(7.5),
            total_debt: Some(50.0),
            cash: Some(10.0),
            market_cap: Some(2000.0),
            enterprise_value: Some(2040.0),
            ev_revenue: Some(10.2),
            ev_ebitda: Some(22.6),
            pe: Some(33.3),
            sector: Some("Software".into()),
            ..Default::default()
        };
        let p = peer_from_metrics(&m, None);
        assert_eq!(p.ticker, "MSFT");
        assert_eq!(p.name, "MSFT");
        assert_eq!(p.tier, 1);
        assert_eq!(p.ltm_revenue, 200.0);
        assert_eq!(p.enterprise_value, 2040.0);
        assert_eq!(p.market_cap, 2000.0);
        assert_eq!(p.ev_ebitda_ltm, Some(22.6));
        assert_eq!(p.ev_ebit_ltm, Some(2040.0 / 80.0));
        assert_eq!(p.rationale, "Software");
        assert_eq!(p.ntm_revenue, 0.0);
        assert_eq!(p.pe_ntm, None);
    }
}
