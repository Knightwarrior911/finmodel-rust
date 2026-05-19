from tieout import config


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
