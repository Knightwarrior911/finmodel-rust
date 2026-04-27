from copy import deepcopy
from schemas.financial_data import (
    ReconciledFinancialData, ModelConfig, ModelOutput, AssumptionsBlock,
)


def _at(assumptions: dict, key: str, i: int):
    """Pull per-period value for index i; falls back to scalar if value isn't a list."""
    v = assumptions.get(key)
    if isinstance(v, list):
        if not v:
            return 0
        return v[i] if i < len(v) else v[-1]
    return v


def flatten_active_scenario(asmp: AssumptionsBlock) -> dict:
    """Convert AssumptionsBlock active scenario into per-period dict for engine consumption."""
    cases = {1: asmp.base, 2: asmp.upside, 3: asmp.downside}
    s = cases.get(asmp.active_case, asmp.base)
    result = {
        "revenue_growth_pct":  list(s.revenue_growth_pct),
        "gross_margin_pct":    list(s.gross_margin_pct),
        "sga_pct_rev":         list(s.sga_pct_rev),
        "rd_pct_rev":          list(s.rd_pct_rev),
        "da_pct_rev":          list(s.da_pct_rev),
        "capex_pct_rev":       list(s.capex_pct_rev),
        "tax_rate_pct":        list(s.tax_rate_pct),
        "interest_rate_pct":   list(s.interest_rate_pct),
        "dso_days":            list(s.dso_days),
        "dio_days":            list(s.dio_days),
        "dpo_days":            list(s.dpo_days),
        "dividend_per_share":  list(s.dividend_per_share),
        "terminal_growth_rate": s.terminal_growth_rate,
        "exit_ebitda_multiple": s.exit_ebitda_multiple,
        "shares_diluted":      asmp.shares_diluted,
    }
    # Per-segment growth drivers: all default to base revenue_growth_pct
    seg_drivers = getattr(asmp, 'revenue_segments', None)
    if seg_drivers:
        base_rg = list(s.revenue_growth_pct)
        for seg in seg_drivers:
            dk = f"{seg['key']}_growth_pct"
            result[dk] = base_rg
    return result


def _avg(values: list[float], n: int = 3) -> float:
    valid = [v for v in values[-n:] if v is not None]
    return sum(valid) / len(valid) if valid else 0.0


def _pct_growth_avg(values: list[float], n: int = 3) -> float:
    rates = []
    for i in range(1, len(values)):
        if values[i - 1] and values[i - 1] != 0:
            rates.append(values[i] / values[i - 1] - 1)
    return _avg(rates, n)


def _days(numerator: list[float], denominator: list[float], scale: float = 365.0) -> float:
    ratios = [
        (n / d * scale) for n, d in zip(numerator, denominator)
        if d and d != 0 and n is not None
    ]
    return _avg(ratios)


