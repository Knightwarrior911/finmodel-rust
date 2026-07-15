//! fm-cli: integration CLI for the finmodel-core pipeline.
use std::path::PathBuf;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "fm", about = "Financial model engine CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Score a model JSON against ground truth (tie-out)
    Score {
        #[arg(short, long)]
        ground_truth: PathBuf,
        #[arg(short, long)]
        model: PathBuf,
    },
    /// Diff a snapshot against the workbook generated from it (cell-by-cell)
    Compare {
        #[arg(short, long)]
        snapshot: PathBuf,
    },
    /// Build a full 3-statement + DCF model for a ticker
    Build {
        ticker: String,
        /// Projection years (2-10).
        #[arg(long, default_value_t = 5)]
        years: usize,
        #[arg(long, default_value = "standard")]
        sector: String,
        /// Risk-free rate as a decimal (e.g. 0.045).
        #[arg(long)]
        risk_free: Option<f64>,
        /// Equity risk premium as a decimal (e.g. 0.055).
        #[arg(long)]
        erp: Option<f64>,
        #[arg(long)]
        target_de: Option<f64>,
        /// Pre-tax cost of debt as a decimal (blank = engine-derived).
        #[arg(long)]
        cost_of_debt: Option<f64>,
        #[arg(long)]
        beta: Option<f64>,
        /// Tax rate as a decimal (blank = engine-derived).
        #[arg(long)]
        tax_rate: Option<f64>,
        /// Base-case terminal growth as a decimal (e.g. 0.025).
        #[arg(long)]
        terminal_growth: Option<f64>,
        /// Exit EBITDA multiple (blank = sector default).
        #[arg(long)]
        exit_multiple: Option<f64>,
        /// Terminal-value method: 1 = EBITDA exit, 2 = Gordon growth.
        #[arg(long, default_value_t = 1)]
        tv_method: u8,
        #[arg(long)]
        share_price: Option<f64>,
        #[arg(long, default_value = "Dec")]
        fye: String,
        /// Output .xlsx path (blank = <STEM>_model.xlsx).
        #[arg(long)]
        out: Option<String>,
        /// Per-driver override: `--set revenue_growth_pct=0.12,0.10,0.08` (repeatable;
        /// empty comma slots keep the derived value: `--set tax_rate_pct=,,0.20`).
        #[arg(long = "set")]
        set: Vec<String>,
        /// Peer tickers for a trading-comps tab, e.g. `--peers "MSFT,GOOGL"`.
        #[arg(long)]
        peers: Option<String>,
        /// Scenario case: base | upside | downside.
        #[arg(long, default_value = "base")]
        case: String,
        /// Also write a `<STEM>_deck.pptx` summary alongside the workbook.
        #[arg(long)]
        deck: bool,
    },
    /// Verify all committed snapshots reproduce to zero diffs
    Verify {},
    /// IFRS 16 ↔ US GAAP lease-accounting conversion + ASC 842 estimation.
    Ifrs {
        /// Reported accounting standard: "IFRS" or "US GAAP".
        #[arg(long, default_value = "IFRS")]
        standard: String,
        /// Reported EBIT / EBITDA / EBITA (as filed).
        #[arg(long, default_value_t = 0.0)]
        ebit: f64,
        #[arg(long, default_value_t = 0.0)]
        ebitda: f64,
        #[arg(long, default_value_t = 0.0)]
        ebita: f64,
        /// Revenue (enables margin lines).
        #[arg(long, default_value_t = 0.0)]
        revenue: f64,
        /// Direct adjustment inputs (skip ASC 842 estimation if both given).
        #[arg(long)]
        rou_depreciation: Option<f64>,
        #[arg(long)]
        lease_interest: Option<f64>,
        /// ASC 842 note inputs — used to estimate ROU dep + lease interest.
        #[arg(long)]
        lease_cost: Option<f64>,
        #[arg(long)]
        lease_liability: Option<f64>,
        /// Weighted-average discount rate as a percent (e.g. 3.4).
        #[arg(long)]
        discount_rate: Option<f64>,
        #[arg(long)]
        lease_term: Option<f64>,
        #[arg(long)]
        rou_assets: Option<f64>,
        /// Also write a polished IFRS-16 bridge worksheet to this .xlsx path.
        #[arg(long)]
        xlsx: Option<String>,
        /// Company / period labels for the worksheet title.
        #[arg(long, default_value = "Company")]
        company: String,
        #[arg(long, default_value = "")]
        period: String,
        /// PPE depreciation & intangible amortization (for the EBITDA derivation).
        #[arg(long, default_value_t = 0.0)]
        standard_depreciation: f64,
        #[arg(long, default_value_t = 0.0)]
        standard_amortization: f64,
        /// Short-term lease rent (shown as an excluded item; never adjusted).
        #[arg(long, default_value_t = 0.0)]
        short_term_rent: f64,
    },
    /// Enterprise-Value bridge: equity value → EV via debt/leases/pension less
    /// cash & non-operating assets (BIWS rules; goodwill never subtracted).
    EvBridge {
        #[arg(long, default_value = "")]
        company: String,
        #[arg(long)]
        share_price: Option<f64>,
        #[arg(long)]
        shares: Option<f64>,
        #[arg(long)]
        market_cap: Option<f64>,
        #[arg(long)]
        total_debt: Option<f64>,
        #[arg(long)]
        finance_leases: Option<f64>,
        #[arg(long)]
        operating_leases: Option<f64>,
        #[arg(long)]
        underfunded_pension: Option<f64>,
        #[arg(long)]
        minority_interest: Option<f64>,
        #[arg(long)]
        preferred_stock: Option<f64>,
        #[arg(long)]
        cash: Option<f64>,
        #[arg(long)]
        short_term_investments: Option<f64>,
        #[arg(long)]
        equity_investments: Option<f64>,
        #[arg(long)]
        nol_dta: Option<f64>,
        /// Also write a polished EV-bridge worksheet to this .xlsx path.
        #[arg(long)]
        xlsx: Option<String>,
        /// LTM revenue (raw units) — enables the Valuation Multiples block.
        #[arg(long)]
        ltm_revenue: Option<f64>,
        /// LTM EBITDA (raw units) — enables EV/EBITDA in the multiples block.
        #[arg(long)]
        ltm_ebitda: Option<f64>,
    },
    /// Benchmark filing figures for a peer set into an IB-grade Excel workbook.
    /// Fetches each ticker's SEC EDGAR XBRL facts, computes scale/growth/
    /// profitability/returns/leverage metrics, and renders a comparison sheet
    /// with a MEDIAN/MEAN/MIN/MAX block and per-cell provenance notes.
    Benchmark {
        /// Comma-separated tickers, e.g. "AAPL,MSFT,GOOGL".
        #[arg(long)]
        tickers: String,
        /// Output .xlsx path (default: "benchmark.xlsx").
        #[arg(long, default_value = "benchmark.xlsx")]
        out: String,
        /// Workbook title (default derived from the tickers).
        #[arg(long)]
        title: Option<String>,
        /// Also write the raw benchmark grid to this .csv path (for own models).
        #[arg(long)]
        csv: Option<String>,
        /// Reporting-period basis: annual (default) | ltm | quarter | semi.
        /// LTM is the standard IB comps basis; quarter/semi use discrete periods.
        #[arg(long, default_value = "annual")]
        period: String,
        /// Add trading multiples (EV/EBITDA, EV/Revenue, P/E) using live market
        /// prices (Yahoo Finance) × filing-derived EV components.
        #[arg(long)]
        multiples: bool,
        /// Convert monetary metrics to USD at spot FX (Yahoo) — for global,
        /// mixed-currency peer sets. Ratios/multiples are already FX-neutral.
        #[arg(long)]
        usd: bool,
    },
    /// List a company's recent SEC filings (form type, dates, and a direct URL
    /// to each filing's primary document in the EDGAR Archives).
    Filings {
        /// Ticker symbol, e.g. "AAPL".
        ticker: String,
        /// Restrict to one form type (e.g. "10-K", "20-F"). Omit for the
        /// default set (10-K/10-Q/8-K/20-F/6-K).
        #[arg(long)]
        form: Option<String>,
        /// Maximum number of filings to list.
        #[arg(long, default_value_t = 5)]
        limit: usize,
    },
    /// Latest news headlines for a ticker or query (Google News RSS).
    News {
        /// Ticker or free-text query, e.g. "AAPL" or "Nvidia acquisition".
        query: String,
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
    /// Research an M&A deal from the web (query routing + regex synthesis).
    Deal {
        /// e.g. "Microsoft acquires Activision" or "Credit Suisse merger with UBS".
        query: String,
    },
}

