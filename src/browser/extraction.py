"""
Content extraction from browser pages.
Text, tables, structured data via Playwright page API.
"""

import logging
from typing import Optional

from .session import BrowserSession

logger = logging.getLogger(__name__)


class BrowserExtract:
    """Extract content from Playwright-backed browser pages."""

    def __init__(self, session: BrowserSession):
        self.session = session

    async def get_text(self) -> str:
        """Get full page text."""
        return await self.session.get_text()

    async def get_title(self) -> str:
        """Get page title."""
        return await self.session.get_title()

    async def get_full_html(self) -> str:
        """Get full HTML of current page."""
        page = self.session.default_page
        if not page:
            return ""
        return await page.content()

    async def screenshot(self, path: str) -> str:
        """Take full-page screenshot."""
        await self.session.screenshot(path)
        return path

    async def extract_table(self, selector: str = "table") -> list[dict]:
        """Extract HTML table as list of dicts."""
        page = self.session.default_page
        if not page:
            return []

        tables = page.locator(selector)
        if await tables.count() == 0:
            return []

        # Get first matching table
        table = tables.first
        rows = table.locator("tr")
        row_count = await rows.count()

        if row_count == 0:
            return []

        # Extract headers from first row
        headers = []
        header_cells = rows.nth(0).locator("th, td")
        hc = await header_cells.count()
        for i in range(hc):
            headers.append(await header_cells.nth(i).inner_text())

        # Extract data rows
        data = []
        for r in range(1, row_count):
            cells = rows.nth(r).locator("td, th")
            cc = await cells.count()
            row = {}
            for c in range(min(cc, len(headers))):
                key = headers[c] if c < len(headers) else f"col_{c}"
                row[key] = await cells.nth(c).inner_text()
            if row:
                data.append(row)

        return data

    async def extract_links(self, pattern: str = "") -> list[dict[str, str]]:
        """Extract all links matching pattern (e.g., 'pdf', '10-K')."""
        page = self.session.default_page
        if not page:
            return []

        links = page.locator("a")
        count = await links.count()
        result = []

        for i in range(min(count, 100)):
            try:
                href = await links.nth(i).get_attribute("href")
                text = await links.nth(i).inner_text()
                if href and (not pattern or pattern.lower() in href.lower()):
                    result.append({"text": text.strip(), "href": href})
            except Exception:
                continue

        return result

    async def extract_structured(self, extraction_prompt: str) -> str:
        """
        Extract structured data by sending page text for analysis.
        For complex extraction, the calling code handles LLM parsing.
        Returns page context for the extractor to work with.
        """
        text = await self.get_text()
        title = await self.get_title()
        return f"Title: {title}\n\n{text[:5000]}"

    async def check_and_accept_cookies(self) -> bool:
        """Quick cookie check + dismiss."""
        from .navigation import BrowserNav
        nav = BrowserNav(self.session)
        return await nav.handle_cookies()
