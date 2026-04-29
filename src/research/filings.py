"""
Non-US filing navigator via Playwright + real Chrome profile.
India, UK, Europe, HKEX filing patterns.
"""

import logging
from typing import Optional

from src.browser.session import BrowserSession
from src.browser.navigation import BrowserNav

logger = logging.getLogger(__name__)


class GlobalFilingNavigator:
    """Navigate non-US company filings using browser."""

    def __init__(self, session: BrowserSession):
        self.session = session
        self.nav = BrowserNav(session)

    async def find_annual_report(self, company_name: str, country: str = "india",
                                 year: str = "2025") -> Optional[str]:
        """Find annual report for non-US company. Returns filing URL or None."""
        if country == "india":
            return await self._find_indian_filing(company_name, year)
        elif country == "uk":
            return await self._find_uk_filing(company_name)
        elif country in ("france", "germany", "eu"):
            return await self._find_eu_filing(company_name, country)
        elif country == "hk":
            return await self._find_hkex_filing(company_name)
        else:
            # Generic: search company IR
            return await self.nav.find_company_ir(company_name)

    async def _find_indian_filing(self, company_name: str, year: str) -> Optional[str]:
        """BSE/NSE filing search for Indian companies."""
        # Try company IR first
        text = await self.nav.google_search(
            f'"{company_name}" annual report {year} BSE NSE filetype:pdf'
        )
        return text  # Caller parses for PDF URLs

    async def _find_uk_filing(self, company_name: str) -> Optional[str]:
        """Companies House search for UK companies."""
        text = await self.nav.google_search(
            f'site:companieshouse.gov.uk "{company_name}" filing history'
        )
        return text

    async def _find_eu_filing(self, company_name: str, country: str) -> Optional[str]:
        """EU company filing search."""
        portals = {
            "france": "site:amf-france.org",
            "germany": "site:bundesanzeiger.de",
        }
        portal = portals.get(country, "")
        text = await self.nav.google_search(
            f'{portal} "{company_name}" annual report OR financial results'
        )
        return text

    async def _find_hkex_filing(self, company_name: str) -> Optional[str]:
        """HKEX filing search."""
        text = await self.nav.google_search(
            f'site:hkexnews.hk "{company_name}" annual report'
        )
        return text
