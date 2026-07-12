//! fm-research — the benchmarking subsystem.
//!
//! Turns filing figures (SEC EDGAR XBRL via `fm-extract`) for a peer set into an
//! investment-banker-grade benchmark workbook: one row per company, columns for
//! scale / growth / profitability / returns / leverage, a MEDIAN/MEAN/MIN/MAX
//! summary block, and per-cell provenance notes back to the filing. Rendering
//! rides the gated `fm-excel` ad-hoc engine.
//!
//! Layers:
//!   * [`metrics_from_extraction`] — pure: `ExtractionResult` → [`BenchmarkMetrics`].
//!   * [`build_benchmark_table`]    — pure: `&[BenchmarkMetrics]` → `AdHocTable`.
//!   * [`render_benchmark`]         — write the workbook to disk.
//!   * [`benchmark_tickers`]        — live: fetch each ticker, then the above.

use std::collections::HashMap;

use fm_excel::adhoc::{AdHocTable, CellVal, ColKind, ColumnSpec, Grain};
use fm_excel::model::Workbook;
use fm_extract::ExtractionResult;
use fm_types::StatementData;

/// One dollar-figure → millions.
const MILLIONS: f64 = 1_000_000.0;

/// All benchmark metrics for a single company, from its latest reported fiscal
/// year. Monetary fields are in the filing's native units (NOT scaled); ratios
/// are fractions (0.30 == 30%). Any field may be `None` when the filing lacks it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BenchmarkMetrics {
    pub ticker: String,
    pub currency: String,
    /// Latest reported fiscal year label (e.g. `"2024"`), if known.
    pub fiscal_year: Option<String>,
    /// SIC industry description from EDGAR (e.g. "National Commercial Banks").
    pub sector: Option<String>,
    /// `canonical_key → matched us-gaap XBRL tag` for exact filing-fact citation.
    pub provenance: std::collections::HashMap<String, String>,

    // ── Raw filing figures (native units) ────────────────────────────────
    pub revenue: Option<f64>,
    pub gross_profit: Option<f64>,
    pub ebit: Option<f64>,
    pub da: Option<f64>,
    pub ebitda: Option<f64>,
    pub net_income: Option<f64>,
    pub total_debt: Option<f64>,
    pub cash: Option<f64>,
    pub total_equity: Option<f64>,
    pub total_assets: Option<f64>,
    pub cfo: Option<f64>,
    pub capex: Option<f64>,
    pub eps_diluted: Option<f64>,
    pub shares_diluted: Option<f64>,
    pub interest_expense: Option<f64>,
    pub total_current_assets: Option<f64>,
    pub total_current_liabilities: Option<f64>,
    pub dividends_paid: Option<f64>,
    pub buybacks: Option<f64>,

    // ── Derived (native units) ───────────────────────────────────────────
    pub net_debt: Option<f64>,
    pub fcf: Option<f64>,

    // ── Derived ratios (fractions / multiples) ───────────────────────────
    pub revenue_growth: Option<f64>,
    pub gross_margin: Option<f64>,
    pub ebit_margin: Option<f64>,
    pub ebitda_margin: Option<f64>,
    pub net_margin: Option<f64>,
    pub roe: Option<f64>,
    pub roa: Option<f64>,
    pub net_debt_to_ebitda: Option<f64>,
    pub revenue_cagr_3y: Option<f64>,
    pub fcf_margin: Option<f64>,
    pub interest_coverage: Option<f64>,
    pub current_ratio: Option<f64>,
    pub payout_ratio: Option<f64>,
    pub total_payout_ratio: Option<f64>,

    // ── Trading multiples (market price × filing figures; opt-in) ─────────
    pub share_price: Option<f64>,
    pub price_currency: Option<String>,
    pub market_cap: Option<f64>,
    pub enterprise_value: Option<f64>,
    pub ev_revenue: Option<f64>,
    pub ev_ebitda: Option<f64>,
    pub pe: Option<f64>,
}

/// Value at `idx` in a statement line (period-aligned, oldest-first).
fn at(stmt: &StatementData, key: &str, idx: usize) -> Option<f64> {
    stmt.get(key).and_then(|v| v.get(idx).copied().flatten())
}

/// `a / b`, guarding against a zero / missing denominator.
fn ratio(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    match (a, b) {
        (Some(a), Some(b)) if b != 0.0 => Some(a / b),
        _ => None,
    }
}

/// `a + b` when both present.
fn add(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a + b),
        _ => None,
    }
}

/// Sum the present operands; `None` only when both are absent (so total debt is
/// still reported when a filing tags just one of LT / current debt).
fn sum_opt(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a + b),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

