//! Shared model-build orchestration used by BOTH the CLI and the desktop app.
//!
//! Keeps the demo-critical logic (currency mapping, projection wiring, Excel
//! sheet assembly) in ONE place so the two front-ends can't drift.

use std::collections::HashMap;

use fm_engine::ModelEngine;
use fm_excel::input::{AssumptionsBlock, Meta, ModelOutput, Verification, WorkbookInput};
use fm_excel::model::Workbook;
use fm_extract::ExtractionResult;
use fm_types::{CompanyConfig, ProjectedStatements, ReconciledData};

/// Map a ticker's exchange suffix to its reporting currency.
pub fn currency_for_ticker(ticker: &str) -> &'static str {
    let up = ticker.to_uppercase();
    if up.ends_with(".ST") {
        "SEK"
    } else if up.ends_with(".CO") {
        "DKK"
    } else if up.ends_with(".SW") {
        "CHF"
    } else if up.ends_with(".AS") || up.ends_with(".PA") || up.ends_with(".DE") {
        "EUR"
    } else if up.ends_with(".L") {
        "GBP"
    } else if up.ends_with(".TO") {
        "CAD"
    } else if up.ends_with(".T") {
        "JPY"
    } else {
        "USD"
    }
}

/// Sanitize a ticker to a filename stem (e.g. "SAND.ST" -> "SAND_ST").
pub fn ticker_to_stem(ticker: &str) -> String {
    ticker.replace(['.', '/'], "_")
}

/// The result of building a model: the forward projection plus the rich,
/// formula-driven Excel workbook (cell-model).
pub struct BuildOutput {
    pub projected: ProjectedStatements,
    pub workbook: Workbook,
    /// Non-fatal valuation diagnostics: WACC clamp (1.12), Gordon TV undefined
    /// (1.11), and DCF structural invariant violations (2.3). Empty on a clean build.
    pub warnings: Vec<String>,
    /// Computed DCF (for the app's valuation preview / agent). `None` if absent.
    pub dcf: Option<fm_value::DCFOutput>,
    /// Computed WACC (for the valuation preview / agent). `None` if absent.
    pub wacc_out: Option<fm_value::WACCOutput>,
}

/// Per-driver, per-year assumption override from the analyst grid (Phase 3.3).
/// `key` is a `ScenarioInputs` field name (e.g. `revenue_growth_pct`); `values`
/// has one entry per projection year (`None` = keep the engine-derived value).
#[derive(Clone, Debug, Default, serde::Deserialize)]
pub struct AssumptionOverride {
    pub key: String,
    pub values: Vec<Option<f64>>,
}

/// Analyst-tunable build options. `Default` reproduces the engine's historical
/// hardcoded values, so `build(extraction, ticker, n)` is exactly
/// `build_with(.., &BuildOptions { proj_years: n, ..Default::default() })` and
/// every parity gate stays byte-identical.
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(default)]
pub struct BuildOptions {
    pub proj_years: usize,
    pub sector: String,
    pub risk_free_rate: f64,
    pub equity_risk_premium: f64,
    pub target_de_ratio: f64,
    /// `None` → engine-derived interest rate.
    pub cost_of_debt_pretax: Option<f64>,
    pub beta: f64,
    /// `None` → engine-derived tax rate.
    pub tax_rate_override: Option<f64>,
    pub terminal_growth: f64,
    /// `None` → sector default exit-multiple table.
    pub exit_ebitda_multiple: Option<f64>,
    /// 1 = EBITDA exit multiple (default), 2 = Gordon growth.
    pub tv_method: u8,
    /// `None` → live quote / 0.0.
    pub share_price: Option<f64>,
    pub fiscal_year_end: String,
    pub assumption_overrides: Vec<AssumptionOverride>,
    /// Caller metadata (app Save-As / CLI `--out`); ignored by the engine.
    pub out_path: Option<String>,
    /// Optional peer tickers for a trading-comps tab. Network assembly happens
    /// in the caller (app / CLI); the engine only consumes `public_comps`.
    #[serde(default)]
    pub peers: Vec<String>,
    /// Pre-assembled public comps (caller fills it; `None` ⇒ no comps tabs).
    #[serde(skip)]
    pub public_comps: Option<fm_value::PublicCompsOutput>,
    /// Active scenario case: 1=Base (default), 2=Upside, 3=Downside.
    #[serde(default = "default_case")]
    pub active_case: u8,
    /// When true, callers also emit a `<stem>_deck.pptx` summary (app/CLI only;
    /// the engine ignores it).
    #[serde(default)]
    pub deck: bool,
}