class ModelEngine:
    def __init__(self, data: ReconciledFinancialData, cfg: ModelConfig,
                 assumptions_block: AssumptionsBlock | None = None):
        self.data = data
        self.cfg = cfg
        self.asmp_block = assumptions_block

    def build(self) -> ModelOutput:
        if self.asmp_block is not None:
            assumptions = flatten_active_scenario(self.asmp_block)
            # Carry shares_diluted from historical if block has 0 (unset)
            if not assumptions.get("shares_diluted"):
                assumptions["shares_diluted"] = (
                    self.data.income_statement.get("shares_diluted", [0])[-1] or 0
                )
        else:
            assumptions = self._derive_assumptions()
        hist_periods = list(self.data.periods)
        proj_periods = self._projection_periods(hist_periods)
        all_periods = hist_periods + proj_periods

        is_hist = deepcopy(self.data.income_statement)
        bs_hist = deepcopy(self.data.balance_sheet)
        cfs_hist = deepcopy(self.data.cash_flow_statement)

        is_proj, bs_proj, cfs_proj = self._project(assumptions, is_hist, bs_hist, cfs_hist)

        n_hist = len(hist_periods)
        def merge(hist, proj):
            result = {}
            for k in set(hist) | set(proj):
                h_vals = list(hist.get(k, []))
                if len(h_vals) < n_hist:
                    h_vals = h_vals + [None] * (n_hist - len(h_vals))
                result[k] = h_vals + list(proj.get(k, []))
            return result

        schedules = self._build_schedules(
            merge(is_hist, is_proj), merge(bs_hist, bs_proj),
            merge(cfs_hist, cfs_proj), all_periods, assumptions
        )

        return ModelOutput(
            periods=all_periods,
            income_statement=merge(is_hist, is_proj),
            balance_sheet=merge(bs_hist, bs_proj),
            cash_flow_statement=merge(cfs_hist, cfs_proj),
            schedules=schedules,
            assumptions=assumptions,
            converged=True,
            plug_used=False,
        )

    def _projection_periods(self, hist_periods: list[str]) -> list[str]:
        try:
            last_year = int(hist_periods[-1][:4])
        except (ValueError, IndexError):
            raise ValueError(
                f"Cannot parse year from period label '{hist_periods[-1]}'. "
                "Expected format: 'YYYYA' (e.g., '2023A')"
            )
        return [f"{last_year + i}E" for i in range(1, self.cfg.periods_projected + 1)]

    def _derive_assumptions(self) -> dict:
        is_d = self.data.income_statement
        bs_d = self.data.balance_sheet
        rev = is_d.get("revenue", [1])
        cogs_raw = is_d.get("cogs") or []
        cogs = cogs_raw if cogs_raw else [0] * len(rev)
        gross = is_d.get("gross_profit", [0] * len(rev))

        # Guard: DIO/DPO require a meaningful COGS denominator.
        # If COGS is absent or all-zero (utility, bank, etc.), pass empty list so
        # _days() returns 0 rather than dividing by the [1]*n fallback and producing
        # nonsensical day counts in the hundreds of thousands.
        has_cogs = any(v and v != 0 for v in cogs)
        cogs_for_days = cogs if has_cogs else []

        sector = getattr(self.cfg, 'sector', 'standard')
        is_utility = sector == 'utility'
        is_bank    = sector == 'bank'

        if is_utility:
            # Derive actual utility cost-line drivers from XBRL-fetched items.
            # gross_margin_pct slot  → O&M % revenue
            # sga_pct_rev slot       → Taxes other than income % revenue
            # rd_pct_rev slot        → Other opex % revenue (residual: not in XBRL)
            ebit_vals = is_d.get("ebit", [])
            da_vals   = is_d.get("da",   [])
            om_vals   = is_d.get("utility_om",          []) or [0] * len(rev)
            tx_vals   = is_d.get("utility_taxes_other", []) or [0] * len(rev)
            fu_vals   = is_d.get("utility_fuel",        []) or [0] * len(rev)

            om_pct = _avg([o / r for o, r in zip(om_vals, rev) if r and o])
            tx_pct = _avg([t / r for t, r in zip(tx_vals, rev) if r and t])
            fu_pct = _avg([f / r for f, r in zip(fu_vals, rev) if r and f])

            # Residual = Revenue − EBIT − O&M − D&A − TaxesOther − Fuel
            # Captures anything XBRL doesn't break out (e.g. NEE's fuel/purchased power)
            if ebit_vals and da_vals:
                other_vals = [
                    r - e - (o or 0) - (d or 0) - (t or 0) - (f or 0)
                    for r, e, o, d, t, f in zip(
                        rev, ebit_vals, om_vals, da_vals, tx_vals, fu_vals)
                    if r and e is not None
                ]
                other_pct = _avg([v / r for v, r in zip(other_vals, rev) if r and v > 0])
            else:
                other_pct = 0.0

            gross_margin = om_pct      # repurposed slot: O&M %
            sga_pct      = tx_pct      # repurposed slot: taxes other %
            rd_pct       = other_pct   # repurposed slot: other opex %
        else:
            # Gross margin: 0 when no COGS (utility/bank) — don't use as a driver
            gross_margin = _avg(
                [gp / r for gp, r in zip(gross, rev) if r and r != 0]
            ) if has_cogs else 0.0
            sga_pct = _avg(
                [s / r for s, r in zip(is_d.get("sga", [0] * len(rev)), rev) if r and r != 0]
            )
            rd_pct = _avg(
                [s / r for s, r in zip(is_d.get("rd", [0] * len(rev)), rev) if r and r != 0]
            )

        dio = _days(bs_d.get("inventory", [0] * len(rev)), cogs_for_days)
        dpo = _days(bs_d.get("accounts_payable", [0] * len(rev)), cogs_for_days)

        # Sanity cap: > 365 days means the denominator was wrong; reset to 0
        if dio > 365:
            dio = 0.0
        if dpo > 365:
            dpo = 0.0

        base_rev_growth = _pct_growth_avg(rev)
        result = {
            "revenue_growth_pct": base_rev_growth,
            "gross_margin_pct": gross_margin,
            "sga_pct_rev": sga_pct,
            "rd_pct_rev":  rd_pct,
            "da_pct_rev": _avg(
                [d / r for d, r in zip(is_d.get("da", [0] * len(rev)), rev) if r and r != 0]
            ),
            "capex_pct_rev": _avg(
                [c / r for c, r in zip(
                    self.data.cash_flow_statement.get("capex", [0] * len(rev)), rev
                ) if r and r != 0 and c is not None]
            ),
            "tax_rate_pct": _avg([
                t / (ni + t)
                for t, ni in zip(
                    is_d.get("income_tax", [1] * len(rev)),
                    is_d.get("net_income", [5] * len(rev)),
                )
                if (ni + t) != 0
            ]),
            "interest_rate_pct": 0.035,
            "dso_days": _days(
                bs_d.get("accounts_receivable", [0] * len(rev)), rev
            ),
            "dpo_days": dpo,
            "dio_days": dio,
            "shares_diluted": (
                self.data.income_statement.get("shares_diluted", [0])[-1] or 0
            ),
            "dividend_per_share": _avg([
                (d or 0) / (s or 1)
                for d, s in zip(
                    self.data.cash_flow_statement.get("dividends_paid", [0] * len(rev)),
                    is_d.get("shares_diluted", [0] * len(rev)),
                )
                if s and s != 0
            ]),
        }
        # Per-segment growth drivers default to same as base revenue growth
        for seg in (getattr(self.cfg, 'revenue_segments', []) or []):
            dk = f"{seg['key']}_growth_pct"
            result[dk] = base_rev_growth
        return result

    def _project(self, assumptions: dict, is_hist: dict, bs_hist: dict, cfs_hist: dict):
        n = self.cfg.periods_projected
        is_proj: dict = {}
        bs_proj: dict = {}
        cfs_proj: dict = {}

        prev_rev    = (is_hist.get("revenue") or [0])[-1] or 0
        prev_cash   = (bs_hist.get("cash") or [0])[-1] or 0
        prev_re     = (bs_hist.get("retained_earnings") or [0])[-1] or 0
        prev_ltd    = (bs_hist.get("long_term_debt") or [0])[-1] or 0
        prev_ppe    = (bs_hist.get("ppe_net") or [0])[-1] or 0
        prev_equity = (bs_hist.get("total_equity") or [0])[-1] or 0
        prev_ar     = (bs_hist.get("accounts_receivable") or [0])[-1] or 0
        prev_inv    = (bs_hist.get("inventory") or [0])[-1] or 0
        prev_ap     = (bs_hist.get("accounts_payable") or [0])[-1] or 0
        last_nci    = (is_hist.get("nci_income_loss") or [0])[-1] or 0
        last_def_cur= (bs_hist.get("deferred_revenue_current") or [0])[-1] or 0
        last_def_lt = (bs_hist.get("deferred_revenue_lt") or [0])[-1] or 0
        last_rnci   = (bs_hist.get("redeemable_nci") or [0])[-1] or 0

        revenue_segments = getattr(self.cfg, 'revenue_segments', []) or []
        prev_seg: dict[str, float] = {}
        if revenue_segments:
            for seg in revenue_segments:
                sk = seg["key"]
                seg_hist = is_hist.get(sk, [])
                prev_seg[sk] = (seg_hist[-1] or 0) if seg_hist else 0

        # Extra opex items (beyond primary cogs/rd/sga) held flat
        extra_opex_keys = getattr(self.cfg, 'extra_opex_keys', []) or []
        prev_flat_opex: dict[str, float] = {}
        for k in extra_opex_keys:
            hist_vals = is_hist.get(k, [0])
            prev_flat_opex[k] = (hist_vals[-1] or 0) if hist_vals else 0

        def append(d, key, val):
            d.setdefault(key, []).append(round(val, 2))

        sector = getattr(self.cfg, 'sector', 'standard')
        is_utility_proj = sector == 'utility'
        is_bank_proj     = sector == 'bank'

        for i in range(n):
            if revenue_segments:
                rev = 0.0
                for seg in revenue_segments:
                    sk = seg["key"]
                    dk = f"{sk}_growth_pct"
                    g = _at(assumptions, dk, i)
                    seg_val = prev_seg[sk] * (1 + g)
                    append(is_proj, sk, round(seg_val, 2))
                    prev_seg[sk] = seg_val
                    rev += seg_val
                rev = round(rev, 2)
            else:
                g = _at(assumptions, "revenue_growth_pct", i)
                rev = prev_rev * (1 + g)
            da = rev * _at(assumptions, "da_pct_rev", i)
            if is_utility_proj:
                om         = rev * _at(assumptions, "gross_margin_pct", i)
                taxes_other = rev * _at(assumptions, "sga_pct_rev", i)
                other_opex  = rev * _at(assumptions, "rd_pct_rev", i)
                gross = 0.0
                cogs  = 0.0
                sga   = 0.0
                rd    = 0.0
                ebit  = rev - om - da - taxes_other - other_opex
            elif is_bank_proj:
                sga = rev * _at(assumptions, "sga_pct_rev", i)          # Non-Interest Exp
                rd  = rev * _at(assumptions, "rd_pct_rev", i)           # Provision
                gross = rev * _at(assumptions, "gross_margin_pct", i)   # NII
                cogs = rev - gross                                      # Interest Exp
                da_bank = rev * _at(assumptions, "da_pct_rev", i)
                # Bank EBIT = NII - Non-Interest Exp - Provision (D&A inside Non-Int Exp)
                ebit = gross - sga - rd
                da = da_bank  # Store for EBITDA add-back
            else:
                sga = rev * _at(assumptions, "sga_pct_rev", i)
                rd = rev * _at(assumptions, "rd_pct_rev", i)
                gross = rev * _at(assumptions, "gross_margin_pct", i)
                cogs = rev - gross
                # Extra opex items held flat; subtract from EBIT
                extra_opex_total = 0.0
                for k in extra_opex_keys:
                    append(is_proj, k, round(prev_flat_opex[k], 2))
                    extra_opex_total += prev_flat_opex[k]
                ebit = gross - sga - rd - da - extra_opex_total
            ebitda = ebit + da                 # EBITDA = EBIT + D&A add-back

            int_exp = prev_ltd * _at(assumptions, "interest_rate_pct", i)
            int_inc = prev_cash * 0.02
            ebt = ebit - int_exp + int_inc
            tax = max(0, ebt * _at(assumptions, "tax_rate_pct", i))
            ni = ebt - tax
            nci = last_nci           # NCI income held flat at last historical
            ni_common = ni - nci

            shares = assumptions["shares_diluted"]
            eps_diluted = ni_common / shares if shares else 0

            # Working capital
            dso = _at(assumptions, "dso_days", i)
            dpo = _at(assumptions, "dpo_days", i)
            dio = _at(assumptions, "dio_days", i)
            ar = rev / 365 * dso if dso else prev_ar
            # For utility/bank sectors: no COGS → use prev values (DIO/DPO=0 already)
            inv = (cogs / 365 * dio if (cogs and dio) else prev_inv)
            ap = (cogs / 365 * dpo if (cogs and dpo) else prev_ap)

            capex = rev * _at(assumptions, "capex_pct_rev", i)
            ppe = prev_ppe + capex - da
            div_per_share = _at(assumptions, "dividend_per_share", i) or 0
            re = prev_re + ni_common - (div_per_share * shares)

            # Cash derived directly from CFS (lagged int_inc avoids circular)
            ltd = prev_ltd
            d_ar = ar - prev_ar
            d_inv = inv - prev_inv
            d_ap = ap - prev_ap
            cfo = ni + da - d_ar - d_inv + d_ap
            cfi = -capex
            dividends = div_per_share * shares
            cff = -dividends
            net_change = cfo + cfi + cff
            cash = prev_cash + net_change

            goodwill = (bs_hist.get("goodwill") or [0])[-1] or 0
            # Hold non-modeled BS items flat at last historical value (preserves balance)
            intangibles    = (bs_hist.get("intangibles_net") or [0])[-1] or 0
            other_assets_hist = (
                ((bs_hist.get("total_assets") or [0])[-1] or 0)
                - ((bs_hist.get("cash") or [0])[-1] or 0)
                - ((bs_hist.get("accounts_receivable") or [0])[-1] or 0)
                - ((bs_hist.get("inventory") or [0])[-1] or 0)
                - ((bs_hist.get("ppe_net") or [0])[-1] or 0)
                - ((bs_hist.get("goodwill") or [0])[-1] or 0)
                - ((bs_hist.get("intangibles_net") or [0])[-1] or 0)
            )
            # other_assets computed as plug below (A = L + E enforced first)

            # Liabilities: AP + LTD + non-modeled liabilities held flat
            other_liab_hist = (
                ((bs_hist.get("total_liabilities") or [0])[-1] or 0)
                - ((bs_hist.get("accounts_payable") or [0])[-1] or 0)
                - ((bs_hist.get("long_term_debt") or [0])[-1] or 0)
            )
            total_liab = ap + ltd + other_liab_hist
            # Equity rolls forward via NI (− dividends).
            total_equity_val = prev_equity + ni - dividends
            # Enforce A = L + E (round sub-cent drift, plug via other_assets_hist)
            total_assets = round(total_liab + total_equity_val, 2)

            append(is_proj, "revenue", rev)
            append(is_proj, "cogs", cogs)
            append(is_proj, "gross_profit", gross)
            append(is_proj, "sga", sga)
            append(is_proj, "rd", rd)
            append(is_proj, "da", da)
            append(is_proj, "ebitda", ebitda)
            append(is_proj, "ebit", ebit)
            append(is_proj, "interest_expense", int_exp)
            append(is_proj, "interest_income", int_inc)
            append(is_proj, "income_tax", tax)
            append(is_proj, "net_income", ni)
            append(is_proj, "nci_income_loss", nci)
            append(is_proj, "ni_common", ni_common)
            append(is_proj, "eps_diluted", round(eps_diluted, 4))
            append(is_proj, "shares_diluted", round(shares, 0))

            total_current_assets = cash + ar + inv
            total_current_liabilities = ap

            append(bs_proj, "cash", cash)
            append(bs_proj, "accounts_receivable", ar)
            append(bs_proj, "inventory", inv)
            append(bs_proj, "total_current_assets", total_current_assets)
            append(bs_proj, "ppe_net", ppe)
            append(bs_proj, "goodwill", goodwill)
            append(bs_proj, "accounts_payable", ap)
            append(bs_proj, "total_current_liabilities", total_current_liabilities)
            append(bs_proj, "deferred_revenue_current", last_def_cur)
            append(bs_proj, "deferred_revenue_lt", last_def_lt)
            append(bs_proj, "redeemable_nci", last_rnci)
            append(bs_proj, "long_term_debt", ltd)
            append(bs_proj, "total_liabilities", total_liab)
            append(bs_proj, "retained_earnings", re)
            append(bs_proj, "total_equity", total_equity_val)
            append(bs_proj, "total_assets", total_assets)

            append(cfs_proj, "cfo", cfo)
            append(cfs_proj, "capex", capex)
            append(cfs_proj, "investments_net_cfi", 0.0)
            append(cfs_proj, "cfi", cfi)
            # Components of CFF — divs as positive outflow magnitude, buybacks/other 0
            append(cfs_proj, "dividends_paid", dividends)
            append(cfs_proj, "buybacks", 0.0)
            append(cfs_proj, "other_cff", 0.0)
            append(cfs_proj, "cff", cff)
            append(cfs_proj, "net_change_cash", net_change)

            prev_rev = rev
            prev_cash = cash
            prev_re = re
            prev_ltd = ltd
            prev_ppe = ppe
            prev_equity = total_equity_val
            prev_ar = ar
            prev_inv = inv
            prev_ap = ap

        return is_proj, bs_proj, cfs_proj

    def _build_schedules(
        self, is_d: dict, bs_d: dict, cfs_d: dict, periods: list[str], assumptions: dict
    ) -> dict:
        n = len(periods)
        ppe_vals = bs_d.get("ppe_net", [0] * n)
        da_vals = is_d.get("da", [0] * n)
        capex_vals = cfs_d.get("capex", [0] * n)

        ppe_schedule = []
        for i in range(1, n):  # start at 1, opening is always ppe_vals[i-1]
            ppe_schedule.append({
                "period": periods[i],
                "opening": round(ppe_vals[i - 1] if i - 1 < len(ppe_vals) and ppe_vals[i - 1] is not None else 0, 2),
                "capex": round(capex_vals[i] if i < len(capex_vals) and capex_vals[i] is not None else 0, 2),
                "da": round(da_vals[i] if i < len(da_vals) and da_vals[i] is not None else 0, 2),
                "closing": round(ppe_vals[i] if i < len(ppe_vals) and ppe_vals[i] is not None else 0, 2),
            })

        return {"ppe_rollforward": ppe_schedule}
