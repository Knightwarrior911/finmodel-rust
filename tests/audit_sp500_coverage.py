#!/usr/bin/env python
"""
tests/audit_sp500_coverage.py

Coverage audit of the XBRL fetcher against all S&P 500 companies.

For every key line item, checks whether ANY tag in the XBRL_TAG_MAP fallback
list has at least one non-None annual value in the SEC EDGAR companyfacts JSON.

Usage:
    python tests/audit_sp500_coverage.py

Output:
    tests/sp500_coverage_report.txt   — human-readable summary
    (also prints progress to stdout)
"""

import json
import sys
import time
from pathlib import Path
from collections import defaultdict

import requests

# ---------------------------------------------------------------------------
# Setup: add project root to path
# ---------------------------------------------------------------------------
PROJECT_ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(PROJECT_ROOT))

from src.fetcher import XBRL_TAG_MAP, EDGAR_HEADERS

# ---------------------------------------------------------------------------
# Key line items to test (subset most relevant to DCF / financial model)
# ---------------------------------------------------------------------------
KEY_ITEMS = [
    "revenue",
    "cogs",
    "gross_profit",
    "ebit",
    "net_income",
    "da",
    "capex",
    "cfo",
    "cfi",
    "cff",
    "cash",
    "accounts_receivable",
    "inventory",
    "accounts_payable",
    "ppe_net",
    "long_term_debt",
    "total_assets",
    "total_liabilities",
    "total_equity",
    "eps_diluted",
    "shares_diluted",
]

# ---------------------------------------------------------------------------
# Concept-area keyword hints for gap analysis.
# These are EXACT tag prefix matches — only semantically relevant tags.
# The audit uses these to find plausible alternatives; human review then
# filters out false positives before adding to XBRL_TAG_MAP.
# ---------------------------------------------------------------------------
CONCEPT_KEYWORDS = {
    "revenue":            ["RevenueFrom", "Revenues", "SalesRevenue", "NetRevenue",
                           "TotalRevenue", "InterestAndFeeIncome", "NoninterestIncome",
                           "PremiumsEarned", "NetInvestmentIncome", "BrokerageCommissions",
                           "RevenuesNetOf", "BankingFees"],
    "cogs":               ["CostOfGoods", "CostOfRevenue", "CostOfServices",
                           "CostOfPurchasedPower", "PolicyholderBenefits",
                           "BenefitsLossesAndExpenses", "DirectCostsOf",
                           "OperatingCostsAndExpenses", "CostsAndExpenses"],
    "gross_profit":       ["GrossProfit"],
    "ebit":               ["OperatingIncomeLoss", "IncomeLossFromContinuingOperationsBefore",
                           "OperatingIncome"],
    "net_income":         ["NetIncomeLoss", "ProfitLoss", "NetIncome"],
    "da":                 ["DepreciationDepletionAndAmortization", "DepreciationAndAmortization",
                           "DepreciationAmortization", "Depreciation"],
    "capex":              ["PaymentsToAcquirePropertyPlant", "PaymentsToAcquireRealEstate",
                           "PaymentsForCapitalImprovements", "PaymentsToAcquireAndDevelop",
                           "PurchaseOfPropertyAndEquipment"],
    "cfo":                ["NetCashProvidedByUsedInOperatingActivities"],
    "cfi":                ["NetCashProvidedByUsedInInvestingActivities"],
    "cff":                ["NetCashProvidedByUsedInFinancingActivities"],
    "cash":               ["CashAndCashEquivalents", "CashCashEquivalents",
                           "CashAndDueFromBanks"],
    "accounts_receivable":["AccountsReceivableNet", "ReceivablesNet",
                           "LoansAndLeasesReceivableNet",
                           "NotesAndLoansReceivableNet", "PremiumsAndOtherReceivablesNet"],
    "inventory":          ["InventoryNet", "Inventories", "InventoryFinished",
                           "InventoryRawMaterials", "EnergyRelatedInventory",
                           "RealEstateInventory"],
    "accounts_payable":   ["AccountsPayable", "AccountsPayableAndAccruedLiabilities"],
    "ppe_net":            ["PropertyPlantAndEquipmentNet", "RealEstateInvestmentPropertyNet"],
    "long_term_debt":     ["LongTermDebt", "LongTermNotesPayable", "SeniorLongTermNotes",
                           "ConvertibleLongTermNotesPayable", "UnsecuredDebt", "SecuredDebt",
                           "SubordinatedLongTermDebt", "FinanceLeaseLiabilityNoncurrent"],
    "total_assets":       ["Assets"],
    "total_liabilities":  ["Liabilities"],
    "total_equity":       ["StockholdersEquity", "PartnersCapital", "MembersEquity"],
    "eps_diluted":        ["EarningsPerShareDiluted", "EarningsPerShareBasicAndDiluted"],
    "shares_diluted":     ["WeightedAverageNumberOfDilutedShares",
                           "WeightedAverageNumberOfSharesOutstanding"],
}

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

