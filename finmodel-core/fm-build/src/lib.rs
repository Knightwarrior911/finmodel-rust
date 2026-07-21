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
    /// Engine-keyed per-year provenance for any analyst-overridden projection
    /// driver (`None` cell = engine-derived). Feeds the workbook Sources tab and
    /// keeps every consumer of the projection agreeing on where a value came from.
    pub provenance: HashMap<String, DriverProvenance>,
    /// Real verification (Phase 6.2): balance-sheet identity + checks. `passed`
    /// is false on a finite imbalance (workbook still built for diagnosis).
    pub verification: fm_excel::input::Verification,
}

/// Where an assumption value originated. `Manual` = analyst-typed; `Research` =
/// derived from a research answer (Phase 5 suggested-assumption bridge).
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AssumptionOrigin {
    Manual,
    Research,
}

/// Provenance for an analyst-supplied assumption override: where it came from
/// and, for research-sourced values, the `S#` source IDs backing it.
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct AssumptionProvenance {
    pub origin: AssumptionOrigin,
    #[serde(default)]
    pub source_ids: Vec<String>,
}

/// Per-driver, per-year assumption override from the analyst grid (Phase 3.3).
/// `key` is a `ScenarioInputs` field name (e.g. `revenue_growth_pct`); `values`
/// has one entry per projection year (`None` = keep the engine-derived value).
#[derive(Clone, Debug, Default, serde::Deserialize, serde::Serialize)]
pub struct AssumptionOverride {
    pub key: String,
    pub values: Vec<Option<f64>>,
    /// Where these values came from; drives the workbook Sources/Assumptions
    /// provenance. `None` (default) is treated as `Manual` with no source IDs.
    #[serde(default)]
    pub provenance: Option<AssumptionProvenance>,
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
        &BuildOptions {
            proj_years: proj_periods,
            ..Default::default()
        },
    )
}

/// Two-outcome extraction gate (Phase 6.1). Returns the blocking reasons that
/// make a workbook unsafe to create: non-finite values, vectors inconsistent
/// with the period count, empty periods, or an invalid currency. An empty
/// result means the extraction may build (a *finite* accounting imbalance is
/// NOT blocking — it surfaces as a failed [`Verification`] instead).
pub fn validate_extraction(extraction: &ExtractionResult) -> Vec<String> {
    let mut reasons = Vec::new();
    let n = extraction.years_found.len();
    if n == 0 {
        reasons.push("no reporting periods in extraction".into());
    }
    // Parse each period label to a sortable key; reject unparseable labels,
    // duplicates, and non-ascending (out-of-order) sequences (Phase 6.1).
    let mut keys: Vec<i64> = Vec::with_capacity(n);
    for label in &extraction.years_found {
        match period_key(label) {
            Some(k) => keys.push(k),
            None => reasons.push(format!("unparseable period label: {label:?}")),
        }
    }
    if keys.len() == extraction.years_found.len() && keys.len() > 1 {
        // Duplicate periods anywhere in the sequence (not just adjacent).
        let mut seen = std::collections::HashSet::new();
        if keys.iter().any(|k| !seen.insert(*k)) {
            reasons.push("duplicate reporting period".into());
        }
        if keys.windows(2).any(|w| w[1] < w[0]) {
            reasons.push("reporting periods are not in ascending chronological order".into());
        }
    }
    let ccy = extraction.currency.trim();
    let ccy_ok = ccy.len() == 3 && ccy.chars().all(|c| c.is_ascii_uppercase());
    if !ccy_ok {
        reasons.push(format!("invalid currency code: {:?}", extraction.currency));
    }
    let check = |label: &str, st: &fm_types::StatementData, reasons: &mut Vec<String>| {
        for (k, v) in st.iter() {
            if n > 0 && v.len() != n {
                reasons.push(format!(
                    "{label}.{k}: {} values for {n} periods (inconsistent vector)",
                    v.len()
                ));
            }
            for x in v.iter().flatten() {
                if !x.is_finite() {
                    reasons.push(format!("{label}.{k}: non-finite value"));
                    break;
                }
            }
        }
    };
    check(
        "income_statement",
        &extraction.income_statement,
        &mut reasons,
    );
    check("balance_sheet", &extraction.balance_sheet, &mut reasons);
    check(
        "cash_flow_statement",
        &extraction.cash_flow_statement,
        &mut reasons,
    );
    reasons
}

/// Parse a reporting-period label into a sortable key: `year*10 + quarter`
/// (quarter 0 = annual/`FY`, 1–4 = quarterly). Accepts `2023`, `FY2023`,
/// `FY 2023`, `2023Q4`, `Q4 2023`, `2023-Q2`. Returns `None` for unparseable
/// labels so the caller can block the build.
fn period_key(label: &str) -> Option<i64> {
    let s = label.trim().to_ascii_uppercase();
    if s.is_empty() {
        return None;
    }
    // Tokenize on any non-alphanumeric so "2023-Q2", "Q1 2023", "FY 2023" all
    // split cleanly; also handle glued forms like "2023Q4" / "FY2023".
    let raw: Vec<String> = s
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_string())
        .collect();
    // Explode glued tokens: "2023Q4" → ["2023","Q4"], "FY2023" → ["FY","2023"].
    let mut tokens: Vec<String> = Vec::new();
    for t in raw {
        if let Some(qpos) = t.find('Q') {
            if qpos > 0 {
                tokens.push(t[..qpos].to_string());
            }
            tokens.push(t[qpos..].to_string());
        } else {
            // Split a leading alpha prefix (FY) from trailing digits.
            let split = t.find(|c: char| c.is_ascii_digit()).unwrap_or(0);
            if split > 0 {
                tokens.push(t[..split].to_string());
            }
            tokens.push(t[split..].to_string());
        }
    }
    let mut year: Option<i64> = None;
    let mut quarter: i64 = 0;
    for t in &tokens {
        if t.len() == 4 && t.chars().all(|c| c.is_ascii_digit()) {
            let y: i64 = t.parse().ok()?;
            if (1900..=2100).contains(&y) {
                if year.is_some() {
                    return None; // two years → ambiguous
                }
                year = Some(y);
            } else {
                return None;
            }
        } else if let Some(rest) = t.strip_prefix('Q') {
            let q: i64 = rest.parse().ok()?;
            if (1..=4).contains(&q) {
                quarter = q;
            } else {
                return None;
            }
        } else if t == "FY" || t.is_empty() {
            // annual marker / empty — ignore
        } else {
            return None; // unrecognized token
        }
    }
    year.map(|y| y * 10 + quarter)
}

