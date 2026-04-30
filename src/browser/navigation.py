"""Browser navigation for IB research.
Google search, company IR, Bloomberg, cookie handling.
Uses nodriver Tab via BrowserSession.
"""

import asyncio
import logging
from urllib.parse import quote
from typing import Optional

from .session import BrowserSession

logger = logging.getLogger(__name__)


class BrowserNav:
    """High-level navigation on BrowserSession (nodriver-backed)."""

    def __init__(self, session: BrowserSession):
        self.session = session

    async def google_search(self, query: str) -> str:
        """Search Google, return page text. Handles consent redirects."""
        url = f"https://www.google.com/search?q={quote(query)}"
        try:
            tab = await self.session.goto(url)
        except Exception:
            await asyncio.sleep(2)
            tab = self.session.default_page
        if tab:
            await self.handle_cookies(tab)
        await asyncio.sleep(1)
        return await self.session.get_text(tab) if tab else ""

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
        for line in text.split("\n"):
            if "investor" in line.lower() and (".com" in line or "http" in line):
                return line.strip()
        return None

    # --- SEC EDGAR ---

    async def open_sec_filing_list(self, cik: str, form_type: str = "10-K") -> str:
        """Open SEC EDGAR filing list. Returns page text."""
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
        """Search Bloomberg. CRITICAL: Never Google site:bloomberg.com — direct URLs 404."""
        url = f"https://www.bloomberg.com/search?query={quote(query)}"
        tab = await self.session.goto(url)
        await asyncio.sleep(2)
        return await self.session.get_text(tab)

    # --- Company IR Navigation ---

    async def navigate_ir_site(self, ir_url: str) -> str:
        """Navigate to company IR page, handle cookies, return text."""
        tab = await self.session.goto(ir_url)
        await self.handle_cookies(tab)
        await asyncio.sleep(1)
        return await self.session.get_text(tab)

    # --- Cookie Handling ---

    async def handle_cookies(self, page=None) -> bool:
        """Detect and dismiss cookie popups via JS (works with nodriver Tab)."""
        tab = page or self.session.default_page
        if not tab:
            return False

        try:
            result = await tab.evaluate("""(() => {
                const labels = [
                    'Accept All', 'Accept All Cookies', 'Accept Cookies',
                    'Accept and Continue', 'Accept & Continue', 'I Accept',
                    'Allow All', 'Allow Cookies', 'Accept', 'Agree',
                    'Akzeptieren', 'Accepter', 'Aceptar', 'Accetta'
                ];

                // OneTrust button (most common)
                const ot = document.getElementById('onetrust-accept-btn-handler');
                if (ot && ot.offsetParent !== null) { ot.click(); return 'onetrust'; }

                // Common aria labels
                const ariaLabels = ['Accept all cookies', 'Accept Cookies'];
                for (const label of ariaLabels) {
                    const el = document.querySelector(`[aria-label="${label}"]`);
                    if (el && el.offsetParent !== null) { el.click(); return label; }
                }

                // Buttons/links by text content
                const clickable = Array.from(
                    document.querySelectorAll('button, a[role="button"], a[class*="cookie"], a[class*="consent"]')
                );
                for (const el of clickable) {
                    const t = el.textContent.trim();
                    if (labels.some(l => t === l || t.startsWith(l))) {
                        if (el.offsetParent !== null) { el.click(); return t; }
                    }
                }
                return null;
            })()""")
            if result:
                logger.info(f"Cookie accepted: {result}")
                await asyncio.sleep(0.5)
                return True
        except Exception:
            pass

        return False