fn cmd_score(gt_path: &PathBuf, model_path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let gt = fm_tieout::load_ground_truth(gt_path)?;
    let model = fm_tieout::load_model(model_path)?;
    let score = fm_tieout::score(&gt, &model);
    println!("Tie-out Score:");
    println!("  Trusted:  {}", score.trusted);
    println!("  Matched:  {}", score.matched);
    println!("  Pct:      {:.1}%", score.percentage);
    if score.mismatches.is_empty() {
        println!("  ✓ All cells match");
    } else {
        println!("  ✗ Mismatches:");
        for m in &score.mismatches {
            println!("    - {:?}", m);
        }
    }
    Ok(())
}

/// Build the workbook from a snapshot and diff it cell-by-cell; returns the diff count.
fn diff_snapshot(path: &str) -> Result<usize, Box<dyn std::error::Error>> {
    use std::collections::BTreeMap;
    let snap = fm_excel::snapshot::load_snapshot(path)?;
    let input = fm_excel::snapshot::workbook_input_from_snapshot(&snap)?;
    let wb = fm_excel::sheets::build_workbook(&input);
    let diffs = fm_excel::snapshot::compare_workbook(&wb, &snap);
    if diffs.is_empty() {
        println!("  ✓ 0 diffs (all sheets match)");
    } else {
        let mut by: BTreeMap<String, usize> = BTreeMap::new();
        for d in &diffs {
            *by.entry(d.sheet.clone()).or_default() += 1;
        }
        for (s, n) in &by {
            println!("  ✗ {s}: {n} diffs");
        }
        for d in diffs.iter().take(20) {
            println!("      {d}");
        }
    }
    Ok(diffs.len())
}

