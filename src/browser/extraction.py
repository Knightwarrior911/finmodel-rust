"""Content extraction from browser pages.
Text, tables, structured data via nodriver Tab API.
"""

import json
import logging
from typing import Optional

from .session import BrowserSession

logger = logging.getLogger(__name__)


class BrowserExtract:
    """Extract content from nodriver-backed browser pages."""

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
        tab = self.session.default_page
        if not tab:
            return ""
        try:
            return await tab.get_content()
        except Exception:
            return ""

    async def screenshot(self, path: str) -> str:
        """Take full-page screenshot."""
        await self.session.screenshot(path)
        return path

    async def extract_table(self, selector: str = "table") -> list[dict]:
        """Extract HTML table as list of dicts using JS evaluation."""
        tab = self.session.default_page
        if not tab:
            return []
        try:
            raw = await tab.evaluate(f"""JSON.stringify((() => {{
                const tables = document.querySelectorAll('{selector}');
                if (!tables.length) return [];
                const table = tables[0];
                const rows = Array.from(table.querySelectorAll('tr'));
                if (!rows.length) return [];
                const headers = Array.from(rows[0].querySelectorAll('th, td'))
                    .map(c => c.innerText.trim());
                return rows.slice(1).map(row => {{
                    const cells = Array.from(row.querySelectorAll('td, th'))
                        .map(c => c.innerText.trim());
                    const obj = {{}};
                    headers.forEach((h, i) => {{ obj[h || 'col_' + i] = cells[i] || ''; }});
                    return obj;
                }}).filter(r => Object.values(r).some(v => v));
            }})())""")
            return json.loads(raw) if raw else []
        except Exception as e:
            logger.warning(f"extract_table failed: {e}")
            return []

    async def extract_links(self, pattern: str = "") -> list[dict[str, str]]:
        """Extract all links matching pattern (e.g., 'pdf', '10-K')."""
        tab = self.session.default_page
        if not tab:
            return []
        try:
            pat = pattern.lower()
            raw = await tab.evaluate(f"""JSON.stringify((() => {{
                return Array.from(document.querySelectorAll('a[href]'))
                    .filter(a => a.href && (!'{pat}' || a.href.toLowerCase().includes('{pat}')))
                    .slice(0, 100)
                    .map(a => ({{ text: a.textContent.trim(), href: a.href }}));
            }})())""")
            return json.loads(raw) if raw else []
        except Exception as e:
            logger.warning(f"extract_links failed: {e}")
            return []

    async def extract_structured(self, extraction_prompt: str) -> str:
        """Return page context for LLM-based extraction."""
        text = await self.get_text()
        title = await self.get_title()
        return f"Title: {title}\n\n{text[:5000]}"

    async def check_and_accept_cookies(self) -> bool:
        """Quick cookie check + dismiss."""
        from .navigation import BrowserNav
        nav = BrowserNav(self.session)
        return await nav.handle_cookies()