/// Compute a company's latest-FY benchmark metrics from an extraction.
///
/// Pure and deterministic — the offline-testable core. The latest period is the
/// last column of `years_found`; growth uses the prior column when present.
pub fn metrics_from_extraction(ticker: &str, ex: &ExtractionResult) -> BenchmarkMetrics {
    let n = ex.years_found.len();
    let mut m = BenchmarkMetrics {
        ticker: ticker.to_string(),
        currency: ex.currency.clone(),
        ..Default::default()
    };
    if n == 0 {
        return m;
    }
    let last = n - 1;
    m.fiscal_year = ex.years_found.get(last).cloned();

    let is = &ex.income_statement;
    let bs = &ex.balance_sheet;
    let cfs = &ex.cash_flow_statement;

    m.revenue = at(is, "revenue", last);
    // Prefer a reported gross profit; else derive revenue − COGS (many tech /
    // retail filers tag COGS but not a GrossProfit line).
    m.gross_profit = at(is, "gross_profit", last).or_else(|| {
        match (at(is, "revenue", last), at(is, "cogs", last)) {
            (Some(rev), Some(cogs)) => Some(rev - cogs),
            _ => None,
        }
    });
    m.ebit = at(is, "ebit", last);
    m.da = at(is, "da", last);
    m.ebitda = add(m.ebit, m.da);
    m.net_income = at(is, "net_income", last);
    m.eps_diluted = at(is, "eps_diluted", last);
    m.shares_diluted = at(is, "shares_diluted", last);
    m.interest_expense = at(is, "interest_expense", last);
    m.total_current_assets = at(bs, "total_current_assets", last);
    m.total_current_liabilities = at(bs, "total_current_liabilities", last);

    // Total debt = long-term (incl. finance leases) + current portion / short-term
    // borrowings, so leverage isn't understated for revolver / CP-heavy names.
    m.total_debt = sum_opt(at(bs, "long_term_debt", last), at(bs, "short_term_debt", last));
    m.cash = at(bs, "cash", last);
    m.total_equity = at(bs, "total_equity", last);
    m.total_assets = at(bs, "total_assets", last);
    m.net_debt = match (m.total_debt, m.cash) {
        (Some(d), Some(c)) => Some(d - c),
        _ => None,
    };

    m.cfo = at(cfs, "cfo", last);
    m.capex = at(cfs, "capex", last);
    m.dividends_paid = at(cfs, "dividends_paid", last);
    m.buybacks = at(cfs, "buybacks", last);
    // FCF = CFO less capital expenditure (treated as an outflow regardless of
    // the filing's sign convention).
    m.fcf = match (m.cfo, m.capex) {
        (Some(cfo), Some(capex)) => Some(cfo - capex.abs()),
        _ => None,
    };

    // Ratios.
    if last >= 1 {
        let prev_rev = at(is, "revenue", last - 1);
        m.revenue_growth = match (m.revenue, prev_rev) {
            (Some(r), Some(p)) if p != 0.0 => Some(r / p - 1.0),
            _ => None,
        };
    }
    m.gross_margin = ratio(m.gross_profit, m.revenue);
    m.ebit_margin = ratio(m.ebit, m.revenue);
    m.ebitda_margin = ratio(m.ebitda, m.revenue);
    m.net_margin = ratio(m.net_income, m.revenue);
    m.roe = ratio(m.net_income, m.total_equity);
    m.roa = ratio(m.net_income, m.total_assets);
    m.net_debt_to_ebitda = ratio(m.net_debt, m.ebitda);
    m.fcf_margin = ratio(m.fcf, m.revenue);
    // Interest coverage = EBIT / |interest expense| (filers tag it +/-).
    m.interest_coverage = match (m.ebit, m.interest_expense) {
        (Some(ebit), Some(int)) if int.abs() != 0.0 => Some(ebit / int.abs()),
        _ => None,
    };
    m.current_ratio = ratio(m.total_current_assets, m.total_current_liabilities);
    // Capital return (payout) — dividends/buybacks are cash outflows in the CFS,
    // tagged +/- by filer; use magnitudes over positive net income.
    m.payout_ratio = match (m.dividends_paid, m.net_income) {
        (Some(d), Some(ni)) if ni > 0.0 => Some(d.abs() / ni),
        _ => None,
    };
    m.total_payout_ratio = match (m.net_income, m.dividends_paid, m.buybacks) {
        (Some(ni), div, bb) if ni > 0.0 && (div.is_some() || bb.is_some()) => {
            Some((div.map(f64::abs).unwrap_or(0.0) + bb.map(f64::abs).unwrap_or(0.0)) / ni)
        }
        _ => None,
    };
    // 3-yr (window) revenue CAGR across the oldest→latest revenue with data.
    m.revenue_cagr_3y = revenue_cagr(is);

    m
}

/// Compound annual growth rate of revenue over the full reported window
/// (oldest non-null to latest non-null). `None` unless ≥2 positive endpoints.
fn revenue_cagr(is: &StatementData) -> Option<f64> {
    let series = is.get("revenue")?;
    let vals: Vec<(usize, f64)> = series
        .iter()
        .enumerate()
        .filter_map(|(i, v)| v.map(|x| (i, x)))
        .collect();
    if vals.len() < 2 {
        return None;
    }
    let (i0, first) = vals[0];
    let (i1, last) = *vals.last().unwrap();
    if first <= 0.0 || last <= 0.0 || i1 == i0 {
        return None;
    }
    let periods = (i1 - i0) as f64;
    Some((last / first).powf(1.0 / periods) - 1.0)
}