fn cmd_compare(snapshot_path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    println!("Comparing {snapshot_path:?}");
    let n = diff_snapshot(&snapshot_path.to_string_lossy())?;
    if n > 0 {
        return Err(format!("{n} cell diff(s)").into());
    }
    Ok(())
}

/// Try to load a committed extraction fixture for a ticker (offline path).
fn load_fixture_extraction(ticker: &str) -> Option<fm_extract::ExtractionResult> {
    let stem = fm_build::ticker_to_stem(ticker);
    let candidates = [
        format!("fm-cli/tests/fixtures/{stem}_model.json"),
        format!("tests/fixtures/{stem}_model.json"),
        format!("finmodel-core/fm-cli/tests/fixtures/{stem}_model.json"),
        format!("../fm-cli/tests/fixtures/{stem}_model.json"),
    ];
    for path in &candidates {
        if let Ok(text) = std::fs::read_to_string(path) {
            if let Ok(mut val) = serde_json::from_str::<serde_json::Value>(&text) {
                if val.get("currency").is_none() {
                    val["currency"] = serde_json::json!(fm_build::currency_for_ticker(ticker));
                }
                if let Ok(result) = serde_json::from_value::<fm_extract::ExtractionResult>(val) {
                    println!("  [offline] loaded committed fixture: {path}");
                    return Some(result);
                }
            }
        }
    }
    None
}

/// Parse `--set key=v1,v2,...` entries into per-driver overrides. Empty comma
/// slots (and non-numeric values) become `None` (keep the engine-derived value).
fn parse_overrides(set: &[String]) -> Vec<fm_build::AssumptionOverride> {
    set.iter()
        .filter_map(|entry| {
            let (key, vals) = entry.split_once('=')?;
            let values = vals
                .split(',')
                .map(|s| s.trim().parse::<f64>().ok())
                .collect();
            Some(fm_build::AssumptionOverride {
                key: key.trim().to_string(),
                values,
            })
        })
        .collect()
}

/// A per-period series for `key` from a statement (missing → 0.0).
fn stmt_series(sd: &fm_types::StatementData, key: &str) -> Vec<f64> {
    sd.get(key)
        .map(|v| v.iter().map(|x| x.unwrap_or(0.0)).collect())
        .unwrap_or_default()
}

/// EBITDA series: prefer the `ebitda` line, else `ebit + da`.
fn ebitda_series(sd: &fm_types::StatementData, n: usize) -> Vec<f64> {
    let direct = stmt_series(sd, "ebitda");
    if direct.iter().any(|v| *v != 0.0) {
        return direct;
    }
    let ebit = stmt_series(sd, "ebit");
    let da = stmt_series(sd, "da");
    (0..n)
        .map(|i| ebit.get(i).copied().unwrap_or(0.0) + da.get(i).copied().unwrap_or(0.0))
        .collect()
}

/// Trading-comps table (headers + rows) for the deck, from assembled comps.
fn comps_deck_table(pc: &fm_value::PublicCompsOutput) -> (Vec<String>, Vec<Vec<String>>) {
    let headers = vec![
        "Ticker".to_string(),
        "EV/Rev".to_string(),
        "EV/EBITDA".to_string(),
        "P/E".to_string(),
    ];
    let mult = |m: Option<f64>| m.map(|v| format!("{v:.1}x")).unwrap_or_else(|| "—".into());
    let rows: Vec<Vec<String>> = pc
        .peers
        .iter()
        .take(14)
        .map(|p| vec![p.ticker.clone(), mult(p.ev_rev_ltm), mult(p.ev_ebitda_ltm), mult(p.pe_ltm)])
        .collect();
    (headers, rows)
}

