# financial_model/src/fetcher.py
import tempfile
import requests
from schemas.financial_data import ReconciledFinancialData, SourceCitation
from src.utils import compute_historical_periods

EDGAR_HEADERS = {"User-Agent": "FinancialModelBot vinit.paul@gmail.com"}

# ---------------------------------------------------------------------------
# XBRL tag map: canonical line item → ordered list of candidate US-GAAP tags.
# First tag with data for ALL target years wins.
# Include only tags that represent the FULL line item — no partial subtotals.
# ---------------------------------------------------------------------------
XBRL_TAG_MAP: dict[str, list[str]] = {
    # ── Income Statement ────────────────────────────────────────────────────
    "revenue": [
        "RevenueFromContractWithCustomerExcludingAssessedTax",   # ASC 606 primary (post-2018)
        "Revenues",                                               # older / financials / utilities
        "SalesRevenueNet",                                        # older manufacturing / retail
        "RevenueFromContractWithCustomerIncludingAssessedTax",    # includes sales tax
        "RegulatedAndUnregulatedOperatingRevenue",                # utilities
        "HealthCareOrganizationRevenue",                          # healthcare
        "RealEstateRevenueNet",                                   # REITs
    ],
    "cogs": [
        "CostOfGoodsAndServicesSold",                             # ASC 606 era primary
        "CostOfRevenue",                                          # most companies
        "CostOfGoodsSold",                                        # older goods companies
        "CostOfServices",                                         # pure service companies
        "CostOfGoodsAndServiceExcludingDepreciationDepletionAndAmortization",
    ],
    "gross_profit": [
        "GrossProfit",
    ],
    "sga": [
        "SellingGeneralAndAdministrativeExpense",                 # combined SG&A
        "GeneralAndAdministrativeExpense",                        # G&A only (some split)
        "SellingAndMarketingExpense",                             # selling only (some split)
    ],
    "rd": [
        "ResearchAndDevelopmentExpense",
        "ResearchAndDevelopmentExpenseExcludingAcquiredInProcessCost",
    ],
    "ebit": [
        "OperatingIncomeLoss",
    ],
    "interest_expense": [
        "InterestExpense",
        "InterestAndDebtExpense",
        "InterestExpenseDebt",
    ],
    "interest_income": [
        "InvestmentIncomeInterest",                               # non-operating interest
        "InterestAndDividendIncomeOperating",                     # financials / banks
        "InterestIncomeOperating",
        "InterestIncomeExpenseNet",                               # net interest (some banks)
    ],
    "income_tax": [
        "IncomeTaxExpenseBenefit",
    ],
    "net_income": [
        "NetIncomeLoss",                                          # primary: attr. to parent
        "NetIncomeLossAttributableToParent",
        "ProfitLoss",                                             # includes NCI (less preferred)
        "NetIncomeLossAvailableToCommonStockholdersBasic",
    ],
    "eps_basic": [
        "EarningsPerShareBasic",
        "EarningsPerShareBasicAndDiluted",
    ],
    "eps_diluted": [
        "EarningsPerShareDiluted",
        "EarningsPerShareBasicAndDiluted",
    ],
    "shares_basic": [
        "WeightedAverageNumberOfSharesOutstandingBasic",
        "CommonStockSharesOutstanding",
    ],
    "shares_diluted": [
        "WeightedAverageNumberOfDilutedSharesOutstanding",
        "WeightedAverageNumberOfSharesOutstandingBasic",
    ],
    "da": [
        "DepreciationDepletionAndAmortization",
        "DepreciationAndAmortization",
        "DepreciationAmortizationAndAccretionNet",
    ],
    # ── Balance Sheet — Assets ──────────────────────────────────────────────
    "cash": [
        "CashAndCashEquivalentsAtCarryingValue",
        "CashCashEquivalentsAndShortTermInvestments",
        "CashAndCashEquivalents",
        "Cash",
        "CashAndDueFromBanks",
    ],
    "accounts_receivable": [
        "AccountsReceivableNetCurrent",
        "AccountsReceivableNet",
        "ReceivablesNetCurrent",
        "TradeAndOtherReceivablesNetCurrent",
    ],
    "inventory": [
        "InventoryNet",
        "Inventories",
        "InventoryFinishedGoodsNetOfReserves",
    ],
    "total_current_assets": [
        "AssetsCurrent",
    ],
    "ppe_net": [
        "PropertyPlantAndEquipmentNet",
        "PropertyPlantAndEquipmentAndFinanceLeaseRightOfUseAssetAfterAccumulatedDepreciationAndAmortization",
    ],
    "goodwill": [
        "Goodwill",
    ],
    "intangibles_net": [
        "FiniteLivedIntangibleAssetsNet",
        "IntangibleAssetsNetExcludingGoodwill",
        "IntangibleAssetsNetIncludingGoodwill",
    ],
    "total_assets": [
        "Assets",
    ],
    # ── Balance Sheet — Liabilities & Equity ───────────────────────────────
    "accounts_payable": [
        "AccountsPayableCurrent",
        "AccountsPayableAndAccruedLiabilitiesCurrent",
        "AccountsPayable",
    ],
    "total_current_liabilities": [
        "LiabilitiesCurrent",
    ],
    "long_term_debt": [
        "LongTermDebtNoncurrent",
        "LongTermDebt",
        "LongTermDebtAndCapitalLeaseObligations",
        "LongTermNotesPayable",
        "SeniorLongTermNotes",
    ],
    "total_liabilities": [
        "Liabilities",
    ],
    "retained_earnings": [
        "RetainedEarningsAccumulatedDeficit",
        "RetainedEarnings",
    ],
    "total_equity": [
        # Prefer the broader tag (includes NCI) — avoids gaps when companies
        # stop filing the narrower StockholdersEquity tag (e.g. UNH stopped at 2014)
        "StockholdersEquityIncludingPortionAttributableToNoncontrollingInterest",
        "StockholdersEquity",
        "PartnersCapital",
        "MembersEquity",
    ],
    # Mezzanine equity: sits between liabilities and equity on some BS layouts.
    # Included separately so the BS balance check can account for it.
    "redeemable_nci": [
        "RedeemableNoncontrollingInterestEquityCarryingAmount",
        "RedeemableNoncontrollingInterestEquityPreferredCarryingAmount",
        "TemporaryEquityCarryingAmountIncludingPortionAttributableToNoncontrollingInterests",
    ],
    # ── Cash Flow Statement ─────────────────────────────────────────────────
    "cfo": [
        "NetCashProvidedByUsedInOperatingActivities",
        "NetCashProvidedByUsedInOperatingActivitiesContinuingOperations",
    ],
    "capex": [
        "PaymentsToAcquirePropertyPlantAndEquipment",
        "PaymentsToAcquireProductiveAssets",
        "PaymentsForCapitalImprovements",
    ],
    "cfi": [
        "NetCashProvidedByUsedInInvestingActivities",
        "NetCashProvidedByUsedInInvestingActivitiesContinuingOperations",
    ],
    "cff": [
        "NetCashProvidedByUsedInFinancingActivities",
        "NetCashProvidedByUsedInFinancingActivitiesContinuingOperations",
    ],
    "dividends_paid": [
        "PaymentsOfDividends",
        "PaymentsOfDividendsCommonStock",
        "PaymentsOfOrdinaryDividends",
        "PaymentsForDividends",
    ],
    "buybacks": [
        "PaymentsForRepurchaseOfCommonStock",
        "PaymentsForRepurchaseOfEquity",
        "TreasuryStockValueAcquiredCostMethod",
    ],
    "net_change_cash": [
        "CashCashEquivalentsRestrictedCashAndRestrictedCashEquivalentsPeriodIncreaseDecreaseIncludingExchangeRateEffect",
        "CashAndCashEquivalentsPeriodIncreaseDecrease",
        "CashCashEquivalentsRestrictedCashAndRestrictedCashEquivalentsPeriodIncreaseDecreaseExcludingExchangeRateEffect",
    ],
    "fx_effect_on_cash": [
        "EffectOfExchangeRateOnCashCashEquivalentsRestrictedCashAndRestrictedCashEquivalents",
        "EffectOfExchangeRateOnCashCashEquivalentsRestrictedCashAndRestrictedCashEquivalentsIncludingDisposalGroupAndDiscontinuedOperations",
        "EffectOfExchangeRateOnCashAndCashEquivalents",
        "EffectOfExchangeRateOnCashAndCashEquivalentsContinuingOperations",
    ],
}