/// Column set for the peer benchmark (order chosen so groups stay contiguous).
/// `multiples` inserts the market-price Trading Multiples group after Sector.
fn benchmark_columns(multiples: bool) -> Vec<ColumnSpec> {
    let mut cols = vec![
        ColumnSpec::label("ticker", "Ticker"),
        ColumnSpec::metric("currency", "Ccy", ColKind::Text)
            .with_definition("Reporting currency (metrics NOT FX-normalized across peers)"),
        ColumnSpec::metric("sector", "Sector", ColKind::Text)
            .with_definition("SIC industry (EDGAR). Financials' leverage/coverage read differently."),
    ];
    if multiples {
        cols.push(
            ColumnSpec::metric("market_cap", "Mkt Cap", ColKind::Dollar)
                .with_group("Trading Multiples")
                .with_units("reporting-ccy millions")
                .with_definition("Share price × diluted shares (price = live market quote)"),
        );
        cols.push(
            ColumnSpec::metric("ev_revenue", "EV / Rev", ColKind::Multiple)
                .with_group("Trading Multiples")
                .with_definition("Enterprise value / revenue (EV = mkt cap + net debt)"),
        );
        cols.push(
            ColumnSpec::metric("ev_ebitda", "EV / EBITDA", ColKind::Multiple)
                .with_group("Trading Multiples")
                .with_definition("Enterprise value / EBITDA"),
        );
        cols.push(
            ColumnSpec::metric("pe", "P / E", ColKind::Multiple)
                .with_group("Trading Multiples")
                .with_definition("Market cap / net income (positive earnings only)"),
        );
    }
    cols.extend([
        ColumnSpec::metric("revenue", "Revenue", ColKind::Dollar)
            .with_group("Scale")
            .with_units("reporting-ccy millions")
            .with_definition("Total revenue, latest reported FY"),
        ColumnSpec::metric("ebitda", "EBITDA", ColKind::Dollar)
            .with_group("Scale")
            .with_units("reporting-ccy millions")
            .with_definition("EBIT + depreciation & amortisation"),
        ColumnSpec::metric("net_income", "Net Income", ColKind::Dollar)
            .with_group("Scale")
            .with_units("reporting-ccy millions"),
        ColumnSpec::metric("rev_growth", "Rev Growth", ColKind::Percent)
            .with_group("Growth")
            .with_definition("YoY revenue growth vs prior FY"),
        ColumnSpec::metric("rev_cagr_3y", "Rev CAGR", ColKind::Percent)
            .with_group("Growth")
            .with_definition("Revenue CAGR over the full reported window (oldest→latest FY)"),
        ColumnSpec::metric("gross_margin", "Gross Margin", ColKind::Percent)
            .with_group("Profitability"),
        ColumnSpec::metric("ebitda_margin", "EBITDA Margin", ColKind::Percent)
            .with_group("Profitability"),
        ColumnSpec::metric("net_margin", "Net Margin", ColKind::Percent)
            .with_group("Profitability"),
        ColumnSpec::metric("fcf_margin", "FCF Margin", ColKind::Percent)
            .with_group("Profitability")
            .with_definition("(CFO − capex) / revenue"),
        ColumnSpec::metric("roe", "ROE", ColKind::Percent)
            .with_group("Returns")
            .with_definition("Net income / total equity"),
        ColumnSpec::metric("roa", "ROA", ColKind::Percent)
            .with_group("Returns")
            .with_definition("Net income / total assets"),
        ColumnSpec::metric("payout_ratio", "Div Payout", ColKind::Percent)
            .with_group("Capital Return")
            .with_definition("|Dividends paid| / net income"),
        ColumnSpec::metric("total_payout", "Total Payout", ColKind::Percent)
            .with_group("Capital Return")
            .with_definition("(|dividends| + |buybacks|) / net income"),
        ColumnSpec::metric("current_ratio", "Current Ratio", ColKind::Multiple)
            .with_group("Liquidity")
            .with_definition("Total current assets / total current liabilities"),
        ColumnSpec::metric("net_debt", "Net Debt", ColKind::Dollar)
            .with_group("Leverage")
            .with_units("reporting-ccy millions")
            .with_definition("Total debt (long-term + current portion) − cash & equivalents"),
        ColumnSpec::metric("nd_ebitda", "Net Debt / EBITDA", ColKind::Multiple)
            .with_group("Leverage"),
        ColumnSpec::metric("interest_coverage", "Int. Coverage", ColKind::Multiple)
            .with_group("Leverage")
            .with_definition("EBIT / |interest expense|"),
    ]);
    cols
}

fn num(v: Option<f64>) -> CellVal {
    match v {
        Some(x) => CellVal::Number(x),
        None => CellVal::Empty,
    }
}

fn millions(v: Option<f64>) -> CellVal {
    num(v.map(|x| x / MILLIONS))
}