/// Assemble a [`fm_pptx::writer::deck::ModelDeckInput`] from a completed build.
fn model_deck_input(
    ticker: &str,
    currency: &str,
    extraction: &fm_extract::ExtractionResult,
    opts: &fm_build::BuildOptions,
    out: &fm_build::BuildOutput,
) -> fm_pptx::writer::deck::ModelDeckInput {
    let hist_n = extraction.years_found.len();
    let mut periods = extraction.years_found.clone();
    periods.extend(out.projected.periods.iter().cloned());
    let mut revenue = stmt_series(&extraction.income_statement, "revenue");
    revenue.extend(stmt_series(&out.projected.income_statement, "revenue"));
    let proj_n = out.projected.periods.len();
    let mut ebitda = ebitda_series(&extraction.income_statement, hist_n);
    ebitda.extend(ebitda_series(&out.projected.income_statement, proj_n));
    let dcf = out.dcf.as_ref();
    let (comps_headers, comps_rows) = match &opts.public_comps {
        Some(pc) if !pc.peers.is_empty() => comps_deck_table(pc),
        _ => (Vec::new(), Vec::new()),
    };
    let tv_method = dcf
        .map(|d| if d.tv_method == 1 { "EBITDA exit multiple" } else { "Gordon growth" })
        .unwrap_or("—")
        .to_string();
    fm_pptx::writer::deck::ModelDeckInput {
        ticker: ticker.to_string(),
        company: ticker.to_string(),
        currency: currency.to_string(),
        periods,
        revenue,
        ebitda,
        hist_n,
        implied_price: dcf.map(|d| d.implied_price).unwrap_or(0.0),
        current_price: dcf.map(|d| d.current_share_price).unwrap_or(0.0),
        upside_pct: dcf.map(|d| d.upside_downside_pct).unwrap_or(0.0),
        wacc: out.wacc_out.as_ref().map(|w| w.wacc).unwrap_or(0.0),
        ev: dcf.map(|d| d.enterprise_value).unwrap_or(0.0),
        tv_method,
        comps_headers,
        comps_rows,
    }
}

fn cmd_build(ticker: &str, opts: &fm_build::BuildOptions, deck: bool) -> Result<(), Box<dyn std::error::Error>> {
    println!("Build pipeline for {ticker}");

    // Step 1: Obtain extraction — live-first when key, else committed fixture.
    let has_key = std::env::var("OPENROUTER_API_KEY").map(|k| !k.trim().is_empty()).unwrap_or(false);
    let mut live = false;
    let extraction = if has_key {
        println!("  OPENROUTER_API_KEY set — attempting live fetch...");
        match fm_extract::fetch_xbrl(ticker) {
            Ok(e) => {
                live = true;
                e
            }
            Err(live_err) => {
                println!("  live fetch failed ({live_err}); falling back to committed fixture");
                load_fixture_extraction(ticker).ok_or_else(|| format!(
                    "live fetch failed and no committed fixture for {ticker}"
                ))?
            }
        }
    } else {
        load_fixture_extraction(ticker).ok_or_else(|| format!(
            "No committed fixture for {ticker} and no OPENROUTER_API_KEY set.\n\
             Offline demo tickers: SAND.ST, ASML.AS, NOVO-B.CO, NESN.SW, ATCO-B.ST.\n\
             Live run: set OPENROUTER_API_KEY (see docs/DEMO.md)."
        ))?
    };

    let n_is = extraction.income_statement.len();
    let n_bs = extraction.balance_sheet.len();
    let n_cfs = extraction.cash_flow_statement.len();
    let ny = extraction.years_found.len();
    println!("  extracted: IS({n_is}) BS({n_bs}) CFS({n_cfs}) across {ny} years in {ccy}",
        ccy = extraction.currency);

    // Step 2: live share price (real DCF upside) for live extractions; the
    // offline fixture path stays instant.
    let mut opts = opts.clone();
    if opts.share_price.is_none() && live {
        match fm_fetch::fetch_quote(ticker) {
            Ok(q) if q.currency == extraction.currency => opts.share_price = Some(q.price),
            Ok(q) => eprintln!(
                "warning: quote currency {} ≠ filing currency {} — live share price not applied",
                q.currency, extraction.currency
            ),
            Err(_) => eprintln!("warning: live quote unavailable — pass --share-price for DCF upside"),
        }
    }
    // Live WACC inputs (real risk-free + regression beta) for live extractions,
    // only when the caller left the defaults. Never fatal.
    if live {
        if opts.risk_free_rate == 0.045 {
            match fm_fetch::market::fetch_risk_free_rate() {
                Ok(rf) => {
                    opts.risk_free_rate = rf;
                    eprintln!("warning: Risk-free rate {:.2}% from ^TNX (live)", rf * 100.0);
                }
                Err(_) => eprintln!(
                    "warning: Risk-free rate defaulted to 4.5% (live 10Y fetch failed)"
                ),
            }
        }
        if opts.beta == 1.0 {
            match fm_fetch::market::fetch_beta(ticker) {
                Ok(beta) => {
                    opts.beta = beta;
                    eprintln!("warning: Beta {beta:.2} from 2y weekly regression vs S&P 500");
                }
                Err(_) => eprintln!("warning: Beta defaulted to 1.0 (history fetch failed)"),
            }
        }
    }

    // Trading-comps peer assembly (network stays out of fm-build). Peer failures
    // land in `excluded`, never fatal. Peers require EDGAR.
    if !opts.peers.is_empty() {
        let mut peers: Vec<fm_value::PublicCompPeer> = Vec::new();
        let mut excluded: Vec<(String, String)> = Vec::new();
        for t in &opts.peers {
            println!("  fetching peer {t}...");
            match fm_extract::fetch_xbrl(t) {
                Ok(ex) => {
                    let m = fm_research::metrics_from_extraction(t, &ex);
                    let quote = fm_fetch::fetch_quote(t).ok();
                    peers.push(fm_research::comps::peer_from_metrics(&m, quote.as_ref()));
                }
                Err(e) => {
                    eprintln!("warning: peer {t} excluded ({e})");
                    excluded.push((t.clone(), e.to_string()));
                }
            }
        }
        let target_metrics = fm_research::metrics_from_extraction(ticker, &extraction);
        opts.public_comps = Some(fm_research::comps::build_public_comps(
            &target_metrics,
            &peers,
            excluded,
            &fm_extract::today_iso(),
        ));
    }
    // reconcile + project + assemble sheets (SHARED fm-build core).
    println!("  reconcile -> project...");
    let out = fm_build::build_with(&extraction, ticker, &opts);
    for w in &out.warnings {
        eprintln!("warning: {w}");
    }

    // Step 3: Write Excel — the actual deliverable.
    let stem = fm_build::ticker_to_stem(ticker);
    let xlsx_path = opts
        .out_path
        .clone()
        .filter(|p| !p.trim().is_empty())
        .unwrap_or_else(|| format!("{stem}_model.xlsx"));
    fm_excel::render::render(&out.workbook, &xlsx_path)?;
    println!("  \u{2713} wrote Excel model -> {xlsx_path}");

    // Optional one-click PPTX summary deck beside the workbook.
    if deck {
        let deck_out = format!("{stem}_deck.pptx");
        let input = model_deck_input(ticker, &extraction.currency, &extraction, &opts, &out);
        match fm_pptx::writer::deck::write_model_deck(&input, &fm_extract::today_iso())
            .and_then(|d| d.save(&deck_out))
        {
            Ok(p) => println!("  \u{2713} wrote deck -> {p}"),
            Err(e) => eprintln!("warning: deck not written ({e})"),
        }
    }

    // Step 4: Save the projection JSON for inspection.
    let model_path = format!("{stem}_projection.json");
    let model_output = serde_json::json!({ "ticker": ticker, "projected": out.projected });
    std::fs::write(&model_path, serde_json::to_string_pretty(&model_output)?)?;
    println!("  \u{2713} wrote projection -> {model_path}");

    Ok(())
}

