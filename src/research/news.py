"""
News search for IB research.
Bloomberg, Reuters, FT, Google News — via Playwright + real Chrome.
"""

import logging
from urllib.parse import quote

from src.browser.session import BrowserSession
from src.browser.navigation import BrowserNav

logger = logging.getLogger(__name__)


class NewsSearcher:
    """Search financial news via browser."""

    def __init__(self, session: BrowserSession):
        self.session = session
        self.nav = BrowserNav(session)

    async def search_bloomberg(self, query: str) -> str:
        """Bloomberg search via their own search page."""
        return await self.nav.search_bloomberg(query)

    async def search_google_news(self, query: str) -> str:
        """Google News search."""
        url = f"https://news.google.com/search?q={quote(query)}"
        page = await self.session.goto(url)
        return await self.session.get_text(page)

    async def search_reuters(self, query: str) -> str:
        """Reuters via Google site: search."""
        return await self.nav.google_search_operators(query, site="reuters.com")

    async def search_ft(self, query: str) -> str:
        """Financial Times via Google site: search."""
        return await self.nav.google_search_operators(query, site="ft.com")

    async def search_all(self, query: str,
                         sources: list[str] = None) -> dict[str, str]:
        """Multi-source fan-out search."""
        if sources is None:
            sources = ["reuters", "bloomberg", "ft"]

        results = {}
        for src in sources:
            try:
                if src == "bloomberg":
                    results[src] = await self.search_bloomberg(query)
                elif src == "reuters":
                    results[src] = await self.search_reuters(query)
                elif src == "ft":
                    results[src] = await self.search_ft(query)
                elif src == "google_news":
                    results[src] = await self.search_google_news(query)
            except Exception as e:
                logger.warning(f"{src} search failed: {e}")
                results[src] = f"ERROR: {e}"

        return results

    async def find_ma_deal(self, target: str, acquirer: str = "",
                           date_range: str = "2024..2026") -> dict:
        """Search for M&A deal across SEC filings, press releases, news."""
        results = {}

        # SEC 8-K search
        sec_q = f'site:sec.gov "{target}" 8-K acquisition {date_range}'
        try:
            results["sec_8k"] = await self.nav.google_search(sec_q)
        except Exception as e:
            results["sec_8k"] = f"ERROR: {e}"

        # Press release search
        pr_q = f'"{target}" acquisition press release {date_range}'
        try:
            results["press_release"] = await self.nav.google_search(pr_q)
        except Exception as e:
            results["press_release"] = f"ERROR: {e}"

        # News coverage
        news_q = f'"{target}" acquisition {date_range} site:reuters.com OR site:bloomberg.com'
        try:
            results["news"] = await self.nav.google_search(news_q)
        except Exception as e:
            results["news"] = f"ERROR: {e}"

        return results

    async def research_company_profile(self, company_name: str) -> dict:
        """Company profile from SEC 10-K Item 1 + IR + investor decks."""
        results = {}

        sec_q = f'site:sec.gov "{company_name}" 10-K "Item 1" business'
        try:
            results["sec_10k"] = await self.nav.google_search(sec_q)
        except Exception as e:
            results["sec_10k"] = f"ERROR: {e}"

        ir_q = f'"{company_name}" investor relations about company'
        try:
            results["company_ir"] = await self.nav.google_search(ir_q)
        except Exception as e:
            results["company_ir"] = f"ERROR: {e}"

        deck_q = f'"{company_name}" investor presentation filetype:pdf'
        try:
            results["investor_deck"] = await self.nav.google_search(deck_q)
        except Exception as e:
            results["investor_deck"] = f"ERROR: {e}"

        return results
