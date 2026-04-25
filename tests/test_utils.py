# financial_model/tests/test_utils.py
from datetime import date
import pytest
from src.utils import latest_reported_fy_year, compute_historical_periods


class TestLatestReportedFyYear:
    def test_dec_fye_after_filing_lag(self):
        # Dec 2025 FY + 90 days = March 31 2026; April 25 is after → FY2025 reported
        assert latest_reported_fy_year("Dec", today=date(2026, 4, 25)) == 2025

    def test_dec_fye_before_filing_lag(self):
        # Dec 2025 FY + 90 days = March 31 2026; Feb 1 is before → FY2024 is latest reported
        assert latest_reported_fy_year("Dec", today=date(2026, 2, 1)) == 2024

    def test_mar_fye_before_filing_lag(self):
        # Mar 2026 FY + 90 days = June 30 2026; April 25 is before → FY2025 is latest reported
        assert latest_reported_fy_year("Mar", today=date(2026, 4, 25)) == 2025

    def test_sep_fye_after_filing_lag(self):
        # Sep 2025 FY + 90 days = Dec 29 2025; April 2026 is after → FY2025 reported
        assert latest_reported_fy_year("Sep", today=date(2026, 4, 25)) == 2025

    def test_jun_fye_after_filing_lag(self):
        # Jun 2025 FY + 90 days = Sep 29 2025; April 2026 is after → FY2025 reported
        assert latest_reported_fy_year("Jun", today=date(2026, 4, 25)) == 2025

    def test_unknown_month_defaults_to_dec(self):
        assert latest_reported_fy_year("", today=date(2026, 4, 25)) == 2025


class TestComputeHistoricalPeriods:
    def test_dec_fye_five_periods(self):
        result = compute_historical_periods("Dec", 5, today=date(2026, 4, 25))
        assert result == ["2021A", "2022A", "2023A", "2024A", "2025A"]

    def test_dec_fye_three_periods(self):
        result = compute_historical_periods("Dec", 3, today=date(2026, 4, 25))
        assert result == ["2023A", "2024A", "2025A"]

    def test_mar_fye_three_periods(self):
        # Mar FY: latest reported as of Apr 2026 = 2025
        result = compute_historical_periods("Mar", 3, today=date(2026, 4, 25))
        assert result == ["2023A", "2024A", "2025A"]

    def test_dec_fye_before_lag_rolls_back(self):
        # Feb 2026: Dec 2025 FY not yet filed → latest = 2024
        result = compute_historical_periods("Dec", 3, today=date(2026, 2, 1))
        assert result == ["2022A", "2023A", "2024A"]

    def test_count_is_exact(self):
        result = compute_historical_periods("Dec", 7, today=date(2026, 4, 25))
        assert len(result) == 7

    def test_labels_end_in_A(self):
        result = compute_historical_periods("Dec", 3, today=date(2026, 4, 25))
        assert all(lbl.endswith("A") for lbl in result)
