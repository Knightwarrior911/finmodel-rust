"""Browser session using nodriver (CDP, anti-detection, no Playwright overhead).

Replaces the old Playwright + Chrome profile copy + CDP port management.
nodriver handles anti-detection automatically — no profile copy needed.
"""

import asyncio
import logging
from typing import Optional

import nodriver as uc
from bs4 import BeautifulSoup

logger = logging.getLogger(__name__)


class BrowserSession:
    """Browser session backed by nodriver (undetectable Chrome via CDP)."""

    def __init__(self, headless: bool = False):
        self.headless = headless
        self._browser: Optional[uc.Browser] = None
        self._tab = None  # active nodriver Tab
        self._started = False

    async def start(self):
        """Launch Chrome via nodriver. No profile copy, no port management."""
        self._browser = await uc.start(headless=self.headless)
        # Get the first tab (nodriver always opens one)
        tabs = [t for t in self._browser.targets if getattr(t, "type_", "") == "page"]
        self._tab = tabs[0] if tabs else None
        self._started = True
        logger.info("nodriver browser started")
        return self._tab

    async def goto(self, url: str, page=None, wait_until: str = "") -> object:
        """Navigate to URL. Returns active Tab."""
        if not self._browser:
            await self.start()
        try:
            self._tab = await self._browser.get(url)
        except Exception as e:
            logger.warning(f"Navigation error for {url[:80]}: {e}")
            # Try to recover current tab
            tabs = [t for t in self._browser.targets if getattr(t, "type_", "") == "page"]
            if tabs:
                self._tab = tabs[0]
        return self._tab

    async def new_page(self):
        """Open a new tab."""
        if not self._browser:
            await self.start()
        try:
            self._tab = await self._browser.get("about:blank", new_tab=True)
        except Exception as e:
            logger.warning(f"new_page failed: {e}")
        return self._tab

    @property
    def default_page(self):
        """Active tab (nodriver Tab object)."""
        return self._tab

    async def get_text(self, tab=None) -> str:
        """Get full page text (HTML stripped)."""
        t = tab or self._tab
        if not t:
            return ""
        try:
            html = await t.get_content()
            return BeautifulSoup(html, "lxml").get_text(separator=" ", strip=True)
        except Exception:
            return ""

    async def get_title(self, tab=None) -> str:
        """Get page title."""
        t = tab or self._tab
        if not t:
            return ""
        try:
            result = await t.evaluate("document.title")
            return str(result) if result else ""
        except Exception:
            return ""

    async def screenshot(self, path: str, tab=None):
        """Save full-page screenshot."""
        t = tab or self._tab
        if t:
            try:
                await t.save_screenshot(path)
            except Exception as e:
                logger.warning(f"Screenshot failed: {e}")

    async def click(self, selector: str, tab=None):
        """Click element by CSS selector."""
        t = tab or self._tab
        if not t:
            return
        try:
            el = await t.select(selector)
            if el:
                await el.click()
        except Exception:
            pass

    async def type_text(self, selector: str, text: str, tab=None):
        """Type text into element."""
        t = tab or self._tab
        if not t:
            return
        try:
            el = await t.select(selector)
            if el:
                await el.send_keys(text)
        except Exception:
            pass

    async def get_cookies(self) -> list:
        """Return current cookies."""
        if not self._browser:
            return []
        try:
            return await self._browser.cookies.get_all()
        except Exception:
            return []

    async def close(self):
        """Shut down the browser."""
        if self._browser:
            try:
                self._browser.stop()
            except Exception:
                pass
        self._browser = None
        self._tab = None
        self._started = False

    @property
    def is_connected(self) -> bool:
        return self._started and self._browser is not None