impl Default for BuildOptions {
    fn default() -> Self {
        Self {
            proj_years: 5,
            sector: "standard".to_string(),
            risk_free_rate: 0.045,
            equity_risk_premium: 0.055,
            target_de_ratio: 0.30,
            cost_of_debt_pretax: None,
            beta: 1.0,
            tax_rate_override: None,
            terminal_growth: 0.025,
            exit_ebitda_multiple: None,
            tv_method: 1,
            share_price: None,
            fiscal_year_end: "Dec".to_string(),
            assumption_overrides: Vec::new(),
            out_path: None,
            peers: Vec::new(),
            public_comps: None,
            active_case: default_case(),
            deck: false,
        }
    }
}

/// Default active scenario case (1 = Base).
fn default_case() -> u8 {
    1
}

/// Reconcile an extraction, project it forward, and build the rich workbook with
/// default options. Thin wrapper over [`build_with`] (kept for existing callers).
pub fn build(extraction: &ExtractionResult, ticker: &str, proj_periods: usize) -> BuildOutput {
    build_with(
        extraction,
        ticker,
        &BuildOptions { proj_years: proj_periods, ..Default::default() },
    )
}

/// Reconcile → project → build the rich workbook, honoring analyst [`BuildOptions`].
/// The single shared core both front-ends call — never duplicated.
pub fn build_with(extraction: &ExtractionResult, ticker: &str, opts: &BuildOptions) -> BuildOutput {
    let data = ReconciledData {
        income_statement: extraction.income_statement.clone(),
        balance_sheet: extraction.balance_sheet.clone(),
        cash_flow_statement: extraction.cash_flow_statement.clone(),
        periods: extraction.years_found.clone(),
        currency: extraction.currency.clone(),
    };
    let config = CompanyConfig {
        name: ticker.to_string(),
        currency: extraction.currency.clone(),
        hist_periods: extraction.years_found.len(),
        proj_periods: opts.proj_years,
        ..Default::default()
    };
    let engine = ModelEngine::new(data, config);
    let projected = engine.project(&HashMap::new());
    let (input, mut warnings) = build_workbook_input_with(extraction, &projected, ticker, opts);
    let workbook = fm_excel::sheets::build_workbook(&input);
    // Collect non-fatal valuation warnings (surfaced by the app/CLI).
    if let Some(w) = &input.wacc {
        warnings.extend(w.warnings.iter().cloned());
    }
    if let Some(d) = &input.dcf {
        warnings.extend(d.warnings.iter().cloned());
        let dcf_input = fm_value::DCFInput {
            fcf: d.fcff_proj.clone(),
            terminal_growth: d.tv_growth_rate,
            wacc: d.wacc,
            projected_periods: d.proj_periods.len(),
        };
        warnings.extend(fm_value::invariants::check_dcf_invariants(&dcf_input, d.wacc));
    }
    let dcf = input.dcf.clone();
    let wacc_out = input.wacc.clone();
    BuildOutput { projected, workbook, warnings, dcf, wacc_out }
}

/// Light path for the assumptions grid (Phase 3.3): reconcile + project +
/// derive the assumptions block, WITHOUT assembling the Excel workbook. Returns
/// the (Base/Upside/Downside) assumptions plus the historical + projection
/// period labels — everything the driver grid needs, at ~half the cost of a
/// full [`build_with`].
pub fn prepare_assumptions(
    extraction: &ExtractionResult,
    ticker: &str,
    opts: &BuildOptions,
) -> (fm_excel::input::AssumptionsBlock, Vec<String>, Vec<String>) {
    let data = ReconciledData {
        income_statement: extraction.income_statement.clone(),
        balance_sheet: extraction.balance_sheet.clone(),
        cash_flow_statement: extraction.cash_flow_statement.clone(),
        periods: extraction.years_found.clone(),
        currency: extraction.currency.clone(),
    };
    let config = CompanyConfig {
        name: ticker.to_string(),
        currency: extraction.currency.clone(),
        hist_periods: extraction.years_found.len(),
        proj_periods: opts.proj_years,
        ..Default::default()
    };
    let engine = ModelEngine::new(data, config);
    let projected = engine.project(&HashMap::new());
    let (input, _warnings) = build_workbook_input_with(extraction, &projected, ticker, opts);
    let hist = extraction.years_found.clone();
    let proj = projected.periods.clone();
    (input.assumptions, hist, proj)
}