fn cmd_verify() -> Result<(), Box<dyn std::error::Error>> {
    let dir = ["tieout/excel_snapshots", "../tieout/excel_snapshots", "../../tieout/excel_snapshots"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.is_dir())
        .ok_or("could not locate tieout/excel_snapshots (run from the repo or finmodel-core)")?;
    println!("Verifying committed snapshots in {dir:?}");

    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|x| x == "json").unwrap_or(false))
        // Verify only the base model-workbook snapshots: a top-level `model_output`
        // (skips the pure-`{sheets}` gate oracles — adhoc / ev_bridge) AND not a
        // `*_full_*` populated-IS/val/xbrl oracle (those need an explicit
        // is_structure and are gated by full_is_parity / valuation_parity).
        .filter(|p| {
            let is_full = p.file_name().and_then(|n| n.to_str())
                .map(|n| n.contains("_full_")).unwrap_or(true);
            let has_model_output = std::fs::read_to_string(p)
                .ok()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                .map(|v| v.get("model_output").is_some())
                .unwrap_or(false);
            has_model_output && !is_full
        })
        .collect();
    files.sort();
    if files.is_empty() {
        return Err(format!("no snapshot .json files found in {dir:?}").into());
    }

    let mut total = 0usize;
    for f in &files {
        println!("{}", f.file_name().unwrap().to_string_lossy());
        total += diff_snapshot(&f.to_string_lossy())?;
    }
    if total > 0 {
        return Err(format!("{total} total cell diff(s) across {} snapshot(s)", files.len()).into());
    }
    println!("\n✓ All {} snapshot(s) reproduce to 0 diffs", files.len());
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn cmd_ifrs(
    standard: &str,
    ebit: f64,
    ebitda: f64,
    ebita: f64,
    revenue: f64,
    rou_depreciation: Option<f64>,
    lease_interest: Option<f64>,
    lease_cost: Option<f64>,
    lease_liability: Option<f64>,
    discount_rate: Option<f64>,
    lease_term: Option<f64>,
    rou_assets: Option<f64>,
    xlsx: Option<&str>,
    company: &str,
    period: &str,
    standard_depreciation: f64,
    standard_amortization: f64,
    short_term_rent: f64,
) -> Result<(), Box<dyn std::error::Error>> {
    // Resolve adjustment inputs: use direct values, else estimate from ASC 842 note.
    let (rou_dep, interest) = match (rou_depreciation, lease_interest) {
        (Some(r), Some(i)) => (r, i),
        _ => {
            let mut d = fm_ifrs::Asc842LeaseData {
                operating_lease_cost: lease_cost,
                operating_lease_liability: lease_liability,
                weighted_avg_discount_rate: discount_rate,
                weighted_avg_lease_term: lease_term,
                operating_rou_assets: rou_assets,
                ..Default::default()
            };
            d.compute_ifrs_adjustments();
            (
                rou_depreciation.or(d.estimated_rou_depreciation).unwrap_or(0.0),
                lease_interest.or(d.estimated_lease_interest).unwrap_or(0.0),
            )
        }
    };

    let inp = fm_ifrs::IfrsAdjustmentInput {
        rou_depreciation: rou_dep,
        lease_interest: interest,
        short_term_rent,
        reported_ebit: ebit,
        reported_ebitda: ebitda,
        reported_ebita: ebita,
        standard_depreciation,
        standard_amortization,
        accounting_standard: standard.to_string(),
        ..Default::default()
    };
    let out = fm_ifrs::auto_convert(&inp, revenue);

    let title = match out.direction {
        fm_ifrs::AdjustmentDirection::IfrsToUsGaap => "IFRS 16 → US GAAP  (strip lease capitalization)",
        fm_ifrs::AdjustmentDirection::UsGaapToIfrs => "US GAAP → IFRS 16  (add lease capitalization)",
    };
    println!("IFRS lease-accounting bridge — {title}");
    println!("  Adjustment items: ROU depreciation {rou_dep:.1}, lease interest {interest:.1}");
    println!("  (excluded: short-term rent — already OPEX in both frameworks)\n");
    let m = |v: f64| if revenue > 0.0 { format!("  ({v:.1}% margin)") } else { String::new() };
    println!("  {:<8} {:>14} → {:>14}   Δ {:>+12.1}{}", "EBIT", fmt2(ebit), fmt2(out.adjusted_ebit), out.ebit_delta, m(out.adjusted_ebit_margin));
    println!("  {:<8} {:>14} → {:>14}   Δ {:>+12.1}{}", "EBITDA", fmt2(ebitda), fmt2(out.adjusted_ebitda), out.ebitda_delta, m(out.adjusted_ebitda_margin));
    println!("  {:<8} {:>14} → {:>14}   Δ {:>+12.1}{}", "EBITA", fmt2(ebita), fmt2(out.adjusted_ebita), out.ebita_delta, m(out.adjusted_ebita_margin));
    if let Some(path) = xlsx {
        let generated = fm_research::generated_stamp(&fm_research::today_iso());
        let bridge = fm_excel::bridge::IfrsBridgeInput {
            company: company.to_string(),
            period: period.to_string(),
            ifrs_to_us_gaap: matches!(out.direction, fm_ifrs::AdjustmentDirection::IfrsToUsGaap),
            reported_ebit: ebit,
            reported_ebitda: ebitda,
            reported_ebita: ebita,
            standard_depreciation,
            standard_amortization,
            rou_depreciation: rou_dep,
            lease_interest: interest,
            short_term_rent,
            revenue,
            items_excluded: out.items_excluded.clone(),
        };
        let mut wb = fm_excel::model::Workbook::new();
        wb.push(fm_excel::bridge::build_ifrs_bridge_sheet(&bridge, &generated));
        fm_excel::render::render(&wb, path)?;
        println!("  \u{2713} wrote IFRS-16 bridge worksheet -> {path}");
    }
    Ok(())
}

