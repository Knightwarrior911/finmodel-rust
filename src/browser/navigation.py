"""
Browser navigation for IB research.
Google search, company IR, Bloomberg, cookie handling.
Uses Playwright via BrowserSession (real Chrome profile).
"""

import asyncio
import logging
from urllib.parse import quote
from typing import Optional

from .session import BrowserSession

logger = logging.getLogger(__name__)


class BrowserNav:
    """High-level navigation on BrowserSession (Playwright-backed)."""

    def __init__(self, session: BrowserSession):
        self.session = session

    async def google_search(self, query: str) -> str:
        """Search Google, return page text."""
        url = f"https://www.google.com/search?q={quote(query)}"
        page = await self.session.goto(url)
        await asyncio.sleep(1)
        return await self.session.get_text(page)

    async def google_search_operators(
        self, query: str, site: str = None, date_range: str = None,
        filetype: str = None, exclude_sites: list[str] = None,
    ) -> str:
        """Google search with IB search operators."""
        parts = [query]
        if site:
            parts.append(f"site:{site}")
        if date_range:
            parts.append(date_range)
        if filetype:
            parts.append(f"filetype:{filetype}")
        if exclude_sites:
            for ex in exclude_sites:
                parts.append(f"-site:{ex}")
        return await self.google_search(" ".join(parts))

    async def find_company_ir(self, company_name: str) -> Optional[str]:
        """Find company IR URL via Google. Returns URL or None."""
        text = await self.google_search(
            f'"{company_name}" investor relations annual report'
        )
        # Parse text for IR URLs (ir.company.com, company.com/investors)
        for line in text.split("\n"):
            if "investor" in line.lower() and (".com" in line or "http" in line):
                return line.strip()
        return None

    # --- SEC EDGAR ---

    async def open_sec_filing_list(self, cik: str, form_type: str = "10-K") -> str:
        """Open SEC EDGAR legacy CGI filing list. Returns page text."""
        cik_stripped = cik.lstrip("0")
        url = (
            f"https://www.sec.gov/cgi-bin/browse-edgar"
            f"?action=getcompany&CIK={cik_stripped}&type={form_type}"
        )
        await self.session.goto(url)
        await asyncio.sleep(1)
        return await self.session.get_text()

    # --- Bloomberg ---

    async def search_bloomberg(self, query: str) -> str:
        """
        Search Bloomberg using their own search page.
        CRITICAL: Never Google site:bloomberg.com — direct URLs 404.
        """
        url = f"https://www.bloomberg.com/search?query={quote(query)}"
        page = await self.session.goto(url)
        await asyncio.sleep(2)  # Bloomberg is JS-heavy
        return await self.session.get_text(page)

    # --- Company IR Navigation ---

    async def navigate_ir_site(self, ir_url: str) -> str:
        """Navigate to company IR page, handle cookies, return text."""
        page = await self.session.goto(ir_url)
        await self.handle_cookies(page)
        await asyncio.sleep(1)
        return await self.session.get_text(page)

    # --- Cookie Handling ---

    async def handle_cookies(self, page=None) -> bool:
        """Detect and dismiss cookie popups."""
        pg = page or self.session.default_page
        if not pg:
            return False

        cookie_texts = [
            "Accept All", "Accept All Cookies", "Accept Cookies",
            "Accept", "Accept and Continue", "I Accept",
            "Allow All", "Allow Cookies",
        ]

        try:
            for text in cookie_texts:
                btn = pg.locator(f"button:has-text('{text}')")
                if await btn.count() > 0:
                    await btn.first.click()
                    await asyncio.sleep(0.5)
                    logger.info(f"Cookie accepted: '{text}'")
                    return True

            # Try common cookie accept selectors
            selectors = [
                "#onetrust-accept-btn-handler",
                '[aria-label="Accept all cookies"]',
                '[aria-label="Accept Cookies"]',
                ".cookie-consent-accept",
            ]
            for sel in selectors:
                el = pg.locator(sel)
                if await el.count() > 0:
                    await el.first.click()
                    await asyncio.sleep(0.5)
                    return True
        except Exception:
            pass

        return False
