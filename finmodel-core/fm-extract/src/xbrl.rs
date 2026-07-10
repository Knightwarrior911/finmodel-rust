//! XBRL tag mapping and parsing for US EDGAR filings.
//!
//! Ported from `src/fetcher.py` — `XBRL_TAG_MAP` and `parse_xbrl_to_raw()`.
//! Contains the authoritative US-GAAP tag → canonical line item mapping.

use std::collections::HashMap;

use serde_json::Value;
use fm_types::StatementData;

// ---------------------------------------------------------------------------
// XBRL Tag Map
// ---------------------------------------------------------------------------

/// XBRL tag map: canonical line item → ordered list of candidate US-GAAP tags.
/// First tag with data for ALL target years wins.
/// Ported verbatim from `src/fetcher.py::XBRL_TAG_MAP`.
pub fn xbrl_tag_map() -> HashMap<&'static str, &'static [&'static str]> {
    let mut m: HashMap<&str, &[&str]> = HashMap::new();

    // ── Income Statement ────────────────────────────────────────────────
    m.insert("revenue", &[
        "RevenueFromContractWithCustomerExcludingAssessedTax",
        "SalesRevenueNet",
        "RevenueFromContractWithCustomerIncludingAssessedTax",
        "RegulatedAndUnregulatedOperatingRevenue",
        "HealthCareOrganizationRevenue",
        "RealEstateRevenueNet",
        "Revenues",
        "RevenueNotFromContractWithCustomer",
        "InterestAndFeeIncomeLoansAndLeases",
        "NoninterestIncome",
        "BrokerageCommissionsRevenue",
        "RevenuesNetOfInterestExpense",
        "BankingFees",
        "NetInvestmentIncome",
        "PremiumsEarnedNet",
    ]);
    m.insert("cogs", &[
        "CostOfGoodsAndServicesSold",
        "CostOfRevenue",
        "CostOfGoodsSold",
        "CostOfServices",
        "CostOfGoodsAndServiceExcludingDepreciationDepletionAndAmortization",
        "CostsAndExpenses",
        "OperatingCostsAndExpenses",
        "CostOfPurchasedPower",
        "DirectCostsOfLeasedAndRentedPropertyOrEquipment",
        "CostOfOtherPropertyOperatingExpense",
        "PolicyholderBenefitsAndClaimsIncurredNet",
        "BenefitsLossesAndExpenses",
        "InterestExpenseOperating",
        "InterestExpense",
        "InterestExpenseDeposits",
    ]);
    m.insert("gross_profit", &[
        "GrossProfit",
        "EquityMethodInvestmentSummarizedFinancialInformationGrossProfitLoss",
        "GrossInvestmentIncomeOperating",
    ]);
    m.insert("sga", &[
        "SellingGeneralAndAdministrativeExpense",
        "GeneralAndAdministrativeExpense",
        "SellingAndMarketingExpense",
        "NoninterestExpense",
        "MarketingExpense",
        "OtherGeneralExpense",
    ]);
    m.insert("rd", &[
        "ResearchAndDevelopmentExpense",
        "ResearchAndDevelopmentExpenseExcludingAcquiredInProcessCost",
        "ProvisionForLoanLeaseAndOtherLosses",
        "FinancingReceivableExcludingAccruedInterestCreditLossExpenseReversal",
        "ProvisionForCreditLosses",
        "ProvisionForLoanAndLeaseLosses",
        "ProvisionForLoanLeaseAndOtherCreditLosses",
        "RestructuringCharges",
    ]);
    m.insert("utility_om", &[
        "UtilitiesOperatingExpenseMaintenanceAndOperations",
        "OperationsAndMaintenanceExpense",
        "UtilitiesOperatingExpenseOperationsAndMaintenance",
    ]);
    m.insert("utility_taxes_other", &[
        "TaxesExcludingIncomeAndExciseTaxes",
        "UtilitiesOperatingExpenseTaxes",
    ]);
    m.insert("utility_fuel", &[
        "UtilitiesOperatingExpenseFuelPurchasedPower",
        "CostOfPurchasedPower",
        "UtilitiesOperatingExpenseFuel",
        "FuelCosts",
    ]);
    m.insert("ebit", &[
        "OperatingIncomeLoss",
        "IncomeLossFromContinuingOperationsBeforeIncomeTaxesExtraordinaryItemsNoncontrollingInterest",
        "IncomeLossFromContinuingOperationsBeforeIncomeTaxesMinorityInterestAndIncomeLossFromEquityMethodInvestments",
        "NoninterestExpense",
    ]);
    m.insert("interest_expense", &[
        "InterestExpense",
        "InterestAndDebtExpense",
        "InterestExpenseDebt",
        "InterestExpenseDeposits",
        "InterestExpenseBorrowings",
        "FinanceLeaseInterestExpense",
    ]);
    m.insert("interest_income", &[
        "InvestmentIncomeInterest",
        "InterestAndDividendIncomeOperating",
        "InterestIncomeOperating",
        "InterestIncomeExpenseNet",
    ]);
    m.insert("income_tax", &[
        "IncomeTaxExpenseBenefit",
    ]);
    m.insert("net_income", &[
        "NetIncomeLoss",
        "NetIncomeLossAttributableToParent",
        "ProfitLoss",
        "NetIncomeLossAvailableToCommonStockholdersBasic",
    ]);
    m.insert("eps_basic", &[
        "EarningsPerShareBasic",
        "EarningsPerShareBasicAndDiluted",
    ]);
    m.insert("eps_diluted", &[
        "EarningsPerShareDiluted",
        "EarningsPerShareBasicAndDiluted",
    ]);
    m.insert("shares_basic", &[
        "WeightedAverageNumberOfSharesOutstandingBasic",
        "CommonStockSharesOutstanding",
    ]);
    m.insert("shares_diluted", &[
        "WeightedAverageNumberOfDilutedSharesOutstanding",
        "WeightedAverageNumberOfSharesOutstandingBasic",
        "CommonStockSharesOutstanding",
    ]);
    m.insert("da", &[
        "DepreciationAndAmortization",
        "DepreciationDepletionAndAmortization",
        "DepreciationAmortizationAndAccretionNet",
        "DepreciationDepletionAndAmortizationExcludingDiscontinuedOperations",
        "DepreciationAndAmortizationDiscontinuedOperations",
        "AmortizationOfIntangibleAssets",
        "Depreciation",
        "DepreciationNonproduction",
    ]);

    // ── Balance Sheet — Assets ──────────────────────────────────────────
    m.insert("cash", &[
        "CashAndCashEquivalentsAtCarryingValue",
        "CashCashEquivalentsAndShortTermInvestments",
        "CashAndCashEquivalents",
        "Cash",
        "CashAndDueFromBanks",
        "CashCashEquivalentsRestrictedCashAndRestrictedCashEquivalents",
    ]);
    m.insert("accounts_receivable", &[
        "AccountsReceivableNetCurrent",
        "AccountsReceivableNet",
        "ReceivablesNetCurrent",
        "TradeAndOtherReceivablesNetCurrent",
        "NotesAndLoansReceivableNetCurrent",
        "LoansAndLeasesReceivableNetReportedAmount",
        "LoansAndLeasesReceivableNetOfDeferredIncome",
        "PremiumsAndOtherReceivablesNet",
        "ReceivablesFromBrokersDealersAndClearingOrganizations",
        "AccountsReceivableNetNoncurrent",
        "ReinsuranceRecoverables",
    ]);
    m.insert("inventory", &[
        "InventoryNet",
        "Inventories",
        "InventoryFinishedGoodsNetOfReserves",
        "InventoryRawMaterialsAndSupplies",
        "EnergyRelatedInventory",
        "EnergyRelatedInventoryOtherFossilFuel",
        "InventoryRawMaterials",
        "InventoryWorkInProcess",
        "RealEstateInventory",
        "InventoryRealEstate",
        "InventoryOperativeBuilders",
        "InventoryOperativeBuildersOther",
        "InventoryHomesUnderConstruction",
        "InventoryLandHeldForDevelopmentAndSale",
        "OtherInventorySupplies",
    ]);
    m.insert("total_current_assets", &[
        "AssetsCurrent",
    ]);
    m.insert("ppe_net", &[
        "PropertyPlantAndEquipmentNet",
        "PropertyPlantAndEquipmentAndFinanceLeaseRightOfUseAssetAfterAccumulatedDepreciationAndAmortization",
        "RealEstateInvestmentPropertyNet",
        "RealEstateAndAccumulatedDepreciation",
    ]);
    m.insert("goodwill", &[
        "Goodwill",
    ]);
    m.insert("intangibles_net", &[
        "FiniteLivedIntangibleAssetsNet",
        "IntangibleAssetsNetExcludingGoodwill",
        "IntangibleAssetsNetIncludingGoodwill",
    ]);
    m.insert("total_assets", &[
        "Assets",
    ]);

    // ── Balance Sheet — Liabilities & Equity ────────────────────────────
    m.insert("accounts_payable", &[
        "AccountsPayableCurrent",
        "AccountsPayableAndAccruedLiabilitiesCurrent",
        "AccountsPayable",
        "AccountsPayableTradeCurrent",
        "AccountsPayableAndAccruedLiabilitiesCurrentAndNoncurrent",
        "AccountsPayableCurrentAndNoncurrent",
        "AccountsPayableTradeCurrentAndNoncurrent",
        "OtherAccountsPayableAndAccruedLiabilities",
    ]);
    m.insert("total_current_liabilities", &[
        "LiabilitiesCurrent",
    ]);
    m.insert("long_term_debt", &[
        "LongTermDebtNoncurrent",
        "LongTermDebt",
        "LongTermDebtAndCapitalLeaseObligations",
        "LongTermNotesPayable",
        "SeniorLongTermNotes",
        "ConvertibleLongTermNotesPayable",
        "LongTermDebtFairValue",
        "UnsecuredDebt",
        "SecuredDebt",
        "SubordinatedLongTermDebt",
        "JuniorSubordinatedNotes",
        "FinanceLeaseLiabilityNoncurrent",
        "Deposits",
        "InterestBearingDeposits",
        "PolicyholderFunds",
        "LiabilityForFuturePolicyBenefits",
    ]);
    m.insert("total_liabilities", &[
        // "Liabilities" — if absent, derivation computes: total_assets - total_equity - rnci
        "Liabilities",
    ]);
    m.insert("retained_earnings", &[
        "RetainedEarningsAccumulatedDeficit",
        "RetainedEarnings",
    ]);
    m.insert("total_equity", &[
        "StockholdersEquityIncludingPortionAttributableToNoncontrollingInterest",
        "StockholdersEquity",
        "PartnersCapital",
        "MembersEquity",
    ]);
    m.insert("redeemable_nci", &[
        "RedeemableNoncontrollingInterestEquityCarryingAmount",
        "RedeemableNoncontrollingInterestEquityPreferredCarryingAmount",
        "TemporaryEquityCarryingAmountIncludingPortionAttributableToNoncontrollingInterests",
    ]);

    // ── Cash Flow Statement ─────────────────────────────────────────────
    m.insert("cfo", &[
        "NetCashProvidedByUsedInOperatingActivities",
        "NetCashProvidedByUsedInOperatingActivitiesContinuingOperations",
    ]);
    m.insert("capex", &[
        "PaymentsToAcquirePropertyPlantAndEquipment",
        "PaymentsToAcquireProductiveAssets",
        "PaymentsForCapitalImprovements",
        "PaymentsToAcquireOtherPropertyPlantAndEquipment",
        "PaymentsToAcquireRealEstate",
        "PaymentsToAcquireAndDevelopRealEstate",
        "PaymentsForConstructionInProcessAndProductiveAssets",
        "PurchaseOfPropertyAndEquipment",
    ]);
    m.insert("cfi", &[
        "NetCashProvidedByUsedInInvestingActivities",
        "NetCashProvidedByUsedInInvestingActivitiesContinuingOperations",
    ]);
    m.insert("cff", &[
        "NetCashProvidedByUsedInFinancingActivities",
        "NetCashProvidedByUsedInFinancingActivitiesContinuingOperations",
    ]);
    m.insert("dividends_paid", &[
        "PaymentsOfDividends",
        "PaymentsOfDividendsCommonStock",
        "PaymentsOfOrdinaryDividends",
        "PaymentsForDividends",
    ]);
    m.insert("buybacks", &[
        "PaymentsForRepurchaseOfCommonStock",
        "PaymentsForRepurchaseOfEquity",
        "TreasuryStockValueAcquiredCostMethod",
    ]);
    m.insert("net_change_cash", &[
        "CashCashEquivalentsRestrictedCashAndRestrictedCashEquivalentsPeriodIncreaseDecreaseIncludingExchangeRateEffect",
        "CashAndCashEquivalentsPeriodIncreaseDecrease",
        "CashCashEquivalentsRestrictedCashAndRestrictedCashEquivalentsPeriodIncreaseDecreaseExcludingExchangeRateEffect",
    ]);
    m.insert("fx_effect_on_cash", &[
        "EffectOfExchangeRateOnCashCashEquivalentsRestrictedCashAndRestrictedCashEquivalents",
        "EffectOfExchangeRateOnCashCashEquivalentsRestrictedCashAndRestrictedCashEquivalentsIncludingDisposalGroupAndDiscontinuedOperations",
        "EffectOfExchangeRateOnCashAndCashEquivalents",
        "EffectOfExchangeRateOnCashAndCashEquivalentsContinuingOperations",
    ]);

    // ── Additional ──────────────────────────────────────────────────────
    m.insert("nci_income_loss", &[
        "NetIncomeLossAttributableToNoncontrollingInterest",
        "MinorityInterestInNetIncomeLossOfConsolidatedEntities",
        "IncomeLossFromContinuingOperationsAttributableToNoncontrollingEntity",
    ]);
    m.insert("deferred_revenue_current", &[
        "DeferredRevenueCurrent",
        "ContractWithCustomerLiabilityCurrent",
        "DeferredRevenueAndCreditsCurrent",
    ]);
    m.insert("deferred_revenue_lt", &[
        "DeferredRevenueNoncurrent",
        "ContractWithCustomerLiabilityNoncurrent",
        "DeferredRevenueAndCreditsNoncurrent",
    ]);
    m.insert("investments_net_cfi", &[
        "PaymentsToAcquireAvailableForSaleSecurities",
        "PaymentsToAcquireShortTermInvestments",
        "PaymentsToAcquireInvestments",
        "ProceedsFromSaleMaturityAndCollectionOfShorttermInvestments",
        "ProceedsFromSaleAndMaturityOfAvailableForSaleSecurities",
    ]);

    m
}

