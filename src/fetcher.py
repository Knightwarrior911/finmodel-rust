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
        "Revenues",                                               # pre-ASC 606 total — banks/insurers/REITs
        "RevenueFromContractWithCustomerExcludingAssessedTax",   # ASC 606 primary (post-2018)
        "SalesRevenueNet",                                        # older manufacturing / retail
        "RevenueFromContractWithCustomerIncludingAssessedTax",    # includes sales tax
        "RegulatedAndUnregulatedOperatingRevenue",                # utilities
        "HealthCareOrganizationRevenue",                          # healthcare
        "RealEstateRevenueNet",                                   # REITs
        # ── added by coverage audit ──────────────────────────────────────
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
        "DepreciationDepletionAndAmortization",
        "DepreciationAndAmortization",
        "DepreciationAmortizationAndAccretionNet",
        # ── added by coverage audit ──────────────────────────────────────
        "DepreciationDepletionAndAmortizationExcludingDiscontinuedOperations",
        "DepreciationAndAmortizationDiscontinuedOperations",      # some filers split
        "AmortizationOfIntangibleAssets",                         # intangible-heavy companies only
        "Depreciation",                                           # tangible-only filers (rare as standalone)
        "DepreciationNonproduction",                              # manufacturing: non-COGS D
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
        # ── added by coverage audit ──────────────────────────────────────
        "LiabilitiesAndStockholdersEquity",                       # some filers omit Liabilities standalone
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
    filing AND restated comparatives included in subsequent years' 10-Ks.
    We prefer the ORIGINAL filing (earliest filed date) so that, e.g., a
    company that reclassifies discontinued operations in a later 10-K doesn't
    silently replace previously-reported GAAP figures.
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
                # Same period end: prefer original filing (earliest filed date)
                if e.get("filed", "9999-99-99") < existing.get("filed", "9999-99-99"):
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
            "values": c["values"],
        })
    return result


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
    ]

    load(is_data, is_keys)
    load(bs_data, bs_keys)
    load(cfs_data, cfs_keys)

    # Detect company-specific revenue segments from non-standard XBRL namespaces
    segment_revenues = _detect_segments(facts, target_years, currency,
                                         is_data.get("revenue", []))
    for seg in segment_revenues:
        is_data[seg["key"]] = seg["values"]

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
    if opex_items:
        notes_meta["opex_items"] = [
            {"label": o["label"], "key": o["key"], "category": o["category"]}
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
