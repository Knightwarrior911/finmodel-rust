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
        "sector": "industrial",
        "pinned": "annual_report.pdf",  # already copied in from extraction_cache
        "search": "Atlas Copco annual report",
    },
    {
        "ticker": "SAND.ST",
        "company": "Sandvik AB",
        "currency": "SEK",
        "sector": "industrial",
        "pinned": None,
        "url": "https://mb.cision.com/Main/208/4114009/3308864.pdf",
        "search": "Sandvik annual report",
    },
    {
        "ticker": "ASML.AS",
        "company": "ASML Holding NV",
        "currency": "EUR",
        "sector": "industrial",
        "pinned": None,
        # FY2025 IFRS-based annual report — covers fiscal 2024 & 2025.
        "url": "https://ourbrand.asml.com/m/6ea363f69344ebd4/original/asml-2025-annual-report-based-on-ifrs.pdf",
        "search": "ASML annual report integrated report",
    },
    {
        "ticker": "NESN.SW",
        "company": "Nestle SA",
        "currency": "CHF",
        "sector": "industrial",
        "pinned": None,
        "url": "https://www.nestle.com/sites/default/files/2025-02/2024-financial-statements-en.pdf",
        "search": "Nestle annual report consolidated financial statements",
    },
    {
        "ticker": "SAP.DE",
        "company": "SAP SE",
        "currency": "EUR",
        "sector": "industrial",
        "pinned": None,
        # sap.com/docs bot-blocks scripted GET (403); srnav mirror serves the
        # identical 2024 Integrated Report PDF. One-time immutable source.
        "url": "https://db.srnav.com/storage/v1/object/public/document-pdfs/44f05cfd-12af-4e22-b9e3-34cffdf3faf1.pdf",
        "search": "SAP integrated report annual report",
    },
    {
        "ticker": "NOVO-B.CO",
        "company": "Novo Nordisk A/S",
        "currency": "DKK",
        "sector": "industrial",
        "pinned": None,
        "url": "https://www.novonordisk.com/content/dam/nncorp/global/en/investors/irmaterial/annual_report/2025/novo-nordisk-annual-report-2024.pdf",
        "search": "Novo Nordisk annual report",
    },
    {
        "ticker": "MC.PA",
        "company": "LVMH Moet Hennessy Louis Vuitton SE",
        "currency": "EUR",
        "sector": "industrial",
        "pinned": None,
        "url": "https://lvmh-com.cdn.prismic.io/lvmh-com/Z-PY3HdAxsiBv6wN_UniversalRegistrationDocument2024.pdf",
        "search": "LVMH annual report consolidated financial statements",
    },
]

SECTORS = ("industrial", "bank", "insurer")

# Canonical line-item universe, per sector. Each sector's keys MUST mirror
# the keys produced by that sector's prompt in src.extractor
# (FINANCIALS_SYSTEM_PROMPT and its bank/insurer variants) so model output
# and ground truth stay directly comparable. The `industrial` schema is
# value-identical to the pre-refactor flat CANONICAL and is pinned by
# tests/test_tieout_sector.py::test_industrial_schema_value_identical.
CANONICAL_BY_SECTOR = {
    "industrial": {
        "income_statement": [
            "revenue", "cogs", "gross_profit", "sga", "rd", "da", "ebit",
            "ebita", "interest_expense", "interest_income", "income_tax",
            "net_income",
        ],
        "balance_sheet": [
            "cash", "accounts_receivable", "inventory",
            "total_current_assets", "ppe_net", "goodwill",
            "intangibles_net", "total_assets", "accounts_payable",
            "long_term_debt", "total_liabilities", "total_equity",
        ],
        "cash_flow_statement": [
            "cfo", "capex", "cfi", "dividends_paid", "cff",
            "net_change_cash",
        ],
    },
    "bank": {
        "income_statement": [
            "interest_income", "interest_expense", "net_interest_income",
            "fee_commission_income", "trading_income",
            "total_operating_income", "loan_loss_provisions",
            "operating_expenses", "pretax_income", "income_tax",
            "net_income",
        ],
        "balance_sheet": [
            "cash_and_central_bank", "loans_to_customers",
            "investment_securities", "total_assets", "customer_deposits",
            "debt_securities_issued", "total_liabilities", "total_equity",
        ],
        "cash_flow_statement": [
            "cfo", "cfi", "cff", "net_change_cash",
        ],
    },
    "insurer": {
        "income_statement": [
            "gross_written_premium", "net_earned_premium",
            "net_investment_income", "net_claims_incurred",
            "acquisition_expenses", "operating_expenses", "pretax_income",
            "income_tax", "net_income",
        ],
        "balance_sheet": [
            "investments", "cash", "total_assets",
            "insurance_contract_liabilities", "total_liabilities",
            "total_equity",
        ],
        "cash_flow_statement": [
            "cfo", "cfi", "cff", "net_change_cash",
        ],
    },
}

ABS_KEYS_BY_SECTOR = {
    "industrial": {
        "cogs", "sga", "rd", "interest_expense", "income_tax",
        "capex", "dividends_paid",
    },
    "bank": {
        "interest_expense", "loan_loss_provisions", "operating_expenses",
        "income_tax",
    },
    "insurer": {
        "net_claims_incurred", "acquisition_expenses", "operating_expenses",
        "income_tax",
    },
}

EXCLUDE_KEYS_BY_SECTOR = {
    "industrial": {"shares_diluted"},
    "bank": set(),
    "insurer": set(),
}


def ticker_filings_dir(ticker: str) -> Path:
    d = FILINGS_DIR / ticker
    d.mkdir(parents=True, exist_ok=True)
    return d
