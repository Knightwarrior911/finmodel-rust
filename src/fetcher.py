# financial_model/src/fetcher.py
import requests
from schemas.financial_data import ReconciledFinancialData, SourceCitation

EDGAR_HEADERS = {"User-Agent": "FinancialModelBot vinit.paul@gmail.com"}

# Maps canonical line item names → candidate XBRL tags (first found wins)
XBRL_TAG_MAP = {
    # Income Statement
    "revenue": [
        "RevenueFromContractWithCustomerExcludingAssessedTax",
        "Revenues", "SalesRevenueNet", "RevenueFromContractWithCustomerIncludingAssessedTax"
    ],
    "cogs": ["CostOfGoodsAndServicesSold", "CostOfRevenue", "CostOfGoodsSold"],
    "gross_profit": ["GrossProfit"],
    "sga": ["SellingGeneralAndAdministrativeExpense"],
    "rd": ["ResearchAndDevelopmentExpense"],
    "ebit": ["OperatingIncomeLoss"],
    "interest_expense": ["InterestExpense", "InterestAndDebtExpense"],
    "interest_income": ["InterestAndDividendIncomeOperating", "InvestmentIncomeInterest"],
    "income_tax": ["IncomeTaxExpenseBenefit"],
    "net_income": ["NetIncomeLoss"],
    "eps_basic": ["EarningsPerShareBasic"],
    "eps_diluted": ["EarningsPerShareDiluted"],
    "shares_basic": ["WeightedAverageNumberOfSharesOutstandingBasic"],
    "shares_diluted": ["WeightedAverageNumberOfDilutedSharesOutstanding"],
    "da": ["DepreciationDepletionAndAmortization", "DepreciationAndAmortization"],
    # Balance Sheet
    "cash": ["CashAndCashEquivalentsAtCarryingValue", "CashCashEquivalentsAndShortTermInvestments"],
    "accounts_receivable": ["AccountsReceivableNetCurrent"],
    "inventory": ["InventoryNet"],
    "total_current_assets": ["AssetsCurrent"],
    "ppe_net": ["PropertyPlantAndEquipmentNet"],
    "goodwill": ["Goodwill"],
    "intangibles_net": ["FiniteLivedIntangibleAssetsNet", "IntangibleAssetsNetExcludingGoodwill"],
    "total_assets": ["Assets"],
    "accounts_payable": ["AccountsPayableCurrent"],
    "total_current_liabilities": ["LiabilitiesCurrent"],
    "long_term_debt": ["LongTermDebtNoncurrent", "LongTermDebt"],
    "total_liabilities": ["Liabilities"],
    "retained_earnings": ["RetainedEarningsAccumulatedDeficit"],
    "total_equity": ["StockholdersEquity"],
    # Cash Flow
    "cfo": ["NetCashProvidedByUsedInOperatingActivities"],
    "capex": ["PaymentsToAcquirePropertyPlantAndEquipment"],
    "cfi": ["NetCashProvidedByUsedInInvestingActivities"],
    "cff": ["NetCashProvidedByUsedInFinancingActivities"],
    "dividends_paid": ["PaymentsOfDividends", "PaymentsOfDividendsCommonStock"],
    "buybacks": ["PaymentsForRepurchaseOfCommonStock"],
    "net_change_cash": ["CashCashEquivalentsRestrictedCashAndRestrictedCashEquivalentsPeriodIncreaseDecreaseIncludingExchangeRateEffect"],
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


def _extract_tag_annual(gaap: dict, tags: list[str], n_periods: int) -> tuple[list[float], str | None]:
    for tag in tags:
        if tag not in gaap:
            continue
        units = gaap[tag].get("units", {})
        usd_entries = units.get("USD") or units.get("shares") or []
        annual = [e for e in usd_entries if e.get("form") == "10-K" and e.get("fp") == "FY"]
        annual.sort(key=lambda e: e["end"])
        if len(annual) >= n_periods:
            vals = [round(e["val"] / 1e6, 2) for e in annual[-n_periods:]]
            return vals, tag
    return [], None


def parse_xbrl_to_raw(facts: dict, periods_historical: int) -> ReconciledFinancialData:
    gaap = facts.get("facts", {}).get("us-gaap", {})
    entity = facts.get("entityName", "Unknown")
    cik = facts.get("cik", 0)

    # Build periods list from revenue tag (most reliably present)
    rev_tags = XBRL_TAG_MAP["revenue"]
    period_labels: list[str] = []
    for tag in rev_tags:
        if tag not in gaap:
            continue
        entries = gaap[tag].get("units", {}).get("USD", [])
        annual = sorted(
            [e for e in entries if e.get("form") == "10-K" and e.get("fp") == "FY"],
            key=lambda e: e["end"],
        )
        if len(annual) >= periods_historical:
            for e in annual[-periods_historical:]:
                year = e["end"][:4]
                period_labels.append(f"{year}A")
            break

    is_data, bs_data, cfs_data, sources = {}, {}, {}, {}

    def load(section: dict, keys: list[str]):
        for key in keys:
            vals, found_tag = _extract_tag_annual(gaap, XBRL_TAG_MAP[key], periods_historical)
            if vals:
                section[key] = vals
                sources[key] = [
                    SourceCitation(
                        filing=f"10-K {lbl}",
                        confidence=1.0,
                        xbrl_tag=f"us-gaap:{found_tag}",
                    )
                    for lbl in period_labels
                ]

    is_keys = ["revenue", "cogs", "gross_profit", "sga", "rd", "ebit",
               "interest_expense", "interest_income", "income_tax", "net_income",
               "eps_basic", "eps_diluted", "shares_basic", "shares_diluted", "da"]
    bs_keys = ["cash", "accounts_receivable", "inventory", "total_current_assets",
               "ppe_net", "goodwill", "intangibles_net", "total_assets", "accounts_payable",
               "total_current_liabilities", "long_term_debt", "total_liabilities",
               "retained_earnings", "total_equity"]
    cfs_keys = ["cfo", "capex", "cfi", "cff", "dividends_paid", "buybacks", "net_change_cash"]

    load(is_data, is_keys)
    load(bs_data, bs_keys)
    load(cfs_data, cfs_keys)

    return ReconciledFinancialData(
        ticker=str(cik),
        company_name=entity,
        currency="USD",
        fiscal_year_end="Dec",
        periods=period_labels,
        income_statement=is_data,
        balance_sheet=bs_data,
        cash_flow_statement=cfs_data,
        notes={},
        sources=sources,
        flags=[],
    )


def fetch_us_filing(cfg) -> ReconciledFinancialData:
    cik = get_cik(cfg.ticker)
    facts = fetch_xbrl_facts(cik)
    raw = parse_xbrl_to_raw(facts, cfg.periods_historical)
    raw.ticker = cfg.ticker
    raw.currency = cfg.currency
    raw.fiscal_year_end = cfg.fiscal_year_end
    return raw