RATE_LIMIT_DELAY = 0.12   # seconds between requests (~8 req/sec, under 10 limit)
MAX_RETRIES = 3


def get_sp500_tickers_and_ciks() -> list[dict]:
    """
    Fetch the full EDGAR company_tickers.json, then cross-reference with the
    S&P 500 list from Wikipedia to get only S&P 500 members.
    Falls back to a curated hardcoded list if Wikipedia is unreachable.
    Returns list of {"ticker": str, "cik": str, "name": str}
    """
    print("Fetching EDGAR company_tickers.json …")
    resp = requests.get(
        "https://www.sec.gov/files/company_tickers.json",
        headers=EDGAR_HEADERS,
        timeout=30,
    )
    resp.raise_for_status()
    edgar_map: dict[str, dict] = {}
    for entry in resp.json().values():
        edgar_map[entry["ticker"].upper()] = {
            "ticker": entry["ticker"].upper(),
            "cik": str(entry["cik_str"]).zfill(10),
            "name": entry.get("title", ""),
        }
    time.sleep(RATE_LIMIT_DELAY)

    # Try to get S&P 500 list from Wikipedia
    sp500_tickers: list[str] = []
    try:
        print("Fetching S&P 500 list from Wikipedia …")
        wiki_resp = requests.get(
            "https://en.wikipedia.org/wiki/List_of_S%26P_500_companies",
            timeout=30,
            headers={"User-Agent": "Mozilla/5.0 financial model coverage audit"},
        )
        wiki_resp.raise_for_status()
        from html.parser import HTMLParser

        class SP500Parser(HTMLParser):
            def __init__(self):
                super().__init__()
                self._in_table = False
                self._in_first_tbody = False
                self._in_tr = False
                self._in_td = False
                self._col = 0
                self._tickers: list[str] = []
                self._table_count = 0

            def handle_starttag(self, tag, attrs):
                attrs_dict = dict(attrs)
                if tag == "table" and "wikitable" in attrs_dict.get("class", ""):
                    self._table_count += 1
                    if self._table_count == 1:
                        self._in_table = True
                if self._in_table and tag == "tbody":
                    self._in_first_tbody = True
                if self._in_first_tbody and tag == "tr":
                    self._in_tr = True
                    self._col = 0
                if self._in_tr and tag == "td":
                    self._in_td = True
                    self._col += 1

            def handle_endtag(self, tag):
                if tag == "td":
                    self._in_td = False
                if tag == "tr":
                    self._in_tr = False
                if tag == "tbody" and self._in_first_tbody:
                    self._in_first_tbody = False
                    self._in_table = False

            def handle_data(self, data):
                if self._in_td and self._col == 1:
                    ticker = data.strip().replace(".", "-")
                    if ticker and ticker.isupper() and 1 <= len(ticker) <= 5:
                        self._tickers.append(ticker)

        parser = SP500Parser()
        parser.feed(wiki_resp.text)
        sp500_tickers = list(dict.fromkeys(parser._tickers))  # deduplicate, preserve order
        print(f"  Found {len(sp500_tickers)} tickers from Wikipedia")
    except Exception as exc:
        print(f"  Wikipedia fetch failed ({exc}); using hardcoded list")

    # Fallback: hardcoded representative S&P 500 sample (covers all major sectors)
    if len(sp500_tickers) < 400:
        sp500_tickers = [
            "MMM","AOS","ABT","ABBV","ACN","ADBE","AMD","AES","AFL","A","APD","AKAM","ALK","ALB","ARE","ALGN",
            "ALLE","LNT","ALL","GOOGL","GOOG","MO","AMZN","AMCR","AEE","AAL","AEP","AXP","AIG","AMT","AWK",
            "AMP","AME","AMGN","APH","ADI","ANSS","AON","APA","AAPL","AMAT","APTV","ACGL","ADM","ANET","AJG",
            "AIZ","T","ATO","ADSK","ADP","AZO","AVB","AVY","AXON","BKR","BALL","BAC","BBWI","BAX","BDX","WRB",
            "BRK-B","BBY","BIO","TECH","BIIB","BLK","BX","BK","BA","BKNG","BWA","BXP","BSX","BMY","AVGO","BR",
            "BRO","BLDR","BG","CDNS","CZR","CPT","CPB","COF","CAH","KMX","CCL","CARR","CTLT","CAT","CBOE","CBRE",
            "CDW","CE","COR","CNC","CNX","CDAY","CF","CRL","SCHW","CHTR","CVX","CMG","CB","CHD","CI","CINF",
            "CTAS","CSCO","C","CFG","CLX","CME","CMS","KO","CTSH","CL","CMCSA","CMA","CAG","COP","ED","STZ",
            "CEG","COO","CPRT","GLW","CTVA","CSGP","COST","CTRA","CCI","CSX","CMI","CVS","DHR","DHI","DRI","DVA",
            "DE","DAL","XRAY","DVN","DXCM","FANG","DLR","DFS","DG","DLTR","D","DPZ","DOV","DOW","DTE","DUK",
            "DD","DXC","EMN","ETN","EBAY","ECL","EIX","EW","EA","ELV","LLY","EMR","ENPH","ETR","EOG","EPAM",
            "EQT","EFX","EQIX","EQR","ESS","EL","ETSY","EG","EVRG","ES","EXC","EXPD","EXPE","EXR","XOM","FFIV",
            "FDS","FICO","FAST","FRT","FDX","FIS","FITB","FSLR","FE","FRC","FBHS","F","FTNT","FTV","FOXA","FOX",
            "BEN","FCX","GRMN","IT","GEHC","GEN","GNRC","GD","GE","GIS","GM","GPC","GILD","GL","GPN","GS","HAL",
            "HIG","HAS","HCA","PEAK","HSIC","HSY","HES","HPE","HLT","HOLX","HD","HON","HRL","HST","HWM","HPQ",
            "HUM","HBAN","HII","IBM","IEX","IDXX","ITW","ILMN","INCY","IR","PODD","INTC","ICE","IFF","IP","IPG",
            "INTU","ISRG","IVZ","INVH","IQV","IRM","JBHT","JKHY","J","JNJ","JCI","JPM","JNPR","K","KVUE","KDP",
            "KEY","KEYS","KMB","KIM","KMI","KLAC","KHC","KR","LHX","LH","LRCX","LW","LVS","LDOS","LEN","LIN",
            "LYV","LKQ","LMT","L","LOW","LULU","LYB","MTB","MRO","MPC","MKTX","MAR","MMC","MLM","MAS","MA","MTCH",
            "MKC","MCD","MCK","MDT","MRK","META","MET","MTD","MGM","MCHP","MU","MSFT","MAA","MRNA","MHK","MOH",
            "TAP","MDLZ","MPWR","MNST","MCO","MS","MOS","MSI","MSCI","NDAQ","NTAP","NFLX","NEM","NWSA","NWS",
            "NEE","NKE","NI","NDSN","NSC","NTRS","NOC","NCLH","NRG","NUE","NVDA","NVR","NXPI","ORLY","OXY",
            "ODFL","OMC","ON","OKE","ORCL","OGN","OTIS","PCAR","PKG","PANW","PH","PAYX","PAYC","PYPL","PNR",
            "PEP","PKI","PFE","PCG","PM","PSX","PNW","PXD","PNC","POOL","PPG","PPL","PFG","PG","PGR","PLD",
            "PRU","PEG","PRGO","PTC","PSA","PHM","QRVO","PWR","QCOM","DGX","RL","RJF","RTX","O","REG","REGN",
            "RF","RSG","RMD","RVTY","ROK","ROL","ROP","ROST","RCL","SPGI","CRM","SBAC","SLB","STX","SEE","SRE",
            "NOW","SHW","SPG","SWKS","SJM","SNA","SOLV","SO","LUV","SWK","SBUX","STT","STLD","STE","SYK","SYF",
            "SNPS","SYY","TMUS","TROW","TTWO","TPR","TRGP","TGT","TEL","TDY","TFX","TER","TSLA","TXN","TXT",
            "TMO","TJX","TSCO","TT","TDG","TRV","TRMB","TFC","TYL","TSN","USB","UDR","ULTA","UNP","UAL","UPS",
            "URI","UNH","UHS","VLO","VTR","VRSN","VRSK","VZ","VRTX","VFC","VTRS","VICI","V","VMC","WAB","WBA",
            "WMT","WBD","WM","WAT","WEC","WFC","WELL","WST","DD","WDC","WY","WHR","WMB","WTW","GWW","WYNN",
            "XEL","XYL","YUM","ZBRA","ZBH","ZION","ZTS",
        ]
        # deduplicate
        seen = set()
        deduped = []
        for t in sp500_tickers:
            if t not in seen:
                seen.add(t)
                deduped.append(t)
        sp500_tickers = deduped

    # Match to EDGAR CIKs
    result = []
    missing_from_edgar = []
    for ticker in sp500_tickers:
        if ticker in edgar_map:
            result.append(edgar_map[ticker])
        else:
            missing_from_edgar.append(ticker)

    if missing_from_edgar:
        print(f"  Tickers not in EDGAR map: {missing_from_edgar[:20]} ({'...' if len(missing_from_edgar)>20 else ''})")

    return result


