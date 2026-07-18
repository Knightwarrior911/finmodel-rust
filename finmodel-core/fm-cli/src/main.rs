//! fm-cli: integration CLI for the finmodel-core pipeline.
use clap::{Parser, Subcommand};
use std::path::PathBuf;

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
    /// Research a factual/current question from the web: search → read →
    /// citation-grounded synthesis. Reads OPENROUTER_API_KEY from the
    /// environment (never an argument); with no key it prints an honest source
    /// digest instead of a synthesized answer.
    Research {
        /// The question, e.g. "What is the current investment case for Nvidia?".
        question: String,
        /// web | company | earnings | filing | deal | comparison.
        #[arg(long, default_value = "web")]
        mode: String,
        /// quick | standard | deep (budgets: 1/3/30s · 3/6/90s · 5/10/180s).
        #[arg(long, default_value = "standard")]
        depth: String,
        /// Ticker for company/earnings/filing/comparison modes (repeatable).
        #[arg(long = "ticker")]
        tickers: Vec<String>,
        /// Filing form to include in filing mode, e.g. "10-K" (repeatable;
        /// default 10-K/10-Q/8-K/20-F/40-F).
        #[arg(long = "form")]
        forms: Vec<String>,
        /// Target company for deal mode (required for --mode deal).
        #[arg(long)]
        target: Option<String>,
        /// Acquirer for deal mode (optional).
        #[arg(long)]
        acquirer: Option<String>,
        /// json | text.
        #[arg(long, default_value = "text")]
        format: String,
        /// Write the result to this file instead of stdout.
        #[arg(long)]
        output: Option<PathBuf>,
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
                provenance: None,
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
        .map(|p| {
            vec![
                p.ticker.clone(),
                mult(p.ev_rev_ltm),
                mult(p.ev_ebitda_ltm),
                mult(p.pe_ltm),
            ]
        })
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
        .map(|d| {
            if d.tv_method == 1 {
                "EBITDA exit multiple"
            } else {
                "Gordon growth"
            }
        })
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

fn cmd_build(
    ticker: &str,
    opts: &fm_build::BuildOptions,
    deck: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Build pipeline for {ticker}");

    // Step 1: Obtain extraction — live-first when key, else committed fixture.
    let has_key = std::env::var("OPENROUTER_API_KEY")
        .map(|k| !k.trim().is_empty())
        .unwrap_or(false);
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
                load_fixture_extraction(ticker).ok_or_else(|| {
                    format!("live fetch failed and no committed fixture for {ticker}")
                })?
            }
        }
    } else {
        load_fixture_extraction(ticker).ok_or_else(|| {
            format!(
                "No committed fixture for {ticker} and no OPENROUTER_API_KEY set.\n\
             Offline demo tickers: SAND.ST, ASML.AS, NOVO-B.CO, NESN.SW, ATCO-B.ST.\n\
             Live run: set OPENROUTER_API_KEY (see docs/DEMO.md)."
            )
        })?
    };

    let n_is = extraction.income_statement.len();
    let n_bs = extraction.balance_sheet.len();
    let n_cfs = extraction.cash_flow_statement.len();
    let ny = extraction.years_found.len();
    println!(
        "  extracted: IS({n_is}) BS({n_bs}) CFS({n_cfs}) across {ny} years in {ccy}",
        ccy = extraction.currency
    );

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
            Err(_) => {
                eprintln!("warning: live quote unavailable — pass --share-price for DCF upside")
            }
        }
    }
    // Live WACC inputs (real risk-free + regression beta) for live extractions,
    // only when the caller left the defaults. Never fatal.
    if live {
        if opts.risk_free_rate == 0.045 {
            match fm_fetch::market::fetch_risk_free_rate() {
                Ok(rf) => {
                    opts.risk_free_rate = rf;
                    eprintln!(
                        "warning: Risk-free rate {:.2}% from ^TNX (live)",
                        rf * 100.0
                    );
                }
                Err(_) => {
                    eprintln!("warning: Risk-free rate defaulted to 4.5% (live 10Y fetch failed)")
                }
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
    let dir = [
        "tieout/excel_snapshots",
        "../tieout/excel_snapshots",
        "../../tieout/excel_snapshots",
    ]
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
            let is_full = p
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.contains("_full_"))
                .unwrap_or(true);
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
        return Err(format!(
            "{total} total cell diff(s) across {} snapshot(s)",
            files.len()
        )
        .into());
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
                rou_depreciation
                    .or(d.estimated_rou_depreciation)
                    .unwrap_or(0.0),
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
        fm_ifrs::AdjustmentDirection::IfrsToUsGaap => {
            "IFRS 16 → US GAAP  (strip lease capitalization)"
        }
        fm_ifrs::AdjustmentDirection::UsGaapToIfrs => {
            "US GAAP → IFRS 16  (add lease capitalization)"
        }
    };
    println!("IFRS lease-accounting bridge — {title}");
    println!("  Adjustment items: ROU depreciation {rou_dep:.1}, lease interest {interest:.1}");
    println!("  (excluded: short-term rent — already OPEX in both frameworks)\n");
    let m = |v: f64| {
        if revenue > 0.0 {
            format!("  ({v:.1}% margin)")
        } else {
            String::new()
        }
    };
    println!(
        "  {:<8} {:>14} → {:>14}   Δ {:>+12.1}{}",
        "EBIT",
        fmt2(ebit),
        fmt2(out.adjusted_ebit),
        out.ebit_delta,
        m(out.adjusted_ebit_margin)
    );
    println!(
        "  {:<8} {:>14} → {:>14}   Δ {:>+12.1}{}",
        "EBITDA",
        fmt2(ebitda),
        fmt2(out.adjusted_ebitda),
        out.ebitda_delta,
        m(out.adjusted_ebitda_margin)
    );
    println!(
        "  {:<8} {:>14} → {:>14}   Δ {:>+12.1}{}",
        "EBITA",
        fmt2(ebita),
        fmt2(out.adjusted_ebita),
        out.ebita_delta,
        m(out.adjusted_ebita_margin)
    );
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
        wb.push(fm_excel::bridge::build_ifrs_bridge_sheet(
            &bridge, &generated,
        ));
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
    let name = if company.is_empty() {
        "Company".to_string()
    } else {
        company.to_string()
    };
    println!("Enterprise Value Bridge — {name}");
    println!(
        "  {:<28} {:>16}",
        "Equity Value (Market Cap)",
        fmt2(b.market_cap.unwrap_or(0.0))
    );
    if !b.additions.is_empty() {
        println!("  (+) additions:");
        for li in &b.additions {
            println!(
                "      {:<24} {:>16}   [{}]",
                li.item,
                fmt2(li.amount),
                li.source
            );
        }
    }
    if !b.subtractions.is_empty() {
        println!("  (−) subtractions:");
        for li in &b.subtractions {
            println!(
                "      {:<24} {:>16}   [{}]",
                li.item,
                fmt2(li.amount),
                li.source
            );
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
        other => {
            return Err(
                format!("invalid --period '{other}' (use: annual | ltm | quarter | semi)").into(),
            );
        }
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
    if multiples {
        tags.push("+multiples");
    }
    if usd {
        tags.push("USD");
    }
    let basis_note = if tags.is_empty() {
        String::new()
    } else {
        format!(" ({})", tags.join(", "))
    };
    let title = title
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("Peer Benchmark{basis_note} — {}", list.join(", ")));
    println!(
        "Benchmarking {} tickers from SEC EDGAR XBRL{basis_note}...",
        list.len()
    );

    let run = fm_research::benchmark_tickers_opts(
        &list,
        &title,
        fm_research::BenchmarkOpts {
            basis,
            multiples,
            to_usd: usd,
        },
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
        if r.acquirer.is_empty() {
            "?"
        } else {
            &r.acquirer
        }
    );
    println!(
        "Sources read: {}   Sufficient: {}",
        r.sources_read.len(),
        r.sufficient
    );
    println!("{}", serde_json::to_string_pretty(&r.summary)?);
    Ok(())
}

fn parse_research_mode(
    s: &str,
) -> Result<fm_research::research::ResearchMode, Box<dyn std::error::Error>> {
    use fm_research::research::ResearchMode::*;
    Ok(match s.to_ascii_lowercase().as_str() {
        "web" => Web,
        "company" => Company,
        "earnings" => Earnings,
        "filing" => Filing,
        "deal" => Deal,
        "comparison" => Comparison,
        other => {
            return Err(format!(
                "unknown mode '{other}' (web|company|earnings|filing|deal|comparison)"
            )
            .into());
        }
    })
}

fn parse_research_depth(
    s: &str,
) -> Result<fm_research::research::ResearchDepth, Box<dyn std::error::Error>> {
    use fm_research::research::ResearchDepth::*;
    Ok(match s.to_ascii_lowercase().as_str() {
        "quick" => Quick,
        "standard" => Standard,
        "deep" => Deep,
        other => return Err(format!("unknown depth '{other}' (quick|standard|deep)").into()),
    })
}

/// SSRF guard: does any DNS resolution of `host:443` map to a forbidden IP?
/// Mirrors the desktop app's `host_resolves_to_forbidden` — resolution failure
/// returns false so the fetch fails naturally as a read error.
fn cli_host_forbidden(host: &str) -> bool {
    use std::net::ToSocketAddrs;
    match (host, 443u16).to_socket_addrs() {
        Ok(addrs) => addrs.map(|a| a.ip()).any(fm_research::is_forbidden_ip),
        Err(_) => false,
    }
}

/// Blocking search → ranked/deduped/budget-capped candidate ledger.
fn cli_search(
    queries: &[String],
    per_query: usize,
    max_sources: u32,
) -> Vec<fm_research::research::SourceRecord> {
    let mut hits = Vec::new();
    for q in queries {
        if let Ok(h) = fm_fetch::websearch::web_search(q, per_query) {
            hits.extend(h);
        }
    }
    let candidates: Vec<fm_research::Candidate> = hits
        .iter()
        .map(|h| fm_research::candidate_from_web_hit(h, fm_research::SourceBackend::BasicHttp))
        .collect();
    fm_research::assemble_ledger(candidates, max_sources, 2)
}

/// Blocking read of the candidate ledger, routed per source URL after URL/SSRF
/// validation: an EDGAR Archives filing is fetched with the EDGAR client and
/// item-selected against `question`; any other URL uses the generic page reader.
/// A source already `Read` with an excerpt (a synthetic quote source) passes
/// through untouched.
fn cli_read(
    ledger: Vec<fm_research::research::SourceRecord>,
    question: &str,
) -> Vec<fm_research::research::SourceRecord> {
    use fm_research::research::SourceStatus;
    let stamp = fm_research::today_iso();
    let mut out = Vec::with_capacity(ledger.len());
    for mut rec in ledger {
        if rec.status == SourceStatus::Read && rec.excerpt.is_some() {
            out.push(rec);
            continue;
        }
        let url = rec.requested_url.clone();
        let outcome = match fm_research::validate_request_url(&url) {
            Err(e) => {
                fm_research::read_outcome_failed(None, stamp.clone(), format!("url_rejected:{e:?}"))
            }
            Ok(v) if cli_host_forbidden(&v.host) => {
                fm_research::read_outcome_failed(None, stamp.clone(), "ssrf_blocked".to_string())
            }
            Ok(_) if fm_research::is_edgar_archive_url(&url) => {
                match fm_fetch::edgar::fetch_filing_doc(&url) {
                    Ok(text) => {
                        let page = fm_fetch::FetchedPage {
                            title: rec.title.clone(),
                            text: fm_research::select_filing_excerpt(&text, question, 4000),
                            status: fm_fetch::PageStatus::Ok,
                        };
                        fm_research::read_outcome_from_page(
                            &page,
                            Some(url.clone()),
                            stamp.clone(),
                            4000,
                        )
                    }
                    Err(_) => fm_research::read_outcome_failed(
                        None,
                        stamp.clone(),
                        "edgar_fetch_error".to_string(),
                    ),
                }
            }
            Ok(_) => match fm_fetch::fetch_page(&url) {
                Ok(page) => fm_research::read_outcome_from_page(
                    &page,
                    Some(url.clone()),
                    stamp.clone(),
                    4000,
                ),
                Err(_) => {
                    fm_research::read_outcome_failed(None, stamp.clone(), "fetch_error".to_string())
                }
            },
        };
        rec.status = outcome.status;
        rec.final_url = outcome.final_url;
        rec.retrieved_at = outcome.retrieved_at;
        rec.excerpt = outcome.excerpt;
        rec.error_code = outcome.error_code;
        out.push(rec);
    }
    out
}

/// Filing-mode source acquisition: map each ticker to its EDGAR CIK, list recent
/// filings of the requested forms (default 10-K/10-Q/8-K/20-F/40-F), and turn
/// them into `Regulatory` candidates. All filings live on sec.gov, so the
/// per-domain cap is raised to the source budget.
fn cli_filing_search(
    request: &fm_research::research::ResearchRequest,
    max_sources: u32,
) -> Vec<fm_research::research::SourceRecord> {
    let forms: Vec<&str> = if request.filing_forms.is_empty() {
        vec!["10-K", "10-Q", "8-K", "20-F", "40-F"]
    } else {
        request.filing_forms.iter().map(String::as_str).collect()
    };
    let per_ticker = max_sources.max(1) as usize;
    let mut candidates = Vec::new();
    for t in &request.tickers {
        match fm_fetch::edgar::cik_from_ticker(t) {
            Ok(cik) => {
                if let Ok(filings) = fm_fetch::edgar::search_filings(&cik, &forms, per_ticker) {
                    candidates.extend(filings.iter().map(fm_research::candidate_from_filing));
                }
            }
            Err(_) => eprintln!("· no EDGAR CIK for ticker {t}"),
        }
    }
    fm_research::assemble_ledger(candidates, max_sources, max_sources)
}

/// Render a market quote as a compact, citable excerpt with visible freshness.
fn render_quote(q: &fm_fetch::Quote, as_of: &str) -> String {
    let range = match (q.week52_low, q.week52_high) {
        (Some(lo), Some(hi)) => format!(", 52-week range {lo:.2}–{hi:.2}"),
        _ => String::new(),
    };
    format!(
        "{} last price {:.2} {}{}; market data as of {as_of}.",
        q.ticker, q.price, q.currency, range
    )
}

/// Company/earnings acquisition: fuse recent filings, web (IR/earnings +
/// independent), and a market-quote synthetic source into one ranked ledger. The
/// quote reserves the final slot for fresh price context. Earnings mode leads
/// with quarterly filings and an earnings-tuned web query; company mode leads
/// with the annual report and the user's question.
fn cli_fused_search(
    request: &fm_research::research::ResearchRequest,
    per_query: usize,
    max_sources: u32,
) -> Vec<fm_research::research::SourceRecord> {
    use fm_research::research::ResearchMode;
    let stamp = fm_research::today_iso();
    let earnings = request.mode == ResearchMode::Earnings;
    let comparison = request.mode == ResearchMode::Comparison;
    let forms: &[&str] = if earnings {
        &["10-Q", "10-K"]
    } else {
        &["10-K", "10-Q"]
    };
    // Comparison contrasts companies, so take one recent filing per ticker; the
    // single-company modes take the two most recent of the primary ticker.
    let filing_limit = if comparison { 1 } else { 2 };
    let mut candidates = Vec::new();
    for t in &request.tickers {
        if let Ok(cik) = fm_fetch::edgar::cik_from_ticker(t) {
            if let Ok(filings) = fm_fetch::edgar::search_filings(&cik, forms, filing_limit) {
                candidates.extend(filings.iter().map(fm_research::candidate_from_filing));
            }
        }
    }
    let joined = request.tickers.join(" ");
    let web_query = if earnings {
        format!("{joined} latest quarterly earnings results guidance")
    } else if comparison {
        format!("{} comparison", request.tickers.join(" vs "))
    } else {
        format!("{joined} {}", request.question)
    };
    if let Ok(hits) = fm_fetch::websearch::web_search(&web_query, per_query) {
        candidates.extend(hits.iter().map(|h| {
            fm_research::candidate_from_web_hit(h, fm_research::SourceBackend::BasicHttp)
        }));
    }
    // Reserve a quote slot per ticker for comparison, else one for the snapshot.
    let quote_slots = if comparison { request.tickers.len() } else { 1 };
    let mut ledger = fm_research::assemble_ledger(
        candidates,
        max_sources.saturating_sub(quote_slots as u32).max(1),
        2,
    );
    for t in &request.tickers {
        if let Ok(q) = fm_fetch::fetch_quote(t) {
            let id = format!("S{}", ledger.len() + 1);
            ledger.push(fm_research::synthetic_source(
                id,
                format!("https://finance.yahoo.com/quote/{t}"),
                format!("{t} market quote"),
                render_quote(&q, &stamp),
                fm_research::research::SourceKind::Secondary,
                stamp.clone(),
            ));
            if !comparison {
                break;
            }
        }
    }
    ledger
}

/// Deal-mode acquisition: a deal-tuned web search built from the parsed
/// target/acquirer (falling back to the question), read as ordinary web sources.
/// The deal synthesis rider reports structured terms and flags conflicts.
fn cli_deal_search(
    request: &fm_research::research::ResearchRequest,
    per_query: usize,
    max_sources: u32,
) -> Vec<fm_research::research::SourceRecord> {
    let target = request.target.as_deref().unwrap_or("").trim();
    let acquirer = request.acquirer.as_deref().unwrap_or("").trim();
    let parties = format!("{acquirer} {target}");
    let query = if parties.trim().is_empty() {
        request.question.clone()
    } else {
        format!("{} acquisition merger deal terms", parties.trim())
    };
    let mut candidates = Vec::new();
    if let Ok(hits) = fm_fetch::websearch::web_search(&query, per_query) {
        candidates.extend(hits.iter().map(|h| {
            fm_research::candidate_from_web_hit(h, fm_research::SourceBackend::BasicHttp)
        }));
    }
    fm_research::assemble_ledger(candidates, max_sources, 2)
}

/// Blocking OpenRouter synthesis: strict json_schema draft → validate against the
/// read ledger → trusted answer. Any infra/parse/validation failure is a rejection.
fn cli_synthesize(
    api_key: &str,
    model: &str,
    request: &fm_research::research::ResearchRequest,
    read: &[fm_research::research::SourceRecord],
) -> Result<fm_research::research::ResearchAnswer, String> {
    use fm_research::research::SourceStatus;
    if !read.iter().any(|r| r.status == SourceStatus::Read) {
        return Err("empty".into());
    }
    let (system, user) = fm_research::synth::synthesis_prompt(request, read);
    let body = serde_json::json!({
        "model": model,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": user },
        ],
        "temperature": 0,
        "stream": false,
        "max_tokens": 8000,
        "response_format": {
            "type": "json_schema",
            "json_schema": { "name": "research_synthesis", "strict": true, "schema": fm_research::synth::synthesis_schema() }
        },
        "provider": { "require_parameters": true },
    });
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|_| "client_error".to_string())?;
    let resp = client
        .post("https://openrouter.ai/api/v1/chat/completions")
        .bearer_auth(api_key)
        .header("HTTP-Referer", "https://github.com/finmodel")
        .header("X-Title", "finmodel")
        .json(&body)
        .send()
        .map_err(|_| "http_error".to_string())?;
    let v: serde_json::Value = resp.json().map_err(|_| "decode_error".to_string())?;
    let content = v["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or_default();
    let draft =
        fm_research::synth::parse_draft(content).ok_or_else(|| "parse_error".to_string())?;
    fm_research::synth::validate_synthesis(&draft, read).map_err(|e| format!("{e:?}"))?;
    Ok(fm_research::synth::build_answer(
        &draft,
        request,
        read.to_vec(),
        model.to_string(),
        fm_research::today_iso(),
    ))
}