// ---------------------------------------------------------------------------
// XBRL Parsing
// ---------------------------------------------------------------------------

/// Errors from XBRL parsing operations.
#[derive(Debug, thiserror::Error)]
pub enum XbrlParseError {
    #[error("Missing us-gaap facts in companyfacts response")]
    MissingGaap,
    #[error("No data found for tag {0}")]
    NoData(String),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Parse XBRL company facts into StatementData maps (IS, BS, CFS).
///
/// Ported from `parse_xbrl_to_raw()` in `src/fetcher.py`.
/// Takes the raw companyfacts JSON and extracts annual (10-K) data
/// for the target periods, applying the XBRL_TAG_MAP.
pub fn parse_xbrl_to_raw(
    facts: &Value,
    periods_historical: usize,
    currency: &str,
) -> Result<ParsedXbrlData, XbrlParseError> {
    let gaap = facts
        .pointer("/facts/us-gaap")
        .and_then(|v| v.as_object())
        .ok_or(XbrlParseError::MissingGaap)?;

    let tag_map = xbrl_tag_map();
    let target_years = compute_target_years(periods_historical);

    let mut is = StatementData::new();
    let mut bs = StatementData::new();
    let mut cfs = StatementData::new();
    let notes: HashMap<String, Value> = HashMap::new();

    // Define which canonical keys go to which statement
    let is_keys: &[&str] = &[
        "revenue", "cogs", "gross_profit", "sga", "rd",
        "ebit", "interest_expense", "interest_income", "income_tax",
        "net_income", "da", "eps_basic", "eps_diluted",
        "shares_basic", "shares_diluted",
    ];
    let bs_keys: &[&str] = &[
        "cash", "accounts_receivable", "inventory", "total_current_assets",
        "ppe_net", "goodwill", "intangibles_net", "total_assets",
        "accounts_payable", "total_current_liabilities", "long_term_debt",
        "total_liabilities", "retained_earnings", "total_equity", "redeemable_nci",
    ];
    let cfs_keys: &[&str] = &[
        "cfo", "capex", "cfi", "cff", "dividends_paid",
        "buybacks", "net_change_cash", "fx_effect_on_cash",
    ];

    // Extract each canonical key
    for key in is_keys {
        if let Some(v) = extract_tag(&gaap, key, &tag_map, &target_years, currency) {
            is.insert(key.to_string(), v);
        }
    }
    for key in bs_keys {
        if let Some(v) = extract_tag(&gaap, key, &tag_map, &target_years, currency) {
            bs.insert(key.to_string(), v);
        }
    }
    for key in cfs_keys {
        if let Some(v) = extract_tag(&gaap, key, &tag_map, &target_years, currency) {
            cfs.insert(key.to_string(), v);
        }
    }

    Ok(ParsedXbrlData { is, bs, cfs, notes })
}

/// Result of parsing XBRL company facts.
#[derive(Debug, Clone)]
pub struct ParsedXbrlData {
    pub is: StatementData,
    pub bs: StatementData,
    pub cfs: StatementData,
    pub notes: HashMap<String, Value>,
}

/// Compute the target years to look for (e.g. 3 periods ending last FY).
fn compute_target_years(periods_historical: usize) -> Vec<String> {
    // Simplified: use the most recent completed year based on current date
    let today_year = 2026; // simplified — in production use current date logic
    let latest_fy = today_year - 1;
    let years: Vec<String> = (0..periods_historical)
        .map(|i| format!("{}", latest_fy - periods_historical + 1 + i))
        .collect();
    years
}

/// Try each tag for a canonical key and return the first one with data for all years.
fn extract_tag(
    gaap: &serde_json::Map<String, Value>,
    canonical_key: &str,
    tag_map: &HashMap<&str, &[&str]>,
    target_years: &[String],
    currency: &str,
) -> Option<Vec<Option<f64>>> {
    let tags = tag_map.get(canonical_key)?;
    for tag_name in *tags {
        if let Some(entry) = gaap.get(*tag_name) {
            if let Some(units) = entry.get("units").and_then(|u| u.as_object()) {
                // Find the right unit (USD, EUR, etc.)
                for (unit_name, values) in units {
                    if !unit_name.contains(currency) && currency != "USD" {
                        continue;
                    }
                    if let Some(arr) = values.as_array() {
                        // Collect annual (10-K) values for target years
                        let mut result: Vec<Option<f64>> = Vec::new();
                        let mut all_found = true;

                        for year_str in target_years {
                            let _year_val: i32 = year_str.parse().unwrap_or(0);
                            let mut found = false;
                            for v in arr {
                                if let Some(end) = v.get("end").and_then(|e| e.as_str()) {
                                    // Check if this value is for the target year (FY end in that year)
                                    if end.starts_with(year_str) || end.ends_with(year_str) {
                                        if let Some(val) = v.get("val").and_then(|x| x.as_f64()) {
                                            // Only accept 10-K filings
                                            let form = v.get("form").and_then(|f| f.as_str()).unwrap_or("");
                                            if form == "10-K" || form == "20-F" {
                                                result.push(Some(val));
                                                found = true;
                                                break;
                                            }
                                        }
                                    }
                                }
                            }
                            if !found {
                                all_found = false;
                                break;
                            }
                        }

                        if all_found && !result.is_empty() {
                            return Some(result);
                        }
                    }
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_xbrl_tag_map_has_keys() {
        let map = xbrl_tag_map();
        assert!(map.contains_key("revenue"));
        assert!(map.contains_key("cash"));
        assert!(map.contains_key("cfo"));
        assert!(map.contains_key("net_income"));
        assert!(map.contains_key("total_assets"));
        assert!(map.len() > 30, "tag map should have 30+ entries");
    }

    #[test]
    fn test_xbrl_tag_map_revenue_tags() {
        let map = xbrl_tag_map();
        let rev_tags = map.get("revenue").expect("revenue entry");
        assert!(!rev_tags.is_empty());
        assert_eq!(rev_tags[0], "RevenueFromContractWithCustomerExcludingAssessedTax");
    }

    #[test]
    fn test_compute_target_years_produces_valid_years() {
        let years = compute_target_years(3);
        assert_eq!(years.len(), 3);
        // All years should be 4-digit strings
        for y in &years {
            assert_eq!(y.len(), 4);
            let n: i32 = y.parse().unwrap_or(0);
            assert!(n >= 2020 && n <= 2030);
        }
    }

    #[test]
    fn test_parse_xbrl_to_raw_empty_facts() {
        let facts = serde_json::json!({"facts": {}});
        let result = parse_xbrl_to_raw(&facts, 3, "USD");
        assert!(result.is_err()); // MissingGaap
    }

    #[test]
    fn test_parse_xbrl_to_raw_no_data() {
        let facts = serde_json::json!({
            "facts": {
                "us-gaap": {
                    "SomeRandomTag": {
                        "label": "Test",
                        "units": {}
                    }
                }
            }
        });
        let result = parse_xbrl_to_raw(&facts, 3, "USD").expect("should parse OK with empty data");
        assert!(result.is.is_empty());
    }
}