/// Build the benchmark [`AdHocTable`] (rows + columns + provenance) from a peer
/// set of computed metrics. Monetary cells are scaled to millions.
pub fn build_benchmark_table(metrics: &[BenchmarkMetrics], title: &str) -> AdHocTable {
    let has_multiples = metrics.iter().any(|m| m.market_cap.is_some() || m.share_price.is_some());
    let columns = benchmark_columns(has_multiples);
    let mut rows: Vec<HashMap<String, CellVal>> = Vec::with_capacity(metrics.len());
    let mut sources: HashMap<(String, String), String> = HashMap::new();

    for m in metrics {
        let mut r: HashMap<String, CellVal> = HashMap::new();
        r.insert("ticker".into(), CellVal::Text(m.ticker.clone()));
        r.insert("currency".into(), CellVal::Text(m.currency.clone()));
        r.insert(
            "sector".into(),
            match &m.sector {
                Some(s) if !s.is_empty() => CellVal::Text(s.clone()),
                _ => CellVal::Empty,
            },
        );
        r.insert("market_cap".into(), millions(m.market_cap));
        r.insert("ev_revenue".into(), num(m.ev_revenue));
        r.insert("ev_ebitda".into(), num(m.ev_ebitda));
        r.insert("pe".into(), num(m.pe));
        r.insert("revenue".into(), millions(m.revenue));
        r.insert("ebitda".into(), millions(m.ebitda));
        r.insert("net_income".into(), millions(m.net_income));
        r.insert("rev_growth".into(), num(m.revenue_growth));
        r.insert("rev_cagr_3y".into(), num(m.revenue_cagr_3y));
        r.insert("gross_margin".into(), num(m.gross_margin));
        r.insert("ebitda_margin".into(), num(m.ebitda_margin));
        r.insert("net_margin".into(), num(m.net_margin));
        r.insert("fcf_margin".into(), num(m.fcf_margin));
        r.insert("roe".into(), num(m.roe));
        r.insert("roa".into(), num(m.roa));
        r.insert("payout_ratio".into(), num(m.payout_ratio));
        r.insert("total_payout".into(), num(m.total_payout_ratio));
        r.insert("current_ratio".into(), num(m.current_ratio));
        r.insert("net_debt".into(), millions(m.net_debt));
        r.insert("nd_ebitda".into(), num(m.net_debt_to_ebitda));
        r.insert("interest_coverage".into(), num(m.interest_coverage));
        rows.push(r);

        // Provenance: raw filing figures cite EDGAR; derived cells cite the
        // formula. Only attach a note where the value is present.
        let fy = m.fiscal_year.clone().unwrap_or_else(|| "latest FY".into());
        let filing = format!("SEC EDGAR XBRL companyfacts — {} {}", m.ticker, fy);
        // Exact taxonomy-qualified XBRL fact for a raw canonical key.
        let tagged = |key: &str| match m.provenance.get(key) {
            Some(tag) => format!("{filing} ({tag})"),
            None => filing.clone(),
        };
        let mut cite = |key: &str, present: bool, text: String| {
            if present {
                sources.insert((m.ticker.clone(), key.to_string()), text);
            }
        };
        cite("revenue", m.revenue.is_some(), tagged("revenue"));
        cite(
            "ebitda",
            m.ebitda.is_some(),
            format!("Derived: EBIT ({}) + D&A ({}) [{filing}]",
                m.provenance.get("ebit").cloned().unwrap_or_else(|| "EBIT".into()),
                m.provenance.get("da").cloned().unwrap_or_else(|| "D&A".into())),
        );
        cite("net_income", m.net_income.is_some(), tagged("net_income"));
        cite(
            "rev_growth",
            m.revenue_growth.is_some(),
            format!("Derived: FY/FY-1 − 1 ({})", m.ticker),
        );
        cite(
            "gross_margin",
            m.gross_margin.is_some(),
            format!("Derived: gross profit / revenue ({})", m.ticker),
        );
        cite(
            "ebitda_margin",
            m.ebitda_margin.is_some(),
            format!("Derived: EBITDA / revenue ({})", m.ticker),
        );
        cite(
            "net_margin",
            m.net_margin.is_some(),
            format!("Derived: net income / revenue ({})", m.ticker),
        );
        cite(
            "roe",
            m.roe.is_some(),
            format!("Derived: net income / total equity ({})", m.ticker),
        );
        cite(
            "roa",
            m.roa.is_some(),
            format!("Derived: net income / total assets ({})", m.ticker),
        );
        cite(
            "payout_ratio",
            m.payout_ratio.is_some(),
            format!("Derived: |dividends paid| / net income ({filing})"),
        );
        cite(
            "total_payout",
            m.total_payout_ratio.is_some(),
            format!("Derived: (|dividends| + |buybacks|) / net income ({filing})"),
        );
        cite(
            "net_debt",
            m.net_debt.is_some(),
            format!("Derived: total debt − cash ({filing})"),
        );
        cite(
            "nd_ebitda",
            m.net_debt_to_ebitda.is_some(),
            format!("Derived: net debt / EBITDA ({})", m.ticker),
        );
        cite(
            "rev_cagr_3y",
            m.revenue_cagr_3y.is_some(),
            format!("Derived: revenue CAGR over reported window ({})", m.ticker),
        );
        cite(
            "fcf_margin",
            m.fcf_margin.is_some(),
            format!("Derived: (CFO − capex) / revenue ({filing})"),
        );
        cite(
            "current_ratio",
            m.current_ratio.is_some(),
            format!("Derived: current assets / current liabilities ({filing})"),
        );
        cite(
            "interest_coverage",
            m.interest_coverage.is_some(),
            format!("Derived: EBIT / |interest expense| ({filing})"),
        );
        // Trading multiples: mark the market-price input (NOT a filing figure).
        let px = format!(
            "Live market quote (Yahoo Finance){}",
            m.price_currency.as_deref().map(|c| format!(", {c}")).unwrap_or_default()
        );
        cite("market_cap", m.market_cap.is_some(),
            format!("Share price × diluted shares. Price: {px}"));
        cite("ev_revenue", m.ev_revenue.is_some(),
            format!("(mkt cap + net debt) / revenue. Price: {px}"));
        cite("ev_ebitda", m.ev_ebitda.is_some(),
            format!("(mkt cap + net debt) / EBITDA. Price: {px}"));
        cite("pe", m.pe.is_some(), format!("Market cap / net income. Price: {px}"));
    }

    AdHocTable {
        title: title.to_string(),
        units: "(reporting-currency millions; ratios per column; multiples in x)".to_string(),
        columns,
        rows,
        sources,
        grain: Grain::Company,
        is_comparative: true,
        needs_sort_filter: true,
        layout_override: None,
    }
}

/// The footer stamp string (`Generated: … | Source: …`).
pub fn generated_stamp(date: &str) -> String {
    format!("Generated: {date} | Source: SEC EDGAR / Company filings")
}

/// Today's UTC date as `YYYY-MM-DD` (no external date dependency).
pub fn today_iso() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86_400);
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

/// Render a benchmark table to an `.xlsx` at `path`.
pub fn render_benchmark(
    table: &AdHocTable,
    path: &str,
    generated: &str,
) -> Result<(), fm_excel::ExcelError> {
    table
        .validate()
        .map_err(fm_excel::ExcelError::Snapshot)?;
    let mut wb = Workbook::new();
    wb.push(table.build_sheet(generated));
    fm_excel::render::render(&wb, path)
}

/// Errors from the live benchmark pipeline.
#[derive(Debug)]
pub enum BenchmarkError {
    /// No ticker produced any usable data.
    NoData,
    /// Excel render failure.
    Excel(fm_excel::ExcelError),
}

impl std::fmt::Display for BenchmarkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BenchmarkError::NoData => write!(f, "no ticker returned usable filing data"),
            BenchmarkError::Excel(e) => write!(f, "excel error: {e}"),
        }
    }
}
impl std::error::Error for BenchmarkError {}