/// Balance-sheet identity check over the first `n_hist` (historical) periods:
/// `total_assets == total_liabilities + total_equity` within a small relative
/// tolerance. Only periods where all three are present + finite are checked; a
/// finite imbalance is a critical Verification failure (Phase 6.2).
fn verify_balance_identity(bs: &fm_excel::input::Statement, n_hist: usize) -> Vec<String> {
    let get = |k: &str, i: usize| bs.get(k).and_then(|v| v.get(i).copied().flatten());
    let mut failures = Vec::new();
    for i in 0..n_hist {
        let (a, l, e) = (
            get("total_assets", i),
            get("total_liabilities", i),
            get("total_equity", i),
        );
        if let (Some(a), Some(l), Some(e)) = (a, l, e) {
            if !a.is_finite() || !l.is_finite() || !e.is_finite() {
                continue;
            }
            let diff = (a - (l + e)).abs();
            let scale = a.abs().max(1.0);
            if diff > 0.005 * scale {
                failures.push(format!(
                    "balance sheet does not balance in period {}: assets {a:.0} vs liabilities+equity {:.0}",
                    i + 1,
                    l + e
                ));
            }
        }
    }
    failures
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
    let resolved = resolve_projection_drivers(&engine, opts);
    let projected = engine.project(&resolved.values);
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
        warnings.extend(fm_value::invariants::check_dcf_invariants(
            &dcf_input, d.wacc,
        ));
    }
    let dcf = input.dcf.clone();
    let wacc_out = input.wacc.clone();
    let verification = input.verification.clone();
    BuildOutput {
        projected,
        workbook,
        warnings,
        dcf,
        wacc_out,
        provenance: resolved.provenance,
        verification,
    }
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
    let projected = engine.project(&resolve_projection_drivers(&engine, opts).values);
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
    use fm_excel::is_structure::{
        CogsDetail, OpexItem, Segment, apply_filing_labels, build_is_structure,
        build_standard_is_detailed,
    };

    let hist: Vec<String> = extraction
        .years_found
        .iter()
        .map(|y| format!("{y}A"))
        .collect();
    let proj: Vec<String> = projected.periods.iter().map(|y| format!("{y}E")).collect();
    let mut periods = hist;
    periods.extend(proj);

    let sector = opts.sector.clone();

    // XBRL detail from footnotes (US filings): revenue segments, opex line items,
    // detailed COGS. Parse + (for standard sector) remap cogs/rd/sga into their
    // canonical slots, mirroring cli.py.
    let items = |k: &str| -> Vec<(String, String, String)> {
        extraction
            .notes
            .get(k)
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|o| {
                        let label = o.get("label")?.as_str()?.to_string();
                        let key = o.get("key")?.as_str()?.to_string();
                        let cat = o
                            .get("category")
                            .and_then(|c| c.as_str())
                            .unwrap_or("")
                            .to_string();
                        Some((label, key, cat))
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let seg_raw = items("revenue_segments");
    let opex_raw = items("opex_items");
    let cogs_raw = items("cogs_detail");

    let mut is = extraction.income_statement.clone();
    let detailed = sector == "standard" && (!seg_raw.is_empty() || !opex_raw.is_empty());
    if sector == "standard" && !opex_raw.is_empty() {
        let first = |cat: &str| {
            opex_raw
                .iter()
                .find(|(_, _, c)| c == cat)
                .map(|(_, k, _)| k.clone())
        };
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
    let merge = |hist: &fm_types::StatementData,
                 proj: &fm_types::StatementData|
     -> fm_excel::input::Statement {
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
            if out.contains_key(k) {
                continue;
            }
            let mut row = vec![None; n_h];
            for x in pv.iter().take(n_p) {
                row.push(*x);
            }
            while row.len() < n_h + n_p {
                row.push(None);
            }
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
    let mut warnings = apply_assumption_overrides(&mut assumptions, &opts.assumption_overrides);
    // An explicit tax override flows into every scenario (and thus the WACC
    // unlever tax, which reads base.tax_rate_pct[0]).
    if let Some(t) = opts.tax_rate_override {
        for sc in [
            &mut assumptions.base,
            &mut assumptions.upside,
            &mut assumptions.downside,
        ] {
            for x in sc.tax_rate_pct.iter_mut() {
                *x = t;
            }
        }
    }
    // Analyst-selected scenario case (1=Base/2=Upside/3=Downside) drives the
    // AssumptionsBlock CHOOSE toggle and the DCF `active` pick below.
    assumptions.active_case = opts.active_case.clamp(1, 3) as i64;
    // Real verification (Phase 6.2): balance-sheet identity + extraction +
    // (below) DCF/WACC checks. `passed` is true only when there are no critical
    // failures — never a default placeholder.
    let mut verification = Verification {
        critical_failures: verify_balance_identity(&model.balance_sheet, n_h),
        ..Default::default()
    };
    // Extraction-reported discrepancies are surfaced as verification warnings.
    verification
        .warnings
        .extend(extraction.discrepancies.iter().cloned());
    // Sector honesty (Phase 6.4): bank/insurance/REIT/utility have a supported
    // IS *layout* + comp-multiple table, but the projection engine still drives
    // every sector with the generic revenue/margin model. Declare that plainly
    // rather than implying a sector-specific forecast that does not exist.
    if matches!(sector.as_str(), "bank" | "insurance" | "reit" | "utility") {
        let note = format!(
            "Sector '{sector}': layout supported; projection methodology not yet \
             sector-specific (generic revenue/margin drivers applied)."
        );
        verification.notes.push(note.clone());
        warnings.push(note);
    }

    let nonzero = |k: &str| {
        is.get(k)
            .map(|v| v.iter().any(|x| x.map(|n| n != 0.0).unwrap_or(false)))
            .unwrap_or(false)
    };
    let mut is_structure = if detailed {
        let segments: Vec<Segment> = seg_raw
            .iter()
            .map(|(l, k, _)| Segment {
                label: l.clone(),
                key: k.clone(),
            })
            .collect();
        let opex_items: Vec<OpexItem> = opex_raw
            .iter()
            .map(|(l, k, c)| OpexItem {
                label: l.clone(),
                key: k.clone(),
                category: c.clone(),
            })
            .collect();
        let cogs_detail: Vec<CogsDetail> = cogs_raw
            .iter()
            .map(|(l, k, _)| CogsDetail {
                label: l.clone(),
                key: k.clone(),
            })
            .collect();
        build_standard_is_detailed(
            nonzero("cogs"),
            nonzero("rd"),
            nonzero("sga"),
            &segments,
            &opex_items,
            &cogs_detail,
        )
    } else {
        build_is_structure(&meta.sector, nonzero("cogs"), nonzero("rd"), nonzero("sga"))
    };

    // Override IS labels with actual XBRL concept labels when the filing provides them.
    if let Some(fl) = extraction
        .notes
        .get("filing_labels")
        .and_then(|v| v.as_object())
    {
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

    // DCF/WACC structural checks (Phase 6.2): a non-positive WACC or a Gordon
    // terminal value with WACC ≤ terminal growth is a critical verification
    // failure (the valuation is undefined), surfaced for diagnosis.
    if wacc.wacc <= 0.0 {
        verification
            .critical_failures
            .push(format!("WACC is non-positive ({:.4})", wacc.wacc));
    }
    if dcf.tv_method == 0 && wacc.wacc <= dcf.tv_growth_rate {
        verification.critical_failures.push(format!(
            "Gordon terminal value undefined: WACC {:.4} ≤ terminal growth {:.4}",
            wacc.wacc, dcf.tv_growth_rate
        ));
    }
    verification.warnings.extend(wacc.warnings.iter().cloned());
    verification.warnings.extend(dcf.warnings.iter().cloned());
    verification.passed = verification.critical_failures.is_empty();

    // Unified source-audit (Phase 6.3): one row per research-sourced override
    // year, tracing the modeled driver to its `S#` evidence. `AssumptionProvenance`
    // only carries origin + source IDs, so `detail` (URL) and `retrieved`
    // (retrieval timestamp) stay blank here — they are populated only by origins
    // that actually carry them. Empty when no research provenance → the Sources
    // body stays snapshot-identical.
    let mut source_audit: Vec<fm_excel::input::SourceAuditRow> = Vec::new();
    for ov in &opts.assumption_overrides {
        if let Some(p) = &ov.provenance {
            if p.origin != AssumptionOrigin::Research {
                continue;
            }
            let evidence = p.source_ids.join(", ");
            // Honest status: this layer sees only source IDs, so a research row
            // is `validated` when it carries at least one `S#`, else `unverified`.
            let status = if p.source_ids.is_empty() {
                "unverified"
            } else {
                "validated"
            };
            for (i, cell) in ov.values.iter().enumerate() {
                if let Some(val) = cell {
                    let period = projected.periods.get(i).cloned().unwrap_or_default();
                    source_audit.push(fm_excel::input::SourceAuditRow {
                        line_item: ov.key.clone(),
                        period,
                        value: format!("{val}"),
                        origin: "research".into(),
                        detail: String::new(),
                        retrieved: String::new(),
                        evidence: evidence.clone(),
                        confidence: String::new(),
                        // Accepted rows passed the Phase 5.6 bridge validation
                        // (known driver, grid-bounded, cited Read); status
                        // reflects whether source IDs back the row.
                        verification: status.into(),
                    });
                }
            }
        }
    }

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
        source_audit,
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
        use fm_excel::derive::{
            UPSIDE_CAPEX_DELTA, UPSIDE_GROSS_MARGIN_DELTA, UPSIDE_REVENUE_GROWTH_DELTA,
        };
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

/// Per-year provenance for one engine driver key: one entry per projected year,
/// `None` where the engine derived the value and `Some` where an analyst
/// override supplied it.
pub type DriverProvenance = Vec<Option<AssumptionProvenance>>;

/// The projection drivers actually fed to [`ModelEngine::project`], plus their
/// provenance and any warnings raised while resolving analyst overrides. The
/// SAME vectors drive the projection, the cached formula values, the valuation
/// preview, and the displayed Assumptions block, so they can never disagree.
#[derive(Clone, Debug, Default)]
pub struct ResolvedProjectionDrivers {
    /// Engine-keyed per-year driver vectors. Sparse: a key appears only when its
    /// resolved value differs from the pure engine-derived projection (an active-
    /// case scenario delta, a per-cell analyst override, or the tax override).
    /// The default Base case with no overrides yields an empty map, so
    /// `engine.project(&values)` stays byte-identical to the parity baseline.
    pub values: HashMap<String, Vec<f64>>,
    /// Non-fatal warnings (unknown override keys), mirroring the display path.
    pub warnings: Vec<String>,
    /// Engine-keyed per-year provenance for overridden cells.
    pub provenance: HashMap<String, DriverProvenance>,
}

/// Map an analyst-grid (`ScenarioInputs`) driver key to its `ModelEngine`
/// projection key. Only three keys differ; the rest match verbatim.
fn workbook_key_to_engine_key(key: &str) -> &str {
    match key {
        "revenue_growth_pct" => "revenue_growth",
        "gross_margin_pct" => "gross_margin",
        "tax_rate_pct" => "tax_rate",
        other => other,
    }
}

/// Whether `key` is a known `ScenarioInputs` driver field (the analyst grid's
/// key set). Gates on the canonical [`fm_excel::input::SCENARIO_DRIVER_KEYS`], so
/// the const is load-bearing at runtime and a driver absent from it is simply
/// never applied — no silent second key list.
fn is_known_scenario_key(key: &str) -> bool {
    fm_excel::input::SCENARIO_DRIVER_KEYS.contains(&key)
}

/// Resolve the projection drivers for the ACTIVE scenario case, honoring analyst
/// per-year overrides, the active-case scenario deltas, and an explicit tax
/// override. Returns the engine-keyed per-year vectors that drive BOTH the
/// forward projection and the displayed active Assumptions scenario, so the
/// cached projection can never diverge from the workbook the analyst sees.
///
/// The Base case with no overrides and no tax override yields an empty `values`
/// map (byte-identical to the pre-flexibility projection); only a non-Base case,
/// a per-cell override, or a tax override materializes a driver vector.
pub fn resolve_projection_drivers(
    engine: &ModelEngine,
    opts: &BuildOptions,
) -> ResolvedProjectionDrivers {
    use fm_excel::derive::{
        UPSIDE_CAPEX_DELTA, UPSIDE_GROSS_MARGIN_DELTA, UPSIDE_REVENUE_GROWTH_DELTA,
    };

    let np = opts.proj_years;
    let scalar = engine.derive_assumptions();
    let case = opts.active_case.clamp(1, 3);
    // Active-case sign for the three scenario-divergent drivers: Base 0, Upside
    // +1, Downside -1 — applied to every year, matching `build_scenario`'s fixed
    // deltas so the projection tracks the displayed scenario.
    let case_sign = match case {
        2 => 1.0,
        3 => -1.0,
        _ => 0.0,
    };
    let delta_for = |engine_key: &str| -> f64 {
        case_sign
            * match engine_key {
                "revenue_growth" => UPSIDE_REVENUE_GROWTH_DELTA,
                "gross_margin" => UPSIDE_GROSS_MARGIN_DELTA,
                "capex_pct_rev" => UPSIDE_CAPEX_DELTA,
                _ => 0.0,
            }
    };
    // The engine's base value for a driver (derived scalar, else the project
    // default), with the active-case delta applied to every year.
    let base_vec = |engine_key: &str| -> Vec<f64> {
        let base = scalar
            .get(engine_key)
            .copied()
            .unwrap_or_else(|| fm_engine::default_driver(engine_key));
        vec![base + delta_for(engine_key); np]
    };

    let mut values: HashMap<String, Vec<f64>> = HashMap::new();
    let mut provenance: HashMap<String, DriverProvenance> = HashMap::new();
    let mut warnings: Vec<String> = Vec::new();

    // A non-Base case shifts the three scenario-divergent drivers even without an
    // analyst override, so the projection reflects the selected scenario.
    if case_sign != 0.0 {
        for ek in ["revenue_growth", "gross_margin", "capex_pct_rev"] {
            values.insert(ek.to_string(), base_vec(ek));
        }
    }

    // Overlay per-year analyst overrides (grid keys → engine keys).
    for ov in &opts.assumption_overrides {
        if !is_known_scenario_key(&ov.key) {
            warnings.push(format!("unknown assumption key '{}' — ignored", ov.key));
            continue;
        }
        let ek = workbook_key_to_engine_key(&ov.key).to_string();
        let delta = delta_for(&ek);
        let vec = values.entry(ek.clone()).or_insert_with(|| base_vec(&ek));
        let prov = provenance
            .entry(ek.clone())
            .or_insert_with(|| vec![None; np]);
        for (y, cell) in ov.values.iter().enumerate() {
            let Some(v) = cell else { continue };
            if y < np {
                vec[y] = v + delta;
                prov[y] = Some(ov.provenance.clone().unwrap_or(AssumptionProvenance {
                    origin: AssumptionOrigin::Manual,
                    source_ids: Vec::new(),
                }));
            }
        }
    }

    // An explicit tax override wins over any per-cell tax override and carries no
    // scenario delta (mirrors the display path, which overwrites every scenario).
    if let Some(t) = opts.tax_rate_override {
        values.insert("tax_rate".to_string(), vec![t; np]);
        provenance.insert(
            "tax_rate".to_string(),
            vec![
                Some(AssumptionProvenance {
                    origin: AssumptionOrigin::Manual,
                    source_ids: Vec::new(),
                });
                np
            ],
        );
    }

    ResolvedProjectionDrivers {
        values,
        warnings,
        provenance,
    }
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
    fn scenario_keys_match_canonical_list() {
        // scenario_field_mut MUST resolve exactly fm-excel's canonical driver set
        // (single source of truth) — no more, no fewer.
        let mut s = fm_excel::input::ScenarioInputs::default();
        for key in fm_excel::input::SCENARIO_DRIVER_KEYS {
            assert!(
                scenario_field_mut(&mut s, key).is_some(),
                "canonical key '{key}' not resolved by scenario_field_mut"
            );
        }
        // The valuation scalars are NOT grid drivers.
        assert!(scenario_field_mut(&mut s, "terminal_growth_rate").is_none());
        assert!(!is_known_scenario_key("bogus_key"));
        assert_eq!(fm_excel::input::SCENARIO_DRIVER_KEYS.len(), 12);
    }

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

    fn extraction_with(
        is: fm_types::StatementData,
        bs: fm_types::StatementData,
        years: Vec<String>,
        currency: &str,
    ) -> ExtractionResult {
        ExtractionResult {
            currency: currency.into(),
            years_found: years,
            income_statement: is,
            balance_sheet: bs,
            cash_flow_statement: fm_types::StatementData::new(),
            notes: HashMap::new(),
            confidence: 1.0,
            discrepancies: vec![],
        }
    }

    #[test]
    fn validate_extraction_blocks_unsafe_inputs() {
        use fm_types::StatementData;
        // Non-finite value.
        let mut is = StatementData::new();
        is.insert("revenue".into(), vec![Some(f64::NAN), Some(1.0)]);
        let bad = extraction_with(
            is,
            StatementData::new(),
            vec!["a".into(), "b".into()],
            "USD",
        );
        assert!(
            validate_extraction(&bad)
                .iter()
                .any(|r| r.contains("non-finite"))
        );

        // Inconsistent vector length vs periods.
        let mut is2 = StatementData::new();
        is2.insert("revenue".into(), vec![Some(1.0)]);
        let bad2 = extraction_with(
            is2,
            StatementData::new(),
            vec!["a".into(), "b".into()],
            "USD",
        );
        assert!(
            validate_extraction(&bad2)
                .iter()
                .any(|r| r.contains("inconsistent"))
        );

        // Empty periods + invalid currency.
        let bad3 = extraction_with(
            StatementData::new(),
            StatementData::new(),
            vec![],
            "dollars",
        );
        let r = validate_extraction(&bad3);
        assert!(r.iter().any(|x| x.contains("no reporting periods")));
        assert!(r.iter().any(|x| x.contains("invalid currency")));

        // A clean, balanced extraction has no blocking reasons.
        let mut is3 = StatementData::new();
        is3.insert("revenue".into(), vec![Some(100.0), Some(110.0)]);
        let ok = extraction_with(
            is3,
            StatementData::new(),
            vec!["2023".into(), "2024".into()],
            "USD",
        );
        assert!(validate_extraction(&ok).is_empty());
    }

    #[test]
    fn period_key_parses_advertised_formats() {
        assert_eq!(period_key("2023"), Some(20230));
        assert_eq!(period_key("FY2023"), Some(20230));
        assert_eq!(period_key("FY 2023"), Some(20230));
        assert_eq!(period_key("2023Q4"), Some(20234));
        assert_eq!(period_key("Q1 2023"), Some(20231));
        assert_eq!(period_key("2023-Q2"), Some(20232));
        assert_eq!(period_key("garbage"), None);
        assert_eq!(period_key("2023Q5"), None);
        assert_eq!(period_key(""), None);
    }

    #[test]
    fn validate_extraction_rejects_bad_period_sequences() {
        use fm_types::StatementData;
        let mk = |years: Vec<&str>| {
            extraction_with(
                StatementData::new(),
                StatementData::new(),
                years.into_iter().map(String::from).collect(),
                "USD",
            )
        };
        let dup = validate_extraction(&mk(vec!["2022", "2023", "2022"]));
        assert!(dup.iter().any(|r| r.contains("duplicate")), "{dup:?}");
        let desc = validate_extraction(&mk(vec!["2024", "2023"]));
        assert!(desc.iter().any(|r| r.contains("ascending")), "{desc:?}");
        let bad = validate_extraction(&mk(vec!["2023", "not-a-year"]));
        assert!(bad.iter().any(|r| r.contains("unparseable")), "{bad:?}");
        let ok = validate_extraction(&mk(vec!["2023Q1", "2023Q2", "2023Q3"]));
        assert!(ok.is_empty(), "{ok:?}");
    }

    #[test]
    fn verification_flags_finite_balance_imbalance() {
        use fm_types::StatementData;
        let mut is = StatementData::new();
        is.insert("revenue".into(), vec![Some(100.0), Some(110.0)]);
        is.insert("net_income".into(), vec![Some(10.0), Some(12.0)]);
        // Balanced: A = L + E.
        let mut bs = StatementData::new();
        bs.insert("total_assets".into(), vec![Some(200.0), Some(220.0)]);
        bs.insert("total_liabilities".into(), vec![Some(120.0), Some(130.0)]);
        bs.insert("total_equity".into(), vec![Some(80.0), Some(90.0)]);
        let ok = extraction_with(is.clone(), bs, vec!["2023".into(), "2024".into()], "USD");
        let out = build(&ok, "BAL", 3);
        assert!(
            out.verification.passed,
            "balanced extraction verifies passed"
        );
        assert!(out.verification.critical_failures.is_empty());

        // Imbalanced: assets != liabilities + equity in period 1.
        let mut bs2 = StatementData::new();
        bs2.insert("total_assets".into(), vec![Some(200.0), Some(220.0)]);
        bs2.insert("total_liabilities".into(), vec![Some(120.0), Some(130.0)]);
        bs2.insert("total_equity".into(), vec![Some(50.0), Some(90.0)]);
        let bad = extraction_with(is, bs2, vec!["2023".into(), "2024".into()], "USD");
        let out2 = build(&bad, "IMB", 3);
        // A workbook is STILL produced (BuildAllowedWithFailures) …
        assert!(!out2.workbook.sheets.is_empty());
        // … but verification fails for diagnosis.
        assert!(!out2.verification.passed);
        assert!(!out2.verification.critical_failures.is_empty());
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
        notes.insert(
            "revenue_segments".into(),
            serde_json::json!([
            {"label": "Products", "key": "rev_seg_a"}, {"label": "Services", "key": "rev_seg_b"}]),
        );
        notes.insert(
            "opex_items".into(),
            serde_json::json!([
            {"label": "Cost of products", "key": "cogs_seg_a", "category": "cogs"},
            {"label": "R&D", "key": "rd", "category": "opex_rd"},
            {"label": "SG&A", "key": "sga", "category": "opex"}]),
        );
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
        let has = |t: &str| {
            is_sheet.cells.values().any(|c| {
                matches!(&c.value,
            Some(fm_excel::model::Value::Text(s)) if s == t)
            })
        };
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
                provenance: None,
            },
            AssumptionOverride {
                key: "bogus_key".into(),
                values: vec![Some(1.0)],
                provenance: None,
            },
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

    // ── resolve_projection_drivers: overrides/case/tax now reach the projection ──
    fn engine_2y(is_extra: &[(&str, [f64; 2])]) -> ModelEngine {
        let mut is = fm_types::StatementData::new();
        is.insert("revenue".into(), vec![Some(100.0), Some(110.0)]);
        for (k, v) in is_extra {
            is.insert((*k).into(), vec![Some(v[0]), Some(v[1])]);
        }
        let data = ReconciledData {
            income_statement: is,
            balance_sheet: fm_types::StatementData::new(),
            cash_flow_statement: fm_types::StatementData::new(),
            periods: vec!["2023".into(), "2024".into()],
            currency: "USD".into(),
        };
        let config = CompanyConfig {
            name: "T".into(),
            currency: "USD".into(),
            hist_periods: 2,
            proj_periods: 3,
            ..Default::default()
        };
        ModelEngine::new(data, config)
    }

    fn opts_np3() -> BuildOptions {
        BuildOptions {
            proj_years: 3,
            ..Default::default()
        }
    }

    #[test]
    fn nonstandard_sector_declares_generic_projection_note() {
        let engine = engine_2y(&[]);
        let ex = extraction_with(
            engine.data.income_statement.clone(),
            engine.data.balance_sheet.clone(),
            vec!["2022".into(), "2023".into()],
            "USD",
        );
        let projected = engine.project(&HashMap::new());
        for sector in ["bank", "insurance", "reit", "utility"] {
            let mut opts = opts_np3();
            opts.sector = sector.into();
            let (input, _) = build_workbook_input_with(&ex, &projected, "TEST", &opts);
            assert!(
                input
                    .verification
                    .notes
                    .iter()
                    .any(|n| n.contains("not yet") && n.contains(sector)),
                "sector '{sector}' must declare the projection-methodology limit: {:?}",
                input.verification.notes
            );
        }
        // The same disclaimer must surface through the public build_with warnings
        // (callers that read warnings instead of opening the Sources sheet).
        let mut bopts = opts_np3();
        bopts.sector = "bank".into();
        let out = build_with(&ex, "TEST", &bopts);
        assert!(
            out.warnings
                .iter()
                .any(|w| w.contains("not yet sector-specific")),
            "build_with warnings must carry the sector disclaimer: {:?}",
            out.warnings
        );
        // Standard sector carries no such disclaimer.
        let std = opts_np3();
        let (si, _) = build_workbook_input_with(&ex, &projected, "TEST", &std);
        assert!(
            si.verification
                .notes
                .iter()
                .all(|n| !n.contains("not yet sector-specific"))
        );
    }

    #[test]
    fn resolve_default_is_empty_and_projection_unchanged() {
        let engine = engine_2y(&[]);
        let resolved = resolve_projection_drivers(&engine, &opts_np3());
        assert!(
            resolved.values.is_empty(),
            "default Base/no-override must override no driver"
        );
        assert!(resolved.warnings.is_empty());
        // The resolved projection must equal the pristine engine projection.
        let a = engine.project(&resolved.values);
        let b = engine.project(&HashMap::new());
        assert_eq!(
            a.income_statement.get("revenue"),
            b.income_statement.get("revenue")
        );
    }

    #[test]
    fn override_reaches_projection_with_provenance() {
        let engine = engine_2y(&[]);
        let mut opts = opts_np3();
        opts.assumption_overrides = vec![AssumptionOverride {
            key: "revenue_growth_pct".into(),
            values: vec![Some(0.20), None, Some(0.15)],
            provenance: Some(AssumptionProvenance {
                origin: AssumptionOrigin::Research,
                source_ids: vec!["S1".into()],
            }),
        }];
        let resolved = resolve_projection_drivers(&engine, &opts);
        // Year 0 forced to 0.20 (Base case → no delta); last historical rev = 110.
        let proj = engine.project(&resolved.values);
        assert_eq!(
            proj.income_statement.get("revenue").unwrap()[0],
            Some(132.0)
        );
        // Provenance recorded on the overridden cells only.
        let prov = resolved
            .provenance
            .get("revenue_growth")
            .expect("provenance");
        assert_eq!(prov[0].as_ref().unwrap().origin, AssumptionOrigin::Research);
        assert_eq!(prov[0].as_ref().unwrap().source_ids, vec!["S1".to_string()]);
        assert!(
            prov[1].is_none(),
            "untouched year keeps engine provenance (None)"
        );
    }

    #[test]
    fn research_override_populates_source_audit() {
        let engine = engine_2y(&[]);
        let mut opts = opts_np3();
        opts.assumption_overrides = vec![AssumptionOverride {
            key: "revenue_growth_pct".into(),
            values: vec![Some(0.20), None, Some(0.15)],
            provenance: Some(AssumptionProvenance {
                origin: AssumptionOrigin::Research,
                source_ids: vec!["S1".into(), "S2".into()],
            }),
        }];
        let projected = engine.project(&resolve_projection_drivers(&engine, &opts).values);
        let ex = extraction_with(
            engine.data.income_statement.clone(),
            engine.data.balance_sheet.clone(),
            vec!["2022".into(), "2023".into()],
            "USD",
        );
        let (input, _w) = build_workbook_input_with(&ex, &projected, "TEST", &opts);
        // Two overridden cells (years 0 and 2) → two audit rows; the `None` year
        // is not traced. Every schema field is asserted on the first row.
        assert_eq!(input.source_audit.len(), 2, "{:?}", input.source_audit);
        let row = &input.source_audit[0];
        assert_eq!(row.line_item, "revenue_growth_pct");
        assert_eq!(row.period, projected.periods[0]);
        assert_eq!(row.value, "0.2");
        assert_eq!(row.origin, "research");
        assert_eq!(row.evidence, "S1, S2");
        assert_eq!(row.verification, "validated");
        // Honest blanks: provenance carries no URL or retrieval timestamp.
        assert!(row.detail.is_empty());
        assert!(row.retrieved.is_empty());
        // A manual override contributes no audit rows.
        let mut manual = opts.clone();
        manual.assumption_overrides[0].provenance = Some(AssumptionProvenance {
            origin: AssumptionOrigin::Manual,
            source_ids: vec![],
        });
        let mp = engine.project(&resolve_projection_drivers(&engine, &manual).values);
        let (mi, _) = build_workbook_input_with(&ex, &mp, "TEST", &manual);
        assert!(mi.source_audit.is_empty());
        // A research override with no source IDs is honestly `unverified`.
        let mut noid = opts.clone();
        noid.assumption_overrides[0].provenance = Some(AssumptionProvenance {
            origin: AssumptionOrigin::Research,
            source_ids: vec![],
        });
        let np = engine.project(&resolve_projection_drivers(&engine, &noid).values);
        let (ni, _) = build_workbook_input_with(&ex, &np, "TEST", &noid);
        assert_eq!(ni.source_audit[0].verification, "unverified");
        assert!(ni.source_audit[0].evidence.is_empty());
    }

    #[test]
    fn downside_case_lowers_projection_below_base() {
        let be = engine_2y(&[]);
        let base = be.project(&resolve_projection_drivers(&be, &opts_np3()).values);
        let de = engine_2y(&[]);
        let mut down_opts = opts_np3();
        down_opts.active_case = 3; // Downside
        let down = de.project(&resolve_projection_drivers(&de, &down_opts).values);
        let b0 = base.income_statement.get("revenue").unwrap()[0].unwrap();
        let d0 = down.income_statement.get("revenue").unwrap()[0].unwrap();
        assert!(d0 < b0, "downside revenue {d0} should be below base {b0}");
    }

    #[test]
    fn tax_override_reaches_projection() {
        let hi_e = engine_2y(&[("gross_profit", [60.0, 66.0])]);
        let mut hi_o = opts_np3();
        hi_o.tax_rate_override = Some(0.40);
        let hi = hi_e.project(&resolve_projection_drivers(&hi_e, &hi_o).values);
        let lo_e = engine_2y(&[("gross_profit", [60.0, 66.0])]);
        let mut lo_o = opts_np3();
        lo_o.tax_rate_override = Some(0.10);
        let lo = lo_e.project(&resolve_projection_drivers(&lo_e, &lo_o).values);
        let tax_hi = hi.income_statement.get("income_tax").unwrap()[0].unwrap();
        let tax_lo = lo.income_statement.get("income_tax").unwrap()[0].unwrap();
        assert!(
            tax_hi > tax_lo,
            "40% tax {tax_hi} should exceed 10% tax {tax_lo}"
        );
        assert!(tax_hi > 0.0);
    }

    #[test]
    fn unknown_override_key_warns_and_is_ignored() {
        let engine = engine_2y(&[]);
        let mut opts = opts_np3();
        opts.assumption_overrides = vec![AssumptionOverride {
            key: "bogus_key".into(),
            values: vec![Some(1.0)],
            provenance: None,
        }];
        let resolved = resolve_projection_drivers(&engine, &opts);
        assert!(resolved.warnings.iter().any(|w| w.contains("bogus_key")));
        assert!(resolved.values.is_empty());
    }

    #[test]
    fn engine_default_driver_values_are_stable() {
        assert_eq!(fm_engine::default_driver("revenue_growth"), 0.03);
        assert_eq!(fm_engine::default_driver("tax_rate"), 0.21);
        assert_eq!(fm_engine::default_driver("dso_days"), 45.0);
        assert_eq!(fm_engine::default_driver("unknown"), 0.0);
    }

    #[test]
    fn dollar_leads_section_first_rows_and_per_share_is_two_decimals() {
        use fm_excel::model::{FMT_NUM, LABEL, Value, fmt_dollar, fmt_per_share};
        use fm_types::StatementData;

        // Build a fully-populated model in the given reporting currency.
        let build_ccy = |ccy: &str| -> BuildOutput {
            let mut is = StatementData::new();
            for (k, v) in [
                ("revenue", vec![Some(1000.0), Some(1100.0)]),
                ("cogs", vec![Some(400.0), Some(440.0)]),
                ("net_income", vec![Some(120.0), Some(140.0)]),
                ("eps_diluted", vec![Some(5.12), Some(6.30)]),
            ] {
                is.insert(k.into(), v);
            }
            let mut bs = StatementData::new();
            for (k, v) in [
                ("cash", vec![Some(300.0), Some(350.0)]),
                ("accounts_receivable", vec![Some(200.0), Some(220.0)]),
                ("accounts_payable", vec![Some(150.0), Some(160.0)]),
                ("retained_earnings", vec![Some(500.0), Some(560.0)]),
            ] {
                bs.insert(k.into(), v);
            }
            let mut cf = StatementData::new();
            for (k, v) in [
                ("net_income", vec![Some(120.0), Some(140.0)]),
                ("da", vec![Some(40.0), Some(44.0)]),
                ("capex", vec![Some(80.0), Some(88.0)]),
                ("dividends_paid", vec![Some(30.0), Some(33.0)]),
            ] {
                cf.insert(k.into(), v);
            }
            let extraction = ExtractionResult {
                currency: ccy.into(),
                years_found: vec!["2023".into(), "2024".into()],
                income_statement: is,
                balance_sheet: bs,
                cash_flow_statement: cf,
                notes: HashMap::new(),
                confidence: 1.0,
                discrepancies: vec![],
            };
            build(&extraction, "TEST", 5)
        };

        // First stamped number-format on the lowest-indexed row whose LABEL cell
        // contains every `needle`. Panics loudly if the row or a format is absent.
        fn row_fmt(sheet: &fm_excel::model::Sheet, needles: &[&str]) -> &'static str {
            let row = sheet
                .cells
                .iter()
                .filter_map(|((r, c), cell)| match &cell.value {
                    Some(Value::Text(s)) if *c == LABEL && needles.iter().all(|n| s.contains(n)) => {
                        Some(*r)
                    }
                    _ => None,
                })
                .min()
                .unwrap_or_else(|| panic!("row {needles:?} not found"));
            sheet
                .cells
                .iter()
                .filter_map(|((r, _), cell)| if *r == row { cell.num_fmt } else { None })
                .next()
                .unwrap_or_else(|| panic!("row {needles:?} has no stamped format"))
        }

        // ── USD: `$` leads the first monetary row of each section; ordinary
        //    rows stay plain; per-share rows carry two decimals. ──
        let usd = build_ccy("USD");
        let dollar = fmt_dollar("USD");
        let per_share = fmt_per_share("USD");
        assert_ne!(dollar, FMT_NUM, "USD dollar format must differ from plain");

        let is = usd.workbook.sheet("IS").expect("IS");
        assert_eq!(row_fmt(is, &["Revenue"]), dollar, "IS top line leads with $");
        assert_eq!(row_fmt(is, &["Gross Profit"]), FMT_NUM, "ordinary IS row stays plain");
        assert_eq!(row_fmt(is, &["EPS", "Diluted"]), per_share, "EPS uses per-share format");

        let bs = usd.workbook.sheet("BS").expect("BS");
        assert_eq!(row_fmt(bs, &["Cash"]), dollar, "BS Assets first row leads with $");
        assert_eq!(row_fmt(bs, &["Accounts Receivable"]), FMT_NUM, "ordinary BS asset stays plain");
        assert_eq!(row_fmt(bs, &["Accounts Payable"]), dollar, "BS Liabilities first row leads with $");
        assert_eq!(row_fmt(bs, &["Retained Earnings"]), dollar, "BS Equity first row leads with $");

        let cf = usd.workbook.sheet("CF").expect("CF");
        assert_eq!(row_fmt(cf, &["Net Income"]), dollar, "CF Operating first row leads with $");
        assert_eq!(row_fmt(cf, &["D&A"]), FMT_NUM, "ordinary CF row stays plain");
        assert_eq!(row_fmt(cf, &["Capital Expenditures"]), dollar, "CF Investing first row leads with $");
        assert_eq!(row_fmt(cf, &["Dividends Paid"]), dollar, "CF Financing first row leads with $");
        assert_eq!(row_fmt(cf, &["Dividend per Share"]), per_share, "dividend-per-share uses per-share format");

        // ── Non-USD: `$` is suppressed everywhere (writer.py parity); per-share
        //    keeps two decimals but no currency symbol. ──
        let eur = build_ccy("EUR");
        let is_e = eur.workbook.sheet("IS").expect("IS");
        assert_eq!(row_fmt(is_e, &["Revenue"]), FMT_NUM, "EUR section-first row carries no $");
        let eps_e = row_fmt(is_e, &["EPS", "Diluted"]);
        assert_eq!(eps_e, fmt_per_share("EUR"), "EUR EPS is plain two-decimal");
        assert!(!eps_e.contains('$'), "EUR per-share must not carry $");
    }
}