fn fmt2(v: f64) -> String {
    format!("{v:.1}")
}

#[allow(clippy::too_many_arguments)]
fn cmd_ev_bridge(
    company: &str,
    share_price: Option<f64>,
    shares: Option<f64>,
    market_cap: Option<f64>,
    total_debt: Option<f64>,
    finance_leases: Option<f64>,
    operating_leases: Option<f64>,
    underfunded_pension: Option<f64>,
    minority_interest: Option<f64>,
    preferred_stock: Option<f64>,
    cash: Option<f64>,
    short_term_investments: Option<f64>,
    equity_investments: Option<f64>,
    nol_dta: Option<f64>,
    xlsx: Option<&str>,
    ltm_revenue: Option<f64>,
    ltm_ebitda: Option<f64>,
) -> Result<(), Box<dyn std::error::Error>> {
    let inp = fm_value::ev_bridge::EvBridgeInput {
        company: company.to_string(),
        share_price,
        shares_outstanding: shares,
        market_cap,
        total_debt,
        finance_leases,
        operating_leases,
        underfunded_pension,
        minority_interest,
        preferred_stock,
        cash,
        short_term_investments,
        equity_investments,
        nol_dta,
        ltm_revenue,
        ltm_ebitda,
        ..Default::default()
    };
    let b = fm_value::ev_bridge::build_ev_bridge(&inp);
    let name = if company.is_empty() { "Company".to_string() } else { company.to_string() };
    println!("Enterprise Value Bridge — {name}");
    println!("  {:<28} {:>16}", "Equity Value (Market Cap)", fmt2(b.market_cap.unwrap_or(0.0)));
    if !b.additions.is_empty() {
        println!("  (+) additions:");
        for li in &b.additions {
            println!("      {:<24} {:>16}   [{}]", li.item, fmt2(li.amount), li.source);
        }
    }
    if !b.subtractions.is_empty() {
        println!("  (−) subtractions:");
        for li in &b.subtractions {
            println!("      {:<24} {:>16}   [{}]", li.item, fmt2(li.amount), li.source);
        }
    }
    println!("  {:<28} {:>16}", "= Enterprise Value", fmt2(b.total_ev));
    if let Some(path) = xlsx {
        let generated = fm_research::generated_stamp(&fm_research::today_iso());
        let mut wb = fm_excel::model::Workbook::new();
        wb.push(fm_excel::bridge::build_ev_bridge_sheet(&inp, &generated));
        fm_excel::render::render(&wb, path)?;
        println!("  \u{2713} wrote EV-bridge worksheet -> {path}");
    }
    Ok(())
}

