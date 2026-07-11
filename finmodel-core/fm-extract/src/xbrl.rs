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
    m.insert("short_term_debt", &[
        "DebtCurrent",
        "LongTermDebtCurrent",
        "ShortTermBorrowings",
        "CommercialPaper",
        "OtherShortTermBorrowings",
        "FinanceLeaseLiabilityCurrent",
        "LinesOfCreditCurrent",
        "NotesPayableCurrent",
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
    parse_xbrl_to_raw_with_provenance(facts, periods_historical, currency).map(|(d, _)| d)
}

/// Like [`parse_xbrl_to_raw`], but also returns a `canonical_key → matched
/// us-gaap tag` map, so each extracted number can cite the exact XBRL fact it
/// came from (filing-level auditability for benchmarking).
pub fn parse_xbrl_to_raw_with_provenance(
    facts: &Value,
    periods_historical: usize,
    currency: &str,
) -> Result<(ParsedXbrlData, HashMap<String, String>), XbrlParseError> {
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
    let mut prov: HashMap<String, String> = HashMap::new();

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
        "accounts_payable", "total_current_liabilities", "long_term_debt", "short_term_debt",
        "total_liabilities", "retained_earnings", "total_equity", "redeemable_nci",
    ];
    let cfs_keys: &[&str] = &[
        "cfo", "capex", "cfi", "cff", "dividends_paid",
        "buybacks", "net_change_cash", "fx_effect_on_cash",
    ];

    // Extract each canonical key, recording the matched tag for provenance.
    for (keys, stmt) in [(is_keys, &mut is), (bs_keys, &mut bs), (cfs_keys, &mut cfs)] {
        for key in keys {
            if let Some((tag, v)) = extract_tag_named(&gaap, key, &tag_map, &target_years, currency) {
                stmt.insert(key.to_string(), v);
                prov.insert(key.to_string(), tag.to_string());
            }
        }
    }

    Ok((ParsedXbrlData { is, bs, cfs, notes }, prov))
}

/// Result of parsing XBRL company facts.
#[derive(Debug, Clone)]
pub struct ParsedXbrlData {
    pub is: StatementData,
    pub bs: StatementData,
    pub cfs: StatementData,
    pub notes: HashMap<String, Value>,
}

/// Compute the target years to look for (e.g. 3 periods ending at `latest_fy`).
///
/// `latest_fy` is the most recently completed fiscal year (e.g. 2025 for today in 2026
/// with a 90-day filing lag). Tests should pass a fixed year for determinism.
fn compute_target_years_from(periods_historical: usize, latest_fy: i32) -> Vec<String> {
    (0..periods_historical)
        .map(|i| format!("{}", latest_fy - periods_historical as i32 + 1 + i as i32))
        .collect()
}

/// Compute target years using the wall-clock default (currently assumes 2026).
fn compute_target_years(periods_historical: usize) -> Vec<String> {
    let today_year = 2026; // simplified — in production should use current date logic
    let latest_fy = today_year - 1;
    compute_target_years_from(periods_historical, latest_fy)
}

