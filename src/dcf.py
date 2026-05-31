"""
DCF valuation engine (per dcf/SPEC_methodology).

UFCF = EBIT × (1 - t) + D&A − CapEx − ΔNWC for each projected year.
Discount factor:  mid-year:  1 / (1 + WACC)^(t - 0.5)
                  year-end:  1 / (1 + WACC)^t
Terminal value — computes BOTH methods side-by-side:
  Gordon Growth:    TV = UFCF_N × (1 + g) / (WACC - g)
  Exit Multiple:    TV = EBITDA_N × Exit_Multiple
Equity bridge:  EV − Debt − Preferred − NCI + Cash + Investments
Implied per-share = Equity Value / Diluted Shares
Cross-checks: TV/EV %, WACC-g, implied multiple, implied perpetuity g
"""
import logging

logger = logging.getLogger(__name__)


def flag_ev_bridge_gaps(ledger, *, preferred: float, investments: float,
                        preferred_from_filing: bool = False,
                        investments_from_filing: bool = False) -> None:
    """Record EV-bridge items. When a value came from the balance sheet, tag it
    FILING; otherwise it is a schema gap assumed 0 -> UNVERIFIED (so the audit
    pass flags it red)."""
    if ledger is None:
        return
    if preferred_from_filing:
        ledger.record_filing("dcf", "preferred_stock", None, value=preferred,
                             provenance={"note": "balance sheet"})
    else:
        ledger.record_unverified("dcf", "preferred_stock", None, value=preferred,
                                 reason="preferred stock not in extraction schema (assumed 0)")
    if investments_from_filing:
        ledger.record_filing("dcf", "investments", None, value=investments,
                             provenance={"note": "balance sheet"})
    else:
        ledger.record_unverified("dcf", "investments", None, value=investments,
                                 reason="short-term investments not in extraction schema (assumed 0)")