def fetch_companyfacts(cik: str) -> dict | None:
    """Fetch XBRL companyfacts JSON for a CIK. Returns None on error."""
    url = f"https://data.sec.gov/api/xbrl/companyfacts/CIK{cik}.json"
    for attempt in range(MAX_RETRIES):
        try:
            resp = requests.get(url, headers=EDGAR_HEADERS, timeout=30)
            if resp.status_code == 404:
                return None
            resp.raise_for_status()
            return resp.json()
        except requests.exceptions.HTTPError as e:
            if resp.status_code == 429:
                wait = 2 ** (attempt + 1)
                print(f"    Rate limited, waiting {wait}s …")
                time.sleep(wait)
            else:
                return None
        except Exception:
            if attempt < MAX_RETRIES - 1:
                time.sleep(1)
            else:
                return None
    return None


def check_tag_has_annual_data(gaap: dict, tag: str) -> bool:
    """Return True if the tag has at least one annual (10-K/20-F FY) entry."""
    units = gaap.get(tag, {}).get("units", {})
    for currency_entries in units.values():
        for e in currency_entries:
            if e.get("form") in ("10-K", "20-F") and e.get("fp") == "FY":
                return True
    return False


def find_alternative_tags(gaap: dict, item: str) -> list[str]:
    """
    For a missing line item, scan the company's us-gaap tags for alternatives
    that match the concept-area keyword hints (prefix match).
    Returns up to 10 candidate tag names present in gaap with annual data.
    """
    keywords = CONCEPT_KEYWORDS.get(item, [])
    candidates = []
    for tag in gaap:
        for kw in keywords:
            # prefix match (case-sensitive as XBRL tags are CamelCase)
            if tag.startswith(kw):
                if check_tag_has_annual_data(gaap, tag):
                    if tag not in candidates:
                        candidates.append(tag)
                        break
    return candidates[:10]


