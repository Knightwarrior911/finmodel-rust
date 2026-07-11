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
    /// (Stub) Build a full model for a ticker
    Build { ticker: String },
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

fn cmd_build(ticker: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("Build pipeline for {ticker}");

    // Step 1: Obtain extraction — live-first when key, else committed fixture.
    let has_key = std::env::var("OPENROUTER_API_KEY").map(|k| !k.trim().is_empty()).unwrap_or(false);
    let extraction = if has_key {
        println!("  OPENROUTER_API_KEY set — attempting live fetch...");
        match fm_extract::fetch_xbrl(ticker) {
            Ok(e) => e,
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

    // Step 2: reconcile + project + assemble sheets (SHARED fm-build core).
    println!("  reconcile -> project...");
    let out = fm_build::build(&extraction, ticker, 5);

    // Step 3: Write Excel — the actual deliverable.
    let stem = fm_build::ticker_to_stem(ticker);
    let xlsx_path = format!("{stem}_model.xlsx");
    fm_excel::render::render(&out.workbook, &xlsx_path)?;
    println!("  \u{2713} wrote Excel model -> {xlsx_path}");

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
        // The populated-IS oracles (`*_full_snapshot.json`) lack `model_output`
        // and are gated separately by the `full_is_parity` cargo test.
        .filter(|p| !p.file_name().and_then(|n| n.to_str()).map(|n| n.contains("_full_")).unwrap_or(false))
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
        reported_ebit: ebit,
        reported_ebitda: ebitda,
        reported_ebita: ebita,
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
) -> Result<(), Box<dyn std::error::Error>> {
    let list: Vec<String> = tickers
        .split(',')
        .map(|t| t.trim().to_uppercase())
        .filter(|t| !t.is_empty())
        .collect();
    if list.is_empty() {
        return Err("no tickers given (use --tickers AAPL,MSFT,...)".into());
    }
    let title = title
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("Peer Benchmark — {}", list.join(", ")));
    println!("Benchmarking {} tickers from SEC EDGAR XBRL...", list.len());

    let run = fm_research::benchmark_tickers(&list, &title)?;
    for (t, why) in &run.failed {
        println!("  ! {t}: {why}");
    }
    println!(
        "  {} of {} tickers produced usable filing data",
        run.metrics.len(),
        list.len()
    );

    let generated = fm_research::generated_stamp(&fm_research::today_iso());
    fm_research::render_benchmark(&run.table, out, &generated)?;
    println!("  \u{2713} wrote benchmark workbook -> {out}");
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    match cli.command {
        Command::Score { ground_truth, model } => cmd_score(&ground_truth, &model),
        Command::Compare { snapshot } => cmd_compare(&snapshot),
        Command::Build { ticker } => cmd_build(&ticker),
        Command::Verify {} => cmd_verify(),
        Command::Ifrs {
            standard, ebit, ebitda, ebita, revenue,
            rou_depreciation, lease_interest, lease_cost, lease_liability,
            discount_rate, lease_term, rou_assets,
        } => cmd_ifrs(
            &standard, ebit, ebitda, ebita, revenue,
            rou_depreciation, lease_interest, lease_cost, lease_liability,
            discount_rate, lease_term, rou_assets,
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
        Command::Benchmark { tickers, out, title } => {
            cmd_benchmark(&tickers, &out, title.as_deref())
        }
    }
}
