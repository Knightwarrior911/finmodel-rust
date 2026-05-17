# financial_model/src/fetcher.py
import logging
import re
import tempfile
import requests
from schemas.financial_data import ReconciledFinancialData, SourceCitation
from src.utils import compute_historical_periods

logger = logging.getLogger(__name__)

EDGAR_HEADERS = {"User-Agent": "FinancialModelBot vinit.paul@gmail.com"}

# ---------------------------------------------------------------------------
# XBRL tag map: canonical line item → ordered list of candidate US-GAAP tags.
# First tag with data for ALL target years wins.
# Include only tags that represent the FULL line item — no partial subtotals.
# ---------------------------------------------------------------------------
XBRL_TAG_MAP: dict[str, list[str]] = {
    # ── Income Statement ────────────────────────────────────────────────────
    "revenue": [
        # Specific ASC-606 tags first — prevent generic "Revenues" from winning for
        # industrial/pharma/tech companies (avoids including discontinued-ops segments).
        "RevenueFromContractWithCustomerExcludingAssessedTax",   # ASC 606 primary (post-2018)
        "SalesRevenueNet",                                        # older manufacturing / retail
        "RevenueFromContractWithCustomerIncludingAssessedTax",    # includes sales tax
        "RegulatedAndUnregulatedOperatingRevenue",                # utilities
        "HealthCareOrganizationRevenue",                          # healthcare
        "RealEstateRevenueNet",                                   # REITs
        "Revenues",                                               # generic total — banks/insurers; fallback for others
        # ── financial-sector specific ─────────────────────────────────────
        "RevenueNotFromContractWithCustomer",                     # non-ASC-606 revenue (financials)
        "InterestAndFeeIncomeLoansAndLeases",                     # banks: primary interest income
        "NoninterestIncome",                                      # banks: fee income
        "BrokerageCommissionsRevenue",                            # broker-dealers (GS, MS etc.)
        "RevenuesNetOfInterestExpense",                           # banks: net revenue (GS, SYF, TFC)
        "BankingFees",                                            # investment banks
        "NetInvestmentIncome",                                    # insurers
        "PremiumsEarnedNet",                                      # P&C insurers
    ],
    "cogs": [
        "CostOfGoodsAndServicesSold",                             # ASC 606 era primary
        "CostOfRevenue",                                          # most companies
        "CostOfGoodsSold",                                        # older goods companies
        "CostOfServices",                                         # pure service companies
        "CostOfGoodsAndServiceExcludingDepreciationDepletionAndAmortization",
        # ── added by coverage audit ──────────────────────────────────────
        "CostsAndExpenses",                                       # total costs when no COGS line (some service co's)
        "OperatingCostsAndExpenses",                              # utilities / energy
        "CostOfPurchasedPower",                                   # electric utilities (NEE, EXC etc.)
        "DirectCostsOfLeasedAndRentedPropertyOrEquipment",        # REITs / leasing
        "CostOfOtherPropertyOperatingExpense",                    # real-estate / hospitality
        "PolicyholderBenefitsAndClaimsIncurredNet",               # insurers: benefit expense
        "BenefitsLossesAndExpenses",                              # insurers: combined benefit+ops
        "InterestExpenseOperating",                               # banks: total interest expense
        "InterestExpense",                                        # banks/other: interest expense
        "InterestExpenseDeposits",                                # banks: deposit interest
    ],
    "gross_profit": [
        "GrossProfit",
        # ── added by coverage audit ──────────────────────────────────────
        "EquityMethodInvestmentSummarizedFinancialInformationGrossProfitLoss",  # equity-method disclosures
        "GrossInvestmentIncomeOperating",                         # insurers: investment gross income
    ],
    "sga": [
        "SellingGeneralAndAdministrativeExpense",                 # combined SG&A
        "GeneralAndAdministrativeExpense",                        # G&A only (some split)
        "SellingAndMarketingExpense",                             # selling only (some split)
        "NoninterestExpense",                                     # banks: operating expense
        "MarketingExpense",                                       # AMZN / some consumer cos
        "OtherGeneralExpense",                                    # miscellaneous
    ],
    "rd": [
        "ResearchAndDevelopmentExpense",
        "ResearchAndDevelopmentExpenseExcludingAcquiredInProcessCost",
        "ProvisionForLoanLeaseAndOtherLosses",                     # banks: CECL-era provision (primary)
        "FinancingReceivableExcludingAccruedInterestCreditLossExpenseReversal",  # banks: loan loss provision
        "ProvisionForCreditLosses",                               # banks: CECL provision (alternate name)
        "ProvisionForLoanAndLeaseLosses",                         # banks: pre-CECL provision
        "ProvisionForLoanLeaseAndOtherCreditLosses",              # banks: combined (older)
        "RestructuringCharges",                                   # misc: restructuring
    ],
    # ── Utility-specific operating expense line items ────────────────────────
    # These replace COGS/SGA/RD for utilities — never appear on standard IS
    "utility_om": [
        "UtilitiesOperatingExpenseMaintenanceAndOperations",      # O&M (most utilities)
        "OperationsAndMaintenanceExpense",
        "UtilitiesOperatingExpenseOperationsAndMaintenance",
    ],
    "utility_taxes_other": [
        "TaxesExcludingIncomeAndExciseTaxes",                     # franchise / property taxes
        "UtilitiesOperatingExpenseTaxes",
    ],
    "utility_fuel": [
        "UtilitiesOperatingExpenseFuelPurchasedPower",            # fuel & purchased power
        "CostOfPurchasedPower",
        "UtilitiesOperatingExpenseFuel",
        "FuelCosts",
    ],
    "ebit": [
        "OperatingIncomeLoss",
        # ── added by coverage audit ──────────────────────────────────────
        "IncomeLossFromContinuingOperationsBeforeIncomeTaxesExtraordinaryItemsNoncontrollingInterest",  # pre-tax income (banks/financials)
        "IncomeLossFromContinuingOperationsBeforeIncomeTaxesMinorityInterestAndIncomeLossFromEquityMethodInvestments",
        "NoninterestExpense",                                     # banks: total operating expense (subtract from revenue for ebit proxy)
    ],
    "interest_expense": [
        "InterestExpense",
        "InterestAndDebtExpense",
        "InterestExpenseDebt",
        # ── added by coverage audit ──────────────────────────────────────
        "InterestExpenseDeposits",                                # banks: interest on deposits
        "InterestExpenseBorrowings",                              # banks
        "FinanceLeaseInterestExpense",                            # finance lease interest
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
        # ── added by coverage audit ──────────────────────────────────────
        "CommonStockSharesOutstanding",                           # fallback for companies without WA diluted
    ],
    "da": [
        # CF-statement add-back concept (preferred — most companies use one of these two in CFS)
        "DepreciationAndAmortization",
        "DepreciationDepletionAndAmortization",
        "DepreciationAmortizationAndAccretionNet",
        "DepreciationDepletionAndAmortizationExcludingDiscontinuedOperations",
        "DepreciationAndAmortizationDiscontinuedOperations",
        "AmortizationOfIntangibleAssets",
        "Depreciation",
        "DepreciationNonproduction",
    ],
    # ── Balance Sheet — Assets ──────────────────────────────────────────────
    "cash": [
        "CashAndCashEquivalentsAtCarryingValue",
        "CashCashEquivalentsAndShortTermInvestments",
        "CashAndCashEquivalents",
        "Cash",
        "CashAndDueFromBanks",
        # ── added by coverage audit ──────────────────────────────────────
        "CashCashEquivalentsRestrictedCashAndRestrictedCashEquivalents",   # post-ASU 2016-18 (common)
    ],
    "accounts_receivable": [
        "AccountsReceivableNetCurrent",
        "AccountsReceivableNet",
        "ReceivablesNetCurrent",
        "TradeAndOtherReceivablesNetCurrent",
        # ── added by coverage audit ──────────────────────────────────────
        "NotesAndLoansReceivableNetCurrent",                      # banks / financial cos
        "LoansAndLeasesReceivableNetReportedAmount",              # banks: loans (primary earning asset)
        "LoansAndLeasesReceivableNetOfDeferredIncome",            # banks (older tag)
        "PremiumsAndOtherReceivablesNet",                         # insurers
        "ReceivablesFromBrokersDealersAndClearingOrganizations",  # broker-dealers
        "AccountsReceivableNetNoncurrent",                        # some companies report noncurrent only
        "ReinsuranceRecoverables",                                # insurers: reinsurance asset
    ],
    "inventory": [
        "InventoryNet",
        "Inventories",
        "InventoryFinishedGoodsNetOfReserves",
        # ── added by coverage audit ──────────────────────────────────────
        "InventoryRawMaterialsAndSupplies",                       # raw-material-only reporters
        "EnergyRelatedInventory",                                 # energy / utilities
        "EnergyRelatedInventoryOtherFossilFuel",                  # oil/gas specific
        "InventoryRawMaterials",                                  # manufacturing split
        "InventoryWorkInProcess",                                 # WIP-only reporters
        "RealEstateInventory",                                    # homebuilders (general)
        "InventoryRealEstate",                                    # homebuilders: total RE inventory
        "InventoryOperativeBuilders",                             # homebuilders: operative/WIP
        "InventoryOperativeBuildersOther",                        # homebuilders: other
        "InventoryHomesUnderConstruction",                        # homebuilders: WIP homes
        "InventoryLandHeldForDevelopmentAndSale",                 # homebuilders: land bank
        "OtherInventorySupplies",                                 # maintenance/supplies inventory
    ],
    "total_current_assets": [
        "AssetsCurrent",
    ],
    "ppe_net": [
        "PropertyPlantAndEquipmentNet",
        "PropertyPlantAndEquipmentAndFinanceLeaseRightOfUseAssetAfterAccumulatedDepreciationAndAmortization",
        # ── added by coverage audit ──────────────────────────────────────
        "RealEstateInvestmentPropertyNet",                        # REITs: net real estate
        "RealEstateAndAccumulatedDepreciation",                   # older REIT tag
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
        # ── added by coverage audit ──────────────────────────────────────
        "AccountsPayableTradeCurrent",                            # trade payables only
        "AccountsPayableAndAccruedLiabilitiesCurrentAndNoncurrent",  # current+noncurrent combined
        "AccountsPayableCurrentAndNoncurrent",                    # current+noncurrent (older)
        "AccountsPayableTradeCurrentAndNoncurrent",               # trade only, both terms
        "OtherAccountsPayableAndAccruedLiabilities",              # catch-all for smaller reporters
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
        # ── added by coverage audit ──────────────────────────────────────
        "ConvertibleLongTermNotesPayable",                        # convertible debt issuers
        "LongTermDebtFairValue",                                  # fair-value reporters
        "UnsecuredDebt",                                          # some financials
        "SecuredDebt",                                            # secured-only reporters
        "SubordinatedLongTermDebt",                               # subordinated debt reporters
        "JuniorSubordinatedNotes",                                # trust preferred / hybrid capital
        "FinanceLeaseLiabilityNoncurrent",                        # finance leases as debt
        "Deposits",                                               # banks: customer deposits
        "InterestBearingDeposits",                                # banks: interest-bearing deposits
        "PolicyholderFunds",                                      # insurers: policyholder funds
        "LiabilityForFuturePolicyBenefits",                       # insurers: LFPB (LDTI)
    ],
    "total_liabilities": [
        "Liabilities",
        # LiabilitiesAndStockholdersEquity == total_assets (A=L+E identity) — NEVER use as total_liabilities.
        # If "Liabilities" is absent the derivation step computes: total_assets − total_equity − rnci.
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
        # ── added by coverage audit ──────────────────────────────────────
        "PaymentsToAcquireOtherPropertyPlantAndEquipment",        # some split filers
        "PaymentsToAcquireRealEstate",                            # REITs / real-estate
        "PaymentsToAcquireAndDevelopRealEstate",                  # developer REITs
        "PaymentsForConstructionInProcessAndProductiveAssets",    # construction-heavy
        "PurchaseOfPropertyAndEquipment",                         # older/alternative label
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
    # ── Income Statement additions ──────────────────────────────────────────
    "nci_income_loss": [
        "NetIncomeLossAttributableToNoncontrollingInterest",
        "MinorityInterestInNetIncomeLossOfConsolidatedEntities",
        "IncomeLossFromContinuingOperationsAttributableToNoncontrollingEntity",
    ],
    # ── Balance Sheet — Deferred Revenue ───────────────────────────────────
    "deferred_revenue_current": [
        "DeferredRevenueCurrent",
        "ContractWithCustomerLiabilityCurrent",
        "DeferredRevenueAndCreditsCurrent",
    ],
    "deferred_revenue_lt": [
        "DeferredRevenueNoncurrent",
        "ContractWithCustomerLiabilityNoncurrent",
        "DeferredRevenueAndCreditsNoncurrent",
    ],
    # ── Cash Flow — Investing ───────────────────────────────────────────────
    "investments_net_cfi": [
        "PaymentsToAcquireAvailableForSaleSecurities",
        "PaymentsToAcquireShortTermInvestments",
        "PaymentsToAcquireInvestments",
        "ProceedsFromSaleMaturityAndCollectionOfShorttermInvestments",
        "ProceedsFromSaleAndMaturityOfAvailableForSaleSecurities",
    ],
}

# ── Operating Expense Concept Catalog ──────────────────────────────────────────
# Phase 2: Every US-GAAP operating expense concept the company actually files
# becomes a row in the IS, labelled with its XBRL concept label.
#
# Format: (tag_name, category, overlap_group, priority)
#   category: "cogs" | "opex_rd" | "opex"  — where it goes in the IS
#   overlap_group: concepts in the same group are alternatives — prefer
#                  components (lower priority) over combined totals
#   priority: lower = more granular (preferred); higher = aggregate
OPEX_CATALOG: list[tuple[str, str, str, int]] = [
    # ── Cost of Revenue ──────────────────────────
    ("CostOfGoodsAndServicesSold", "cogs", "cogs", 0),
    ("CostOfRevenue",              "cogs", "cogs", 1),
    ("CostOfGoodsSold",            "cogs", "cogs", 2),
    ("CostOfServices",             "cogs", "cogs", 3),
    ("CostOfGoodsAndServiceExcludingDepreciationDepletionAndAmortization",
     "cogs", "cogs", 4),
    # ── R&D ──────────────────────────────────────
    ("ResearchAndDevelopmentExpense", "opex_rd", "rd", 0),
    ("ResearchAndDevelopmentExpenseExcludingAcquiredInProcessCost",
     "opex_rd", "rd", 1),
    # ── SG&A — components preferred over combined ─
    ("SellingAndMarketingExpense",          "opex", "sga_comp", 0),
    ("GeneralAndAdministrativeExpense",     "opex", "sga_comp", 0),
    ("SellingGeneralAndAdministrativeExpense", "opex", "sga_agg",  1),
    # ── Marketing / Advertising (standalone) ─────
    ("MarketingExpense",  "opex", "mkt", 0),
    ("AdvertisingExpense","opex", "mkt", 1),
    # ── Other common OpEx ────────────────────────
    ("RestructuringCharges",              "opex", "restruct", 0),
    ("RestructuringCostsAndAssetImpairmentCharges", "opex", "restruct", 1),
    ("BusinessCombinationAcquisitionRelatedCosts",  "opex", "ma", 0),
    ("GoodwillImpairmentLoss",             "opex", "impair", 0),
    ("ImpairmentOfLongLivedAssetsHeldForUse","opex","impair", 1),
    ("AssetImpairmentCharges",             "opex", "impair", 2),
    ("LitigationSettlementExpense",        "opex", "legal", 0),
    ("LossContingencyLossInPeriod",        "opex", "legal", 1),
    ("GainLossOnDispositionOfAssets",      "opex", "gainloss", 0),
    ("SeveranceCosts1",                    "opex", "severance", 0),
    ("OtherGeneralExpense",               "opex", "other", 0),
    ("OtherOperatingExpenses",             "opex", "other", 1),
    ("OtherOperatingIncomeExpenseNet",     "opex", "other", 2),
    # ── Sector-specific operating costs ──────────
    ("FulfillmentExpense",         "opex", "fulfill", 0),
    ("TechnologyServicesExpense",  "opex", "tech", 0),
    ("CostOfPurchasedPower",       "cogs", "util_cogs", 0),
    ("UtilitiesOperatingExpenseMaintenanceAndOperations", "opex", "util", 0),
    ("UtilitiesOperatingExpenseFuelPurchasedPower",       "opex", "util", 0),
]

# Concepts to skip — aggregates whose components we may already have, or
# non-operating / balance-sheet / cash-flow items that pollute the survey
_OPEX_SKIP = frozenset({
    "OperatingExpenses",
    "CostsAndExpenses",
    "OperatingCostsAndExpenses",
    "DepreciationDepletionAndAmortization",
    "Depreciation",
    "DepreciationNonproduction",
    "AmortizationOfIntangibleAssets",
    "DepreciationAmortizationAndAccretionNet",
    "ShareBasedCompensation",
    "AllocatedShareBasedCompensationExpense",
    "InterestExpense",
    "InterestExpenseDebt",
    "InterestExpenseDeposits",
    "InterestExpenseBorrowings",
    "InvestmentIncomeInterest",
    "InterestAndDividendIncomeOperating",
    "InterestIncomeOperating",
    "IncomeTaxExpenseBenefit",
    "OperatingLeaseCost",
    "VariableLeaseCost",
    "LeaseCost",
    "FinanceLeaseInterestExpense",
})


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
    """Return dict of {year_str: best_entry} for a single XBRL tag.

    For a given calendar year, multiple 10-K entries can exist: the original
    filing AND restated comparatives in subsequent years' 10-Ks.
    We prefer the MOST RECENT filing so that restated comparatives — e.g., after a
    spin-off the 2023 10-K restates 2021/2022 to show continuing-ops only — are used
    rather than the original inclusive figures. This gives accurate projection baselines.
    """
    units = gaap.get(tag, {}).get("units", {})
    entries = units.get(currency) or units.get("USD") or units.get("shares") or []
    annual = [e for e in entries if e.get("form") in ("10-K", "20-F") and e.get("fp") == "FY"]
    by_year: dict[str, dict] = {}
    for e in annual:
        yr = e["end"][:4]
        if yr not in by_year:
            by_year[yr] = e
        else:
            existing = by_year[yr]
            if e["end"] > existing["end"]:
                # Later fiscal year end within same calendar year → more complete
                by_year[yr] = e
            elif e["end"] == existing["end"]:
                # Same period end: prefer most-recent filing (latest filed date) to pick
                # up restated comparatives (spin-offs, discontinued-ops reclassifications).
                if e.get("filed", "0000-00-00") > existing.get("filed", "0000-00-00"):
                    by_year[yr] = e
    return by_year


def _extract_tag_annual(
    gaap: dict,
    tags: list[str],
    n_periods: int,
    currency: str = "USD",
    target_years: list[str] | None = None,
) -> tuple[list[float | None], str | None]:
    """
    Return (values_list, tag_used) for the first tag in `tags` that satisfies:
      - If target_years provided: tag has data for ALL of those specific years.
      - Otherwise: tag has >= n_periods annual entries (takes the most recent n).

    Using target_years prevents cross-era contamination (e.g. a tag that stopped
    filing in 2014 matching a 3-period request for 2023-2025).

    Pass 2 (partial coverage): if no tag covers all years, returns the best-coverage
    tag with None for missing years. Common for companies that change XBRL taxonomy
    mid-window (e.g. switch to custom extension tags for recent filings).
    """
    # Pass 1: strict — all target years present in one tag
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

    # Pass 2: partial coverage — pick tag with most years covered, fill gaps with None
    if target_years:
        best_tag: str | None = None
        best_vals: list | None = None
        best_count = 0
        for tag in tags:
            if tag not in gaap:
                continue
            by_year = _annual_by_year(gaap, tag, currency)
            covered = sum(1 for yr in target_years if yr in by_year)
            if covered > best_count:
                best_count = covered
                best_tag = tag
                best_vals = [
                    round(by_year[yr]["val"] / 1e6, 2) if yr in by_year else None
                    for yr in target_years
                ]
        if best_tag and best_vals and any(v is not None for v in best_vals):
            n = len(target_years)
            return best_vals, f"{best_tag}(partial:{best_count}/{n})"

    return [], None


# XBRL component tags for breaking out D&A into depreciation vs. amortization
_DA_COMPONENTS = {
    "depreciation": ["Depreciation"],
    "amortization": ["AmortizationOfIntangibleAssets"],
}


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

    # ── REIT: FFO = Net Income + D&A (approximate, excludes gains/losses) ──
    if not present(is_data, "ffo") and present(is_data, "net_income") and present(is_data, "da"):
        vals = [
            round((ni or 0) + (da or 0), 2)
            for ni, da in zip(safe(is_data, "net_income"), safe(is_data, "da"))
        ]
        add(is_data, "ffo", vals, "derived:NI+DA", 0.85,
            "ffo: derived from net_income + depreciation & amortization (approximate; excludes gains/losses on property sales)")
        # AFFO = FFO − recurring CapEx (approximate); use CapEx as proxy
        if present(cfs_data, "capex"):
            affo_vals = [
                round((ffo or 0) - abs((cfs_data.get("capex", [0]*n)[i] or 0)), 2)
                for i, ffo in enumerate(vals)
                if i < n
            ]
            add(is_data, "affo", affo_vals, "derived:FFO-Capex", 0.75,
                "affo: derived from FFO − capex (approximate; should use maintenance capex only)")

    # ── IS: interest_expense — fill gaps from long-term debt × assumed rate ──
    ie_vals = safe(is_data, "interest_expense")
    debt_vals = safe(bs_data, "long_term_debt")
    ie_has_gaps = any(v is None for v in ie_vals[:n]) if len(ie_vals) >= n else True
    if ie_has_gaps and any(d is not None for d in debt_vals[:n]):
        rate = 0.035
        filled = [
            v if v is not None else round((debt_vals[i] or 0) * rate, 2)
            for i, v in enumerate(ie_vals[:n])
        ]
        if len(ie_vals) < n:
            filled = filled + [round((debt_vals[i] or 0) * rate, 2) for i in range(len(ie_vals), n)]
        add(is_data, "interest_expense", filled, "derived:debt×3.5%", 0.70,
            "interest_expense: gaps filled from long-term debt × 3.5%")

    # ── IS: interest_income — derive from cash × assumed yield ──
    ii_vals = safe(is_data, "interest_income")
    cash_vals = safe(bs_data, "cash")
    ii_has_gaps = any(v is None for v in ii_vals[:n]) if len(ii_vals) >= n else True
    if ii_has_gaps and any(c is not None for c in cash_vals[:n]):
        yield_pct = 0.02
        filled = [
            v if v is not None else round((cash_vals[i] or 0) * yield_pct, 2)
            for i, v in enumerate(ii_vals[:n])
        ]
        if len(ii_vals) < n:
            filled = filled + [round((cash_vals[i] or 0) * yield_pct, 2) for i in range(len(ii_vals), n)]
        add(is_data, "interest_income", filled, "derived:cash×2%", 0.70,
            "interest_income: gaps filled from cash × 2% yield")

    # ── CFS: capex gap warning ────────────────────────────────────────────────
    # Companies sometimes switch to custom XBRL extension tags mid-window,
    # causing partial or complete capex gaps. Detect and flag for Sources tab.
    capex_vals = safe(cfs_data, "capex")
    cfi_vals   = safe(cfs_data, "cfi")
    missing_capex = [i for i, v in enumerate(capex_vals) if v is None or v == 0]
    if missing_capex and any(abs(cfi_vals[i] or 0) > 100 for i in missing_capex):
        gap_years = [target_years[i] for i in missing_capex if i < len(target_years)]
        notes.append(
            f"WARN capex: no standard XBRL tag found for {gap_years}. "
            "Company may use a custom extension tag not exposed by EDGAR companyfacts API. "
            "Capex shown as 0 / blank for these periods — CFI total is correct."
        )

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


def _detect_segments(
    facts: dict, target_years: list[str], currency: str, total_revenue: list
) -> list[dict]:
    """Scan company-specific XBRL namespaces for revenue segment tags.

    Returns [{"label": str, "key": str, "values": [float|None]}] for tags that
    look like revenue segments — i.e., revenue-related names in a non-standard
    namespace where at least half the target years have data, and their sum
    approximates total revenue. Returns empty list if no meaningful segments found.
    """
    _REV_KEYWORDS = ("revenue", "sales", "income", "service")
    _SKIP_KEYWORDS = ("regulatory", "credit", "lease", "leasing", "other")

    candidates: dict[str, dict] = {}  # tag_name → {label, values}
    for namespace, tags in facts.get("facts", {}).items():
        if namespace in ("us-gaap", "dei", "srt"):
            continue
        for tag_name, tag_data in tags.items():
            lc = tag_name.lower()
            if not any(kw in lc for kw in _REV_KEYWORDS):
                continue
            if all(kw in lc for kw in _SKIP_KEYWORDS):
                continue
            units = tag_data.get("units", {})
            entries = units.get(currency) or units.get("USD") or []
            annual = [
                e for e in entries
                if e.get("form") in ("10-K", "20-F") and e.get("fp") == "FY"
            ]
            by_year: dict[str, float] = {}
            for e in annual:
                yr = e["end"][:4]
                if yr not in by_year:
                    by_year[yr] = round(e["val"] / 1e6, 2)
            hits = sum(1 for yr in target_years if yr in by_year)
            if hits < max(1, len(target_years) // 2):
                continue
            vals = [by_year.get(yr) for yr in target_years]
            label = tag_data.get("label", tag_name)
            candidates[tag_name] = {"label": label, "values": vals}

    if not candidates:
        return []

    # Verify segments sum to ~total revenue (within 15%) to confirm they are sub-segments
    if total_revenue and any(v is not None for v in total_revenue):
        seg_sum = [
            sum(c["values"][j] or 0 for c in candidates.values())
            for j in range(len(target_years))
        ]
        total_sum = sum(v or 0 for v in total_revenue)
        seg_total = sum(seg_sum)
        if total_sum > 0 and abs(seg_total / total_sum - 1) > 0.15:
            return []

    # Build structured return: clean key from tag_name, use XBRL label for display
    import re
    result = []
    for tag_name, c in candidates.items():
        slug = re.sub(r'[^a-z0-9]', '_', tag_name.lower())
        key = f"rev_seg_{slug}"
        result.append({"label": c["label"], "key": key, "values": c["values"]})
    return result


def _detect_opex_items(
    gaap: dict, target_years: list[str], currency: str
) -> list[dict]:
    """Scan company XBRL facts for all filed operating expense concepts.

    Returns [{label, key, category, values}] sorted by magnitude (largest first).
    Uses OPEX_CATALOG to identify operating expense concepts and resolve overlaps.
    """
    import re

    # Step 1: gather all candidate concepts with data
    candidates: dict[str, dict] = {}  # tag_name → {category, group, prio, label, values}
    for tag_name, category, group, priority in OPEX_CATALOG:
        if tag_name in _OPEX_SKIP:
            continue
        if tag_name not in gaap:
            continue
        by_year = _annual_by_year(gaap, tag_name, currency)
        hits = sum(1 for yr in target_years if yr in by_year)
        if hits < max(1, len(target_years) // 2):
            continue
        vals = [round(by_year[yr]["val"] / 1e6, 2) if yr in by_year else None
                for yr in target_years]
        label = gaap[tag_name].get("label", tag_name)
        candidates[tag_name] = {
            "category": category, "group": group, "priority": priority,
            "label": label, "values": vals,
        }

    if not candidates:
        return []

    # Step 2: resolve overlap groups — prefer components over aggregates
    groups: dict[str, list[str]] = {}
    for tag, c in candidates.items():
        g = c["group"]
        groups.setdefault(g, []).append(tag)
    groups[g].sort(key=lambda t: candidates[t]["priority"])

    keep: set[str] = set()
    for group_name, tags in groups.items():
        if group_name in ("cogs",):
            if tags:
                keep.add(tags[0])
        elif group_name == "sga_comp":
            for t in tags:
                keep.add(t)
        elif group_name == "sga_agg":
            # SG&A combined — only keep if no components from sga_comp exist
            if not any(t for t in candidates if candidates[t]["group"] == "sga_comp"):
                if tags:
                    keep.add(tags[0])
        elif group_name == "rd":
            if tags:
                keep.add(tags[0])
        else:
            best = tags[0]
            best_vals = candidates[best]["values"]
            max_val = max(abs(v) for v in best_vals if v is not None)
            if max_val > 1:
                keep.add(best)

    # Step 2b: prune known parent→child relationships to prevent double counting
    _PARENT_HAS_CHILD: dict[str, set[str]] = {
        "SellingAndMarketingExpense":          {"AdvertisingExpense"},
        "SellingGeneralAndAdministrativeExpense": {"SellingAndMarketingExpense", "GeneralAndAdministrativeExpense", "AdvertisingExpense", "MarketingExpense"},
        "MarketingExpense":                    {"AdvertisingExpense"},
    }
    for parent_tag, child_tags in _PARENT_HAS_CHILD.items():
        if parent_tag in keep:
            keep -= child_tags

    # Step 3: build result sorted by magnitude
    def avg_mag(tag: str) -> float:
        vals = [abs(v) for v in candidates[tag]["values"] if v is not None]
        return sum(vals) / len(vals) if vals else 0

    result = []
    for tag in sorted(keep, key=avg_mag, reverse=True):
        c = candidates[tag]
        slug = re.sub(r'[^a-z0-9]', '_', tag.lower())
        key = f"opex_{slug}"
        result.append({
            "label": c["label"], "key": key, "category": c["category"],
            "group": c["group"],
            "values": c["values"],
        })
    return result


def _fetch_is_rfile_detail(cik_int: int, target_years: list[str]) -> dict:
    """Parse the income statement R-file from the most-recent 10-K XBRL viewer.

    Returns {"rev_detail": [...], "cogs_detail": [...]} where each item is
    {"label": str, "key": str, "values": [float|None]}.
    Empty dict on any failure (non-fatal — falls back to aggregate XBRL totals).
    """
    try:
        import re as _re
        import requests as _req
        from bs4 import BeautifulSoup as _BS

        # ── Step 1: find most-recent 10-K accession from submissions API ────
        cik_padded = str(cik_int).zfill(10)
        subs = _req.get(
            f"https://data.sec.gov/submissions/CIK{cik_padded}.json",
            headers=EDGAR_HEADERS,
        ).json()
        filings = subs.get("filings", {}).get("recent", {})
        accn = next(
            (an.replace("-", "") for form, an in zip(
                filings.get("form", []), filings.get("accessionNumber", [])
            ) if form == "10-K"),
            None,
        )
        if not accn:
            return {}

        # ── Step 2: find IS R-file number from FilingSummary.xml ────────────
        import xml.etree.ElementTree as _ET
        base = f"https://www.sec.gov/Archives/edgar/data/{cik_int}/{accn}"
        summary_xml = _req.get(f"{base}/FilingSummary.xml", headers=EDGAR_HEADERS).text
        root = _ET.fromstring(summary_xml)
        is_rfile = None
        for report in root.findall(".//Report"):
            long_name = (report.findtext("LongName") or "").lower()
            rfile = report.findtext("HtmlFileName") or ""
            if any(w in long_name for w in ("statement of operations", "income from operations",
                                             "statements of operations")):
                is_rfile = rfile
                break
        if not is_rfile:
            return {}

        # ── Step 3: parse the R-file HTML table ─────────────────────────────
        html = _req.get(f"{base}/{is_rfile}", headers=EDGAR_HEADERS).text
        soup = _BS(html, "html.parser")
        rows = []
        for tr in soup.find_all("tr"):
            cells = [td.get_text(strip=True) for td in tr.find_all(["th", "td"])]
            if cells:
                rows.append(cells)

        if not rows:
            return {}

        # ── Step 4: map column indices → years from header row ───────────────
        # Header row contains date strings like "Jan. 31, 2026" or "12 Months Ended"
        col_year: dict[int, str] = {}
        header_row_len: int = 0
        for row in rows[:5]:
            for ci, cell in enumerate(row):
                m = _re.search(r"\b(20\d{2})\b", cell)
                if m:
                    col_year[ci] = m.group(1)
            if col_year:
                header_row_len = len(row)
                break

        if not col_year:
            return {}

        # Detect column offset: XBRL viewer header rows often have fewer columns
        # than data rows because data rows include label + footnote prefix columns.
        # Example: header has [2026, 2025, 2024] (3 cols) but data rows have
        # [label, footnote, val_2026, val_2025, val_2024] (5 cols) → offset=2.
        data_ncols = 0
        for row in rows[2:8]:
            if len(row) > header_row_len:
                data_ncols = max(data_ncols, len(row))
        col_offset = data_ncols - header_row_len if data_ncols > header_row_len else 0
        if col_offset > 0:
            col_year = {ci + col_offset: yr for ci, yr in col_year.items()}

        def _parse_val(s: str) -> float | None:
            s = _re.sub(r"[\$,\(\)\[\]\s]", "", s)
            s = _re.sub(r"^\d+$", "", s) if len(s) <= 3 else s  # strip short footnotes
            if not s or not _re.search(r"\d", s):
                return None
            try:
                return float(s.replace(",", ""))
            except ValueError:
                return None

        def _row_vals(cells: list[str]) -> dict[str, float | None]:
            """Map year → parsed value for a data row."""
            out: dict[str, float | None] = {}
            for ci, yr in col_year.items():
                if ci < len(cells):
                    out[yr] = _parse_val(cells[ci])
            return out

        def _make_key(label: str, prefix: str) -> str:
            slug = _re.sub(r"[^a-z0-9]+", "_", label.lower()).strip("_")
            return f"{prefix}{slug}"[:60]

        # ── Step 5: state-machine scan for segment sections ──────────────────
        # Consolidated totals appear first; segment sections repeat below them.
        # A segment section looks like:
        #   <segment name row, empty values>
        #   Revenues:
        #   Total revenues | ... | val1 | val2 | val3
        #   Cost of revenues:
        #   Total cost of revenues | ... | val1 | val2 | val3
        _REV_TOTAL   = {"total revenues", "total revenue"}
        _COST_TOTAL  = {"total cost of revenues", "total cost of revenue"}
        _REV_HDR     = {"revenues:", "revenues", "revenue:"}
        _COST_HDR    = {"cost of revenues:", "cost of revenues", "cost of revenue:"}
        _SKIP_LABELS = {"revenues:", "cost of revenues:", "operating expenses:",
                        "operating expenses", "cost of revenues", "revenues",
                        "12 months ended", "consolidated statements of operations"}

        # Standard IS line labels that are NOT segment names
        _STD_LABELS = {
            "total revenues", "total revenue",
            "total cost of revenues", "gross profit",
            "research and development", "sales and marketing",
            "general and administrative", "restructuring",
            "total operating expenses", "income from operations",
            "net income", "basic net income per share", "diluted net income per share",
        }

        rev_detail:  list[dict] = []
        cogs_detail: list[dict] = []

        # Skip until we've seen at least one consolidated "Total revenues" row
        found_consolidated_rev = False
        found_consolidated_cost = False
        current_segment: str | None = None
        in_rev_section   = False
        in_cost_section  = False

        for row in rows:
            if not row:
                continue
            label = row[0].strip().rstrip(":")
            label_lc = label.lower().strip()

            # Detect consolidated totals (first occurrences)
            if not found_consolidated_rev and label_lc in _REV_TOTAL:
                found_consolidated_rev = True
                current_segment = None
                in_rev_section = in_cost_section = False
                continue
            if not found_consolidated_cost and label_lc in _COST_TOTAL and found_consolidated_rev:
                found_consolidated_cost = True
                current_segment = None
                in_rev_section = in_cost_section = False
                continue

            # Only process sub-items after we've seen consolidated totals
            if not found_consolidated_rev:
                continue

            # Section headers
            if label_lc in _REV_HDR:
                in_rev_section = True
                in_cost_section = False
                continue
            if label_lc in _COST_HDR:
                in_cost_section = True
                in_rev_section = False
                continue

            # Check if this row is a new segment name (label-only, no meaningful values)
            vals_in_row = {ci: _parse_val(c) for ci, c in enumerate(row) if ci > 0}
            has_values = any(v is not None for v in vals_in_row.values())

            if (not has_values and label and label_lc not in _SKIP_LABELS
                    and label_lc not in _STD_LABELS and label_lc not in _REV_HDR
                    and label_lc not in _COST_HDR and len(label) > 3):
                current_segment = label
                in_rev_section = in_cost_section = False
                continue

            # Capture segment rev/cost totals
            if current_segment and has_values:
                yr_vals = _row_vals(row)
                values = [yr_vals.get(yr) for yr in target_years]

                if in_rev_section and label_lc in _REV_TOTAL:
                    rev_detail.append({
                        "label": current_segment,
                        "key": _make_key(current_segment, "rev_seg_"),
                        "values": values,
                    })
                    in_rev_section = False
                elif in_cost_section and label_lc in _COST_TOTAL:
                    cost_label = f"Cost of {current_segment.lower()}"
                    cogs_detail.append({
                        "label": cost_label,
                        "key": _make_key(cost_label, "cogs_seg_"),
                        "values": values,
                    })
                    in_cost_section = False

        # Sanity: verify rev_detail sums within 2% of total revenue from XBRL
        if rev_detail:
            total_check = [
                sum(item["values"][i] or 0 for item in rev_detail)
                for i in range(len(target_years))
            ]
            # if any year's sum is far off from 0, check plausibility
            # (can't verify against XBRL here, but non-zero is a good sign)
            if all(t == 0 for t in total_check):
                rev_detail = []
                cogs_detail = []

        return {"rev_detail": rev_detail, "cogs_detail": cogs_detail}

    except Exception as e:
        logger.debug("IS R-file detail fetch failed for CIK %s: %s", cik_int, e)
        return {}


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
        "nci_income_loss",
        "eps_basic", "eps_diluted", "shares_basic", "shares_diluted", "da",
        # Utility-specific cost items (empty for non-utilities — harmless)
        "utility_om", "utility_taxes_other", "utility_fuel",
    ]
    bs_keys = [
        "cash", "accounts_receivable", "inventory", "total_current_assets",
        "ppe_net", "goodwill", "intangibles_net", "total_assets",
        "accounts_payable", "total_current_liabilities",
        "deferred_revenue_current", "deferred_revenue_lt",
        "long_term_debt", "total_liabilities",
        "retained_earnings", "total_equity", "redeemable_nci",
    ]
    cfs_keys = [
        "cfo", "capex", "investments_net_cfi", "cfi", "cff",
        "dividends_paid", "buybacks", "net_change_cash", "fx_effect_on_cash",
        "da",  # D&A add-back belongs to CF statement — authoritative source for total D&A
    ]

    load(is_data, is_keys)
    load(bs_data, bs_keys)
    load(cfs_data, cfs_keys)

    # D&A: CF statement is the authoritative source (captures total add-back, including
    # amortization of acquired intangibles that is embedded inside COGS/SG&A on the IS).
    # If cfs_data["da"] > is_data["da"] (or IS is missing), override IS with CFS value.
    cfs_da = cfs_data.get("da", [])
    is_da  = is_data.get("da",  [])
    if cfs_da and any(v and v != 0 for v in cfs_da):
        cfs_sum = sum(v or 0 for v in cfs_da)
        is_sum  = sum(v or 0 for v in is_da)
        if cfs_sum > is_sum or not is_da:
            is_data["da"] = cfs_da

    # Detect company-specific revenue segments from non-standard XBRL namespaces
    segment_revenues = _detect_segments(facts, target_years, currency,
                                         is_data.get("revenue", []))
    for seg in segment_revenues:
        is_data[seg["key"]] = seg["values"]

    # Try to fetch IS R-file detail (revenue breakdown + cost breakdown from XBRL viewer)
    # This captures dimensional data not exposed by the companyfacts API.
    rfile_detail = _fetch_is_rfile_detail(int(cik), target_years) if cik else {}
    rev_detail  = rfile_detail.get("rev_detail",  [])
    cogs_detail = rfile_detail.get("cogs_detail", [])
    # R-file segments supersede custom-namespace segments (more reliable source)
    if rev_detail:
        segment_revenues = rev_detail
        for seg in rev_detail:
            is_data[seg["key"]] = seg["values"]
    for cd in cogs_detail:
        is_data[cd["key"]] = cd["values"]

    # Detect actual operating expense line items from XBRL concepts
    opex_items = _detect_opex_items(gaap, target_years, currency)
    for oi in opex_items:
        is_data[oi["key"]] = oi["values"]

    derivation_notes = _apply_derivations(
        is_data, bs_data, cfs_data, gaap, sources, period_labels,
        periods_historical, currency, target_years
    )

    # Build filing_labels: standard key → XBRL concept label from company's actual filing
    filing_labels: dict[str, str] = {}
    for key, citations in sources.items():
        if not citations or not citations[0].xbrl_tag:
            continue
        xbrl = citations[0].xbrl_tag
        if xbrl.startswith("derived:"):
            continue  # skip derived/computed items
        # xbrl format: "us-gaap:ConceptName" or "us-gaap:ConceptName(partial:2/3)"
        tag = xbrl.split("(")[0]  # strip partial suffix
        tag = tag.split(":")[-1]  # strip namespace prefix
        if tag in gaap:
            label = gaap[tag].get("label", "")
            if label:
                filing_labels[key] = label

    notes_meta = {"filing_labels": filing_labels}
    if segment_revenues:
        notes_meta["revenue_segments"] = [
            {"label": s["label"], "key": s["key"]}
            for s in segment_revenues
        ]
    if cogs_detail:
        notes_meta["cogs_detail"] = [
            {"label": c["label"], "key": c["key"]} for c in cogs_detail
        ]
    if opex_items:
        notes_meta["opex_items"] = [
            {"label": o["label"], "key": o["key"], "category": o["category"], "group": o.get("group", "")}
            for o in opex_items
        ]

    return ReconciledFinancialData(
        ticker=str(cik),
        company_name=entity,
        currency=currency,
        fiscal_year_end="",
        periods=period_labels,
        income_statement=is_data,
        balance_sheet=bs_data,
        cash_flow_statement=cfs_data,
        notes=notes_meta,
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


def _latest_complete_fy_year(fiscal_year_end: str) -> int:
    """Return the most recently completed fiscal year (allowing 90 days for report publication)."""
    import datetime as _dt
    today = _dt.date.today()
    month, day = int(fiscal_year_end[:2]), int(fiscal_year_end[3:])
    fy_end = _dt.date(today.year, month, day)
    if today >= fy_end + _dt.timedelta(days=90):
        return today.year
    return today.year - 1


def _report_years_needed(latest_fy: int, periods_historical: int) -> list[int]:
    """Return report years to fetch, oldest first.

    Each annual report covers 2 fiscal years (primary + comparative).
    E.g. latest_fy=2025, periods=3 → [2023, 2025]
         (2025 AR covers 2024+2025, 2023 AR covers 2022+2023 → union = 2022..2025 → take last 3)
    """
    import math as _math
    n = _math.ceil(periods_historical / 2)
    years = [latest_fy - 2 * i for i in range(n)]
    return list(reversed(years))  # oldest first


_LEGAL_SUFFIXES = re.compile(
    r"\b(ab|ag|sa|nv|bv|plc|ltd|limited|inc|corp|corporation|group|holding|holdings|se|oy|as|asa)\b",
    re.IGNORECASE,
)


def _company_domain_tokens(company_name: str) -> list[str]:
    """Return lowercase tokens from company name usable for domain matching.

    'Atlas Copco AB' → ['atlas', 'copco', 'atlascopco']
    'Siemens Energy AG' → ['siemens', 'energy', 'siemensenergy']
    """
    cleaned = _LEGAL_SUFFIXES.sub("", company_name)
    tokens = [t.lower() for t in re.split(r"[\s\-_&,\.]+", cleaned) if len(t) >= 3]
    if len(tokens) >= 2:
        tokens.append("".join(tokens))  # joined form e.g. atlascopco
    return tokens


def _url_matches_company(url: str, company_name: str) -> bool:
    """True if any company name token appears in the URL domain."""
    from urllib.parse import urlparse
    domain = urlparse(url).netloc.lower().replace("www.", "")
    return any(tok in domain for tok in _company_domain_tokens(company_name))


def _find_annual_report_pdf_url(company_name: str, ticker: str, year: int | None = None) -> str | None:
    """DDG search cascade to find the annual report PDF URL for the given fiscal year.

    Validates that returned URLs belong to the target company's domain — prevents
    lookalike tickers (e.g. ATCO Ltd.) from poisoning Atlas Copco AB searches.
    """
    from bs4 import BeautifulSoup as _BS
    import datetime
    if year is None:
        year = datetime.date.today().year - 1
    queries = [
        f"{company_name} annual report {year} filetype:pdf",
        f"{company_name} {ticker} annual report {year} PDF",
        f"{company_name} annual report {year} PDF investor relations",
    ]
    headers = {
        "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
        "Accept": "text/html,application/xhtml+xml,*/*;q=0.8",
        "Accept-Language": "en-US,en;q=0.5",
    }
    skip = {"duckduckgo.com", "google.com", "bing.com", "youtube.com", "facebook.com"}

    fallback_pdf: str | None = None  # best non-matching PDF if no match found

    for query in queries:
        try:
            resp = requests.post(
                "https://html.duckduckgo.com/html/",
                data={"q": query, "kl": "us-en"},
                headers=headers, timeout=15,
            )
            soup = _BS(resp.text, "lxml")
            # First pass: direct PDF links — prefer company-domain match
            direct_pdfs: list[str] = []
            for a in soup.select("a.result__a"):
                href = a.get("href", "")
                if href.lower().endswith(".pdf") and href.startswith("http"):
                    if not any(s in href for s in skip):
                        direct_pdfs.append(href)
            for href in direct_pdfs:
                if _url_matches_company(href, company_name):
                    return href
            if direct_pdfs and fallback_pdf is None:
                fallback_pdf = direct_pdfs[0]

            # Second pass: fetch IR page from top result and scan for PDF links
            for a in soup.select("a.result__a"):
                href = a.get("href", "")
                if not href.startswith("http") or any(s in href for s in skip):
                    continue
                # Prefer IR pages on company domain
                if not _url_matches_company(href, company_name):
                    continue
                try:
                    ir_resp = requests.get(href, headers={"User-Agent": headers["User-Agent"]}, timeout=10)
                    ir_soup = _BS(ir_resp.text, "lxml")
                    for link in ir_soup.find_all("a", href=True):
                        lhref = link["href"]
                        if not lhref.startswith("http"):
                            from urllib.parse import urljoin
                            lhref = urljoin(href, lhref)
                        ltext = link.get_text(strip=True).lower()
                        if lhref.lower().endswith(".pdf") and any(
                            kw in ltext for kw in ["annual", "report", "20-f", "results"]
                        ):
                            return lhref
                except Exception:
                    continue
        except Exception:
            continue

    return fallback_pdf  # last resort: best non-matching PDF (may be wrong company)


def _download_pdf_to_tmpfile(url: str) -> str:
    """Download PDF from URL to a temp file; return the temp path."""
    import os as _os
    resp = requests.get(
        url,
        headers={"User-Agent": "FinancialModelBot vinit.paul@gmail.com"},
        timeout=90,
    )
    resp.raise_for_status()
    with tempfile.NamedTemporaryFile(suffix=".pdf", delete=False) as f:
        f.write(resp.content)
        return f.name


def fetch_non_us_filing(cfg, ir_url: str | None = None) -> ReconciledFinancialData:
    """Fetch IS/BS/CF for a non-US company by downloading and extracting annual report PDFs.

    Dynamically figures out how many reports are needed:
      - Each IFRS annual report covers 2 fiscal years (primary + comparative)
      - ceil(periods_historical / 2) reports are fetched, stepping back 2 years each time
      - Data from all reports is merged, oldest-first, then trimmed to periods_historical
    """
    import os as _os
    from src.extractor import extract_financials_from_pdf, save_extraction_cache, _load_cache

    periods = compute_historical_periods(cfg.fiscal_year_end, cfg.periods_historical)

    # Cache hit -- skip all network calls
    cached = _load_cache(cfg.ticker)
    if cached is not None:
        is_dict, bs_dict, cfs_dict, notes, _ = cached
        logger.info("[extraction cache] loaded %s", cfg.ticker)
        print(f"[extraction cache] loaded {cfg.ticker}")
        return ReconciledFinancialData(
            ticker=cfg.ticker,
            company_name=cfg.company_name,
            currency=cfg.currency,
            fiscal_year_end=cfg.fiscal_year_end,
            periods=periods,
            income_statement=is_dict,
            balance_sheet=bs_dict,
            cash_flow_statement=cfs_dict,
            notes=notes,
            sources={"extraction_cache": "local"},
            flags=["Non-US company -- data loaded from extraction cache"],
        )

    # Determine which report years to fetch
    latest_fy = _latest_complete_fy_year(cfg.fiscal_year_end)
    report_years = _report_years_needed(latest_fy, cfg.periods_historical)
    target_years = [int(p[:4]) for p in periods]

    logger.info("Multi-report fetch: %d periods -> reports %s", cfg.periods_historical, report_years)
    print(f"[multi-report] fetching {len(report_years)} reports for years {report_years}")

    # Accumulate per-year data from each report
    is_by_year:   dict[int, dict] = {}
    bs_by_year:   dict[int, dict] = {}
    cfs_by_year:  dict[int, dict] = {}
    merged_notes: dict = {}
    pdf_sources:  dict[int, str] = {}

    for i, report_year in enumerate(report_years):
        # The latest report may have an ir_url override
        url = (
            (ir_url if i == len(report_years) - 1 else None)
            or _find_annual_report_pdf_url(cfg.company_name, cfg.ticker, report_year)
        )
        if not url:
            logger.warning("No PDF found for %s %d AR -- skipping", cfg.company_name, report_year)
            print(f"[multi-report] WARNING: no PDF found for {report_year} AR -- skipping")
            continue

        print(f"[multi-report] downloading {report_year} AR: {url[:80]}...")
        tmp_path = _download_pdf_to_tmpfile(url)
        try:
            # Extract only the 2 years this report covers
            report_periods = [f"{report_year - 1}A", f"{report_year}A"]
            is_d, bs_d, cfs_d, notes, years_found = extract_financials_from_pdf(
                tmp_path, report_periods, ticker=""  # no cache key -- we manage cache here
            )
        finally:
            _os.unlink(tmp_path)

        # Map each extracted value to its fiscal year
        report_yrs = [int(y) for y in years_found] if years_found else [report_year - 1, report_year]
        for stmt, by_year in [(is_d, is_by_year), (bs_d, bs_by_year), (cfs_d, cfs_by_year)]:
            for key, values in stmt.items():
                if isinstance(values, list):
                    for yr, val in zip(report_yrs, values):
                        by_year.setdefault(yr, {})[key] = val

        if not merged_notes:
            merged_notes = notes
        pdf_sources[report_year] = url

    if not is_by_year:
        raise ValueError(f"No data extracted for {cfg.company_name} -- all report fetches failed")

    # Build final arrays aligned to target_years
    def _build_arrays(by_year: dict[int, dict]) -> dict:
        all_keys = {k for d in by_year.values() for k in d}
        result = {}
        for key in all_keys:
            vals = [by_year[yr][key] for yr in target_years if yr in by_year and key in by_year[yr]]
            if len(vals) == len(target_years):
                result[key] = vals
        return result

    is_dict  = _build_arrays(is_by_year)
    bs_dict  = _build_arrays(bs_by_year)
    cfs_dict = _build_arrays(cfs_by_year)

    # Trim periods if some years could not be fetched
    actual_n = max((len(v) for v in is_dict.values() if isinstance(v, list)), default=0)
    if actual_n < len(periods):
        periods = periods[-actual_n:]

    # Save merged result to cache so future runs skip all downloads
    save_extraction_cache(cfg.ticker, {
        "currency": cfg.currency,
        "years_found": [str(y) for y in target_years],
        "income_statement": is_dict,
        "balance_sheet": bs_dict,
        "cash_flow_statement": cfs_dict,
        "notes": merged_notes,
        "confidence": 0.85,
        "discrepancies": [f"Auto-merged from {len(pdf_sources)} annual reports: {list(pdf_sources)}"],
    })

    return ReconciledFinancialData(
        ticker=cfg.ticker,
        company_name=cfg.company_name,
        currency=cfg.currency,
        fiscal_year_end=cfg.fiscal_year_end,
        periods=periods,
        income_statement=is_dict,
        balance_sheet=bs_dict,
        cash_flow_statement=cfs_dict,
        notes=merged_notes,
        sources=pdf_sources,
        flags=[f"Non-US company -- data merged from {len(pdf_sources)} annual reports"],
    )

