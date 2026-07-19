//! Market quotes via the public Yahoo Finance chart endpoint (no API key).
//!
//! Trading multiples (EV/EBITDA, P/E, EV/Revenue) are the heart of IB comps.
//! Every EV component — net debt, diluted shares, EBITDA, net income — comes
//! from filings; the ONLY market input is the current share price, fetched here.
//! Provenance downstream marks the price as market-sourced, not a filing figure.
//!
//! Resilience: one shared client, query1→query2 host failover, a short retry,
//! and a typed [`FetchError`] so callers can record *why* a value is blank
//! (network vs parse vs no-price) rather than silently dropping it.

use std::sync::LazyLock;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::cache::{FX_CACHE, QUOTE_CACHE};
use crate::retry::{RetryClass, classify_status, with_retries};

/// Yahoo chart hosts, tried in order (query2 is the failover mirror).
const QUOTE_HOSTS: [&str; 2] = [
    "https://query1.finance.yahoo.com/v8/finance/chart/",
    "https://query2.finance.yahoo.com/v8/finance/chart/",
];

/// A market quote for one ticker (price + context).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Quote {
    pub ticker: String,
    /// Company display name from the quote feed (longName || shortName).
    /// Lets query builders name a company whose only handle is a local
    /// exchange ticker ("MC.PA" → "LVMH Moët Hennessy …").
    #[serde(default)]
    pub name: Option<String>,
    pub price: f64,
    pub currency: String,
    pub week52_high: Option<f64>,
    pub week52_low: Option<f64>,
    /// Quote timestamp (unix seconds), for the "as of" provenance.
    pub as_of_epoch: Option<i64>,
}

/// Why a market fetch failed — lets callers record a specific data warning
/// instead of a silent blank. `Display` gives a human-readable reason.
#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    #[error("network error: {0}")]
    Network(String),
    #[error("parse error: {0}")]
    Parse(String),
    #[error("no price available for {0}")]
    NoPrice(String),
}

/// Shared blocking client (one TLS pool). Yahoo rejects the default reqwest UA,
/// so a browser UA is required.
static MARKET_CLIENT: LazyLock<reqwest::blocking::Client> = LazyLock::new(|| {
    reqwest::blocking::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) finmodel/0.1")
        .build()
        .unwrap_or_else(|_| reqwest::blocking::Client::new())
});

/// Fetch the `meta` object of a Yahoo chart response for `sym`. Hosts are tried
/// in order; each host uses the Phase 3.4 retry policy (connection / 408 / 429 /
/// 5xx only, twice max). Parse/schema failures are terminal for that host.
fn fetch_chart_meta(sym: &str) -> Result<Value, FetchError> {
    let path = format!("{sym}?interval=1d&range=1d");
    let mut last: Option<FetchError> = None;
    for host in QUOTE_HOSTS {
        let url = format!("{host}{path}");
        match with_retries(|| match MARKET_CLIENT.get(&url).send() {
            Ok(resp) => {
                let status = resp.status();
                let code = status.as_u16();
                match classify_status(code) {
                    RetryClass::Success => match resp.json::<Value>() {
                        Ok(v) => {
                            if let Some(meta) = v.pointer("/chart/result/0/meta") {
                                Ok(meta.clone())
                            } else {
                                Err((
                                    false,
                                    None,
                                    FetchError::Parse(format!("no chart meta for {sym}")),
                                ))
                            }
                        }
                        Err(e) => Err((false, None, FetchError::Parse(e.to_string()))),
                    },
                    RetryClass::Retriable => {
                        let ra = resp
                            .headers()
                            .get("retry-after")
                            .and_then(|v| v.to_str().ok())
                            .map(|s| s.to_string());
                        drop(resp);
                        Err((
                            true,
                            ra,
                            FetchError::Network(format!("HTTP {status} for {sym}")),
                        ))
                    }
                    RetryClass::Terminal => Err((
                        false,
                        None,
                        FetchError::Network(format!("HTTP {status} for {sym}")),
                    )),
                }
            }
            Err(e) => Err((true, None, FetchError::Network(e.to_string()))),
        }) {
            Ok(meta) => return Ok(meta),
            Err(e) => last = Some(e),
        }
    }
    Err(last.unwrap_or_else(|| FetchError::Network(format!("no response for {sym}"))))
}