# Extra single-component tags used only inside the D&A derivation
_DA_COMPONENTS = {
    "depreciation": ["Depreciation", "DepreciationNonproduction"],
    "amortization":  ["AmortizationOfIntangibleAssets", "AdjustmentForAmortization"],
}


def get_cik(ticker: str) -> str:
    resp = requests.get(
        "https://www.sec.gov/files/company_tickers.json", headers=EDGAR_HEADERS
    )
    resp.raise_for_status()
    for entry in resp.json().values():
        if entry["ticker"] == ticker.upper():
            return str(entry["cik_str"]).zfill(10)
    raise ValueError(f"Ticker {ticker} not found in EDGAR")


def fetch_xbrl_facts(cik: str) -> dict:
    resp = requests.get(
        f"https://data.sec.gov/api/xbrl/companyfacts/CIK{cik}.json",
        headers=EDGAR_HEADERS,
    )
    resp.raise_for_status()
    return resp.json()


def _annual_by_year(gaap: dict, tag: str, currency: str) -> dict[str, dict]:
    """Return dict of {year_str: best_entry} for a single XBRL tag."""
    units = gaap.get(tag, {}).get("units", {})
    entries = units.get(currency) or units.get("USD") or units.get("shares") or []
    annual = [e for e in entries if e.get("form") in ("10-K", "20-F") and e.get("fp") == "FY"]
    by_year: dict[str, dict] = {}
    for e in annual:
        yr = e["end"][:4]
        if yr not in by_year or e["end"] > by_year[yr]["end"]:
            by_year[yr] = e
    return by_year


