"""
Usage:
  python model.py --ticker AAPL
  python model.py --ticker AAPL --periods-historical 5 --periods-projected 5
  python model.py --ticker AAPL --force

Orchestrator-mode: no API key required. Pipeline runs from EDGAR/yfinance.
Peer selection uses curated lookup tables (managed by Claude as orchestrator).
"""
import os
import sys


def main():
    if hasattr(sys.stdout, 'reconfigure'):
        sys.stdout.reconfigure(encoding='utf-8', errors='replace')

    import argparse
    parser = argparse.ArgumentParser(description="Build a 3-statement financial model from company filings")
    parser.add_argument("--ticker", required=True, help="Company ticker or name (e.g. AAPL, Toyota)")
    parser.add_argument("--periods-historical", type=int, default=3)
    parser.add_argument("--periods-projected", type=int, default=5)
    parser.add_argument("--filing", default=None, help="Path to annual report PDF (overrides fetched data)")
    parser.add_argument("--ir-url", default=None, help="Investor relations page URL for non-US companies")
    parser.add_argument("--direct", action="store_true", help="Skip LLM preflight — look up US ticker directly from EDGAR (no API key needed)")
    parser.add_argument("--force", action="store_true", help="Bypass verification halt on critical failures")
    parser.add_argument("--no-dcf",   action="store_true", help="Skip DCF valuation tab")
    parser.add_argument("--no-comps", action="store_true", help="Skip trading comps tab")
    parser.add_argument("--output", default=None, help="Output .xlsx path (default: <ticker>_model.xlsx)")
    args = parser.parse_args()

    out_path = args.output or f"{args.ticker.replace('.', '_')}_model.xlsx"

    # preflight, fetch, reconcile, assumptions, engine, verify, write, validate + (peers, wacc, dcf, comps)
    total = 8 + (3 if not args.no_dcf else 0) + (1 if not args.no_comps else 0)
    step  = [0]
    def _hdr(label: str) -> str:
        step[0] += 1
        return f"[{step[0]}/{total}] {label}"

    print(_hdr(f"Pre-flight: resolving {args.ticker}..."))
    # Orchestrator mode: always direct (no LLM required). API-driven preflight is opt-in via --llm.
    from src.preflight import run_preflight_direct, run_preflight
    use_llm = bool(os.environ.get("ANTHROPIC_API_KEY")) and not args.direct
    preflight_fn = (run_preflight if use_llm else run_preflight_direct)
    try:
        cfg = preflight_fn(
            args.ticker,
            periods_historical=args.periods_historical,
            periods_projected=args.periods_projected,
            filing_override=args.filing,
            force=args.force,
        )
    except ValueError as e:
        print(f"ERROR: {e}")
        sys.exit(1)
    sector_note = f" | sector={cfg.sector}" + (f" (SIC {cfg.sic})" if cfg.sic else "")
    print(f"      → {cfg.company_name} ({cfg.ticker}), {cfg.domicile}, {cfg.currency}, FY ends {cfg.fiscal_year_end}{sector_note}")

    print(_hdr("Fetching filings..."))
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
        print(f"ERROR fetching: {e}")
        sys.exit(1)
    print(f"      → {len(raw_data.periods)} historical periods: {raw_data.periods}")

    # Detect IS field presence for dynamic structure
    _is = raw_data.income_statement
    has_cogs = any(v and v != 0 for v in (_is.get("cogs") or []))
    has_rd   = any(v and v != 0 for v in (_is.get("rd")   or []))
    has_sga  = any(v and v != 0 for v in (_is.get("sga")  or []))
    revenue_segments = raw_data.notes.get("revenue_segments", []) if raw_data.notes else []
    opex_items = raw_data.notes.get("opex_items", []) if raw_data.notes else []

    # Map dynamic opex data into traditional key slots for engine backward compat.
    # Only for standard sector — bank/insurance/reit/utility use their own mapping.
    extra_opex_keys: list[str] = []
    if opex_items and cfg.sector == "standard":
        cogs_items = [o for o in opex_items if o["category"] == "cogs"]
        rd_items   = [o for o in opex_items if o["category"] == "opex_rd"]
        other_oe   = [o for o in opex_items if o["category"] == "opex"]
        if cogs_items:
            _is["cogs"] = _is.get(cogs_items[0]["key"], [])
        if rd_items:
            _is["rd"] = _is.get(rd_items[0]["key"], [])
            for idx, ri in enumerate(rd_items):
                if idx > 0:
                    extra_opex_keys.append(ri["key"])
        if other_oe:
            _is["sga"] = _is.get(other_oe[0]["key"], [])
            for idx, oi in enumerate(other_oe):
                if idx > 0:
                    extra_opex_keys.append(oi["key"])
        has_cogs = bool(cogs_items)
        has_rd   = bool(rd_items)
        has_sga  = bool(other_oe)

    from src.is_builder import build_is_structure
    filing_labels = raw_data.notes.get("filing_labels", {}) if raw_data.notes else {}
    is_structure = build_is_structure(
        cfg.sector, has_cogs=has_cogs, has_rd=has_rd, has_sga=has_sga,
        revenue_segments=revenue_segments,
        opex_items=opex_items,
        filing_labels=filing_labels,
    )
    if revenue_segments:
        print(f"      → {len(revenue_segments)} revenue segments detected: "
              f"{', '.join(s['label'] for s in revenue_segments)}")
        cfg.revenue_segments = revenue_segments
    else:
        cfg.revenue_segments = []
    if opex_items:
        print(f"      → {len(opex_items)} opex items from actual XBRL disclosure: "
              f"{', '.join(o['label'] for o in opex_items)}")
        cfg.opex_items = opex_items
        cfg.extra_opex_keys = extra_opex_keys
    else:
        cfg.opex_items = []
        cfg.extra_opex_keys = []

    print(_hdr("Reconciling data across all filing sources..."))
    try:
        from src.reconciler import reconcile
        reconciled, discrepancy_report = reconcile(raw_data)
    except Exception as e:
        print(f"ERROR reconciling: {e}")
        sys.exit(1)
    if discrepancy_report.items:
        print(f"      ⚠ {len(discrepancy_report.items)} discrepancies flagged:")
        for d in discrepancy_report.items:
            print(f"        - {d}")

    print(_hdr("Deriving assumptions and building scenarios..."))
    try:
        from src.engine import ModelEngine
        from src.assumptions import build_assumptions_block
        # Use a no-block engine first to derive historical-based assumptions dict
        derive_engine = ModelEngine(reconciled, cfg)
        hist_assumptions = derive_engine._derive_assumptions()
        # Wrap in a temporary ModelOutput-like dict for build_assumptions_block
        # build_assumptions_block needs proj_periods → derive from cfg
        from src.utils import compute_historical_periods
        hist_periods = compute_historical_periods(cfg.fiscal_year_end, cfg.periods_historical)
        try:
            last_year = int(hist_periods[-1][:4])
            proj_periods = [f"{last_year + i + 1}E" for i in range(cfg.periods_projected)]
        except (ValueError, IndexError):
            proj_periods = [f"P{i+1}E" for i in range(cfg.periods_projected)]
        # Stub ModelOutput for assumptions builder
        class _Stub:
            assumptions = hist_assumptions
            periods = hist_periods + proj_periods
        assumptions = build_assumptions_block(_Stub(), cfg.ticker, sector=cfg.sector)
        if revenue_segments:
            assumptions.revenue_segments = revenue_segments
        print(f"      → 3 scenarios | active={assumptions.active_case} | "
              f"current price=${assumptions.current_share_price:.2f}")
    except Exception as e:
        print(f"      ⚠ Assumptions build failed ({e}) — falling back to historical-only")
        assumptions = None

    print(_hdr("Building financial model..."))
    try:
        engine = ModelEngine(reconciled, cfg, assumptions_block=assumptions)
        model_output = engine.build()
    except Exception as e:
        print(f"ERROR model engine: {e}")
        sys.exit(1)
    print(f"      → {len(model_output.periods)} total periods | converged={model_output.converged}")

    print(_hdr("Verifying model..."))
    try:
        from src.verifier import verify
        report = verify(model_output, sector=cfg.sector)
    except Exception as e:
        print(f"ERROR verification: {e}")
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

    dcf_output = None
    wacc_output = None
    peer_set = None
    if not args.no_dcf and assumptions is not None:
        print(_hdr("Selecting peer set..."))
        try:
            from src.peers import build_peer_set
            peer_set = build_peer_set(cfg.ticker, cfg.company_name,
                                      target_de_ratio=assumptions.target_de_ratio)
            print(f"      → {len(peer_set.peers)} peers ({peer_set.source}): "
                  f"{', '.join(p.ticker for p in peer_set.peers) if peer_set.peers else '—'}")
        except Exception as e:
            print(f"      ⚠ Peer selection failed ({e}) — DCF will use single-ticker fallback")
            from schemas.financial_data import PeerSet
            peer_set = PeerSet(target_ticker=cfg.ticker, target_market_cap=0,
                               target_de_ratio=assumptions.target_de_ratio,
                               peers=[], source="fallback")

        print(_hdr("Computing WACC..."))
        try:
            from src.wacc import compute_wacc
            target_debt = (model_output.balance_sheet.get("long_term_debt") or [0])[-1] or 0
            wacc_output = compute_wacc(
                peer_set=peer_set,
                target_market_cap=peer_set.target_market_cap or 0,
                target_debt=target_debt,
                risk_free_rate=assumptions.risk_free_rate,
                equity_risk_premium=assumptions.equity_risk_premium,
                cost_of_debt_pretax=assumptions.cost_of_debt_pretax,
                target_de_ratio=assumptions.target_de_ratio,
            )
            print(f"      → median Bu={wacc_output.median_unlevered_beta:.2f}  "
                  f"Be_target={wacc_output.target_levered_beta:.2f}  "
                  f"Ke={wacc_output.cost_of_equity:.1%}  WACC={wacc_output.wacc:.1%}")
        except Exception as e:
            print(f"      ⚠ WACC computation failed ({e}) — skipping DCF")

        if wacc_output is not None:
            print(_hdr("Computing DCF valuation..."))
            try:
                from src.dcf import compute_dcf
                dcf_output = compute_dcf(model_output, cfg.ticker, wacc_output, assumptions)
                print(f"      → Implied Price: ${dcf_output.implied_price:.2f}  |  "
                      f"EV: ${dcf_output.enterprise_value:,.0f}M  |  "
                      f"Upside: {dcf_output.upside_downside_pct:+.1%}")
            except Exception as e:
                print(f"      ⚠ DCF failed ({e}) — skipping DCF tab")

    public_comps_output = None
    if not args.no_comps:
        print(_hdr("Building public comps..."))
        try:
            from src.public_comps import build_public_comps
            # Pull target LTM metrics from the latest historical period of the model
            n_h = sum(1 for p in model_output.periods if p.endswith("A"))
            last = n_h - 1
            tgt_rev    = (model_output.income_statement.get("revenue") or [0])[last] or 0
            tgt_ebit   = (model_output.income_statement.get("ebit") or [0])[last] or 0
            tgt_da     = (model_output.income_statement.get("da") or [0])[last] or 0
            tgt_ebitda = tgt_ebit + tgt_da
            tgt_ni     = (model_output.income_statement.get("net_income") or [0])[last] or 0
            tgt_debt   = (model_output.balance_sheet.get("long_term_debt") or [0])[last] or 0
            tgt_cash   = (model_output.balance_sheet.get("cash") or [0])[last] or 0
            tgt_shares = (model_output.income_statement.get("shares_diluted") or [0])[last] or 0

            public_comps_output = build_public_comps(
                target_ticker=cfg.ticker,
                target_company_name=cfg.company_name,
                target_revenue=tgt_rev,
                target_ebitda=tgt_ebitda,
                target_ebit=tgt_ebit,
                target_net_income=tgt_ni,
                target_total_debt=tgt_debt,
                target_cash=tgt_cash,
                target_shares_diluted=tgt_shares,
            )
            print(f"      → {len(public_comps_output.peers)} peers ({public_comps_output.source})  "
                  f"|  implied range: ${public_comps_output.implied_price_low:.2f} – "
                  f"${public_comps_output.implied_price_high:.2f}")
        except Exception as e:
            print(f"      ⚠ Public comps failed ({e}) — skipping Comps tabs")
    comps_output = None  # legacy; kept None always now

    print(_hdr(f"Writing Excel model to {out_path}..."))
    try:
        from src.writer import ExcelWriter
        writer = ExcelWriter(model_output, report, cfg.company_name, out_path,
                             sources=reconciled.sources, currency=reconciled.currency,
                             dcf=dcf_output, comps=comps_output,
                             assumptions=assumptions, ticker=cfg.ticker,
                             fiscal_year_end=cfg.fiscal_year_end,
                             wacc=wacc_output, peer_set=peer_set,
                             public_comps=public_comps_output,
                             sector=cfg.sector,
                             is_structure=is_structure)
        writer.write()
    except Exception as e:
        print(f"ERROR writing Excel: {e}")
        sys.exit(1)
    print(f"      ✓ Saved: {out_path}")

    # ── Validator gate (per shared/SPEC_spreadsheet_engineering Section 6) ───
    print(_hdr(f"Validating {out_path}..."))
    try:
        from src.validator import validate_xlsx
        v = validate_xlsx(out_path)
        c = v.counts
        print(f"      → status={v.status}  blue={c['blue_inputs']}  "
              f"black={c['black_formulas']}  green={c['green_xrefs']}  "
              f"warnings={c['warnings']}  failures={c['failures']}")
        if v.failures:
            print("      FAILURES:")
            for f in v.failures[:10]:
                print(f"        ✗ {f}")
            if len(v.failures) > 10:
                print(f"        ... and {len(v.failures)-10} more")
            if not args.force:
                sys.exit(1)
    except Exception as e:
        print(f"      ⚠ Validator error: {e}")


if __name__ == "__main__":
    main()