fn cmd_benchmark(
    tickers: &str,
    out: &str,
    title: Option<&str>,
    csv: Option<&str>,
    period: &str,
    multiples: bool,
    usd: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let list: Vec<String> = tickers
        .split(',')
        .map(|t| t.trim().to_uppercase())
        .filter(|t| !t.is_empty())
        .collect();
    if list.is_empty() {
        return Err("no tickers given (use --tickers AAPL,MSFT,...)".into());
    }
    let basis: fm_research::PeriodBasis = match period.to_lowercase().as_str() {
        "annual" => fm_research::PeriodBasis::AnnualFy,
        "ltm" => fm_research::PeriodBasis::Ltm,
        "quarter" => fm_research::PeriodBasis::Quarter,
        "semi" => fm_research::PeriodBasis::SemiAnnual,
        other => return Err(format!("invalid --period '{other}' (use: annual | ltm | quarter | semi)").into()),
    };
    let mut tags: Vec<&str> = Vec::new();
    if !matches!(basis, fm_research::PeriodBasis::AnnualFy) {
        tags.push(match basis {
            fm_research::PeriodBasis::Ltm => "LTM",
            fm_research::PeriodBasis::Quarter => "quarterly",
            fm_research::PeriodBasis::SemiAnnual => "semi-annual",
            fm_research::PeriodBasis::AnnualFy => "",
        });
    }
    if multiples { tags.push("+multiples"); }
    if usd { tags.push("USD"); }
    let basis_note = if tags.is_empty() { String::new() } else { format!(" ({})", tags.join(", ")) };
    let title = title
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("Peer Benchmark{basis_note} — {}", list.join(", ")));
    println!("Benchmarking {} tickers from SEC EDGAR XBRL{basis_note}...", list.len());

    let run = fm_research::benchmark_tickers_opts(
        &list,
        &title,
        fm_research::BenchmarkOpts { basis, multiples, to_usd: usd },
    )?;
    for (t, why) in &run.failed {
        println!("  ! {t}: {why}");
    }
    for w in &run.data_warnings {
        eprintln!("warning: {w}");
    }
    println!(
        "  {} of {} tickers produced usable filing data",
        run.metrics.len(),
        list.len()
    );

    let generated = fm_research::generated_stamp(&fm_research::today_iso());
    fm_research::render_benchmark(&run.table, out, &generated)?;
    println!("  \u{2713} wrote benchmark workbook -> {out}");
    if let Some(csv_path) = csv {
        std::fs::write(csv_path, run.table.to_csv())?;
        println!("  \u{2713} wrote benchmark CSV -> {csv_path}");
    }
    Ok(())
}

fn cmd_filings(
    ticker: &str,
    form: Option<&str>,
    limit: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let cik = fm_fetch::cik_from_ticker(ticker)?;
    println!("{} -> CIK {cik}", ticker.trim().to_uppercase());
    let filings = match form {
        Some(f) => fm_fetch::recent_filings(&cik, f, limit)?,
        None => fm_fetch::search_filings(&cik, fm_fetch::DEFAULT_FORM_TYPES, limit)?,
    };
    if filings.is_empty() {
        println!("  (no matching filings)");
        return Ok(());
    }
    for f in &filings {
        println!(
            "  {:<5}  filed {}  period {:<10}  {}",
            f.form_type, f.filing_date, f.fiscal_period_end, f.url
        );
    }
    Ok(())
}