def _extract_tag_annual(
    gaap: dict,
    tags: list[str],
    n_periods: int,
    currency: str = "USD",
    target_years: list[str] | None = None,
) -> tuple[list[float], str | None]:
    """
    Return (values_list, tag_used) for the first tag in `tags` that satisfies:
      - If target_years provided: tag has data for ALL of those specific years.
      - Otherwise: tag has >= n_periods annual entries (takes the most recent n).

    Using target_years prevents cross-era contamination (e.g. a tag that stopped
    filing in 2014 matching a 3-period request for 2023-2025).
    """
    for tag in tags:
        if tag not in gaap:
            continue
        by_year = _annual_by_year(gaap, tag, currency)
        if target_years:
            if all(yr in by_year for yr in target_years):
                vals = [round(by_year[yr]["val"] / 1e6, 2) for yr in target_years]
                return vals, tag
        else:
            annual = sorted(by_year.values(), key=lambda e: e["end"])
            if len(annual) >= n_periods:
                vals = [round(e["val"] / 1e6, 2) for e in annual[-n_periods:]]
                return vals, tag
    return [], None


def _apply_derivations(
    is_data: dict,
    bs_data: dict,
    cfs_data: dict,
    gaap: dict,
    sources: dict,
    period_labels: list[str],
    n: int,
    currency: str,
    target_years: list[str],
) -> list[str]:
    """
    Fill gaps left after direct XBRL tag lookup by computing items from components.
    Returns list of derivation notes for the Sources tab.

    Confidence levels:
      1.0 — accounting identity (exact): BS A=L+E+RNCI, CFS cash reconciliation
      0.90 — near-exact: gross_profit from rev-cogs, da from components
      0.80 — approximate: ebit/net_income may miss one-time items
    """
    notes: list[str] = []

    def present(d: dict, key: str) -> bool:
        vals = d.get(key, [])
        return len(vals) == n and any(v is not None for v in vals)

    def safe(d: dict, key: str) -> list:
        v = d.get(key, [None] * n)
        return v if len(v) == n else [None] * n

    def add(d: dict, key: str, vals: list, xbrl_tag: str, conf: float, note: str):
        if any(v is not None for v in vals):
            d[key] = vals
            sources[key] = [
                SourceCitation(filing=f"10-K {lbl}", confidence=conf, xbrl_tag=xbrl_tag)
                for lbl in period_labels
            ]
            notes.append(note)

    # ── IS: D&A from depreciation + amortization components ─────────────────
    if not present(is_data, "da"):
        dep, dep_tag = _extract_tag_annual(gaap, _DA_COMPONENTS["depreciation"], n, currency, target_years)
        amort, amort_tag = _extract_tag_annual(gaap, _DA_COMPONENTS["amortization"], n, currency, target_years)
        if dep:
            combined = [round((d or 0) + (a or 0), 2) for d, a in zip(dep, amort or [0] * n)]
            add(is_data, "da", combined, f"derived:{dep_tag}+{amort_tag or 'nil'}", 0.90,
                "da: derived from Depreciation + AmortizationOfIntangibleAssets")

    # ── IS: gross_profit = revenue − cogs ───────────────────────────────────
    if not present(is_data, "gross_profit") and present(is_data, "revenue") and present(is_data, "cogs"):
        vals = [round(r - c, 2) if r is not None and c is not None else None
                for r, c in zip(safe(is_data, "revenue"), safe(is_data, "cogs"))]
        add(is_data, "gross_profit", vals, "derived:revenue-cogs", 0.90,
            "gross_profit: derived from revenue − cogs")

    # ── IS: ebit = gross_profit − sga − rd − da (approximate) ───────────────
    if not present(is_data, "ebit") and present(is_data, "gross_profit"):
        vals = [
            round(gp - (sg or 0) - (rd or 0) - (da or 0), 2) if gp is not None else None
            for gp, sg, rd, da in zip(
                safe(is_data, "gross_profit"), safe(is_data, "sga"),
                safe(is_data, "rd"),           safe(is_data, "da"),
            )
        ]
        add(is_data, "ebit", vals, "derived:GP-SGA-RD-DA", 0.80,
            "ebit: derived from gross_profit − sga − rd − da (excludes restructuring/impairments)")

    # ── IS: net_income = ebit ± interest − tax (approximate) ────────────────
    if not present(is_data, "net_income") and present(is_data, "ebit"):
        vals = [
            round(e + (ii or 0) - (ie or 0) - (tx or 0), 2) if e is not None else None
            for e, ii, ie, tx in zip(
                safe(is_data, "ebit"),             safe(is_data, "interest_income"),
                safe(is_data, "interest_expense"), safe(is_data, "income_tax"),
            )
        ]
        add(is_data, "net_income", vals, "derived:EBIT+II-IE-Tax", 0.80,
            "net_income: derived from ebit ± interest − tax (excludes non-operating one-time items)")

    # ── BS: total_liabilities = total_assets − total_equity − redeemable_nci ─
    # Accounting identity; exact when equity = StockholdersEquityIncludingNCI
    if not present(bs_data, "total_liabilities") and present(bs_data, "total_assets") and present(bs_data, "total_equity"):
        rnci = safe(bs_data, "redeemable_nci")
        vals = [
            round(a - e - (r or 0), 2) if a is not None and e is not None else None
            for a, e, r in zip(safe(bs_data, "total_assets"), safe(bs_data, "total_equity"), rnci)
        ]
        add(bs_data, "total_liabilities", vals, "derived:Assets-Equity-RNCI", 1.0,
            "total_liabilities: derived from total_assets − total_equity − redeemable_nci")

    # ── BS: total_equity = total_assets − total_liabilities − redeemable_nci ─
    if not present(bs_data, "total_equity") and present(bs_data, "total_assets") and present(bs_data, "total_liabilities"):
        rnci = safe(bs_data, "redeemable_nci")
        vals = [
            round(a - l - (r or 0), 2) if a is not None and l is not None else None
            for a, l, r in zip(safe(bs_data, "total_assets"), safe(bs_data, "total_liabilities"), rnci)
        ]
        add(bs_data, "total_equity", vals, "derived:Assets-Liabilities-RNCI", 1.0,
            "total_equity: derived from total_assets − total_liabilities − redeemable_nci")

    # ── CFS: net_change_cash = cfo + cfi + cff + fx (identity) ─────────────
    if not present(cfs_data, "net_change_cash") and present(cfs_data, "cfo") and present(cfs_data, "cfi") and present(cfs_data, "cff"):
        fx = safe(cfs_data, "fx_effect_on_cash")
        vals = [
            round((op or 0) + (inv or 0) + (fin or 0) + (f or 0), 2)
            if any(v is not None for v in [op, inv, fin]) else None
            for op, inv, fin, f in zip(safe(cfs_data, "cfo"), safe(cfs_data, "cfi"),
                                        safe(cfs_data, "cff"), fx)
        ]
        add(cfs_data, "net_change_cash", vals, "derived:CFO+CFI+CFF+FX", 1.0,
            "net_change_cash: derived from cfo + cfi + cff + fx_effect")

    return notes