/// Human-readable rendering of a terminal research output.
fn render_research_text(out: &fm_research::research::ResearchOutput) -> String {
    use fm_research::research::ResearchOutput;
    let mut s = String::new();
    match out {
        ResearchOutput::Answer(a) => {
            s.push_str(&format!(
                "Q: {}\nConfidence: {:?}\n\n{}\n",
                a.question, a.confidence, a.summary.text
            ));
            for c in &a.summary.citations {
                s.push_str(&format!("    [{}] \"{}\"\n", c.source_id, c.quote));
            }
            for sec in &a.sections {
                s.push_str(&format!("\n## {}\n", sec.heading));
                for p in &sec.paragraphs {
                    s.push_str(&format!("{}\n", p.text));
                    for c in &p.citations {
                        s.push_str(&format!("    [{}] \"{}\"\n", c.source_id, c.quote));
                    }
                }
            }
            if !a.limitations.is_empty() {
                s.push_str("\nLimitations:\n");
                for l in &a.limitations {
                    s.push_str(&format!("  - {l}\n"));
                }
            }
            s.push_str("\nSources:\n");
            for src in &a.sources {
                let url = src.final_url.as_deref().unwrap_or(&src.requested_url);
                s.push_str(&format!(
                    "  [{}] {} ({:?}) {}\n",
                    src.id, src.domain, src.status, url
                ));
            }
            s.push_str(&format!(
                "\nmodel: {}   as of {}\n",
                a.model, a.generated_at
            ));
        }
        ResearchOutput::Digest(d) => {
            s.push_str(&format!(
                "Q: {}\n\nSource digest (no synthesis):\n",
                d.question
            ));
            for it in &d.items {
                s.push_str(&format!(
                    "  [{}] {} ({:?})\n      {}\n",
                    it.source_id, it.title, it.status, it.url
                ));
                if let Some(sn) = &it.snippet {
                    s.push_str(&format!("      {sn}\n"));
                }
            }
            if !d.limitations.is_empty() {
                s.push_str("\nLimitations:\n");
                for l in &d.limitations {
                    s.push_str(&format!("  - {l}\n"));
                }
            }
            s.push_str(&format!("\nas of {}\n", d.generated_at));
        }
    }
    s
}

