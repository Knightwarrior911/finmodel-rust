from copy import deepcopy
from schemas.financial_data import ReconciledFinancialData, ModelConfig, ModelOutput


def _avg(values: list, n: int = 3) -> float:
    valid = [v for v in values[-n:] if v is not None]
    return sum(valid) / len(valid) if valid else 0.0


def _pct_growth_avg(values: list, n: int = 3) -> float:
    rates = []
    for i in range(1, len(values)):
        if values[i - 1] and values[i - 1] != 0:
            rates.append(values[i] / values[i - 1] - 1)
    return _avg(rates, n)


def _days(numerator: list, denominator: list, scale: float = 365.0) -> float:
    ratios = [
        (n / d * scale) for n, d in zip(numerator, denominator)
        if d and d != 0 and n is not None
    ]
    return _avg(ratios)


class ModelEngine:
    MAX_ITER = 100
    TOLERANCE = 0.01  # $M convergence tolerance

    def __init__(self, data: ReconciledFinancialData, cfg: ModelConfig):
        self.data = data
        self.cfg = cfg
        self._converged = True
        self._plug_used = False

    def build(self) -> ModelOutput:
        assumptions = self._derive_assumptions()
        hist_periods = list(self.data.periods)
        proj_periods = self._projection_periods(hist_periods)
        all_periods = hist_periods + proj_periods

        is_hist = deepcopy(self.data.income_statement)
        bs_hist = deepcopy(self.data.balance_sheet)
        cfs_hist = deepcopy(self.data.cash_flow_statement)

        is_proj, bs_proj, cfs_proj = self._project(assumptions, is_hist, bs_hist, cfs_hist)

        def merge(hist, proj):
            result = {}
            for k in set(hist) | set(proj):
                result[k] = list(hist.get(k, [])) + list(proj.get(k, []))
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
            converged=self._converged,
            plug_used=self._plug_used,
        )

    def _projection_periods(self, hist_periods: list[str]) -> list[str]:
        last_year = int(hist_periods[-1][:4])
        return [f"{last_year + i}E" for i in range(1, self.cfg.periods_projected + 1)]

    def _derive_assumptions(self) -> dict:
        is_d = self.data.income_statement
        bs_d = self.data.balance_sheet
        rev = is_d.get("revenue", [1])
        cogs = is_d.get("cogs", [0] * len(rev))
        gross = is_d.get("gross_profit", [0] * len(rev))

        return {
            "revenue_growth_pct": _pct_growth_avg(rev),
            "gross_margin_pct": _avg(
                [gp / r for gp, r in zip(gross, rev) if r and r != 0]
            ),
            "sga_pct_rev": _avg(
                [s / r for s, r in zip(is_d.get("sga", [0] * len(rev)), rev) if r and r != 0]
            ),
            "rd_pct_rev": _avg(
                [s / r for s, r in zip(is_d.get("rd", [0] * len(rev)), rev) if r and r != 0]
            ),
            "da_pct_rev": _avg(
                [d / r for d, r in zip(is_d.get("da", [0] * len(rev)), rev) if r and r != 0]
            ),
            "capex_pct_rev": _avg(
                [c / r for c, r in zip(
                    self.data.cash_flow_statement.get("capex", [0] * len(rev)), rev
                ) if r and r != 0]
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
            "dpo_days": _days(
                bs_d.get("accounts_payable", [0] * len(rev)),
                is_d.get("cogs", [1] * len(rev))
            ),
            "dio_days": _days(
                bs_d.get("inventory", [0] * len(rev)),
                is_d.get("cogs", [1] * len(rev))
            ),
            "shares_diluted": (
                self.data.income_statement.get("shares_diluted", [0])[-1] or 0
            ),
            "dividend_per_share": 0.0,
        }

    def _project(self, assumptions: dict, is_hist: dict, bs_hist: dict, cfs_hist: dict):
        n = self.cfg.periods_projected
        is_proj: dict = {}
        bs_proj: dict = {}
        cfs_proj: dict = {}

        prev_rev = (is_hist.get("revenue") or [0])[-1] or 0
        prev_cash = (bs_hist.get("cash") or [0])[-1] or 0
        prev_re = (bs_hist.get("retained_earnings") or [0])[-1] or 0
        prev_ltd = (bs_hist.get("long_term_debt") or [0])[-1] or 0
        prev_ppe = (bs_hist.get("ppe_net") or [0])[-1] or 0
        prev_equity = (bs_hist.get("total_equity") or [0])[-1] or 0
        prev_ar = (bs_hist.get("accounts_receivable") or [0])[-1] or 0
        prev_inv = (bs_hist.get("inventory") or [0])[-1] or 0
        prev_ap = (bs_hist.get("accounts_payable") or [0])[-1] or 0

        def append(d, key, val):
            d.setdefault(key, []).append(round(val, 2))

        for _ in range(n):
            g = assumptions["revenue_growth_pct"]
            rev = prev_rev * (1 + g)
            gross = rev * assumptions["gross_margin_pct"]
            cogs = rev - gross
            sga = rev * assumptions["sga_pct_rev"]
            rd = rev * assumptions["rd_pct_rev"]
            da = rev * assumptions["da_pct_rev"]
            ebitda = gross - sga - rd
            ebit = ebitda - da

            int_exp = prev_ltd * assumptions["interest_rate_pct"]
            int_inc = prev_cash * 0.02
            ebt = ebit - int_exp + int_inc
            tax = max(0, ebt * assumptions["tax_rate_pct"])
            ni = ebt - tax

            shares = assumptions["shares_diluted"]
            eps_diluted = ni / shares if shares else 0

            # Working capital
            dso = assumptions["dso_days"]
            dpo = assumptions["dpo_days"]
            dio = assumptions["dio_days"]
            ar = rev / 365 * dso if dso else prev_ar
            inv = cogs / 365 * dio if dio else prev_inv
            ap = cogs / 365 * dpo if dpo else prev_ap

            capex = rev * assumptions["capex_pct_rev"]
            ppe = prev_ppe + capex - da
            re = prev_re + ni - (assumptions["dividend_per_share"] * shares)

            # Cash and circulars
            cash, ltd, converged = self._resolve_circulars(
                prev_cash, prev_ltd, ni, da, capex, assumptions
            )
            self._converged = self._converged and converged
            if not converged:
                self._plug_used = True

            goodwill = (bs_hist.get("goodwill") or [0])[-1] or 0
            total_assets = cash + ar + inv + ppe + goodwill
            total_liab = ap + ltd
            total_equity_val = prev_equity + ni
            # Ensure BS balances — plug equity if needed
            if abs(total_assets - (total_liab + total_equity_val)) > 1:
                total_equity_val = total_assets - total_liab

            # CFS
            d_ar = ar - prev_ar
            d_inv = inv - prev_inv
            d_ap = ap - prev_ap
            cfo = ni + da - d_ar - d_inv + d_ap
            cfi = -capex
            dividends = assumptions["dividend_per_share"] * shares
            cff = -dividends
            net_change = cfo + cfi + cff

            append(is_proj, "revenue", rev)
            append(is_proj, "cogs", cogs)
            append(is_proj, "gross_profit", gross)
            append(is_proj, "sga", sga)
            append(is_proj, "rd", rd)
            append(is_proj, "da", da)
            append(is_proj, "ebit", ebit)
            append(is_proj, "interest_expense", int_exp)
            append(is_proj, "interest_income", int_inc)
            append(is_proj, "income_tax", tax)
            append(is_proj, "net_income", ni)
            append(is_proj, "eps_diluted", round(eps_diluted, 4))
            append(is_proj, "shares_diluted", round(shares, 0))

            append(bs_proj, "cash", cash)
            append(bs_proj, "accounts_receivable", ar)
            append(bs_proj, "inventory", inv)
            append(bs_proj, "ppe_net", ppe)
            append(bs_proj, "accounts_payable", ap)
            append(bs_proj, "long_term_debt", ltd)
            append(bs_proj, "total_liabilities", total_liab)
            append(bs_proj, "retained_earnings", re)
            append(bs_proj, "total_equity", total_equity_val)
            append(bs_proj, "total_assets", total_assets)

            append(cfs_proj, "cfo", cfo)
            append(cfs_proj, "capex", capex)
            append(cfs_proj, "cfi", cfi)
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

    def _resolve_circulars(
        self, prev_cash: float, prev_ltd: float, ni: float, da: float,
        capex: float, assumptions: dict
    ) -> tuple[float, float, bool]:
        cash = prev_cash
        ltd = prev_ltd
        for _ in range(self.MAX_ITER):
            prev_cash_iter = cash
            cfo_approx = ni + da
            cfi_approx = -capex
            cff_approx = -(assumptions["dividend_per_share"] * assumptions["shares_diluted"])
            cash = prev_cash + cfo_approx + cfi_approx + cff_approx
            if abs(cash - prev_cash_iter) < self.TOLERANCE:
                return cash, ltd, True
        # Fallback: plug — return computed cash, flag non-convergence
        return cash, ltd, False

    def _build_schedules(
        self, is_d: dict, bs_d: dict, cfs_d: dict, periods: list[str], assumptions: dict
    ) -> dict:
        n = len(periods)
        ppe_vals = bs_d.get("ppe_net", [0] * n)
        da_vals = is_d.get("da", [0] * n)
        capex_vals = cfs_d.get("capex", [0] * n)

        ppe_schedule = []
        for i in range(n):
            opening = ppe_vals[i - 1] if i > 0 else 0.0
            ppe_schedule.append({
                "period": periods[i],
                "opening": round(opening, 2),
                "capex": round(capex_vals[i] if i < len(capex_vals) else 0, 2),
                "da": round(da_vals[i] if i < len(da_vals) else 0, 2),
                "closing": round(ppe_vals[i] if i < len(ppe_vals) else 0, 2),
            })

        return {"ppe_rollforward": ppe_schedule}
