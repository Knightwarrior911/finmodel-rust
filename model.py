"""
Usage:
  python model.py --ticker AAPL
  python model.py --ticker AAPL --periods-historical 5 --periods-projected 5
  python model.py --ticker 7203.T --filing /path/to/annual_report.pdf
  python model.py --ticker 7203.T --ir-url https://toyota-global.com/investors/
  python model.py --ticker AAPL --force
"""
import os
import sys


def _check_api_key():
    if not os.environ.get("ANTHROPIC_API_KEY"):
        print("ERROR: ANTHROPIC_API_KEY not set.")
        print("  Set it once in this terminal:  set ANTHROPIC_API_KEY=sk-ant-...")
        print("  Or permanently:                setx ANTHROPIC_API_KEY sk-ant-...")
        print("  No API key? Use --direct flag for US tickers (pulls from SEC, no API needed).")
        sys.exit(1)


def main():
    if hasattr(sys.stdout, 'reconfigure'):
        sys.stdout.reconfigure(encoding='utf-8', errors='replace')

    import argparse
    parser = argparse.ArgumentParser(description="Build a 3-statement financial model from company filings")
    parser.add_argument("--ticker", required=True, help="Company ticker or name (e.g. AAPL, Toyota)")
    parser.add_argument("--periods-historical", type=int, default=5)
    parser.add_argument("--periods-projected", type=int, default=5)
    parser.add_argument("--filing", default=None, help="Path to annual report PDF (overrides fetched data)")
    parser.add_argument("--ir-url", default=None, help="Investor relations page URL for non-US companies")
    parser.add_argument("--direct", action="store_true", help="Skip LLM preflight — look up US ticker directly from EDGAR (no API key needed)")
    parser.add_argument("--force", action="store_true", help="Bypass verification halt on critical failures")
    parser.add_argument("--output", default=None, help="Output .xlsx path (default: <ticker>_model.xlsx)")
    args = parser.parse_args()

    out_path = args.output or f"{args.ticker.replace('.', '_')}_model.xlsx"

    if not args.direct:
        _check_api_key()

    print(f"[1/6] Pre-flight: resolving {args.ticker}...")
    if args.direct:
        from src.preflight import run_preflight_direct
        preflight_fn = lambda: run_preflight_direct(
            args.ticker,
            periods_historical=args.periods_historical,
            periods_projected=args.periods_projected,
            filing_override=args.filing,
            force=args.force,
        )
    else:
        from src.preflight import run_preflight
        preflight_fn = lambda: run_preflight(
            args.ticker,
            periods_historical=args.periods_historical,
            periods_projected=args.periods_projected,
            filing_override=args.filing,
            force=args.force,
        )
    try:
        cfg = preflight_fn()
    except ValueError as e:
        print(f"ERROR: {e}")
        sys.exit(1)
    print(f"      → {cfg.company_name} ({cfg.ticker}), {cfg.domicile}, {cfg.currency}, FY ends {cfg.fiscal_year_end}")

    print(f"[2/6] Fetching filings...")
    try:
        if args.filing:
            from src.extractor import extract_notes_from_pdf
            from src.utils import compute_historical_periods
            from schemas.financial_data import ReconciledFinancialData
            periods = compute_historical_periods(cfg.fiscal_year_end, cfg.periods_historical)
            notes = extract_notes_from_pdf(args.filing, periods)
            raw_data = ReconciledFinancialData(
                ticker=cfg.ticker, company_name=cfg.company_name,
                currency=cfg.currency, fiscal_year_end=cfg.fiscal_year_end,
                periods=periods, income_statement={}, balance_sheet={},
                cash_flow_statement={}, notes=notes, sources={}, flags=[]
            )
        elif cfg.domicile == "US":
            from src.fetcher import fetch_us_filing
            raw_data = fetch_us_filing(cfg)
        else:
            from src.fetcher import fetch_non_us_filing
            raw_data = fetch_non_us_filing(cfg, ir_url=args.ir_url)
    except Exception as e:
        print(f"ERROR in [2/6] fetching: {e}")
        sys.exit(1)
    print(f"      → {len(raw_data.periods)} historical periods: {raw_data.periods}")

    print(f"[3/6] Reconciling data across all filing sources...")
    try:
        from src.reconciler import reconcile
        reconciled, discrepancy_report = reconcile(raw_data)
    except Exception as e:
        print(f"ERROR in [3/6] reconciling: {e}")
        sys.exit(1)
    if discrepancy_report.items:
        print(f"      ⚠ {len(discrepancy_report.items)} discrepancies flagged:")
        for d in discrepancy_report.items:
            print(f"        - {d}")

    print(f"[4/6] Building financial model...")
    try:
        from src.engine import ModelEngine
        engine = ModelEngine(reconciled, cfg)
        model_output = engine.build()
    except Exception as e:
        print(f"ERROR in [4/6] model engine: {e}")
        sys.exit(1)
    print(f"      → {len(model_output.periods)} total periods | converged={model_output.converged}")

    print(f"[5/6] Verifying model...")
    try:
        from src.verifier import verify
        report = verify(model_output)
    except Exception as e:
        print(f"ERROR in [5/6] verification: {e}")
        sys.exit(1)
    if report.critical_failures:
        print(f"      CRITICAL FAILURES:")
        for f in report.critical_failures:
            print(f"        ✗ {f}")
        if not args.force:
            print("      Halting. Use --force to override.")
            sys.exit(1)
        else:
            print("      WARNING: continuing despite critical failures (--force active)")
    if report.warnings:
        for w in report.warnings:
            print(f"      ⚠ {w}")
    if report.passed:
        print(f"      ✓ All checks passed")

    print(f"[6/6] Writing Excel model to {out_path}...")
    try:
        from src.writer import ExcelWriter
        writer = ExcelWriter(model_output, report, cfg.company_name, out_path, sources=reconciled.sources, currency=reconciled.currency)
        writer.write()
    except Exception as e:
        print(f"ERROR in [6/6] writing Excel: {e}")
        sys.exit(1)
    print(f"      ✓ Saved: {out_path}")


if __name__ == "__main__":
    main()
