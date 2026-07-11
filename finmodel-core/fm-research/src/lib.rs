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

    m
}

/// Column set for the peer benchmark (order chosen so groups stay contiguous).
fn benchmark_columns() -> Vec<ColumnSpec> {
    vec![
        ColumnSpec::label("ticker", "Ticker"),
        ColumnSpec::metric("currency", "Ccy", ColKind::Text)
            .with_definition("Reporting currency (metrics NOT FX-normalized across peers)"),
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
        ColumnSpec::metric("gross_margin", "Gross Margin", ColKind::Percent)
            .with_group("Profitability"),
        ColumnSpec::metric("ebitda_margin", "EBITDA Margin", ColKind::Percent)
            .with_group("Profitability"),
        ColumnSpec::metric("net_margin", "Net Margin", ColKind::Percent)
            .with_group("Profitability"),
        ColumnSpec::metric("roe", "ROE", ColKind::Percent)
            .with_group("Returns")
            .with_definition("Net income / total equity"),
        ColumnSpec::metric("roa", "ROA", ColKind::Percent)
            .with_group("Returns")
            .with_definition("Net income / total assets"),
        ColumnSpec::metric("net_debt", "Net Debt", ColKind::Dollar)
            .with_group("Leverage")
            .with_units("reporting-ccy millions")
            .with_definition("Total debt (long-term + current portion) − cash & equivalents"),
        ColumnSpec::metric("nd_ebitda", "Net Debt / EBITDA", ColKind::Multiple)
            .with_group("Leverage"),
    ]
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
    let columns = benchmark_columns();
    let mut rows: Vec<HashMap<String, CellVal>> = Vec::with_capacity(metrics.len());
    let mut sources: HashMap<(String, String), String> = HashMap::new();

    for m in metrics {
        let mut r: HashMap<String, CellVal> = HashMap::new();
        r.insert("ticker".into(), CellVal::Text(m.ticker.clone()));
        r.insert("currency".into(), CellVal::Text(m.currency.clone()));
        r.insert("revenue".into(), millions(m.revenue));
        r.insert("ebitda".into(), millions(m.ebitda));
        r.insert("net_income".into(), millions(m.net_income));
        r.insert("rev_growth".into(), num(m.revenue_growth));
        r.insert("gross_margin".into(), num(m.gross_margin));
        r.insert("ebitda_margin".into(), num(m.ebitda_margin));
        r.insert("net_margin".into(), num(m.net_margin));
        r.insert("roe".into(), num(m.roe));
        r.insert("roa".into(), num(m.roa));
        r.insert("net_debt".into(), millions(m.net_debt));
        r.insert("nd_ebitda".into(), num(m.net_debt_to_ebitda));
        rows.push(r);

        // Provenance: raw filing figures cite EDGAR; derived cells cite the
        // formula. Only attach a note where the value is present.
        let fy = m.fiscal_year.clone().unwrap_or_else(|| "latest FY".into());
        let filing = format!("SEC EDGAR XBRL companyfacts — {} {}", m.ticker, fy);
        let mut cite = |key: &str, present: bool, text: String| {
            if present {
                sources.insert((m.ticker.clone(), key.to_string()), text);
            }
        };
        cite("revenue", m.revenue.is_some(), filing.clone());
        cite(
            "ebitda",
            m.ebitda.is_some(),
            format!("Derived: EBIT + D&A ({filing})"),
        );
        cite("net_income", m.net_income.is_some(), filing.clone());
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
            "net_debt",
            m.net_debt.is_some(),
            format!("Derived: total debt − cash ({filing})"),
        );
        cite(
            "nd_ebitda",
            m.net_debt_to_ebitda.is_some(),
            format!("Derived: net debt / EBITDA ({})", m.ticker),
        );
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
    let mut metrics = Vec::new();
    let mut failed = Vec::new();
    for t in tickers {
        match fm_extract::fetch_xbrl(t) {
            Ok(ex) => {
                let m = metrics_from_extraction(t, &ex);
                if m.fiscal_year.is_some() && m.revenue.is_some() {
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
            ]),
            balance_sheet: stmt(&[
                ("long_term_debt", &[Some(500.0), Some(600.0)]),
                ("cash", &[Some(100.0), Some(200.0)]),
                ("total_equity", &[Some(800.0), Some(1000.0)]),
                ("total_assets", &[Some(2000.0), Some(2400.0)]),
            ]),
            cash_flow_statement: stmt(&[
                ("cfo", &[Some(220.0), Some(320.0)]),
                ("capex", &[Some(-80.0), Some(-100.0)]),
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