fn cmd_research(
    question: &str,
    mode: &str,
    depth: &str,
    tickers: Vec<String>,
    forms: Vec<String>,
    target: Option<String>,
    acquirer: Option<String>,
    format: &str,
    output: Option<&std::path::Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    use fm_research::machine::{Action, Input, ResearchBudgets, ResearchMachine, SynthesisReject};
    use fm_research::research::{ResearchMode, ResearchRequest, SourceRecord};

    let mode = parse_research_mode(mode)?;
    let depth = parse_research_depth(depth)?;
    let request = ResearchRequest {
        question: question.to_string(),
        mode,
        tickers,
        periods: Vec::new(),
        filing_forms: forms,
        target,
        acquirer,
        depth,
    };
    request.validate()?;

    let budgets = ResearchBudgets::from_depth(depth);
    let max_sources = budgets.max_sources;
    let mut machine = ResearchMachine::new(request.clone(), budgets, fm_research::today_iso());

    // The key is an environment secret — NEVER a CLI argument. No key → digest.
    let api_key = std::env::var("OPENROUTER_API_KEY")
        .ok()
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty());
    let model = std::env::var("FINMODEL_MODEL")
        .ok()
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| "deepseek/deepseek-v4-flash".to_string());
    if api_key.is_none() {
        eprintln!("note: OPENROUTER_API_KEY not set — emitting a source digest (no synthesis).");
    }

    let mut input = Input::Start;
    let mut searched: Vec<SourceRecord> = Vec::new();
    let mut read_records: Vec<SourceRecord> = Vec::new();
    let terminal = loop {
        match machine.next(input) {
            Action::Done(out) => break out,
            Action::Cancelled => return Err("research cancelled".into()),
            Action::Error { code } => return Err(format!("research error: {code}").into()),
            Action::Plan => {
                // The CLI does not run a model planning round; fall back to the question.
                input = Input::Planned(None);
            }
            Action::Search { queries } => {
                searched = match request.mode {
                    ResearchMode::Filing => {
                        eprintln!("· listing filings…");
                        cli_filing_search(&request, max_sources)
                    }
                    ResearchMode::Company | ResearchMode::Earnings | ResearchMode::Comparison => {
                        eprintln!("· gathering filings, web, and market data…");
                        cli_fused_search(&request, 6, max_sources)
                    }
                    ResearchMode::Deal => {
                        eprintln!("· searching deal coverage…");
                        cli_deal_search(&request, 6, max_sources)
                    }
                    _ => {
                        eprintln!("· searching ({} queries)…", queries.len());
                        cli_search(&queries, 6, max_sources)
                    }
                };
                input = Input::Searched(searched.clone());
            }
            Action::Read { .. } => {
                eprintln!("· reading {} source(s)…", searched.len());
                read_records = cli_read(searched.clone(), &request.question);
                input = Input::ReadDone(read_records.clone());
            }
            Action::Synthesize { attempt } => {
                eprintln!("· synthesizing (attempt {attempt})…");
                let result = match &api_key {
                    Some(key) => cli_synthesize(key, &model, &request, &read_records)
                        .map_err(|code| SynthesisReject { code }),
                    None => Err(SynthesisReject {
                        code: "no_api_key".into(),
                    }),
                };
                input = Input::Synthesized(result);
            }
        }
    };

    let rendered = match format.to_ascii_lowercase().as_str() {
        "json" => serde_json::to_string_pretty(&terminal)?,
        "text" => render_research_text(&terminal),
        other => return Err(format!("unknown format '{other}' (json|text)").into()),
    };
    match output {
        Some(p) => {
            std::fs::write(p, &rendered)?;
            eprintln!("wrote {}", p.display());
        }
        None => println!("{rendered}"),
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    match cli.command {
        Command::Score {
            ground_truth,
            model,
        } => cmd_score(&ground_truth, &model),
        Command::Compare { snapshot } => cmd_compare(&snapshot),
        Command::Build {
            ticker,
            years,
            sector,
            risk_free,
            erp,
            target_de,
            cost_of_debt,
            beta,
            tax_rate,
            terminal_growth,
            exit_multiple,
            tv_method,
            share_price,
            fye,
            out,
            set,
            peers,
            case,
            deck,
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
            standard,
            ebit,
            ebitda,
            ebita,
            revenue,
            rou_depreciation,
            lease_interest,
            lease_cost,
            lease_liability,
            discount_rate,
            lease_term,
            rou_assets,
            xlsx,
            company,
            period,
            standard_depreciation,
            standard_amortization,
            short_term_rent,
        } => cmd_ifrs(
            &standard,
            ebit,
            ebitda,
            ebita,
            revenue,
            rou_depreciation,
            lease_interest,
            lease_cost,
            lease_liability,
            discount_rate,
            lease_term,
            rou_assets,
            xlsx.as_deref(),
            &company,
            &period,
            standard_depreciation,
            standard_amortization,
            short_term_rent,
        ),
        Command::EvBridge {
            company,
            share_price,
            shares,
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
            xlsx,
            ltm_revenue,
            ltm_ebitda,
        } => cmd_ev_bridge(
            &company,
            share_price,
            shares,
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
            xlsx.as_deref(),
            ltm_revenue,
            ltm_ebitda,
        ),
        Command::Benchmark {
            tickers,
            out,
            title,
            csv,
            period,
            multiples,
            usd,
        } => cmd_benchmark(
            &tickers,
            &out,
            title.as_deref(),
            csv.as_deref(),
            &period,
            multiples,
            usd,
        ),
        Command::Filings {
            ticker,
            form,
            limit,
        } => cmd_filings(&ticker, form.as_deref(), limit),
        Command::News { query, limit } => cmd_news(&query, limit),
        Command::Deal { query } => cmd_deal(&query),
        Command::Research {
            question,
            mode,
            depth,
            tickers,
            forms,
            target,
            acquirer,
            format,
            output,
        } => cmd_research(
            &question,
            &mode,
            &depth,
            tickers,
            forms,
            target,
            acquirer,
            &format,
            output.as_deref(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fm_research::research::{
        DigestItem, ResearchDepth, ResearchDigest, ResearchMode, ResearchOutput, SourceStatus,
    };

    #[test]
    fn research_mode_and_depth_parse_case_insensitively_and_reject_junk() {
        assert_eq!(parse_research_mode("Web").unwrap(), ResearchMode::Web);
        assert_eq!(
            parse_research_mode("COMPARISON").unwrap(),
            ResearchMode::Comparison
        );
        assert_eq!(parse_research_depth("deep").unwrap(), ResearchDepth::Deep);
        assert!(parse_research_mode("banana").is_err());
        assert!(parse_research_depth("thorough").is_err());
    }

    #[test]
    fn render_digest_text_lists_sources_status_and_limitations() {
        let out = ResearchOutput::Digest(ResearchDigest {
            question: "Q?".into(),
            items: vec![DigestItem {
                source_id: "S1".into(),
                title: "Example filing".into(),
                url: "https://ex.com/a".into(),
                snippet: Some("a snippet".into()),
                status: SourceStatus::Read,
            }],
            limitations: vec!["no primary source".into()],
            generated_at: "2026-01-01".into(),
        });
        let text = render_research_text(&out);
        assert!(text.contains("Source digest"));
        assert!(text.contains("[S1] Example filing (Read)"));
        assert!(text.contains("https://ex.com/a"));
        assert!(text.contains("a snippet"));
        assert!(text.contains("no primary source"));
    }
}
