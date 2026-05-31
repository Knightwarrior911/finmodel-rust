from src import peers


def test_de_and_tax_tagged_flags_default_tax():
    # FAKE_TICKER_XYZ is not a real ticker; yfinance yields no effective tax
    # rate, so _de_and_tax falls back to 0.21 and the flag must report True.
    de, tax, tax_is_default = peers._de_and_tax_tagged("FAKE_TICKER_XYZ")
    assert tax == 0.21
    assert tax_is_default is True
    assert isinstance(de, float)
