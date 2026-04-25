from schemas.financial_data import ModelOutput, VerificationReport

BS_TOLERANCE = 1.0   # $1M tolerance for balance sheet check
CFS_TOLERANCE = 1.0


def verify(output: ModelOutput) -> VerificationReport:
    critical: list[str] = []
    warnings: list[str] = []
    notes: list[str] = []
    period_checks: dict = {}

    bs = output.balance_sheet
    is_d = output.income_statement
    cfs = output.cash_flow_statement

    for i, period in enumerate(output.periods):
        checks = {}

        # CRITICAL: BS balance
        assets = _get(bs, "total_assets", i)
        liab = _get(bs, "total_liabilities", i)
        equity = _get(bs, "total_equity", i)
        if all(v is not None for v in [assets, liab, equity]):
            diff = abs(assets - (liab + equity))
            checks["bs_balance"] = diff <= BS_TOLERANCE
            if diff > BS_TOLERANCE:
                critical.append(
                    f"Balance sheet mismatch {period}: assets={assets:.0f}, L+E={liab+equity:.0f}, diff={diff:.0f}"
                )

        # CRITICAL: CFS cash tie
        cfo = _get(cfs, "cfo", i)
        cfi = _get(cfs, "cfi", i)
        cff = _get(cfs, "cff", i)
        net_change = _get(cfs, "net_change_cash", i)
        if all(v is not None for v in [cfo, cfi, cff, net_change]):
            computed = cfo + cfi + cff
            diff = abs(computed - net_change)
            checks["cfs_tie"] = diff <= CFS_TOLERANCE
            if diff > CFS_TOLERANCE:
                critical.append(
                    f"CFS mismatch {period}: computed={computed:.0f}, stated={net_change:.0f}"
                )

        # WARNINGS
        rev = _get(is_d, "revenue", i)
        if rev is not None and rev < 0:
            warnings.append(f"Negative revenue {period}: {rev:.0f}")

        gross = _get(is_d, "gross_profit", i)
        if gross is not None and rev and rev != 0:
            gm = gross / rev
            if not (0 <= gm <= 1):
                warnings.append(f"Gross margin outside 0-100% {period}: {gm:.1%}")

        ni = _get(is_d, "net_income", i)
        da = _get(is_d, "da", i)
        if ni is not None and da is not None and rev and rev != 0:
            ebitda = ni + da  # simplified
            if ebitda != 0 and liab is not None and equity is not None:
                net_debt = (liab or 0) - (_get(bs, "cash", i) or 0)
                if net_debt > ebitda * 10:
                    warnings.append(f"High leverage {period}: net debt/EBITDA > 10x")

        period_checks[period] = checks

    if output.plug_used:
        notes.append("Circular reference resolved via plug (not iterative convergence)")
    if not output.converged:
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