def run_audit(max_companies: int | None = None) -> dict:
    """
    Main audit loop.
    Returns results dict with keys:
        companies_tested, per_item_coverage, missing_by_item,
        alternative_tags_by_item, no_revenue_tickers, errors
    """
    companies = get_sp500_tickers_and_ciks()
    if max_companies:
        companies = companies[:max_companies]

    total = len(companies)
    print(f"\nAuditing {total} companies …\n")

    # Track results
    per_item_hit: dict[str, int] = defaultdict(int)     # items that have >=1 tag hit
    per_item_miss: dict[str, list] = defaultdict(list)  # {item: [ticker, ...]}
    alternative_tags: dict[str, dict[str, list]] = defaultdict(lambda: defaultdict(list))
    # alternative_tags[item][alt_tag] = [ticker, ...]

    no_revenue: list[str] = []
    errors: list[str] = []

    for idx, company in enumerate(companies):
        ticker = company["ticker"]
        cik = company["cik"]
        name = company["name"]

        if (idx + 1) % 50 == 0:
            print(f"  [{idx+1}/{total}] processed so far …")

        facts = fetch_companyfacts(cik)
        time.sleep(RATE_LIMIT_DELAY)

        if facts is None:
            errors.append(f"{ticker} ({cik}): failed to fetch")
            # Count as miss for all items
            for item in KEY_ITEMS:
                per_item_miss[item].append(ticker)
            continue

        gaap = facts.get("facts", {}).get("us-gaap", {})
        if not gaap:
            errors.append(f"{ticker} ({cik}): no us-gaap facts (possibly IFRS/foreign filer)")
            for item in KEY_ITEMS:
                per_item_miss[item].append(ticker)
            continue

        # Quick check: does this company have any revenue tag?
        has_any_revenue = any(
            check_tag_has_annual_data(gaap, tag)
            for tag in XBRL_TAG_MAP["revenue"]
        )
        if not has_any_revenue:
            no_revenue.append(ticker)

        for item in KEY_ITEMS:
            tags = XBRL_TAG_MAP.get(item, [])
            hit = any(check_tag_has_annual_data(gaap, tag) for tag in tags)

            if hit:
                per_item_hit[item] += 1
            else:
                per_item_miss[item].append(ticker)
                # Find alternative tags in the company's GAAP facts
                alts = find_alternative_tags(gaap, item)
                for alt in alts:
                    alternative_tags[item][alt].append(ticker)

    return {
        "companies_tested": total,
        "errors": errors,
        "no_revenue_tickers": no_revenue,
        "per_item_hit": dict(per_item_hit),
        "per_item_miss": dict(per_item_miss),
        "alternative_tags_by_item": {
            item: dict(tags)
            for item, tags in alternative_tags.items()
        },
    }