/// Assemble the rich-writer input (model output + derived assumptions + meta)
/// from an extraction and its forward projection.
///
/// Historical + engine-projected statement columns are merged into `model` so
/// formula cells can cache projected values for offline LibreOffice opens.
/// Workbook still emits Excel formulas for projected periods. The two external
/// market inputs (risk-free rate, share price) default until a live feed is wired.
pub fn build_workbook_input_with(
    extraction: &ExtractionResult,
    projected: &ProjectedStatements,
    ticker: &str,
    opts: &BuildOptions,
) -> (WorkbookInput, Vec<String>) {
    use fm_excel::is_structure::{apply_filing_labels, build_is_structure, build_standard_is_detailed, CogsDetail, OpexItem, Segment};

    let hist: Vec<String> = extraction.years_found.iter().map(|y| format!("{y}A")).collect();
    let proj: Vec<String> = projected.periods.iter().map(|y| format!("{y}E")).collect();
    let mut periods = hist;
    periods.extend(proj);

    let sector = opts.sector.clone();

    // XBRL detail from footnotes (US filings): revenue segments, opex line items,
    // detailed COGS. Parse + (for standard sector) remap cogs/rd/sga into their
    // canonical slots, mirroring cli.py.
    let items = |k: &str| -> Vec<(String, String, String)> {
        extraction.notes.get(k).and_then(|v| v.as_array()).map(|a| {
            a.iter().filter_map(|o| {
                let label = o.get("label")?.as_str()?.to_string();
                let key = o.get("key")?.as_str()?.to_string();
                let cat = o.get("category").and_then(|c| c.as_str()).unwrap_or("").to_string();
                Some((label, key, cat))
            }).collect()
        }).unwrap_or_default()
    };
    let seg_raw = items("revenue_segments");
    let opex_raw = items("opex_items");
    let cogs_raw = items("cogs_detail");

    let mut is = extraction.income_statement.clone();
    let detailed = sector == "standard" && (!seg_raw.is_empty() || !opex_raw.is_empty());
    if sector == "standard" && !opex_raw.is_empty() {
        let first = |cat: &str| opex_raw.iter().find(|(_, _, c)| c == cat).map(|(_, k, _)| k.clone());
        for (slot, cat) in [("cogs", "cogs"), ("rd", "opex_rd"), ("sga", "opex")] {
            if let Some(src) = first(cat) {
                if let Some(vals) = is.get(&src).cloned() {
                    is.insert(slot.to_string(), vals);
                }
            }
        }
    }

    // Materialize hist+proj arrays so formula cells can carry cached engine
    // results (LibreOffice shows numbers offline without recalculation).
    let n_h = extraction.years_found.len();
    let n_p = projected.periods.len();
    let merge = |hist: &fm_types::StatementData, proj: &fm_types::StatementData| -> fm_excel::input::Statement {
        let mut out: fm_excel::input::Statement = hist
            .iter()
            .map(|(k, v)| {
                let mut row = v.clone();
                row.resize(n_h, None);
                // append projected years when present
                if let Some(pv) = proj.get(k) {
                    for x in pv.iter().take(n_p) {
                        row.push(*x);
                    }
                }
                while row.len() < n_h + n_p {
                    row.push(None);
                }
                (k.clone(), row)
            })
            .collect();
        // keys only in proj
        for (k, pv) in proj {
            if out.contains_key(k) { continue; }
            let mut row = vec![None; n_h];
            for x in pv.iter().take(n_p) { row.push(*x); }
            while row.len() < n_h + n_p { row.push(None); }
            out.insert(k.clone(), row);
        }
        out
    };
    let model = ModelOutput {
        periods,
        income_statement: merge(&is, &projected.income_statement),
        balance_sheet: merge(&extraction.balance_sheet, &projected.balance_sheet),
        cash_flow_statement: merge(&extraction.cash_flow_statement, &projected.cash_flow),
        plug_used: false,
    };

    let meta = Meta {
        company: ticker.to_string(),
        ticker: ticker.to_string(),
        currency: extraction.currency.clone(),
        fiscal_year_end: opts.fiscal_year_end.clone(),
        sector: sector.clone(),
        as_of: today_iso(),
    };

    // Valuation params from options (Default reproduces the legacy hardcoded set,
    // so a default BuildOptions keeps every parity gate byte-identical).
    let val_params = fm_excel::derive::ValuationParams {
        risk_free_rate: opts.risk_free_rate,
        equity_risk_premium: opts.equity_risk_premium,
        target_de_ratio: opts.target_de_ratio,
        cost_of_debt_pretax: opts.cost_of_debt_pretax,
        share_price: opts.share_price.unwrap_or(0.0),
        terminal_growth: opts.terminal_growth,
        exit_multiple: opts.exit_ebitda_multiple,
    };
    let mut assumptions: AssumptionsBlock =
        fm_excel::derive::build_assumptions_block(&model, &meta.sector, &val_params);
    // Overlay analyst grid overrides onto Base (Upside/Downside inherit deltas).
    let warnings = apply_assumption_overrides(&mut assumptions, &opts.assumption_overrides);
    // An explicit tax override flows into every scenario (and thus the WACC
    // unlever tax, which reads base.tax_rate_pct[0]).
    if let Some(t) = opts.tax_rate_override {
        for sc in [&mut assumptions.base, &mut assumptions.upside, &mut assumptions.downside] {
            for x in sc.tax_rate_pct.iter_mut() {
                *x = t;
            }
        }
    }
    // Analyst-selected scenario case (1=Base/2=Upside/3=Downside) drives the
    // AssumptionsBlock CHOOSE toggle and the DCF `active` pick below.
    assumptions.active_case = opts.active_case.clamp(1, 3) as i64;
    let verification = Verification { passed: true, ..Default::default() };

    let nonzero = |k: &str| {
        is.get(k).map(|v| v.iter().any(|x| x.map(|n| n != 0.0).unwrap_or(false))).unwrap_or(false)
    };
    let mut is_structure = if detailed {
        let segments: Vec<Segment> = seg_raw.iter().map(|(l, k, _)| Segment { label: l.clone(), key: k.clone() }).collect();
        let opex_items: Vec<OpexItem> = opex_raw.iter().map(|(l, k, c)| OpexItem { label: l.clone(), key: k.clone(), category: c.clone() }).collect();
        let cogs_detail: Vec<CogsDetail> = cogs_raw.iter().map(|(l, k, _)| CogsDetail { label: l.clone(), key: k.clone() }).collect();
        build_standard_is_detailed(nonzero("cogs"), nonzero("rd"), nonzero("sga"), &segments, &opex_items, &cogs_detail)
    } else {
        build_is_structure(&meta.sector, nonzero("cogs"), nonzero("rd"), nonzero("sga"))
    };

    // Override IS labels with actual XBRL concept labels when the filing provides them.
    if let Some(fl) = extraction.notes.get("filing_labels").and_then(|v| v.as_object()) {
        let labels: std::collections::HashMap<String, String> = fl
            .iter()
            .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
            .collect();
        apply_filing_labels(&mut is_structure, &labels);
    }

    // Valuation: fallback peer-set WACC + DCF so Cover/DCF/WACC/Sens tabs ship
    // with every workbook. Market inputs still default until a live feed lands.
    // Offline fallback beta = 1.0 (cli.py unlevers a fetched own-beta when peers
    // are empty; we have no market beta feed yet).
    let last = |stmt: &fm_excel::input::Statement, key: &str| -> f64 {
        stmt.get(key)
            .and_then(|v| v.iter().rev().find_map(|x| *x))
            .unwrap_or(0.0)
    };
    let shares = if assumptions.shares_diluted != 0.0 {
        assumptions.shares_diluted
    } else {
        last(&model.income_statement, "shares_diluted")
    };
    let mkt_cap = assumptions.current_share_price * shares;
    let debt = last(&model.balance_sheet, "long_term_debt");
    // cli.py uses base.tax_rate_pct[0] (first projected year) for beta unlever tax.
    let tax = assumptions
        .base
        .tax_rate_pct
        .first()
        .copied()
        .unwrap_or(0.21);
    let peer_set = fm_value::fallback_peer_set(&meta.ticker, mkt_cap, assumptions.target_de_ratio);
    let wacc = fm_value::compute_wacc(
        &peer_set,
        mkt_cap,
        debt,
        assumptions.risk_free_rate,
        assumptions.equity_risk_premium,
        assumptions.cost_of_debt_pretax,
        tax,
        Some(assumptions.target_de_ratio),
        opts.beta,
    );
    let active = match assumptions.active_case {
        2 => &assumptions.upside,
        3 => &assumptions.downside,
        _ => &assumptions.base,
    };
    let dcf_asmp = fm_value::DCFAssumptions {
        mid_year_convention: assumptions.mid_year_convention,
        current_share_price: assumptions.current_share_price,
        shares_diluted: shares,
        active: fm_value::DCFScenario {
            terminal_growth_rate: active.terminal_growth_rate,
            exit_ebitda_multiple: active.exit_ebitda_multiple,
        },
    };
    let dcf = fm_value::compute_dcf(
        &model.periods,
        &model.income_statement,
        &model.balance_sheet,
        &model.cash_flow_statement,
        &meta.ticker,
        &wacc,
        &dcf_asmp,
        i32::from(opts.tv_method),
    );

    let input = WorkbookInput {
        meta,
        model,
        assumptions,
        verification,
        is_structure,
        wacc: Some(wacc),
        peer_source: peer_set.source,
        dcf: Some(dcf),
        public_comps: opts.public_comps.clone(),
    };
    (input, warnings)
}