/// Fetch the latest market quote for a ticker. `Err` on network/parse failure
/// or a missing/non-positive price — callers degrade gracefully (blank), never
/// fabricate, and can surface the reason.
pub fn fetch_quote(ticker: &str) -> Result<Quote, FetchError> {
    let sym = ticker.trim().to_uppercase();
    if let Some(cached) = QUOTE_CACHE.get(&sym)
        && let Ok(q) = serde_json::from_str::<Quote>(&cached)
    {
        return Ok(q);
    }
    let meta = fetch_chart_meta(&sym)?;
    let price = meta
        .get("regularMarketPrice")
        .and_then(Value::as_f64)
        .filter(|p| *p > 0.0)
        .ok_or_else(|| FetchError::NoPrice(sym.clone()))?;
    let raw_ccy = meta
        .get("currency")
        .and_then(Value::as_str)
        .unwrap_or("USD");
    // Normalize minor-unit venues (LSE "GBp" pence, JSE "ZAc", TASE "ILA") to the
    // major unit so every price field in the Quote shares consistent units.
    let div = minor_unit_divisor(raw_ccy);
    let (currency, price) = normalize_minor_unit(raw_ccy, price);
    let q = Quote {
        ticker: sym.clone(),
        name: meta
            .get("longName")
            .or_else(|| meta.get("shortName"))
            .and_then(Value::as_str)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        price,
        currency,
        week52_high: meta
            .get("fiftyTwoWeekHigh")
            .and_then(Value::as_f64)
            .map(|v| v / div),
        week52_low: meta
            .get("fiftyTwoWeekLow")
            .and_then(Value::as_f64)
            .map(|v| v / div),
        as_of_epoch: meta.get("regularMarketTime").and_then(Value::as_i64),
    };
    if let Ok(blob) = serde_json::to_string(&q) {
        QUOTE_CACHE.insert(sym, blob);
    }
    Ok(q)
}

/// Divisor mapping a minor-unit quote to its major unit (100 for pence/cents),
/// else 1.0.
fn minor_unit_divisor(ccy: &str) -> f64 {
    match ccy {
        "GBp" | "GBX" | "ZAc" | "ZAX" | "ILA" => 100.0,
        _ => 1.0,
    }
}

/// Normalize a quote currency reported in a minor unit to its major unit,
/// scaling the price. LSE "GBp" (pence = 1/100 GBP), JSE "ZAc" (cents), TASE
/// "ILA" (agorot). Matched case-sensitively on Yahoo's exact (mixed-case) code;
/// any other code passes through upper-cased.
pub fn normalize_minor_unit(ccy: &str, price: f64) -> (String, f64) {
    let major = match ccy {
        "GBp" | "GBX" => "GBP",
        "ZAc" | "ZAX" => "ZAR",
        "ILA" => "ILS",
        other => return (other.to_uppercase(), price),
    };
    (major.to_string(), price / minor_unit_divisor(ccy))
}

/// Spot FX rate: units of USD per 1 unit of `from` currency (e.g. EUR→~1.08,
/// TWD→~0.031). `USD` returns 1.0. `Err` on failure. Yahoo pair `{FROM}USD=X`.
pub fn fetch_fx_rate(from: &str) -> Result<f64, FetchError> {
    let from = from.trim().to_uppercase();
    if from == "USD" {
        return Ok(1.0);
    }
    if from.len() != 3 {
        return Err(FetchError::Parse(format!("invalid currency code {from:?}")));
    }
    let key = format!("{from}USD");
    if let Some(r) = FX_CACHE.get(&key) {
        return Ok(r);
    }
    let meta = fetch_chart_meta(&format!("{from}USD=X"))?;
    let rate = meta
        .get("regularMarketPrice")
        .and_then(Value::as_f64)
        .filter(|r| *r > 0.0)
        .ok_or_else(|| FetchError::NoPrice(format!("{from}USD")))?;
    FX_CACHE.insert(key, rate);
    Ok(rate)
}

/// Fetch a full Yahoo chart response body (indicators included) for `sym`,
/// with the same query1→query2 host failover + one retry as [`fetch_chart_meta`].
fn fetch_chart_json(sym: &str, range: &str, interval: &str) -> Result<Value, FetchError> {
    let path = format!("{sym}?range={range}&interval={interval}");
    let mut last: Option<FetchError> = None;
    for host in QUOTE_HOSTS {
        for attempt in 0..2 {
            let url = format!("{host}{path}");
            match MARKET_CLIENT.get(&url).send() {
                Ok(resp) => match resp.error_for_status() {
                    Ok(ok) => match ok.json::<Value>() {
                        Ok(v) => {
                            if v.pointer("/chart/result/0").is_some() {
                                return Ok(v);
                            }
                            last = Some(FetchError::Parse(format!("no chart result for {sym}")));
                        }
                        Err(e) => last = Some(FetchError::Parse(e.to_string())),
                    },
                    Err(e) => last = Some(FetchError::Network(e.to_string())),
                },
                Err(e) => last = Some(FetchError::Network(e.to_string())),
            }
            if attempt == 0 {
                std::thread::sleep(Duration::from_millis(400));
            }
        }
    }
    Err(last.unwrap_or_else(|| FetchError::Network(format!("no response for {sym}"))))
}

/// Fetch the live 10-year US Treasury yield via the `^TNX` index (quoted in
/// tenths of a percent, i.e. price 42.0 → 4.20%). Returns the decimal rate.
/// Rejects values outside `0.001..0.15` as bad symbol data so a junk feed can
/// never poison a WACC.
pub fn fetch_risk_free_rate() -> Result<f64, FetchError> {
    let q = fetch_quote("^TNX")?;
    let rf = q.price / 100.0;
    if !(0.001..0.15).contains(&rf) {
        return Err(FetchError::Parse(format!(
            "risk-free rate out of range: {rf}"
        )));
    }
    Ok(rf)
}

