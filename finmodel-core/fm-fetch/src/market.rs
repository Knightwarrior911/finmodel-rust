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

/// Yahoo chart hosts, tried in order (query2 is the failover mirror).
const QUOTE_HOSTS: [&str; 2] = [
    "https://query1.finance.yahoo.com/v8/finance/chart/",
    "https://query2.finance.yahoo.com/v8/finance/chart/",
];

/// A market quote for one ticker (price + context).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Quote {
    pub ticker: String,
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

/// Fetch the `meta` object of a Yahoo chart response for `sym`, failing over
/// query1 → query2 with one retry each. The returned `Err` distinguishes a
/// network failure from a parse failure so callers can report the cause.
fn fetch_chart_meta(sym: &str) -> Result<Value, FetchError> {
    let path = format!("{sym}?interval=1d&range=1d");
    let mut last: Option<FetchError> = None;
    for host in QUOTE_HOSTS {
        for attempt in 0..2 {
            let url = format!("{host}{path}");
            match MARKET_CLIENT.get(&url).send() {
                Ok(resp) => match resp.error_for_status() {
                    Ok(ok) => match ok.json::<Value>() {
                        Ok(v) => {
                            if let Some(meta) = v.pointer("/chart/result/0/meta") {
                                return Ok(meta.clone());
                            }
                            last = Some(FetchError::Parse(format!("no chart meta for {sym}")));
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

/// Fetch the latest market quote for a ticker. `Err` on network/parse failure
/// or a missing/non-positive price — callers degrade gracefully (blank), never
/// fabricate, and can surface the reason.
pub fn fetch_quote(ticker: &str) -> Result<Quote, FetchError> {
    let sym = ticker.trim().to_uppercase();
    let meta = fetch_chart_meta(&sym)?;
    let price = meta
        .get("regularMarketPrice")
        .and_then(Value::as_f64)
        .filter(|p| *p > 0.0)
        .ok_or_else(|| FetchError::NoPrice(sym.clone()))?;
    let raw_ccy = meta.get("currency").and_then(Value::as_str).unwrap_or("USD");
    // Normalize minor-unit venues (LSE "GBp" pence, JSE "ZAc", TASE "ILA") to the
    // major unit so every price field in the Quote shares consistent units.
    let div = minor_unit_divisor(raw_ccy);
    let (currency, price) = normalize_minor_unit(raw_ccy, price);
    Ok(Quote {
        ticker: sym,
        price,
        currency,
        week52_high: meta.get("fiftyTwoWeekHigh").and_then(Value::as_f64).map(|v| v / div),
        week52_low: meta.get("fiftyTwoWeekLow").and_then(Value::as_f64).map(|v| v / div),
        as_of_epoch: meta.get("regularMarketTime").and_then(Value::as_i64),
    })
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
    let meta = fetch_chart_meta(&format!("{from}USD=X"))?;
    meta.get("regularMarketPrice")
        .and_then(Value::as_f64)
        .filter(|r| *r > 0.0)
        .ok_or_else(|| FetchError::NoPrice(format!("{from}USD")))
}