fn cmd_news(query: &str, limit: usize) -> Result<(), Box<dyn std::error::Error>> {
    let headlines = fm_fetch::fetch_headlines(query, limit)?;
    if headlines.is_empty() {
        println!("  (no headlines)");
        return Ok(());
    }
    for h in &headlines {
        let src = if h.source.is_empty() { "" } else { &h.source };
        println!("  {}  [{}]", h.title, src);
        println!("    {}", h.url);
    }
    Ok(())
}
fn cmd_deal(query: &str) -> Result<(), Box<dyn std::error::Error>> {
    let r = fm_research::agent::run_deal_research(query, None);
    println!("Query type:   {:?}", r.query_type);
    println!(
        "Target: {}   Acquirer: {}",
        r.target,
        if r.acquirer.is_empty() { "?" } else { &r.acquirer }
    );
    println!("Sources read: {}   Sufficient: {}", r.sources_read.len(), r.sufficient);
    println!("{}", serde_json::to_string_pretty(&r.summary)?);
    Ok(())
}


fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    match cli.command {
        Command::Score { ground_truth, model } => cmd_score(&ground_truth, &model),
        Command::Compare { snapshot } => cmd_compare(&snapshot),
        Command::Build {
            ticker, years, sector, risk_free, erp, target_de, cost_of_debt, beta,
            tax_rate, terminal_growth, exit_multiple, tv_method, share_price, fye, out, set,
            peers, case, deck,
        } => {
            let d = fm_build::BuildOptions::default();
            let active_case = match case.to_lowercase().as_str() {
                "upside" => 2,
                "downside" => 3,
                _ => 1,
            };
            let peer_list: Vec<String> = peers
                .as_deref()
                .unwrap_or("")
                .split(',')
                .map(|s| s.trim().to_uppercase())
                .filter(|s| !s.is_empty())
                .collect();
            let opts = fm_build::BuildOptions {
                proj_years: years,
                sector,
                risk_free_rate: risk_free.unwrap_or(d.risk_free_rate),
                equity_risk_premium: erp.unwrap_or(d.equity_risk_premium),
                target_de_ratio: target_de.unwrap_or(d.target_de_ratio),
                cost_of_debt_pretax: cost_of_debt,
                beta: beta.unwrap_or(d.beta),
                tax_rate_override: tax_rate,
                terminal_growth: terminal_growth.unwrap_or(d.terminal_growth),
                exit_ebitda_multiple: exit_multiple,
                tv_method,
                share_price,
                fiscal_year_end: fye,
                assumption_overrides: parse_overrides(&set),
                out_path: out,
                peers: peer_list,
                public_comps: None,
                active_case,
                deck,
            };
            cmd_build(&ticker, &opts, deck)
        }
        Command::Verify {} => cmd_verify(),
        Command::Ifrs {
            standard, ebit, ebitda, ebita, revenue,
            rou_depreciation, lease_interest, lease_cost, lease_liability,
            discount_rate, lease_term, rou_assets,
            xlsx, company, period, standard_depreciation, standard_amortization, short_term_rent,
        } => cmd_ifrs(
            &standard, ebit, ebitda, ebita, revenue,
            rou_depreciation, lease_interest, lease_cost, lease_liability,
            discount_rate, lease_term, rou_assets,
            xlsx.as_deref(), &company, &period,
            standard_depreciation, standard_amortization, short_term_rent,
        ),
        Command::EvBridge {
            company, share_price, shares, market_cap, total_debt, finance_leases,
            operating_leases, underfunded_pension, minority_interest, preferred_stock,
            cash, short_term_investments, equity_investments, nol_dta,
            xlsx, ltm_revenue, ltm_ebitda,
        } => cmd_ev_bridge(
            &company, share_price, shares, market_cap, total_debt, finance_leases,
            operating_leases, underfunded_pension, minority_interest, preferred_stock,
            cash, short_term_investments, equity_investments, nol_dta,
            xlsx.as_deref(), ltm_revenue, ltm_ebitda,
        ),
        Command::Benchmark { tickers, out, title, csv, period, multiples, usd } => {
            cmd_benchmark(&tickers, &out, title.as_deref(), csv.as_deref(), &period, multiples, usd)
        }
        Command::Filings { ticker, form, limit } => {
            cmd_filings(&ticker, form.as_deref(), limit)
        }
        Command::News { query, limit } => cmd_news(&query, limit),
        Command::Deal { query } => cmd_deal(&query),
    }
}