/// Mutable access to a `ScenarioInputs` driver vector by its field name.
fn scenario_field_mut<'a>(
    s: &'a mut fm_excel::input::ScenarioInputs,
    key: &str,
) -> Option<&'a mut Vec<f64>> {
    Some(match key {
        "revenue_growth_pct" => &mut s.revenue_growth_pct,
        "gross_margin_pct" => &mut s.gross_margin_pct,
        "sga_pct_rev" => &mut s.sga_pct_rev,
        "rd_pct_rev" => &mut s.rd_pct_rev,
        "da_pct_rev" => &mut s.da_pct_rev,
        "capex_pct_rev" => &mut s.capex_pct_rev,
        "tax_rate_pct" => &mut s.tax_rate_pct,
        "interest_rate_pct" => &mut s.interest_rate_pct,
        "dso_days" => &mut s.dso_days,
        "dio_days" => &mut s.dio_days,
        "dpo_days" => &mut s.dpo_days,
        "dividend_per_share" => &mut s.dividend_per_share,
        _ => return None,
    })
}

/// Overlay analyst per-year overrides onto the Base scenario, mirroring them onto
/// Upside/Downside with those scenarios' fixed deltas. A `None` cell keeps the
/// engine-derived value for that year. Unknown keys produce a warning (not an
/// error). Returns the collected warnings.
fn apply_assumption_overrides(
    block: &mut fm_excel::input::AssumptionsBlock,
    overrides: &[AssumptionOverride],
) -> Vec<String> {
    let mut warnings = Vec::new();
    for ov in overrides {
        if scenario_field_mut(&mut block.base, &ov.key).is_none() {
            warnings.push(format!("unknown assumption key '{}' — ignored", ov.key));
            continue;
        }
        // Deltas the Upside/Downside scenarios apply to this driver (0 for keys
        // flat across scenarios) — shared with fm-excel so they never desync.
        use fm_excel::derive::{UPSIDE_CAPEX_DELTA, UPSIDE_GROSS_MARGIN_DELTA, UPSIDE_REVENUE_GROWTH_DELTA};
        let (up_d, down_d) = match ov.key.as_str() {
            "revenue_growth_pct" => (UPSIDE_REVENUE_GROWTH_DELTA, -UPSIDE_REVENUE_GROWTH_DELTA),
            "gross_margin_pct" => (UPSIDE_GROSS_MARGIN_DELTA, -UPSIDE_GROSS_MARGIN_DELTA),
            "capex_pct_rev" => (UPSIDE_CAPEX_DELTA, -UPSIDE_CAPEX_DELTA),
            _ => (0.0, 0.0),
        };
        for (y, cell) in ov.values.iter().enumerate() {
            let Some(v) = cell else { continue };
            if let Some(f) = scenario_field_mut(&mut block.base, &ov.key) {
                if y < f.len() {
                    f[y] = *v;
                }
            }
            if let Some(f) = scenario_field_mut(&mut block.upside, &ov.key) {
                if y < f.len() {
                    f[y] = *v + up_d;
                }
            }
            if let Some(f) = scenario_field_mut(&mut block.downside, &ov.key) {
                if y < f.len() {
                    f[y] = *v + down_d;
                }
            }
        }
    }
    warnings
}

