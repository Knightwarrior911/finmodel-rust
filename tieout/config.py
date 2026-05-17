"""Immutable basket + canonical line-item universe for the tie-out instrument.

7-company European basket. Currency/sector/filing-format diversity is the
point — it stress-tests the non-US extraction path (no SEC EDGAR).

Periods are NOT hardcoded: the ground-truth pass detects the two fiscal years
actually printed on the filing's face statements, and the model path is then
asked for exactly those years. This keeps the comparison apples-to-apples and
makes the test "does the model reproduce THIS filing".
"""
from pathlib import Path

PKG_DIR = Path(__file__).parent
FILINGS_DIR = PKG_DIR / "filings"
GT_DIR = PKG_DIR / "groundtruth"
RESULTS_DIR = PKG_DIR / "results"
for _d in (FILINGS_DIR, GT_DIR, RESULTS_DIR):
    _d.mkdir(parents=True, exist_ok=True)

# Each company: ticker, display name, expected reporting currency, and an
# optional pre-pinned PDF filename under filings/<ticker>/. ATCO is pinned from
# the report already on disk so we get a real baseline tonight with zero
# downloads. The rest are discovered+pinned once on first run, then immutable.
BASKET = [
    {
        "ticker": "ATCO-B.ST",
        "company": "Atlas Copco AB",
        "currency": "SEK",
        "pinned": "annual_report.pdf",  # already copied in from extraction_cache
        "search": "Atlas Copco annual report",
    },
    {
        "ticker": "SAND.ST",
        "company": "Sandvik AB",
        "currency": "SEK",
        "pinned": None,
        "search": "Sandvik annual report",
    },
    {
        "ticker": "ASML.AS",
        "company": "ASML Holding NV",
        "currency": "EUR",
        "pinned": None,
        "search": "ASML annual report integrated report",
    },
    {
        "ticker": "NESN.SW",
        "company": "Nestle SA",
        "currency": "CHF",
        "pinned": None,
        "search": "Nestle annual report consolidated financial statements",
    },
    {
        "ticker": "SAP.DE",
        "company": "SAP SE",
        "currency": "EUR",
        "pinned": None,
        "search": "SAP integrated report annual report",
    },
    {
        "ticker": "NOVO-B.CO",
        "company": "Novo Nordisk A/S",
        "currency": "DKK",
        "pinned": None,
        "search": "Novo Nordisk annual report",
    },
    {
        "ticker": "MC.PA",
        "company": "LVMH Moet Hennessy Louis Vuitton SE",
        "currency": "EUR",
        "pinned": None,
        "search": "LVMH annual report consolidated financial statements",
    },
]

# Canonical line-item universe — MUST mirror the keys produced by
# src.extractor.FINANCIALS_SYSTEM_PROMPT so model output and ground truth are
# directly comparable.
CANONICAL = {
    "income_statement": [
        "revenue", "cogs", "gross_profit", "sga", "rd", "da", "ebit",
        "ebita", "interest_expense", "interest_income", "income_tax",
        "net_income",
    ],
    "balance_sheet": [
        "cash", "accounts_receivable", "inventory", "total_current_assets",
        "ppe_net", "goodwill", "intangibles_net", "total_assets",
        "accounts_payable", "long_term_debt", "total_liabilities",
        "total_equity",
    ],
    "cash_flow_statement": [
        "cfo", "capex", "cfi", "dividends_paid", "cff", "net_change_cash",
    ],
}

# Keys the model prompt defines as POSITIVE magnitudes even though the filing
# prints them as negatives / outflows. Compared on absolute value so a correct
# extraction is not falsely scored as a mismatch.
ABS_KEYS = {
    "cogs", "sga", "rd", "interest_expense", "income_tax",
    "capex", "dividends_paid",
}

# Excluded from the metric denominator: shares_diluted is fractional and lives
# in notes (not the face statements) — it would add noise, not signal. ebita is
# naturally excluded whenever the filing omits it (denominator only counts
# cells the filing actually reports), so no need to list it here.
EXCLUDE_KEYS = {"shares_diluted"}


def ticker_filings_dir(ticker: str) -> Path:
    d = FILINGS_DIR / ticker
    d.mkdir(parents=True, exist_ok=True)
    return d
