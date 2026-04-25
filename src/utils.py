import calendar
from datetime import date, timedelta

_MONTH = {
    "Jan": 1, "Feb": 2, "Mar": 3, "Apr": 4, "May": 5, "Jun": 6,
    "Jul": 7, "Aug": 8, "Sep": 9, "Oct": 10, "Nov": 11, "Dec": 12,
}
_FILING_LAG_DAYS = 90  # 10-K due 60-90d after FY end; 90 is conservative


def latest_reported_fy_year(fiscal_year_end: str, today: date | None = None) -> int:
    """Calendar year of latest REPORTED fiscal year. Assumes 90-day filing lag.

    Two cases:
    - FY end already passed this calendar year: check if 90-day lag has also passed.
    - FY end not yet reached this year: check last year's FY end instead.
    """
    if today is None:
        today = date.today()
    month = _MONTH.get(fiscal_year_end, 12)

    last_day_this = calendar.monthrange(today.year, month)[1]
    this_year_fye = date(today.year, month, last_day_this)

    if this_year_fye < today:
        # This year's FY has closed; filed if lag has passed
        if this_year_fye + timedelta(days=_FILING_LAG_DAYS) <= today:
            return today.year
        return today.year - 1
    else:
        # This year's FY hasn't closed yet; check prior year
        last_day_prev = calendar.monthrange(today.year - 1, month)[1]
        prev_year_fye = date(today.year - 1, month, last_day_prev)
        if prev_year_fye + timedelta(days=_FILING_LAG_DAYS) <= today:
            return today.year - 1
        return today.year - 2


def compute_historical_periods(fiscal_year_end: str, n: int, today: date | None = None) -> list[str]:
    """Return n period labels ending at latest reported FY.

    E.g. fiscal_year_end="Dec", n=3, today=2026-04-25 -> ["2023A", "2024A", "2025A"]
    E.g. fiscal_year_end="Mar", n=3, today=2026-04-25 -> ["2023A", "2024A", "2025A"]
      (FY2025 ends Mar 2026, filed by Jun 2026 -- not yet as of Apr 2026, so latest = Mar 2025)
    """
    latest = latest_reported_fy_year(fiscal_year_end, today)
    return [f"{latest - n + 1 + i}A" for i in range(n)]