def _build_period_labels(gaap: dict, periods_historical: int, currency: str) -> list[str]:
    """
    Find the revenue tag whose most recent FY entry is latest, then return period labels.
    Picking by recency (not by tag order) prevents old archived tags from winning.
    """
    best_labels: list[str] = []
    best_last_date = ""

    for tag in XBRL_TAG_MAP["revenue"]:
        if tag not in gaap:
            continue
        by_year = _annual_by_year(gaap, tag, currency)
        deduped = sorted(by_year.values(), key=lambda e: e["end"])
        if len(deduped) < periods_historical:
            continue
        last_date = deduped[-1]["end"]
        if last_date > best_last_date:
            best_last_date = last_date
            best_labels = [f"{e['end'][:4]}A" for e in deduped[-periods_historical:]]

    return best_labels


def parse_xbrl_to_raw(
    facts: dict, periods_historical: int, currency: str = "USD"
) -> ReconciledFinancialData:
    gaap = facts.get("facts", {}).get("us-gaap", {})
    entity = facts.get("entityName", "Unknown")
    cik = facts.get("cik", 0)

    # Period labels: use most-recently-filed revenue tag (avoids picking stale archived tags)
    period_labels = _build_period_labels(gaap, periods_historical, currency)
    target_years = [p[:4] for p in period_labels]   # e.g. ["2023", "2024", "2025"]

    is_data: dict = {}
    bs_data: dict = {}
    cfs_data: dict = {}
    sources: dict = {}

    def load(section: dict, keys: list[str]):
        for key in keys:
            vals, found_tag = _extract_tag_annual(
                gaap, XBRL_TAG_MAP[key], periods_historical, currency, target_years
            )
            if vals:
                section[key] = vals
                sources[key] = [
                    SourceCitation(filing=f"10-K {lbl}", confidence=1.0, xbrl_tag=f"us-gaap:{found_tag}")
                    for lbl in period_labels
                ]

    is_keys = [
        "revenue", "cogs", "gross_profit", "sga", "rd", "ebit",
        "interest_expense", "interest_income", "income_tax", "net_income",
        "eps_basic", "eps_diluted", "shares_basic", "shares_diluted", "da",
    ]
    bs_keys = [
        "cash", "accounts_receivable", "inventory", "total_current_assets",
        "ppe_net", "goodwill", "intangibles_net", "total_assets",
        "accounts_payable", "total_current_liabilities", "long_term_debt",
        "total_liabilities", "retained_earnings", "total_equity", "redeemable_nci",
    ]
    cfs_keys = [
        "cfo", "capex", "cfi", "cff", "dividends_paid", "buybacks",
        "net_change_cash", "fx_effect_on_cash",
    ]

    load(is_data, is_keys)
    load(bs_data, bs_keys)
    load(cfs_data, cfs_keys)

    derivation_notes = _apply_derivations(
        is_data, bs_data, cfs_data, gaap, sources, period_labels,
        periods_historical, currency, target_years
    )

    return ReconciledFinancialData(
        ticker=str(cik),
        company_name=entity,
        currency=currency,
        fiscal_year_end="",
        periods=period_labels,
        income_statement=is_data,
        balance_sheet=bs_data,
        cash_flow_statement=cfs_data,
        notes={},
        sources=sources,
        flags=derivation_notes,
    )