/// Result of a live benchmark run.
pub struct BenchmarkRun {
    pub metrics: Vec<BenchmarkMetrics>,
    /// Tickers that returned no XBRL data (reported, never fabricated).
    pub failed: Vec<(String, String)>,
    pub table: AdHocTable,
}

/// Fetch each ticker's filing figures from SEC EDGAR, compute metrics, and
/// assemble the benchmark table. Failures are collected, never faked. A ticker
/// with an empty extraction (no fiscal year) is treated as a failure.
pub fn benchmark_tickers(tickers: &[String], title: &str) -> Result<BenchmarkRun, BenchmarkError> {
    benchmark_tickers_opts(tickers, title, BenchmarkOpts::default())
}

/// Benchmark options. `ltm` switches point-in-time metrics to a trailing-twelve-
/// months basis (growth stays annual); `multiples` adds market-price EV/EBITDA,
/// EV/Revenue and P/E (the only non-filing input is the live share price).
#[derive(Clone, Copy, Debug, Default)]
pub struct BenchmarkOpts {
    pub ltm: bool,
    pub multiples: bool,
}

/// Like [`benchmark_tickers`], with [`BenchmarkOpts`]. One companyfacts download
/// per ticker (+ one quote fetch per ticker when `multiples`).
pub fn benchmark_tickers_opts(
    tickers: &[String],
    title: &str,
    opts: BenchmarkOpts,
) -> Result<BenchmarkRun, BenchmarkError> {
    let mut metrics = Vec::new();
    let mut failed = Vec::new();
    for t in tickers {
        match fm_extract::fetch_xbrl_bundle(t) {
            Ok((ex, prov, ltm_data)) => {
                let mut m = metrics_from_extraction(t, &ex);
                if m.fiscal_year.is_some() && m.revenue.is_some() {
                    // Best-effort sector tag from EDGAR SIC (never fails the run).
                    m.sector = fm_fetch::cik_from_ticker(t)
                        .and_then(|cik| fm_fetch::fetch_company_sic(&cik))
                        .ok()
                        .map(|s| s.sic_description)
                        .filter(|s| !s.is_empty());
                    m.provenance = prov; // exact us-gaap tag per canonical key
                    if opts.ltm {
                        if let Some(l) = ltm_data {
                            apply_ltm(&mut m, &l);
                        }
                    }
                    // Trading multiples: filing-derived EV components × live price.
                    if opts.multiples {
                        if let Some(q) = fm_fetch::fetch_quote(t) {
                            apply_multiples(&mut m, &q);
                        }
                    }
                    metrics.push(m);
                } else {
                    failed.push((t.clone(), "no revenue / fiscal year in XBRL".into()));
                }
            }
            Err(e) => failed.push((t.clone(), e.to_string())),
        }
    }
    if metrics.is_empty() {
        return Err(BenchmarkError::NoData);
    }
    let table = build_benchmark_table(&metrics, title);
    Ok(BenchmarkRun { metrics, failed, table })
}

/// Trading multiples from a live quote + filing-derived EV components.
/// Market cap = price × diluted shares; EV = market cap + net debt (both from
/// filings). Blank when a component is missing — never fabricated.
fn apply_multiples(m: &mut BenchmarkMetrics, q: &fm_fetch::Quote) {
    m.share_price = Some(q.price);
    m.price_currency = Some(q.currency.clone());
    let mc = match m.shares_diluted {
        Some(sh) if sh > 0.0 => Some(q.price * sh),
        _ => None,
    };
    m.market_cap = mc;
    m.enterprise_value = match (mc, m.net_debt) {
        (Some(mc), Some(nd)) => Some(mc + nd),
        (Some(mc), None) => Some(mc), // no debt data → EV ≈ market cap
        _ => None,
    };
    m.ev_revenue = ratio(m.enterprise_value, m.revenue);
    m.ev_ebitda = ratio(m.enterprise_value, m.ebitda);
    // P/E on positive earnings only (negative P/E is meaningless in comps).
    m.pe = match (mc, m.net_income) {
        (Some(mc), Some(ni)) if ni > 0.0 => Some(mc / ni),
        _ => None,
    };
}

/// Override a company's point-in-time metrics with a last-twelve-months basis
/// (BS = latest instant). Growth / CAGR stay annual (trend). The period label
/// becomes `LTM <as-of>` so a comps row never mislabels LTM figures as an FY.
fn apply_ltm(m: &mut BenchmarkMetrics, l: &fm_extract::LtmData) {
    // Pure LTM — never blend an annual figure into an LTM-denominated ratio.
    m.revenue = l.revenue;
    m.gross_profit = l.gross_profit;
    m.ebit = l.ebit;
    m.da = l.da;
    m.ebitda = add(l.ebit, l.da);
    m.net_income = l.net_income;
    m.interest_expense = l.interest_expense;
    m.cfo = l.cfo;
    m.capex = l.capex;
    m.dividends_paid = l.dividends_paid;
    m.buybacks = l.buybacks;
    m.total_debt = l.total_debt();
    m.cash = l.cash;
    m.total_equity = l.total_equity;
    m.total_assets = l.total_assets;
    m.total_current_assets = l.total_current_assets;
    m.total_current_liabilities = l.total_current_liabilities;
    // Recompute derived from LTM.
    m.net_debt = match (m.total_debt, m.cash) {
        (Some(d), Some(c)) => Some(d - c),
        _ => None,
    };
    m.fcf = match (m.cfo, m.capex) {
        (Some(c), Some(x)) => Some(c - x.abs()),
        _ => None,
    };
    m.gross_margin = ratio(m.gross_profit, m.revenue);
    m.ebit_margin = ratio(m.ebit, m.revenue);
    m.ebitda_margin = ratio(m.ebitda, m.revenue);
    m.net_margin = ratio(m.net_income, m.revenue);
    m.roe = ratio(m.net_income, m.total_equity);
    m.roa = ratio(m.net_income, m.total_assets);
    m.net_debt_to_ebitda = ratio(m.net_debt, m.ebitda);
    m.fcf_margin = ratio(m.fcf, m.revenue);
    m.interest_coverage = match (m.ebit, m.interest_expense) {
        (Some(e), Some(i)) if i.abs() != 0.0 => Some(e / i.abs()),
        _ => None,
    };
    m.current_ratio = ratio(m.total_current_assets, m.total_current_liabilities);
    m.payout_ratio = match (m.dividends_paid, m.net_income) {
        (Some(d), Some(ni)) if ni > 0.0 => Some(d.abs() / ni),
        _ => None,
    };
    m.total_payout_ratio = match (m.net_income, m.dividends_paid, m.buybacks) {
        (Some(ni), div, bb) if ni > 0.0 && (div.is_some() || bb.is_some()) => {
            Some((div.map(f64::abs).unwrap_or(0.0) + bb.map(f64::abs).unwrap_or(0.0)) / ni)
        }
        _ => None,
    };
    if !l.as_of.is_empty() {
        m.fiscal_year = Some(if l.is_ltm {
            format!("LTM {}", l.as_of)
        } else {
            format!("FY {}", l.as_of)
        });
    }
}