/// Fetch a price history (adjusted close, else close) for `ticker` over
/// `range`/`interval` (Yahoo chart params, e.g. "2y"/"1wk"). Nulls are dropped;
/// `Err(Parse)` when fewer than 2 usable points remain.
pub fn fetch_price_history(
    ticker: &str,
    range: &str,
    interval: &str,
) -> Result<Vec<f64>, FetchError> {
    let sym = ticker.trim().to_uppercase();
    let v = fetch_chart_json(&sym, range, interval)?;
    let series = v
        .pointer("/chart/result/0/indicators/adjclose/0/adjclose")
        .or_else(|| v.pointer("/chart/result/0/indicators/quote/0/close"))
        .and_then(Value::as_array)
        .ok_or_else(|| FetchError::Parse(format!("no price series for {sym}")))?;
    let prices: Vec<f64> = series
        .iter()
        .filter_map(Value::as_f64)
        .filter(|p| *p > 0.0)
        .collect();
    if prices.len() < 2 {
        return Err(FetchError::Parse(format!(
            "insufficient price points for {sym}"
        )));
    }
    Ok(prices)
}

/// Compute a regression beta of `asset` returns against `market` returns.
/// Pure: truncates both to the common length, forms simple period returns,
/// requires >=40 return pairs, and returns `beta = cov(a,m) / var(m)`.
/// Returns `None` outside `0.2..=3.0` (junk regression) or on degenerate input.
pub fn compute_beta(asset: &[f64], market: &[f64]) -> Option<f64> {
    let n = asset.len().min(market.len());
    if n < 2 {
        return None;
    }
    let asset = &asset[..n];
    let market = &market[..n];
    let ret = |s: &[f64]| -> Vec<f64> {
        s.windows(2)
            .filter(|w| w[0] != 0.0)
            .map(|w| w[1] / w[0] - 1.0)
            .collect()
    };
    let ar = ret(asset);
    let mr = ret(market);
    let m = ar.len().min(mr.len());
    if m < 40 {
        return None;
    }
    let ar = &ar[..m];
    let mr = &mr[..m];
    let mean = |s: &[f64]| s.iter().sum::<f64>() / s.len() as f64;
    let am = mean(ar);
    let mm = mean(mr);
    let mut cov = 0.0;
    let mut var = 0.0;
    for i in 0..m {
        cov += (ar[i] - am) * (mr[i] - mm);
        var += (mr[i] - mm) * (mr[i] - mm);
    }
    if var == 0.0 {
        return None;
    }
    let beta = cov / var;
    if (0.2..=3.0).contains(&beta) {
        Some(beta)
    } else {
        None
    }
}

/// Fetch a 2-year weekly regression beta for `ticker` against the S&P 500
/// (`^GSPC`, the benchmark for ALL tickers, foreign included). `Err(NoPrice)`
/// when the regression is degenerate or out of the sane `0.2..=3.0` band.
pub fn fetch_beta(ticker: &str) -> Result<f64, FetchError> {
    let asset = fetch_price_history(ticker, "2y", "1wk")?;
    let market = fetch_price_history("^GSPC", "2y", "1wk")?;
    compute_beta(&asset, &market).ok_or_else(|| FetchError::NoPrice(format!("beta for {ticker}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_beta_scales_two_x() {
        // Market is a linear ramp; asset moves at 2x the market's returns.
        let market: Vec<f64> = (0..60).map(|i| 100.0 + i as f64).collect();
        let mut asset = vec![100.0];
        for w in market.windows(2) {
            let mret = w[1] / w[0] - 1.0;
            let prev = *asset.last().unwrap();
            asset.push(prev * (1.0 + 2.0 * mret));
        }
        let beta = compute_beta(&asset, &market).expect("beta");
        assert!((beta - 2.0).abs() < 0.05, "beta {beta} not ~2.0");
    }

    #[test]
    fn compute_beta_too_short_is_none() {
        let market: Vec<f64> = (0..10).map(|i| 100.0 + i as f64).collect();
        let asset = market.clone();
        assert!(compute_beta(&asset, &market).is_none());
    }

    #[test]
    fn compute_beta_out_of_band_is_none() {
        // Asset uncorrelated / near-zero beta -> below 0.2 band -> None.
        let market: Vec<f64> = (0..60).map(|i| 100.0 + i as f64).collect();
        let asset: Vec<f64> = vec![100.0; 60];
        assert!(compute_beta(&asset, &market).is_none());
    }

    #[test]
    #[ignore = "live network"]
    fn fetch_risk_free_live() {
        let rf = fetch_risk_free_rate().expect("rf");
        assert!((0.01..0.10).contains(&rf), "rf {rf} not in 1-10%");
    }

    #[test]
    #[ignore = "live network"]
    fn fetch_beta_live() {
        let b = fetch_beta("AAPL").expect("beta");
        assert!((0.2..=3.0).contains(&b), "beta {b} out of band");
    }
}
