//! Market quotes via the public Yahoo Finance chart endpoint (no API key).
//!
//! Trading multiples (EV/EBITDA, P/E, EV/Revenue) are the heart of IB comps.
//! Every EV component — net debt, diluted shares, EBITDA, net income — comes
//! from filings; the ONLY market input is the current share price, fetched here.
//! Provenance downstream marks the price as market-sourced, not a filing figure.

use serde::{Deserialize, Serialize};

const QUOTE_URL: &str = "https://query1.finance.yahoo.com/v8/finance/chart/";

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

fn client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        // Yahoo rejects the default reqwest UA; a browser UA is required.
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) finmodel/0.1")
        .build()
        .expect("reqwest client build")
}

/// Fetch the latest market quote for a US ticker. `None` on any network/parse
/// failure or missing price — callers degrade gracefully, never fabricate.
pub fn fetch_quote(ticker: &str) -> Option<Quote> {
    let url = format!("{QUOTE_URL}{}?interval=1d&range=1d", ticker.trim().to_uppercase());
    let resp = client().get(&url).send().ok()?.error_for_status().ok()?;
    let v: serde_json::Value = resp.json().ok()?;
    let meta = v.pointer("/chart/result/0/meta")?;
    let price = meta.get("regularMarketPrice").and_then(serde_json::Value::as_f64)?;
    if price <= 0.0 {
        return None;
    }
    Some(Quote {
        ticker: ticker.trim().to_uppercase(),
        price,
        currency: meta
            .get("currency")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("USD")
            .to_string(),
        week52_high: meta.get("fiftyTwoWeekHigh").and_then(serde_json::Value::as_f64),
        week52_low: meta.get("fiftyTwoWeekLow").and_then(serde_json::Value::as_f64),
        as_of_epoch: meta.get("regularMarketTime").and_then(serde_json::Value::as_i64),
    })
}

/// Spot FX rate: units of USD per 1 unit of `from` currency (e.g. EUR→~1.08,
/// TWD→~0.031). `USD` returns 1.0. `None` on failure. Yahoo pair `{FROM}USD=X`.
pub fn fetch_fx_rate(from: &str) -> Option<f64> {
    let from = from.trim().to_uppercase();
    if from == "USD" {
        return Some(1.0);
    }
    if from.len() != 3 {
        return None;
    }
    let url = format!("{QUOTE_URL}{from}USD=X?interval=1d&range=1d");
    let resp = client().get(&url).send().ok()?.error_for_status().ok()?;
    let v: serde_json::Value = resp.json().ok()?;
    let rate = v
        .pointer("/chart/result/0/meta/regularMarketPrice")
        .and_then(serde_json::Value::as_f64)?;
    if rate > 0.0 { Some(rate) } else { None }
}