#[cfg(test)]
mod e2e_tests {
    use super::*;
    use fm_excel::model::Value;

    fn stmt(pairs: &[(&str, &[Option<f64>])]) -> StatementData {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_vec())).collect()
    }

    fn big(ticker_rev: f64) -> ExtractionResult {
        ExtractionResult {
            currency: "USD".into(),
            years_found: vec!["2023".into(), "2024".into()],
            income_statement: stmt(&[
                ("revenue", &[Some(ticker_rev * 0.9), Some(ticker_rev)]),
                ("gross_profit", &[Some(ticker_rev * 0.4), Some(ticker_rev * 0.45)]),
                ("ebit", &[Some(ticker_rev * 0.2), Some(ticker_rev * 0.25)]),
                ("da", &[Some(ticker_rev * 0.05), Some(ticker_rev * 0.05)]),
                ("net_income", &[Some(ticker_rev * 0.15), Some(ticker_rev * 0.2)]),
            ]),
            balance_sheet: stmt(&[
                ("long_term_debt", &[Some(ticker_rev * 0.3), Some(ticker_rev * 0.3)]),
                ("cash", &[Some(ticker_rev * 0.1), Some(ticker_rev * 0.1)]),
                ("total_equity", &[Some(ticker_rev), Some(ticker_rev * 1.1)]),
                ("total_assets", &[Some(ticker_rev * 2.0), Some(ticker_rev * 2.2)]),
            ]),
            cash_flow_statement: StatementData::new(),
            notes: HashMap::new(),
            confidence: 1.0,
            discrepancies: vec![],
        }
    }

    /// Full pipeline: two synthetic filings → metrics → table → rendered sheet.
    /// Guards the wiring (headers, group banners, millions scaling, summary
    /// formulas over exactly the entity rows) that unit tests alone don't cover.
    #[test]
    fn benchmark_pipeline_renders_expected_cells() {
        // 400,000,000,000 revenue → 400,000 in millions.
        let a = metrics_from_extraction("AAA", &big(400_000_000_000.0));
        let b = metrics_from_extraction("BBB", &big(200_000_000_000.0));
        let table = build_benchmark_table(&[a, b], "E2E");
        table.validate().unwrap();
        let sheet = table.build_sheet("Generated: X | Source: Y");

        let texts: Vec<String> = sheet
            .cells
            .values()
            .filter_map(|c| match &c.value {
                Some(Value::Text(t)) => Some(t.clone()),
                _ => None,
            })
            .collect();
        // Headers + group banners rendered.
        for expected in ["Revenue", "Net Debt", "Sector", "SCALE", "LEVERAGE", "Median", "Max"] {
            assert!(texts.iter().any(|t| t == expected), "missing cell text {expected:?}");
        }
        // Revenue scaled to millions and rendered as a number.
        let numbers: Vec<f64> = sheet
            .cells
            .values()
            .filter_map(|c| match &c.value {
                Some(Value::Number(n)) => Some(*n),
                _ => None,
            })
            .collect();
        assert!(numbers.iter().any(|n| (*n - 400_000.0).abs() < 1e-6), "AAA revenue (millions) not rendered");
        assert!(numbers.iter().any(|n| (*n - 200_000.0).abs() < 1e-6), "BBB revenue (millions) not rendered");

        // Every summary formula spans exactly the two entity rows (data_start..data_end).
        let medians: Vec<String> = sheet
            .cells
            .values()
            .filter_map(|c| c.formula.clone())
            .filter(|f| f.starts_with("=MEDIAN("))
            .collect();
        assert!(!medians.is_empty(), "no MEDIAN summary formulas rendered");
        for f in &medians {
            // Range like =MEDIAN(E10:E11): the two row numbers must differ by 1.
            let inner = f.trim_start_matches("=MEDIAN(").trim_end_matches(')');
            let (lo, hi) = inner.split_once(':').expect("range");
            let rownum = |cell: &str| cell.trim_start_matches(|c: char| c.is_ascii_alphabetic()).parse::<u32>().unwrap();
            assert_eq!(rownum(hi) - rownum(lo), 1, "summary range {f} must span 2 entity rows");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stmt(pairs: &[(&str, &[Option<f64>])]) -> StatementData {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_vec()))
            .collect()
    }

    fn sample() -> ExtractionResult {
        // Two fiscal years so growth is defined. Latest = index 1.
        ExtractionResult {
            currency: "USD".into(),
            years_found: vec!["2023".into(), "2024".into()],
            income_statement: stmt(&[
                ("revenue", &[Some(1000.0), Some(1200.0)]),
                ("gross_profit", &[Some(400.0), Some(540.0)]),
                ("ebit", &[Some(200.0), Some(300.0)]),
                ("da", &[Some(50.0), Some(60.0)]),
                ("net_income", &[Some(150.0), Some(240.0)]),
                ("eps_diluted", &[Some(1.5), Some(2.4)]),
                ("shares_diluted", &[Some(100.0), Some(100.0)]),
                ("interest_expense", &[Some(20.0), Some(30.0)]),
            ]),
            balance_sheet: stmt(&[
                ("long_term_debt", &[Some(500.0), Some(600.0)]),
                ("cash", &[Some(100.0), Some(200.0)]),
                ("total_equity", &[Some(800.0), Some(1000.0)]),
                ("total_assets", &[Some(2000.0), Some(2400.0)]),
                ("total_current_assets", &[Some(700.0), Some(900.0)]),
                ("total_current_liabilities", &[Some(350.0), Some(450.0)]),
            ]),
            cash_flow_statement: stmt(&[
                ("cfo", &[Some(220.0), Some(320.0)]),
                ("capex", &[Some(-80.0), Some(-100.0)]),
                ("dividends_paid", &[Some(-48.0), Some(-60.0)]),
                ("buybacks", &[Some(-24.0), Some(-60.0)]),
            ]),
            notes: HashMap::new(),
            confidence: 1.0,
            discrepancies: vec![],
        }
    }

    fn approx(a: Option<f64>, b: f64) {
        let a = a.expect("value present");
        assert!((a - b).abs() < 1e-9, "expected {b}, got {a}");
    }

    #[test]
    fn metrics_latest_fy_and_derivations() {
        let m = metrics_from_extraction("TEST", &sample());
        assert_eq!(m.fiscal_year.as_deref(), Some("2024"));
        approx(m.revenue, 1200.0);
        approx(m.ebitda, 360.0); // 300 + 60
        approx(m.net_income, 240.0);
        approx(m.net_debt, 400.0); // 600 - 200
        approx(m.fcf, 220.0); // 320 - |−100|
        approx(m.revenue_growth, 0.2); // 1200/1000 - 1
        approx(m.gross_margin, 0.45); // 540/1200
        approx(m.ebitda_margin, 0.3); // 360/1200
        approx(m.net_margin, 0.2); // 240/1200
        approx(m.roe, 0.24); // 240/1000
        approx(m.roa, 0.1); // 240/2400
        approx(m.net_debt_to_ebitda, 400.0 / 360.0);
        approx(m.fcf_margin, 220.0 / 1200.0); // fcf / revenue
        approx(m.interest_coverage, 10.0); // 300 / 30
        approx(m.current_ratio, 2.0); // 900 / 450
        approx(m.payout_ratio, 0.25); // |−60| / 240
        approx(m.total_payout_ratio, 0.5); // (60 + 60) / 240
        approx(m.revenue_cagr_3y, 0.2); // (1200/1000)^(1/1) - 1
    }

    #[test]
    fn revenue_cagr_uses_full_window_exponent() {
        // 3 periods: 1000 → ? → 1440 over 2 intervals → CAGR = sqrt(1.44)-1 = 0.20.
        let ex = ExtractionResult {
            currency: "USD".into(),
            years_found: vec!["2022".into(), "2023".into(), "2024".into()],
            income_statement: stmt(&[("revenue", &[Some(1000.0), Some(1200.0), Some(1440.0)])]),
            balance_sheet: StatementData::new(),
            cash_flow_statement: StatementData::new(),
            notes: HashMap::new(),
            confidence: 1.0,
            discrepancies: vec![],
        };
        let m = metrics_from_extraction("X", &ex);
        approx(m.revenue_cagr_3y, 0.2); // (1440/1000)^(1/2) - 1
        approx(m.revenue_growth, 0.2); // latest YoY 1440/1200 - 1
    }

    #[test]
    fn apply_ltm_overrides_levels_keeps_growth() {
        let mut m = metrics_from_extraction("X", &sample());
        // Annual latest-FY revenue was 1200; growth 0.2.
        approx(m.revenue, 1200.0);
        let growth_before = m.revenue_growth;
        let cagr_before = m.revenue_cagr_3y;
        let l = fm_extract::LtmData {
            currency: "USD".into(),
            as_of: "2025-09-30".into(),
            is_ltm: true,
            revenue: Some(1300.0),
            ebit: Some(330.0),
            da: Some(70.0),
            net_income: Some(260.0),
            total_equity: Some(1050.0),
            total_assets: Some(2500.0),
            long_term_debt: Some(650.0),
            short_term_debt: Some(150.0),
            cash: Some(250.0),
            ..Default::default()
        };
        apply_ltm(&mut m, &l);
        approx(m.revenue, 1300.0); // LTM level
        approx(m.ebitda, 400.0); // 330 + 70 (pure LTM)
        approx(m.ebitda_margin, 400.0 / 1300.0);
        approx(m.net_margin, 260.0 / 1300.0);
        approx(m.net_debt, 550.0); // (650+150) - 250
        approx(m.roe, 260.0 / 1050.0);
        assert_eq!(m.fiscal_year.as_deref(), Some("LTM 2025-09-30"));
        // Growth / CAGR preserved from the annual series (trend, not LTM).
        assert_eq!(m.revenue_growth, growth_before);
        assert_eq!(m.revenue_cagr_3y, cagr_before);
    }

    #[test]
    fn apply_multiples_computes_ev_and_ratios() {
        let mut m = metrics_from_extraction("X", &sample());
        // sample latest FY: revenue 1200, ebitda 360, net_income 240,
        // shares_diluted 100, net_debt 400 (600 debt − 200 cash).
        let q = fm_fetch::Quote {
            ticker: "X".into(),
            price: 50.0,
            currency: "USD".into(),
            week52_high: None,
            week52_low: None,
            as_of_epoch: None,
        };
        apply_multiples(&mut m, &q);
        approx(m.market_cap, 5000.0); // 50 × 100 shares
        approx(m.enterprise_value, 5400.0); // 5000 + 400 net debt
        approx(m.ev_revenue, 5400.0 / 1200.0);
        approx(m.ev_ebitda, 5400.0 / 360.0);
        approx(m.pe, 5000.0 / 240.0);
        assert_eq!(m.share_price, Some(50.0));
    }

    #[test]
    fn gross_profit_falls_back_to_revenue_minus_cogs() {
        let ex = ExtractionResult {
            currency: "USD".into(),
            years_found: vec!["2024".into()],
            income_statement: stmt(&[
                ("revenue", &[Some(1000.0)]),
                ("cogs", &[Some(600.0)]), // no gross_profit tagged
            ]),
            balance_sheet: StatementData::new(),
            cash_flow_statement: StatementData::new(),
            notes: HashMap::new(),
            confidence: 1.0,
            discrepancies: vec![],
        };
        let m = metrics_from_extraction("X", &ex);
        approx(m.gross_profit, 400.0); // 1000 - 600
        approx(m.gross_margin, 0.4);
    }

    #[test]
    fn total_debt_sums_long_term_and_short_term() {
        let ex = ExtractionResult {
            currency: "USD".into(),
            years_found: vec!["2024".into()],
            income_statement: stmt(&[("revenue", &[Some(100.0)])]),
            balance_sheet: stmt(&[
                ("long_term_debt", &[Some(500.0)]),
                ("short_term_debt", &[Some(120.0)]),
                ("cash", &[Some(70.0)]),
            ]),
            cash_flow_statement: StatementData::new(),
            notes: HashMap::new(),
            confidence: 1.0,
            discrepancies: vec![],
        };
        let m = metrics_from_extraction("X", &ex);
        approx(m.total_debt, 620.0); // 500 + 120
        approx(m.net_debt, 550.0); // 620 - 70
    }

    #[test]
    fn missing_fields_stay_none_never_fabricated() {
        let ex = ExtractionResult {
            currency: "USD".into(),
            years_found: vec!["2024".into()],
            income_statement: stmt(&[("revenue", &[Some(500.0)])]),
            balance_sheet: StatementData::new(),
            cash_flow_statement: StatementData::new(),
            notes: HashMap::new(),
            confidence: 1.0,
            discrepancies: vec![],
        };
        let m = metrics_from_extraction("X", &ex);
        approx(m.revenue, 500.0);
        assert_eq!(m.ebitda, None); // no ebit/da
        assert_eq!(m.net_debt, None); // no debt/cash
        assert_eq!(m.revenue_growth, None); // single year
        assert_eq!(m.roe, None);
    }

    #[test]
    fn sector_flows_into_table_when_present() {
        let mut m = metrics_from_extraction("BANK", &sample());
        m.sector = Some("National Commercial Banks".into());
        let table = build_benchmark_table(&[m], "T");
        assert!(table.columns.iter().any(|c| c.key == "sector"));
        assert_eq!(
            table.rows[0].get("sector"),
            Some(&CellVal::Text("National Commercial Banks".into()))
        );
        // Absent sector → blank cell, never fabricated.
        let m2 = metrics_from_extraction("X", &sample());
        let t2 = build_benchmark_table(&[m2], "T");
        assert_eq!(t2.rows[0].get("sector"), Some(&CellVal::Empty));
    }

    #[test]
    fn empty_extraction_yields_empty_metrics() {
        let ex = ExtractionResult {
            currency: "USD".into(),
            years_found: vec![],
            income_statement: StatementData::new(),
            balance_sheet: StatementData::new(),
            cash_flow_statement: StatementData::new(),
            notes: HashMap::new(),
            confidence: 1.0,
            discrepancies: vec![],
        };
        let m = metrics_from_extraction("X", &ex);
        assert_eq!(m.fiscal_year, None);
        assert_eq!(m.revenue, None);
    }

    #[test]
    fn table_scales_to_millions_and_cites_sources() {
        let big = ExtractionResult {
            currency: "USD".into(),
            years_found: vec!["2023".into(), "2024".into()],
            income_statement: stmt(&[
                ("revenue", &[Some(300_000_000.0), Some(391_035_000_000.0)]),
                ("ebit", &[Some(1.0), Some(120_000_000_000.0)]),
                ("da", &[Some(1.0), Some(14_661_000_000.0)]),
                ("net_income", &[Some(1.0), Some(93_736_000_000.0)]),
            ]),
            balance_sheet: stmt(&[
                ("long_term_debt", &[Some(1.0), Some(100_000_000_000.0)]),
                ("cash", &[Some(1.0), Some(30_000_000_000.0)]),
            ]),
            cash_flow_statement: StatementData::new(),
            notes: HashMap::new(),
            confidence: 1.0,
            discrepancies: vec![],
        };
        let m = metrics_from_extraction("AAPL", &big);
        let table = build_benchmark_table(&[m], "Test Benchmark");
        table.validate().unwrap();
        let r0 = &table.rows[0];
        assert_eq!(r0.get("revenue"), Some(&CellVal::Number(391_035.0)));
        assert_eq!(r0.get("net_debt"), Some(&CellVal::Number(70_000.0)));
        assert_eq!(r0.get("ticker"), Some(&CellVal::Text("AAPL".into())));
        // Provenance attached for present cells.
        assert!(table
            .sources
            .contains_key(&("AAPL".to_string(), "revenue".to_string())));
        // Comparative wide table → summary stats in the decision.
        let d = table.decision();
        assert!(d.summary_stats);
        assert!(d.freeze_first_col); // 11 metrics ∈ [9,20]
    }
}