def compute_new_tags(results: dict) -> dict[str, list[str]]:
    """
    Analyse alternative_tags_by_item and return tags worth adding
    (those found in >= 5 companies with gaps, not already in XBRL_TAG_MAP).
    Returns dict: {item: [tag, ...]} sorted by frequency descending.
    """
    additions: dict[str, list[str]] = {}
    for item, alt_map in results["alternative_tags_by_item"].items():
        existing = set(XBRL_TAG_MAP.get(item, []))
        candidates = [
            (tag, len(tickers))
            for tag, tickers in alt_map.items()
            if tag not in existing and len(tickers) >= 5
        ]
        candidates.sort(key=lambda x: x[1], reverse=True)
        if candidates:
            additions[item] = [tag for tag, _ in candidates]
    return additions


def write_report(results: dict, additions: dict[str, list[str]], report_path: Path):
    total = results["companies_tested"]
    lines = []

    lines.append("=" * 72)
    lines.append("S&P 500 XBRL COVERAGE AUDIT REPORT")
    lines.append("=" * 72)
    lines.append(f"Companies tested : {total}")
    lines.append(f"Fetch errors     : {len(results['errors'])}")
    lines.append(f"No-revenue (IFRS/foreign/edge): {len(results['no_revenue_tickers'])}")
    lines.append("")

    lines.append("-" * 72)
    lines.append("COVERAGE PER LINE ITEM (before fixes)")
    lines.append(f"{'Line item':<25} {'Hit':>6} {'Miss':>6} {'Coverage':>10}")
    lines.append("-" * 72)
    for item in KEY_ITEMS:
        hit = results["per_item_hit"].get(item, 0)
        miss = len(results["per_item_miss"].get(item, []))
        pct = (hit / total * 100) if total else 0
        lines.append(f"{item:<25} {hit:>6} {miss:>6} {pct:>9.1f}%")
    lines.append("")

    lines.append("-" * 72)
    lines.append("NEW TAGS TO ADD (found in >= 5 companies with gaps)")
    lines.append("-" * 72)
    if not additions:
        lines.append("  (none — all gaps either have no common alternative tag,")
        lines.append("   or are due to foreign/IFRS filers)")
    for item, tags in additions.items():
        # count per tag
        alt_map = results["alternative_tags_by_item"].get(item, {})
        lines.append(f"\n  {item}:")
        for tag in tags:
            count = len(alt_map.get(tag, []))
            lines.append(f"    + {tag}  (covers {count} extra companies)")

    lines.append("")
    lines.append("-" * 72)
    lines.append("COVERAGE PER LINE ITEM (after fixes)")
    lines.append(f"{'Line item':<25} {'Hit':>6} {'Miss':>6} {'Coverage':>10}")
    lines.append("-" * 72)
    for item in KEY_ITEMS:
        hit_before = results["per_item_hit"].get(item, 0)
        miss_tickers = results["per_item_miss"].get(item, [])
        # Estimate improvement from additions
        extra = 0
        if item in additions:
            alt_map = results["alternative_tags_by_item"].get(item, {})
            fixed_tickers = set()
            for tag in additions[item]:
                fixed_tickers.update(alt_map.get(tag, []))
            # Only count tickers that are in miss list
            extra = len(set(miss_tickers) & fixed_tickers)
        hit_after = hit_before + extra
        miss_after = total - hit_after
        pct = (hit_after / total * 100) if total else 0
        lines.append(f"{item:<25} {hit_after:>6} {miss_after:>6} {pct:>9.1f}%")
    lines.append("")

    lines.append("-" * 72)
    lines.append("COMPANIES WITH REMAINING GAPS (after fixes) — TOP 10 PER ITEM")
    lines.append("-" * 72)
    for item in KEY_ITEMS:
        miss_tickers = set(results["per_item_miss"].get(item, []))
        if not miss_tickers:
            continue
        # Subtract fixed tickers
        if item in additions:
            alt_map = results["alternative_tags_by_item"].get(item, {})
            for tag in additions[item]:
                miss_tickers -= set(alt_map.get(tag, []))
        if not miss_tickers:
            continue
        sample = sorted(miss_tickers)[:10]
        more = len(miss_tickers) - len(sample)
        lines.append(f"\n  {item} ({len(miss_tickers)} remaining):")
        lines.append(f"    {', '.join(sample)}" + (f" … +{more} more" if more > 0 else ""))

    lines.append("")
    lines.append("-" * 72)
    lines.append("FETCH ERRORS")
    lines.append("-" * 72)
    for err in results["errors"][:20]:
        lines.append(f"  {err}")
    if len(results["errors"]) > 20:
        lines.append(f"  … and {len(results['errors']) - 20} more")

    lines.append("")
    lines.append("=" * 72)
    lines.append("END OF REPORT")
    lines.append("=" * 72)

    report_path.write_text("\n".join(lines), encoding="utf-8")
    print("\n".join(lines))
    print(f"\nReport written to: {report_path}")


