import inspect

import src.extractor as ex
from tieout import config
from tieout import groundtruth
from tieout import run_tieout


# The exact industrial schema as it existed before the refactor — frozen here
# so a refactor that silently drops/renames an industrial key fails loudly.
_INDUSTRIAL_FROZEN = {
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


def test_industrial_schema_value_identical():
    assert config.CANONICAL_BY_SECTOR["industrial"] == _INDUSTRIAL_FROZEN


def test_sectors_present():
    assert set(config.SECTORS) == {"industrial", "bank", "insurer"}
    for s in config.SECTORS:
        assert set(config.CANONICAL_BY_SECTOR[s]) == {
            "income_statement", "balance_sheet", "cash_flow_statement"}


def test_per_sector_abs_and_exclude_keys_exist():
    for s in config.SECTORS:
        assert s in config.ABS_KEYS_BY_SECTOR
        assert s in config.EXCLUDE_KEYS_BY_SECTOR


def test_industrial_abs_exclude_value_identical():
    assert config.ABS_KEYS_BY_SECTOR["industrial"] == {
        "cogs", "sga", "rd", "interest_expense", "income_tax",
        "capex", "dividends_paid"}
    assert config.EXCLUDE_KEYS_BY_SECTOR["industrial"] == {"shares_diluted"}


def test_every_basket_row_has_known_sector():
    for row in config.BASKET:
        assert row["sector"] in config.SECTORS


def test_existing_seven_are_industrial():
    expected = {"ATCO-B.ST", "SAND.ST", "ASML.AS", "NESN.SW",
                "SAP.DE", "NOVO-B.CO", "MC.PA"}
    industrial = {r["ticker"] for r in config.BASKET
                  if r["sector"] == "industrial"}
    assert expected <= industrial


def test_build_ground_truth_accepts_sector():
    sig = inspect.signature(groundtruth.build_ground_truth)
    assert "sector" in sig.parameters


def test_hard_asserts_registry_has_atco():
    assert "ATCO-B.ST" in groundtruth.HARD_ASSERTS
    blk = groundtruth.HARD_ASSERTS["ATCO-B.ST"]["income_statement"]
    assert blk["revenue"][2023] == 172664
    assert blk["net_income"][2022] == 23482


def test_bank_income_data_row_matches_net_interest():
    rx = groundtruth.SECTOR_DATA_ROW["bank"]
    assert rx.search("Net interest income 12 345 11 200")
    assert not rx.search("Revenue 12 345 11 200")


def test_industrial_data_row_unchanged():
    rx = groundtruth.SECTOR_DATA_ROW["industrial"]
    assert rx.search("Net sales 172 664 141 325")


def test_insurer_data_row_matches_premium():
    rx = groundtruth.SECTOR_DATA_ROW["insurer"]
    assert rx.search("Gross written premium 5 000 4 800")
    assert not rx.search("Net sales 172 664 141 325")


def test_compare_uses_sector_schema():
    gt = {
        "years": [2022, 2023],
        "sector": "bank",
        "values": {"income_statement": {
            "net_interest_income": {"2022": 100, "2023": 110}}},
        "citations": {},
    }
    model = {"income_statement": {"net_interest_income": [100, 110]}}
    pct, denom, matched, per_stmt, rows = run_tieout._compare(gt, model)
    assert denom == 2 and matched == 2 and pct == 100.0


def test_detect_sector_bank():
    pages = ["Consolidated income statement",
             "Net interest income 12 345 11 200\n"
             "Loans and advances to customers 998 877"]
    assert ex.detect_sector(pages) == "bank"


def test_detect_sector_insurer():
    pages = ["Consolidated income statement",
             "Gross written premium 5 000 4 800\n"
             "Net claims incurred 3 100 2 900\n"
             "Insurance contract liabilities 9 000"]
    assert ex.detect_sector(pages) == "insurer"


def test_detect_sector_industrial_default():
    pages = ["Consolidated income statement",
             "Net sales 172 664 141 325\nCost of goods sold 97 547"]
    assert ex.detect_sector(pages) == "industrial"