/// Today's date as `YYYY-MM-DD` (UTC) for the Cover "As of …" line.
fn today_iso() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
    format!("{y:04}-{m:02}-{d:02}")
}

/// Inverse of Howard Hinnant's `days_from_civil`: days-since-epoch → (y, m, d).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (y + if m <= 2 { 1 } else { 0 }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_currency_for_ticker() {
        assert_eq!(currency_for_ticker("SAND.ST"), "SEK");
        assert_eq!(currency_for_ticker("NOVO-B.CO"), "DKK");
        assert_eq!(currency_for_ticker("NESN.SW"), "CHF");
        assert_eq!(currency_for_ticker("ASML.AS"), "EUR");
        assert_eq!(currency_for_ticker("MC.PA"), "EUR");
        assert_eq!(currency_for_ticker("AAPL"), "USD");
    }

    #[test]
    fn test_ticker_to_stem() {
        assert_eq!(ticker_to_stem("SAND.ST"), "SAND_ST");
        assert_eq!(ticker_to_stem("NOVO-B.CO"), "NOVO-B_CO");
    }

    #[test]
    fn test_build_produces_workbook_and_projection() {
        use fm_types::StatementData;
        // Minimal 2-year extraction
        let mut is = StatementData::new();
        is.insert("revenue".into(), vec![Some(100.0), Some(110.0)]);
        is.insert("net_income".into(), vec![Some(10.0), Some(12.0)]);
        is.insert("income_tax".into(), vec![Some(3.0), Some(4.0)]);
        let extraction = ExtractionResult {
            currency: "USD".into(),
            years_found: vec!["2023".into(), "2024".into()],
            income_statement: is,
            balance_sheet: StatementData::new(),
            cash_flow_statement: StatementData::new(),
            notes: HashMap::new(),
            confidence: 1.0,
            discrepancies: vec![],
        };
        let out = build(&extraction, "TEST", 5);
        assert_eq!(out.projected.periods.len(), 5);
        // Rich workbook: 3-statement + valuation tabs + Sources.
        let names: Vec<&str> = out.workbook.sheets.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(
            names,
            ["Cover", "Assumptions", "IS", "BS", "CF", "DCF", "WACC", "Sensitivities", "Sources"]
        );
        // Cover carries the ticker and periods (Hist: 2023A–2024A | Proj: 2025E–2029E).
        let cover = out.workbook.sheet("Cover").expect("cover");
        assert!(cover.cells.values().any(|c| matches!(&c.value,
            Some(fm_excel::model::Value::Text(t)) if t.contains("2025E"))));
    }

    #[test]
    fn test_build_detailed_is_from_notes() {
        use fm_types::StatementData;
        let mut is = StatementData::new();
        for (k, v) in [
            ("revenue", vec![Some(100.0), Some(110.0)]),
            ("rev_seg_a", vec![Some(60.0), Some(66.0)]),
            ("rev_seg_b", vec![Some(40.0), Some(44.0)]),
            ("cogs_seg_a", vec![Some(30.0), Some(33.0)]),
            ("net_income", vec![Some(10.0), Some(12.0)]),
            ("income_tax", vec![Some(3.0), Some(4.0)]),
            ("rd", vec![Some(5.0), Some(6.0)]),
            ("sga", vec![Some(8.0), Some(9.0)]),
        ] {
            is.insert(k.into(), v);
        }
        let mut notes = HashMap::new();
        notes.insert("revenue_segments".into(), serde_json::json!([
            {"label": "Products", "key": "rev_seg_a"}, {"label": "Services", "key": "rev_seg_b"}]));
        notes.insert("opex_items".into(), serde_json::json!([
            {"label": "Cost of products", "key": "cogs_seg_a", "category": "cogs"},
            {"label": "R&D", "key": "rd", "category": "opex_rd"},
            {"label": "SG&A", "key": "sga", "category": "opex"}]));
        let extraction = ExtractionResult {
            currency: "USD".into(),
            years_found: vec!["2023".into(), "2024".into()],
            income_statement: is,
            balance_sheet: StatementData::new(),
            cash_flow_statement: StatementData::new(),
            notes,
            confidence: 1.0,
            discrepancies: vec![],
        };
        let out = build(&extraction, "TEST", 5);
        // Detailed IS: segment rows render (label "  Products" / "  Services").
        let is_sheet = out.workbook.sheet("IS").expect("IS");
        let has = |t: &str| is_sheet.cells.values().any(|c| matches!(&c.value,
            Some(fm_excel::model::Value::Text(s)) if s == t));
        assert!(has("  Products"), "segment row missing");
        assert!(has("Total Revenue"), "Total Revenue subtotal missing");
    }
    #[test]
    fn overrides_overlay_base_and_mirror_deltas() {
        use fm_excel::input::{AssumptionsBlock, ScenarioInputs};
        let sc = |name: &str, rg: f64| ScenarioInputs {
            name: name.into(),
            revenue_growth_pct: vec![rg; 3],
            gross_margin_pct: vec![0.30; 3],
            sga_pct_rev: vec![0.10; 3],
            rd_pct_rev: vec![0.05; 3],
            da_pct_rev: vec![0.04; 3],
            capex_pct_rev: vec![0.05; 3],
            tax_rate_pct: vec![0.21; 3],
            interest_rate_pct: vec![0.035; 3],
            dso_days: vec![45.0; 3],
            dio_days: vec![60.0; 3],
            dpo_days: vec![50.0; 3],
            dividend_per_share: vec![0.0; 3],
            terminal_growth_rate: 0.025,
            exit_ebitda_multiple: 16.0,
        };
        let mut block = AssumptionsBlock {
            proj_periods: vec!["2025E".into(), "2026E".into(), "2027E".into()],
            active_case: 1,
            base: sc("Base", 0.05),
            upside: sc("Upside", 0.07),
            downside: sc("Downside", 0.03),
            risk_free_rate: 0.045,
            equity_risk_premium: 0.055,
            target_de_ratio: 0.30,
            cost_of_debt_pretax: 0.035,
            current_share_price: 0.0,
            shares_diluted: 100.0,
            mid_year_convention: true,
        };
        let overrides = vec![
            AssumptionOverride {
                key: "revenue_growth_pct".into(),
                values: vec![Some(0.12), None, Some(0.08)],
            },
            AssumptionOverride { key: "bogus_key".into(), values: vec![Some(1.0)] },
        ];
        let warnings = apply_assumption_overrides(&mut block, &overrides);
        let approx = |a: &[f64], b: &[f64]| {
            assert_eq!(a.len(), b.len());
            for (x, y) in a.iter().zip(b) {
                assert!((x - y).abs() < 1e-9, "expected {b:?}, got {a:?}");
            }
        };
        // Base: year0/year2 overridden, year1 keeps the derived 0.05.
        approx(&block.base.revenue_growth_pct, &[0.12, 0.05, 0.08]);
        // Upside/Downside mirror overridden cells with their ±0.02 delta;
        // non-overridden cells keep their own derived value.
        approx(&block.upside.revenue_growth_pct, &[0.14, 0.07, 0.10]);
        approx(&block.downside.revenue_growth_pct, &[0.10, 0.03, 0.06]);
        // Unknown key warned, never applied.
        assert!(warnings.iter().any(|w| w.contains("bogus_key")));
    }

}