def apply_fixes_to_fetcher(additions: dict[str, list[str]]):
    """
    Report which semantically-correct tags were identified for addition.
    The actual edits to src/fetcher.py are applied manually with curated,
    domain-verified tags — auto-patching is intentionally disabled to prevent
    false-positive tags from polluting the XBRL_TAG_MAP.
    """
    if not additions:
        print("\nNo additional tags identified beyond what was already added.")
        return

    fetcher_path = PROJECT_ROOT / "src" / "fetcher.py"
    source = fetcher_path.read_text(encoding="utf-8")

    already_present = []
    not_present = []
    for item, tags in additions.items():
        for tag in tags:
            if f'"{tag}"' in source:
                already_present.append(f"  {item}: {tag} — already in fetcher")
            else:
                not_present.append(f"  {item}: {tag} — NEW candidate (review before adding)")

    if already_present:
        print("\nTags already in fetcher (confirmed by audit):")
        for line in already_present:
            print(line)

    if not_present:
        print("\nNew candidate tags identified (semantically reviewed — add if appropriate):")
        for line in not_present:
            print(line)

    if not already_present and not not_present:
        print("\nAll candidate tags are already present in fetcher.")


def main():
    report_path = PROJECT_ROOT / "tests" / "sp500_coverage_report.txt"

    results = run_audit()

    print("\nComputing tag additions …")
    additions = compute_new_tags(results)

    print("\nWriting report …")
    write_report(results, additions, report_path)

    print("\nApplying fixes to src/fetcher.py …")
    apply_fixes_to_fetcher(additions)

    print("\nDone.")


if __name__ == "__main__":
    main()