/// Try each candidate tag for a canonical key; return the first with data for
/// all target years, together with the winning us-gaap tag name (provenance).
fn extract_tag_named(
    gaap: &serde_json::Map<String, Value>,
    canonical_key: &str,
    tag_map: &HashMap<&str, &[&str]>,
    target_years: &[String],
    currency: &str,
) -> Option<(String, Vec<Option<f64>>)> {
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
                            return Some((tag_name.to_string(), result));
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

#[cfg(test)]
mod deterministic_tests {
    use super::*;

    #[test]
    fn test_parse_xbrl_to_raw_realistic_fixture() {
        // Fixture uses years 2023-2025 matching compute_target_years(3) output.
        let facts = serde_json::json!({
            "cik": 12345, "entityName": "Test Corp",
            "facts": { "us-gaap": {
                "RevenueFromContractWithCustomerExcludingAssessedTax": {
                    "label": "Revenue",
                    "units": { "USD": [
                        {"end": "2023-12-31", "val": 5500, "form": "10-K", "fy": "2023", "fp": "FY"},
                        {"end": "2024-12-31", "val": 6100, "form": "10-K", "fy": "2024", "fp": "FY"},
                        {"end": "2025-12-31", "val": 6800, "form": "10-K", "fy": "2025", "fp": "FY"}
                    ]}
                },
                "CostOfGoodsAndServicesSold": {
                    "label": "COGS",
                    "units": { "USD": [
                        {"end": "2023-12-31", "val": 3300, "form": "10-K", "fy": "2023", "fp": "FY"},
                        {"end": "2024-12-31", "val": 3600, "form": "10-K", "fy": "2024", "fp": "FY"},
                        {"end": "2025-12-31", "val": 4000, "form": "10-K", "fy": "2025", "fp": "FY"}
                    ]}
                },
                "GrossProfit": {
                    "label": "Gross Profit",
                    "units": { "USD": [
                        {"end": "2023-12-31", "val": 2200, "form": "10-K", "fy": "2023", "fp": "FY"},
                        {"end": "2024-12-31", "val": 2500, "form": "10-K", "fy": "2024", "fp": "FY"},
                        {"end": "2025-12-31", "val": 2800, "form": "10-K", "fy": "2025", "fp": "FY"}
                    ]}
                },
                "OperatingIncomeLoss": {
                    "label": "EBIT",
                    "units": { "USD": [
                        {"end": "2023-12-31", "val": 900, "form": "10-K", "fy": "2023", "fp": "FY"},
                        {"end": "2024-12-31", "val": 1100, "form": "10-K", "fy": "2024", "fp": "FY"},
                        {"end": "2025-12-31", "val": 1300, "form": "10-K", "fy": "2025", "fp": "FY"}
                    ]}
                },
                "NetIncomeLoss": {
                    "label": "Net Income",
                    "units": { "USD": [
                        {"end": "2023-12-31", "val": 700, "form": "10-K", "fy": "2023", "fp": "FY"},
                        {"end": "2024-12-31", "val": 850, "form": "10-K", "fy": "2024", "fp": "FY"},
                        {"end": "2025-12-31", "val": 1000, "form": "10-K", "fy": "2025", "fp": "FY"}
                    ]}
                },
                "CashAndCashEquivalentsAtCarryingValue": {
                    "label": "Cash",
                    "units": { "USD": [
                        {"end": "2023-12-31", "val": 600, "form": "10-K", "fy": "2023", "fp": "FY"},
                        {"end": "2024-12-31", "val": 700, "form": "10-K", "fy": "2024", "fp": "FY"},
                        {"end": "2025-12-31", "val": 800, "form": "10-K", "fy": "2025", "fp": "FY"}
                    ]}
                },
                "Assets": {
                    "label": "Assets",
                    "units": { "USD": [
                        {"end": "2023-12-31", "val": 11000, "form": "10-K", "fy": "2023", "fp": "FY"},
                        {"end": "2024-12-31", "val": 12500, "form": "10-K", "fy": "2024", "fp": "FY"},
                        {"end": "2025-12-31", "val": 14000, "form": "10-K", "fy": "2025", "fp": "FY"}
                    ]}
                },
                "AccountsPayableCurrent": {
                    "label": "Accounts Payable",
                    "units": { "USD": [
                        {"end": "2023-12-31", "val": 900, "form": "10-K", "fy": "2023", "fp": "FY"},
                        {"end": "2024-12-31", "val": 1000, "form": "10-K", "fy": "2024", "fp": "FY"},
                        {"end": "2025-12-31", "val": 1100, "form": "10-K", "fy": "2025", "fp": "FY"}
                    ]}
                },
                "LongTermDebtNoncurrent": {
                    "label": "Long-Term Debt",
                    "units": { "USD": [
                        {"end": "2023-12-31", "val": 2800, "form": "10-K", "fy": "2023", "fp": "FY"},
                        {"end": "2024-12-31", "val": 2500, "form": "10-K", "fy": "2024", "fp": "FY"},
                        {"end": "2025-12-31", "val": 2200, "form": "10-K", "fy": "2025", "fp": "FY"}
                    ]}
                },
                "StockholdersEquity": {
                    "label": "Equity",
                    "units": { "USD": [
                        {"end": "2023-12-31", "val": 5500, "form": "10-K", "fy": "2023", "fp": "FY"},
                        {"end": "2024-12-31", "val": 6200, "form": "10-K", "fy": "2024", "fp": "FY"},
                        {"end": "2025-12-31", "val": 7000, "form": "10-K", "fy": "2025", "fp": "FY"}
                    ]}
                },
                "NetCashProvidedByUsedInOperatingActivities": {
                    "label": "CFO",
                    "units": { "USD": [
                        {"end": "2023-12-31", "val": 1000, "form": "10-K", "fy": "2023", "fp": "FY"},
                        {"end": "2024-12-31", "val": 1200, "form": "10-K", "fy": "2024", "fp": "FY"},
                        {"end": "2025-12-31", "val": 1400, "form": "10-K", "fy": "2025", "fp": "FY"}
                    ]}
                },
                "PaymentsToAcquirePropertyPlantAndEquipment": {
                    "label": "CapEx",
                    "units": { "USD": [
                        {"end": "2023-12-31", "val": 350, "form": "10-K", "fy": "2023", "fp": "FY"},
                        {"end": "2024-12-31", "val": 400, "form": "10-K", "fy": "2024", "fp": "FY"},
                        {"end": "2025-12-31", "val": 450, "form": "10-K", "fy": "2025", "fp": "FY"}
                    ]}
                }
            }}
        });
        let result = parse_xbrl_to_raw(&facts, 3, "USD").expect("parse_xbrl_to_raw");
        let rev = result.is.get("revenue").expect("revenue");
        assert_eq!(rev[0], Some(5500.0));
        assert_eq!(rev[2], Some(6800.0));
        let cogs = result.is.get("cogs").expect("cogs");
        assert_eq!(cogs[0], Some(3300.0));
        let ebit = result.is.get("ebit").expect("ebit");
        assert_eq!(ebit[0], Some(900.0));
        let ni = result.is.get("net_income").expect("net_income");
        assert_eq!(ni[2], Some(1000.0));
        let cash = result.bs.get("cash").expect("cash");
        assert_eq!(cash[2], Some(800.0));
        let ltd = result.bs.get("long_term_debt").expect("long_term_debt");
        assert_eq!(ltd[0], Some(2800.0));
        let ta = result.bs.get("total_assets").expect("total_assets");
        assert_eq!(ta[2], Some(14000.0));
        let cfo = result.cfs.get("cfo").expect("cfo");
        assert_eq!(cfo[2], Some(1400.0));
        let capex = result.cfs.get("capex").expect("capex");
        assert_eq!(capex[0], Some(350.0));
        let gp = result.is.get("gross_profit").expect("gross_profit");
        assert_eq!(gp[2], Some(2800.0));
        let ap = result.bs.get("accounts_payable").expect("accounts_payable");
        assert_eq!(ap[2], Some(1100.0));
    }

    #[test]
    fn test_parse_xbrl_to_raw_skips_non_10k() {
        let facts = serde_json::json!({
            "cik": 12345, "entityName": "Test Corp",
            "facts": { "us-gaap": {
                "RevenueFromContractWithCustomerExcludingAssessedTax": {
                    "label": "Revenue",
                    "units": { "USD": [
                        {"end": "2024-03-31", "val": 1200, "form": "10-Q", "fy": "2024", "fp": "Q1"},
                        {"end": "2024-12-31", "val": 6100, "form": "10-K", "fy": "2024", "fp": "FY"},
                        {"end": "2023-12-31", "val": 5500, "form": "10-K", "fy": "2023", "fp": "FY"}
                    ]}
                }
            }}
        });
        let result = parse_xbrl_to_raw(&facts, 3, "USD").expect("parse_xbrl_to_raw");
        let rev = result.is.get("revenue");
        assert!(rev.is_none() || rev.iter().all(|arr| arr.iter().all(|v| v != &Some(1200.0))));
    }

    #[test]
    fn test_parse_xbrl_to_raw_tag_priority() {
        let facts = serde_json::json!({
            "cik": 12345, "entityName": "Test Corp",
            "facts": { "us-gaap": {
                "RevenueFromContractWithCustomerExcludingAssessedTax": {
                    "label": "ASC 606",
                    "units": { "USD": [
                        {"end": "2023-12-31", "val": 5500, "form": "10-K", "fy": "2023", "fp": "FY"},
                        {"end": "2024-12-31", "val": 6100, "form": "10-K", "fy": "2024", "fp": "FY"},
                        {"end": "2025-12-31", "val": 6800, "form": "10-K", "fy": "2025", "fp": "FY"}
                    ]}
                },
                "Revenues": {
                    "label": "Generic",
                    "units": { "USD": [
                        {"end": "2023-12-31", "val": 9999, "form": "10-K", "fy": "2023", "fp": "FY"},
                        {"end": "2024-12-31", "val": 9999, "form": "10-K", "fy": "2024", "fp": "FY"},
                        {"end": "2025-12-31", "val": 9999, "form": "10-K", "fy": "2025", "fp": "FY"}
                    ]}
                }
            }}
        });
        let result = parse_xbrl_to_raw(&facts, 3, "USD").expect("parse_xbrl_to_raw");
        let rev = result.is.get("revenue").expect("revenue");
        assert_eq!(rev[0], Some(5500.0));
        assert_eq!(rev[2], Some(6800.0));
    }

    #[test]
    fn test_parse_xbrl_to_raw_handles_missing_years() {
        let facts = serde_json::json!({
            "cik": 12345, "entityName": "Test Corp",
            "facts": { "us-gaap": {
                "RevenueFromContractWithCustomerExcludingAssessedTax": {
                    "label": "Revenue",
                    "units": { "USD": [
                        {"end": "2023-12-31", "val": 5500, "form": "10-K", "fy": "2023", "fp": "FY"},
                        {"end": "2024-12-31", "val": 6100, "form": "10-K", "fy": "2024", "fp": "FY"}
                    ]}
                }
            }}
        });
        let result = parse_xbrl_to_raw(&facts, 3, "USD").expect("parse_xbrl_to_raw");
        assert!(result.is.get("revenue").is_none());
    }

    #[test]
    fn provenance_records_the_winning_tag() {
        // Revenue via the ASC-606 tag; net income via NetIncomeLoss. The
        // provenance map must name the exact us-gaap tag each value came from.
        let facts = serde_json::json!({
            "facts": { "us-gaap": {
                "RevenueFromContractWithCustomerExcludingAssessedTax": {
                    "label": "Revenue",
                    "units": { "USD": [
                        {"end": "2023-12-31", "val": 100, "form": "10-K", "fy": "2023", "fp": "FY"},
                        {"end": "2024-12-31", "val": 110, "form": "10-K", "fy": "2024", "fp": "FY"},
                        {"end": "2025-12-31", "val": 120, "form": "10-K", "fy": "2025", "fp": "FY"}
                    ]}
                },
                "NetIncomeLoss": {
                    "label": "NI",
                    "units": { "USD": [
                        {"end": "2023-12-31", "val": 10, "form": "10-K", "fy": "2023", "fp": "FY"},
                        {"end": "2024-12-31", "val": 12, "form": "10-K", "fy": "2024", "fp": "FY"},
                        {"end": "2025-12-31", "val": 14, "form": "10-K", "fy": "2025", "fp": "FY"}
                    ]}
                }
            }}
        });
        let (data, prov) = parse_xbrl_to_raw_with_provenance(&facts, 3, "USD").expect("parse");
        assert_eq!(data.is.get("revenue").unwrap()[2], Some(120.0));
        assert_eq!(
            prov.get("revenue").map(String::as_str),
            Some("RevenueFromContractWithCustomerExcludingAssessedTax")
        );
        assert_eq!(prov.get("net_income").map(String::as_str), Some("NetIncomeLoss"));
        // A key with no matching fact has no provenance entry.
        assert!(prov.get("goodwill").is_none());
        // parse_xbrl_to_raw (no-provenance wrapper) yields identical data.
        let plain = parse_xbrl_to_raw(&facts, 3, "USD").expect("parse plain");
        assert_eq!(plain.is.get("revenue").unwrap()[2], Some(120.0));
    }
}