def fetch_us_filing(cfg) -> ReconciledFinancialData:
    cik = get_cik(cfg.ticker)
    facts = fetch_xbrl_facts(cik)
    raw = parse_xbrl_to_raw(facts, cfg.periods_historical, cfg.currency)
    raw.ticker = cfg.ticker
    raw.fiscal_year_end = cfg.fiscal_year_end
    return raw


def fetch_non_us_filing(cfg, ir_url: str | None = None) -> ReconciledFinancialData:
    from src.extractor import scrape_ir_page_for_pdfs, extract_notes_from_pdf

    pdf_urls = scrape_ir_page_for_pdfs(cfg.ticker, cfg.company_name, ir_url=ir_url)
    if not pdf_urls:
        raise ValueError(f"No annual report PDFs found for {cfg.company_name}")

    periods = compute_historical_periods(cfg.fiscal_year_end, cfg.periods_historical)

    all_notes: dict = {}
    for url in pdf_urls:
        resp = requests.get(
            url, headers={"User-Agent": "FinancialModelBot vinit.paul@gmail.com"}, timeout=30
        )
        with tempfile.NamedTemporaryFile(suffix=".pdf", delete=False) as f:
            f.write(resp.content)
            tmp_path = f.name
        notes = extract_notes_from_pdf(tmp_path, periods)
        all_notes.update(notes)

    return ReconciledFinancialData(
        ticker=cfg.ticker,
        company_name=cfg.company_name,
        currency=cfg.currency,
        fiscal_year_end=cfg.fiscal_year_end,
        periods=periods,
        income_statement={},
        balance_sheet={},
        cash_flow_statement={},
        notes=all_notes,
        sources={},
        flags=["Non-US company — data sourced from IR page PDFs"],
    )
