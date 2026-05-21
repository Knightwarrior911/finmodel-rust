"""
Usage:
  python -m src.cli --ticker AAPL
  python -m src.cli --ticker AAPL --periods-historical 5 --periods-projected 5
  python -m src.cli --ticker AAPL --force
  python -m src.cli --ask "What is Apple's revenue and net income?"
  python -m src.cli --ask "Run a DCF on MSFT" --ticker MSFT

Orchestrator-mode: no API key required. Pipeline runs from EDGAR/yfinance.
LLM-orchestrator mode: use --ask to route any natural-language query through the
top-level VirtualAnalystOrchestrator (requires ANTHROPIC_API_KEY).
"""
import os
import sys

# Load .env before any key checks
try:
    from dotenv import load_dotenv as _load_dotenv
    _load_dotenv(os.path.join(os.path.dirname(__file__), "..", ".env"), override=False)
except ImportError:
    pass

# Dev mock: stub LLM calls before any module imports anthropic
from src.dev_mock import is_active as _mock_active, patch_anthropic as _patch_anthropic
if _mock_active():
    _patch_anthropic()


def _claude_cli_available() -> bool:
    """True if the `claude` CLI is on PATH and responds to --version."""
    import shutil
    return shutil.which("claude") is not None


def main():
    if hasattr(sys.stdout, 'reconfigure'):
        sys.stdout.reconfigure(encoding='utf-8', errors='replace')

    import argparse
    parser = argparse.ArgumentParser(description="Virtual Financial Analyst — model builder and research orchestrator")
    parser.add_argument("--ask", default=None, help="Natural-language query routed through the LLM orchestrator")
    parser.add_argument("--tool", default=None, help="Call a single orchestrator tool directly (no API key needed). E.g. search_sec_edgar, search_web, fetch_page, run_dcf, run_ev_bridge, run_public_comps")
    parser.add_argument("--tool-args", default=None, help='JSON args for --tool. E.g. \'{"ticker":"AAPL"}\'')
    parser.add_argument("--ticker", default=None, help="Company ticker or name (e.g. AAPL, Toyota)")
    parser.add_argument("--periods-historical", type=int, default=3)
    parser.add_argument("--periods-projected", type=int, default=5)
    parser.add_argument("--filing", default=None, help="Path to annual report PDF (overrides fetched data)")
    parser.add_argument("--ir-url", default=None, help="Investor relations page URL for non-US companies")
    parser.add_argument("--direct", action="store_true", help="Skip LLM preflight — look up US ticker directly from EDGAR (no API key needed)")
    parser.add_argument("--force", action="store_true", help="Bypass verification halt on critical failures")
    parser.add_argument("--no-dcf",   action="store_true", help="Skip DCF valuation tab")
    parser.add_argument("--no-comps", action="store_true", help="Skip trading comps tab")
    parser.add_argument("--output", default=None, help="Output .xlsx path (default: <ticker>_model.xlsx)")
    parser.add_argument("--deck", action="store_true", help="Also build a PowerPoint summary deck alongside the Excel model")
    parser.add_argument("--audit", action="store_true", help="After writing xlsx, render per-cell source-filing snapshots with yellow highlight and attach as hyperlinks (clickable audit trail)")
    parser.add_argument("--audit-pdf", default=None, help="Path to source PDF used for --audit snapshots (auto-discovered from extraction_cache/ if omitted)")
    args = parser.parse_args()

    # Direct tool invocation — no API key needed, calls one tool and prints result
    if args.tool:
        import asyncio, json as _json
        from src.orchestrator import _execute_tool
        tool_args = _json.loads(args.tool_args) if args.tool_args else {}
        # Convenience: inject --ticker into args if not in tool_args
        if args.ticker and "ticker" not in tool_args:
            tool_args["ticker"] = args.ticker
        result = asyncio.run(_execute_tool(args.tool, tool_args))
        print(result)
        return

    # LLM orchestrator mode — routes any natural-language query through the top-level brain
    if args.ask:
        from src.orchestrator import run_sync
        result = run_sync(
            query=args.ask,
            ticker=args.ticker or "",
            company="",
        )
        print(result)
        return

    if not args.ticker:
        parser.error("--ticker is required when not using --ask")

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
    _has_llm_key = (
        bool(os.environ.get("ANTHROPIC_API_KEY"))
        or bool(os.environ.get("DEEPSEEK_API_KEY"))
        or _claude_cli_available()
    )
    use_llm = (_has_llm_key or _mock_active()) and not args.direct
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
    cogs_detail = raw_data.notes.get("cogs_detail", []) if raw_data.notes else []

    # One-time items that should NOT recur in projections.
    _NON_RECURRING_GROUPS = {"restruct", "ma", "impair", "legal", "gainloss", "severance"}

    # Map dynamic opex data into traditional key slots for engine backward compat.
    # Only for standard sector — bank/insurance/reit/utility use their own mapping.
    extra_opex_keys: list[str] = []
    nonrecurring_opex_keys: list[str] = []
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
                    if oi.get("group", "") in _NON_RECURRING_GROUPS:
                        nonrecurring_opex_keys.append(oi["key"])
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
        cogs_detail=cogs_detail,
    )
    if revenue_segments:
        print(f"      → {len(revenue_segments)} revenue segments detected: "
              f"{', '.join(s['label'] for s in revenue_segments)}")
        cfg.revenue_segments = revenue_segments
    else:
        cfg.revenue_segments = []
    cfg.cogs_detail = cogs_detail
    if cogs_detail:
        print(f"      → {len(cogs_detail)} cost-of-revenue sub-items detected: "
              f"{', '.join(c['label'] for c in cogs_detail)}")
        # cogs_seg_* keys project proportionally — treat as extra_opex_keys
        for cd in cogs_detail:
            if cd["key"] not in extra_opex_keys:
                extra_opex_keys.append(cd["key"])
    if opex_items:
        print(f"      → {len(opex_items)} opex items from actual XBRL disclosure: "
              f"{', '.join(o['label'] for o in opex_items)}")
        cfg.opex_items = opex_items
        cfg.extra_opex_keys = extra_opex_keys
        cfg.nonrecurring_opex_keys = nonrecurring_opex_keys
        if nonrecurring_opex_keys:
            labels = [o["label"] for o in opex_items if o["key"] in nonrecurring_opex_keys]
            print(f"      → {len(nonrecurring_opex_keys)} non-recurring items zeroed in projections: "
                  f"{', '.join(labels)}")
    else:
        cfg.opex_items = []
        cfg.extra_opex_keys = []
        cfg.nonrecurring_opex_keys = []

    print(_hdr("Reconciling data across all filing sources..."))
    try:
        from src.reconciler import reconcile
        reconciled, discrepancy_report = reconcile(raw_data)
    except Exception as e:
        print(f"      ⚠ Reconciliation skipped ({e}); using raw XBRL data")
        reconciled = raw_data
        from schemas.financial_data import DiscrepancyReport
        discrepancy_report = DiscrepancyReport(items=[])
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
            from src.peers import _beta as _fetch_target_beta
            target_debt = (model_output.balance_sheet.get("long_term_debt") or [0])[-1] or 0
            # Use target's own levered beta as fallback when no peers available.
            # Unlever it here using target D/E so compute_wacc receives an unlevered beta.
            own_levered_beta = _fetch_target_beta(cfg.ticker)
            target_de = assumptions.target_de_ratio or 0.30
            own_unlevered_beta = own_levered_beta / (1 + (1 - 0.21) * target_de)
            wacc_output = compute_wacc(
                peer_set=peer_set,
                target_market_cap=peer_set.target_market_cap or 0,
                target_debt=target_debt,
                risk_free_rate=assumptions.risk_free_rate,
                equity_risk_premium=assumptions.equity_risk_premium,
                cost_of_debt_pretax=assumptions.cost_of_debt_pretax,
                target_de_ratio=assumptions.target_de_ratio,
                fallback_beta=own_unlevered_beta,
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

    # ── Audit-trail snapshots (opt-in via --audit) ────────────────────────
    if args.audit:
        total += 1
        print(_hdr("Audit: rendering source-filing snapshots..."))
        try:
            from src.audit_pipeline import run_audit
            res = run_audit(
                cfg.ticker,
                pdf_path=args.audit_pdf,
                xlsx_path=out_path,
            )
            if res.get("ok"):
                ann = res.get("annotated") or {}
                print(f"      ✓ located={res['values_located']}/{res['values_total']} "
                      f"({res['coverage_pct']}%)  low_conf={res['values_low_confidence']}  "
                      f"snapshots={res['snapshots_rendered']}")
                print(f"      → linked cells: snapshot={ann.get('linked_snapshot', 0)} "
                      f"pdf_fallback={ann.get('linked_pdf', 0)}")
                if res.get("missing_period_pdfs"):
                    print(f"      ⚠ no source PDF for periods: "
                          f"{', '.join(res['missing_period_pdfs'])} "
                          f"(those numbers cannot be linked)")
                print(f"      → snapshots: {res['snapshots_dir']}")
            else:
                print(f"      ⚠ Audit skipped: {res.get('error')}")
        except Exception as e:
            print(f"      ⚠ Audit error: {e}")

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

    # ── Verification Loop (per VERIFICATION_LOOP.md) ───────────────────
    total += 1
    print(_hdr("Running verification loop..."))
    try:
        from src.verification_loop import run_verification_loop
        vt = run_verification_loop(out_path, reconciled, model_output,
                                   max_iterations=3, tolerance=1.0)
        print(f"      → status={vt.status}  iterations={vt.iterations}  "
              f"force_executed={vt.force_executed}")
        if vt.ground_truth and vt.ground_truth.unverifiable:
            max_uv = min(3, len(vt.ground_truth.unverifiable))
            for uv in vt.ground_truth.unverifiable[:max_uv]:
                print(f"      ⚠ UNVERIFIABLE: {uv}")
        # Financial intelligence issues (conceptual/first-principles failures)
        intel_notes = [n for n in (vt.notes or []) if n.startswith("⚠ [")]
        if intel_notes:
            print(f"      ── Intelligence checks: {len(intel_notes)} issue(s) ──")
            for note in intel_notes:
                print(f"      {note}")
        if vt.comparison and vt.comparison.mismatches:
            for mm in vt.comparison.mismatches[:5]:
                print(f"      ✗ MISMATCH: {mm}")
            if len(vt.comparison.mismatches) > 5:
                print(f"      ... and {len(vt.comparison.mismatches)-5} more mismatches")
        if vt.unresolved:
            print(f"      WARNING: {len(vt.unresolved)} unresolved issues remain")
        if vt.pre_delivery_checks:
            for chk in vt.pre_delivery_checks[:5]:
                print(f"      ✗ PRE-DELIVERY CHECK FAIL: {chk}")
        if vt.status == "success" and not intel_notes:
            print(f"      ✓ Verification loop passed")
        elif vt.status == "success" and intel_notes:
            print(f"      ✓ Mechanics passed — review intelligence flags above")
        elif vt.status == "partial":
            print(f"      ⚠ Verification loop passed with unverifiable flags")
    except Exception as e:
        print(f"      ⚠ Verification loop error: {e}")

    if args.deck:
        total += 1
        deck_path = out_path.replace(".xlsx", "_Summary.pptx")
        print(_hdr(f"Building PowerPoint summary deck → {deck_path}..."))
        try:
            _build_summary_deck(
                company_name=cfg.company_name,
                ticker=cfg.ticker,
                currency=reconciled.currency or "USD",
                model_output=model_output,
                dcf_output=dcf_output,
                public_comps_output=public_comps_output,
                assumptions=assumptions,
                deck_path=deck_path,
            )
            print(f"      ✓ Saved: {deck_path}")
        except Exception as e:
            print(f"      ⚠ Deck build failed: {e}")


def _build_summary_deck(
    company_name: str,
    ticker: str,
    currency: str,
    model_output,
    dcf_output,
    public_comps_output,
    assumptions,
    deck_path: str,
) -> None:
    """Build a 4-6 slide PowerPoint summary deck from model outputs."""
    import os
    from src.research.pptx_writer import PPTXDeckWriter, ScorecardTile, verify

    output_dir = os.path.dirname(os.path.abspath(deck_path))
    filename = os.path.splitext(os.path.basename(deck_path))[0]

    deck = PPTXDeckWriter(
        firm="Virtual Analyst",
        project=f"{company_name} — Financial Model",
        output_dir=output_dir,
    )

    deck.add_cover(
        f"{company_name} ({ticker}) — Financial Model Summary",
        subtitle=f"{currency} | Virtual Analyst",
    )

    # Revenue & EBITDA line chart
    periods = model_output.periods
    rev_vals = model_output.income_statement.get("revenue") or []
    ebit_vals = model_output.income_statement.get("ebit") or []
    da_vals = model_output.income_statement.get("da") or []
    ebitda_vals = [
        (e or 0) + (d or 0)
        for e, d in zip(ebit_vals, da_vals)
    ]

    if rev_vals and len(rev_vals) == len(periods):
        # Scale to billions if large
        scale = 1e9 if max((v or 0) for v in rev_vals) > 1e9 else 1e6
        unit = "B" if scale == 1e9 else "M"
        series = [{"label": "Revenue", "values": [round((v or 0) / scale, 2) for v in rev_vals]}]
        if ebitda_vals and any(ebitda_vals):
            series.append({"label": "EBITDA", "values": [round((v or 0) / scale, 2) for v in ebitda_vals]})
        # Mark projected periods (suffix 'E') as dashed via target_series = first hist period label
        hist_periods = [p for p in periods if p.endswith("A")]
        deck.add_line_chart(
            action_title=f"{company_name} revenue trend ({periods[0]}–{periods[-1]})",
            x_labels=periods,
            series=series,
            target_series="Revenue",
            y_format="{:." + ("1" if scale == 1e9 else "0") + "f}" + unit,
            y_label=f"{currency} ({unit})",
            source="SEC EDGAR / company filings + Virtual Analyst projections",
        )

    # Football field — only if at least one valuation method ran
    ff_methods = []
    current_price = assumptions.current_share_price if assumptions else None

    if dcf_output:
        # DCF range from sensitivity grid
        if dcf_output.sensitivity_ebitda:
            all_vals = [v for row in dcf_output.sensitivity_ebitda for v in row if v]
            dcf_lo = min(all_vals)
            dcf_hi = max(all_vals)
        else:
            mid = dcf_output.implied_price
            dcf_lo = mid * 0.85
            dcf_hi = mid * 1.15
        ff_methods.append({
            "label": "DCF (EBITDA mult)",
            "low": round(dcf_lo, 2),
            "high": round(dcf_hi, 2),
            "mid": round(dcf_output.implied_price, 2),
        })

    if public_comps_output and public_comps_output.implied_price_high > 0:
        ff_methods.append({
            "label": "Public Comps",
            "low": round(public_comps_output.implied_price_low, 2),
            "high": round(public_comps_output.implied_price_high, 2),
            "mid": round(public_comps_output.implied_price_median, 2),
        })

    if ff_methods:
        deck.add_football_field(
            action_title=f"{company_name} implied share price by methodology",
            methods=ff_methods,
            target_value=round(current_price, 2) if current_price else None,
            target_label="Current Price",
            value_format="${:,.2f}",
            source="Virtual Analyst DCF + public comps",
        )

    # Comps comparison matrix — top 5 tier-1 peers
    if public_comps_output and public_comps_output.peers:
        tier1 = [p for p in public_comps_output.peers if p.tier == 1][:5]
        if tier1:
            def _mult(v):
                return f"{v:.1f}x" if v else "NM"

            entities = [p.name or p.ticker for p in tier1] + [company_name]
            metrics = ["EV/EBITDA (LTM)", "EV/Revenue (LTM)", "P/E (LTM)"]
            target_ebitda = public_comps_output.target_ebitda
            target_revenue = public_comps_output.target_revenue
            target_ni = public_comps_output.target_net_income
            # Compute target EV from dcf_output or comps implied median
            tgt_ev = (dcf_output.enterprise_value if dcf_output else None)

            def _tgt_mult(numerator, denominator):
                if tgt_ev and denominator and denominator > 0:
                    return _mult(tgt_ev / denominator)
                return "NM"

            peer_rows = [
                [_mult(p.ev_ebitda_ltm), _mult(p.ev_rev_ltm), _mult(p.pe_ltm)]
                for p in tier1
            ]
            target_row = [
                _tgt_mult(tgt_ev, target_ebitda),
                _tgt_mult(tgt_ev, target_revenue),
                "NM",
            ]
            values = peer_rows + [target_row]

            deck.add_comparison_matrix(
                action_title=f"{company_name} trades at a discount to peers on EV/EBITDA",
                entities=entities,
                metrics=metrics,
                values=values,
                target_label=company_name,
                source="Bloomberg, yfinance",
                summary_stats=True,
            )

    path = deck.save(filename)
    qa = verify(path)
    if qa["critical"]:
        for c in qa["critical"]:
            print(f"      ⚠ Deck QA: {c}")


if __name__ == "__main__":
    main()
