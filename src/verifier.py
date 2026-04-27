from schemas.financial_data import ModelOutput, VerificationReport

BS_TOLERANCE = 1.0   # $1M tolerance for balance sheet check
CFS_TOLERANCE = 1.0
CFS_WARN_PCT = 0.001    # 0.1% of total activity → warning
CFS_CRITICAL_PCT = 0.01  # 1.0% of total activity → critical failure


def verify(output: ModelOutput, sector: str = "standard") -> VerificationReport:
    critical: list[str] = []
    warnings: list[str] = []
    notes: list[str] = []
    period_checks: dict = {}
    is_utility = sector in ('utility', 'bank', 'reit', 'insurance')

    bs = output.balance_sheet
    is_d = output.income_statement
    cfs = output.cash_flow_statement

    for i, period in enumerate(output.periods):
        checks = {}

        # CRITICAL: BS balance (Assets = Liabilities + Equity + Redeemable NCI)
        assets = _get(bs, "total_assets", i)
        liab = _get(bs, "total_liabilities", i)
        equity = _get(bs, "total_equity", i)
        rnci = _get(bs, "redeemable_nci", i) or 0.0
        if all(v is not None for v in [assets, liab, equity]):
            diff = abs(assets - (liab + equity + rnci))
            checks["bs_balance"] = diff <= BS_TOLERANCE
            if diff > BS_TOLERANCE:
                critical.append(
                    f"Balance sheet mismatch {period}: assets={assets:.0f}, L+E+RNCI={liab+equity+rnci:.0f}, diff={diff:.0f}"
                )

        # CRITICAL: CFS cash tie
        # Full equation: net_change = CFO + CFI + CFF + FX effect
        cfo = _get(cfs, "cfo", i)
        cfi = _get(cfs, "cfi", i)
        cff = _get(cfs, "cff", i)
        net_change = _get(cfs, "net_change_cash", i)
        fx = _get(cfs, "fx_effect_on_cash", i) or 0.0
        if all(v is not None for v in [cfo, cfi, cff, net_change]):
            computed = cfo + cfi + cff + fx
            diff = abs(computed - net_change)
            total_activity = abs(cfo) + abs(cfi) + abs(cff)
            warn_tol = max(CFS_TOLERANCE, total_activity * CFS_WARN_PCT)
            crit_tol = max(CFS_TOLERANCE, total_activity * CFS_CRITICAL_PCT)
            checks["cfs_tie"] = diff <= warn_tol
            if diff > crit_tol:
                critical.append(
                    f"CFS mismatch {period}: computed={computed:.0f}, stated={net_change:.0f}, diff={diff:.0f}"
                )
            elif diff > warn_tol:
                warnings.append(
                    f"CFS minor gap {period}: diff={diff:.0f} ({diff/total_activity:.2%} of activity) — may reflect discontinued operations"
                )

        # WARNINGS — per SPEC_methodology §8.2 Reasonableness Checks
        capex_v = _get(cfs, "capex", i)
        if (capex_v is None or capex_v == 0) and cfi is not None and cfi < -100:
            warnings.append(
                f"Capex missing/zero {period} but CFI={cfi:.0f} — "
                "company likely uses custom XBRL extension tag not in EDGAR companyfacts"
            )

        rev = _get(is_d, "revenue", i)
        if rev is not None and rev < 0:
            warnings.append(f"Negative revenue {period}: {rev:.0f}")

        cogs_v = _get(is_d, "cogs", i)
        gross = _get(is_d, "gross_profit", i)
        if not is_utility and gross is not None and rev is not None and rev > 0:
            gm = gross / rev
            if not (0 <= gm <= 1):
                warnings.append(f"Gross margin outside 0-100% {period}: {gm:.1%}")
            elif gm == 0.0 and (cogs_v is None or cogs_v == 0):
                warnings.append(
                    f"Gross profit = 0 with no COGS {period} — likely utility/bank. "
                    "IS structure may need sector adaptation (SPEC_methodology §7)."
                )

        # Tax rate sanity (SPEC §8.2: 15-30% for US corporates; flag if outside 0-50%)
        tax_v = _get(is_d, "income_tax", i)
        ni_v  = _get(is_d, "net_income", i)
        if tax_v is not None and ni_v is not None:
            ebt_approx = ni_v + tax_v
            if ebt_approx != 0:
                eff_rate = tax_v / ebt_approx
                if not (0 <= eff_rate <= 0.50):
                    warnings.append(
                        f"Effective tax rate {period}: {eff_rate:.1%} outside 0-50% — "
                        "verify (PTCs/ITCs, NOLs, or deferred tax can explain <0%)"
                    )

        ebit = _get(is_d, "ebit", i)
        da = _get(is_d, "da", i)
        if ebit is not None and da is not None and liab is not None:
            ebitda = ebit + da
            if ebitda != 0:
                net_debt = (liab or 0) - (_get(bs, "cash", i) or 0)
                if net_debt > ebitda * 10:
                    warnings.append(f"High leverage {period}: net debt/EBITDA > 10x")

        period_checks[period] = checks

    # WC days sanity — check derived assumptions (SPEC §8.3 edge case checks)
    asmp = output.assumptions or {}
    for key in ("dio_days", "dpo_days", "dso_days"):
        val = asmp.get(key)
        if val is None:
            continue
        # Scalar or list — take first value
        check_val = val[0] if isinstance(val, list) else val
        if check_val > 365:
            label = {"dio_days": "DIO", "dpo_days": "DPO", "dso_days": "DSO"}[key]
            critical.append(
                f"{label} = {check_val:.0f} days (>365) — data quality failure. "
                "Likely no-COGS company (utility/bank) with wrong denominator in WC days formula."
            )

    if output.plug_used:
        notes.append("Circular reference resolved via plug (not iterative convergence)")
    elif not output.converged:
        notes.append("Iterative circular resolution did not converge — plug applied")

    passed = len(critical) == 0
    return VerificationReport(
        passed=passed,
        critical_failures=critical,
        warnings=warnings,
        notes=notes,
        period_checks=period_checks,
    )


def _get(d: dict, key: str, i: int):
    vals = d.get(key)
    if vals and i < len(vals):
        return vals[i]
    return None