def compute_dcf(
    output,                          # ModelOutput
    ticker: str,
    wacc_output,                     # WACCOutput
    assumptions_block,               # AssumptionsBlock
    tv_method: int = 1,              # 1 = EBITDA Multiple primary, 2 = Gordon primary
    ledger=None,                     # SourceLedger | None
):
    from schemas.financial_data import DCFOutput

    periods = output.periods
    n_hist = sum(1 for p in periods if p.endswith("A"))
    proj_periods = [p for p in periods if p.endswith("E")]
    n_proj = len(proj_periods)
    n_all = len(periods)
    mid_year = assumptions_block.mid_year_convention

    def _get(section: dict, key: str) -> list:
        v = section.get(key) or []
        # Pad missing values with 0; normal hist+proj merged dicts have full length
        return (list(v) + [0.0] * n_all)[:n_all]

    # ── Pull WACC from WACCOutput ─────────────────────────────────────────────
    wacc       = wacc_output.wacc
    tax_rate   = wacc_output.tax_rate
    cases = {1: assumptions_block.base, 2: assumptions_block.upside, 3: assumptions_block.downside}
    s = cases.get(assumptions_block.active_case, assumptions_block.base)
    terminal_g = s.terminal_growth_rate
    exit_mult  = s.exit_ebitda_multiple

    # ── Unlevered FCF (FCFF = NOPAT + D&A − CapEx − ΔNWC) ───────────────────
    ebit_all = _get(output.income_statement,    "ebit")
    da_all   = _get(output.income_statement,    "da")
    cap_all  = _get(output.cash_flow_statement, "capex")
    ar_all   = _get(output.balance_sheet,       "accounts_receivable")
    inv_all  = _get(output.balance_sheet,       "inventory")
    ap_all   = _get(output.balance_sheet,       "accounts_payable")

    def _nwc(idx: int) -> float:
        return (ar_all[idx] or 0.0) + (inv_all[idx] or 0.0) - (ap_all[idx] or 0.0)

    fcff_proj: list[float] = []
    dwc_proj:  list[float] = []
    for i in range(n_proj):
        t     = n_hist + i
        nopat = (ebit_all[t] or 0.0) * (1 - tax_rate)
        da    = da_all[t]  or 0.0
        capex = cap_all[t] or 0.0
        dnwc  = _nwc(t) - _nwc(t - 1) if t > 0 else 0.0
        dwc_proj.append(round(dnwc, 2))
        fcff_proj.append(round(nopat + da - capex - dnwc, 2))

    # ── Discount factors (mid-year or year-end) ───────────────────────────────
    discount_factors = []
    for i in range(n_proj):
        t = (i + 1) - 0.5 if mid_year else (i + 1)
        discount_factors.append(round(1 / (1 + wacc) ** t, 6))

    pv_fcfs_per_period = [round(f * df, 2) for f, df in zip(fcff_proj, discount_factors)]
    pv_fcfs = round(sum(pv_fcfs_per_period), 2)

    # ── Terminal value (BOTH methods) ─────────────────────────────────────────
    # Use last projected EBIT + D&A to derive terminal EBITDA (avoids missing-key issue)
    terminal_ebit = ebit_all[-1] or 0.0
    terminal_da   = da_all[-1] or 0.0
    terminal_ebitda = terminal_ebit + terminal_da
    last_fcff       = fcff_proj[-1] if fcff_proj else 0.0

    tv_ebitda = terminal_ebitda * exit_mult
    tv_gordon = ((last_fcff * (1 + terminal_g)) / (wacc - terminal_g)
                 if wacc > terminal_g else 0.0)

    # PV of TV uses same discount basis as last projected period
    n_terminal = (n_proj - 0.5) if mid_year else n_proj
    df_terminal = 1 / (1 + wacc) ** n_terminal
    tv_ebitda_pv = round(tv_ebitda * df_terminal, 2)
    tv_gordon_pv = round(tv_gordon * df_terminal, 2)

    tv_selected = tv_ebitda if tv_method == 1 else tv_gordon
    pv_tv       = tv_ebitda_pv if tv_method == 1 else tv_gordon_pv

    # ── Equity bridge ─────────────────────────────────────────────────────────
    cash_all = _get(output.balance_sheet, "cash")
    ltd_all  = _get(output.balance_sheet, "long_term_debt")
    last_cash = cash_all[-1] or 0.0
    last_debt = ltd_all[-1] or 0.0
    # Preferred + NCI: consume from BS when present, else default 0
    pref_arr = output.balance_sheet.get("preferred_stock")
    preferred = (pref_arr or [0.0])[-1] or 0.0
    nci_balance = (output.balance_sheet.get("redeemable_nci") or [0.0])[-1] or 0.0
    inv_arr = output.balance_sheet.get("short_term_investments")
    investments = (inv_arr or [0.0])[-1] or 0.0
    flag_ev_bridge_gaps(ledger, preferred=preferred, investments=investments,
                        preferred_from_filing=pref_arr is not None,
                        investments_from_filing=inv_arr is not None)

    enterprise_value = round(pv_fcfs + pv_tv, 2)
    net_debt = last_debt - last_cash + preferred + nci_balance - investments
    equity_value = round(enterprise_value - net_debt, 2)

    shares_all = _get(output.income_statement, "shares_diluted")
    shares = shares_all[-1] or assumptions_block.shares_diluted or 0.0
    # equity_value in $M, shares in millions (fetcher ÷1e6) → price = equity_value / shares
    implied_price = round(equity_value / shares, 2) if shares else 0.0

    # ── Cross-checks ──────────────────────────────────────────────────────────
    current_px = assumptions_block.current_share_price
    upside_pct = round(implied_price / current_px - 1, 4) if current_px else 0.0
    tv_pct_ev = round(pv_tv / enterprise_value, 4) if enterprise_value else 0.0
    wacc_minus_g = round(wacc - terminal_g, 4)
    # Implied exit multiple from Gordon TV:  exit_mult_implied = TV_gordon / terminal_EBITDA
    implied_exit = round(tv_gordon / terminal_ebitda, 2) if terminal_ebitda else 0.0
    # Implied perpetuity g from exit multiple:  solve TV_ebitda = FCFF_N+1 / (WACC - g)
    if tv_ebitda > 0 and last_fcff != 0:
        implied_g = round(wacc - (last_fcff * (1 + terminal_g)) / tv_ebitda, 4)
    else:
        implied_g = 0.0

    # ── Sensitivity grids (for legacy single-tab; full Sensitivities tab in P4.6) ───
    wacc_range = [round(wacc - 0.01 + i * 0.005, 4) for i in range(5)]
    ebitda_mult_range = [exit_mult - 4, exit_mult - 2, exit_mult, exit_mult + 2, exit_mult + 4]
    gordon_growth_range = [round(terminal_g - 0.010 + i * 0.005, 4) for i in range(5)]

    def _implied_price_at(w: float, tv_val: float) -> float:
        n_t = (n_proj - 0.5) if mid_year else n_proj
        pv_f = sum(f / (1 + w) ** ((i + 1) - 0.5 if mid_year else (i + 1))
                   for i, f in enumerate(fcff_proj))
        pv_t = tv_val / (1 + w) ** n_t
        eq = pv_f + pv_t - net_debt
        return round(eq / shares, 2) if shares else 0.0

    sensitivity_ebitda = [
        [_implied_price_at(w, terminal_ebitda * mult) for mult in ebitda_mult_range]
        for w in wacc_range
    ]
    sensitivity_gordon = [
        [
            _implied_price_at(w, (last_fcff * (1 + g)) / (w - g) if w > g else 0.0)
            for g in gordon_growth_range
        ]
        for w in wacc_range
    ]

    return DCFOutput(
        ticker=ticker,
        mid_year_convention=mid_year,
        beta=wacc_output.target_levered_beta,
        risk_free_rate=wacc_output.risk_free_rate,
        equity_risk_premium=wacc_output.equity_risk_premium,
        cost_of_equity=wacc_output.cost_of_equity,
        cost_of_debt_pretax=wacc_output.cost_of_debt_pretax,
        tax_rate=tax_rate,
        after_tax_cost_of_debt=wacc_output.after_tax_cost_of_debt,
        equity_weight=wacc_output.equity_weight,
        debt_weight=wacc_output.debt_weight,
        wacc=wacc,
        proj_periods=proj_periods,
        fcff_proj=fcff_proj,
        dwc_proj=dwc_proj,
        discount_factors=discount_factors,
        pv_fcfs_per_period=pv_fcfs_per_period,
        pv_fcfs=pv_fcfs,
        terminal_ebitda=terminal_ebitda,
        tv_ebitda_multiple=exit_mult,
        tv_ebitda=tv_ebitda,
        tv_ebitda_pv=tv_ebitda_pv,
        tv_growth_rate=terminal_g,
        tv_gordon=tv_gordon,
        tv_gordon_pv=tv_gordon_pv,
        tv_method=tv_method,
        tv_selected=tv_selected,
        pv_tv=pv_tv,
        enterprise_value=enterprise_value,
        total_debt=last_debt,
        preferred_stock=preferred,
        noncontrolling_interest=nci_balance,
        cash=last_cash,
        investments=investments,
        net_debt=net_debt,
        equity_value=equity_value,
        shares_diluted=shares,
        implied_price=implied_price,
        current_share_price=current_px,
        upside_downside_pct=upside_pct,
        tv_pct_of_ev=tv_pct_ev,
        wacc_minus_g=wacc_minus_g,
        implied_exit_mult_from_gordon=implied_exit,
        implied_g_from_exit_mult=implied_g,
        wacc_range=wacc_range,
        ebitda_multiple_range=ebitda_mult_range,
        gordon_growth_range=gordon_growth_range,
        sensitivity_ebitda=sensitivity_ebitda,
        sensitivity_gordon=sensitivity_gordon,
    )
