"""
Browser session manager via Playwright + real Chrome profile.
Copies user's Chrome profile to temp dir, launches Chrome with CDP,
connects Playwright. Cookies, extensions, logins preserved.
Zero bot detection — real Chrome fingerprint.

Foolproof: retry with backoff, port conflict resolution, process cleanup.
"""

import asyncio
import logging
import os
import shutil
import signal
import subprocess
import tempfile
import time
from typing import Optional

import requests
from playwright.async_api import async_playwright, Browser, BrowserContext, Page

logger = logging.getLogger(__name__)

CHROME_PATH = r"C:\Program Files\Google\Chrome\Application\chrome.exe"
USER_DATA_SRC = r"C:\Users\vinit\AppData\Local\Google\Chrome\User Data"
CDP_PORT = 9222
MAX_RETRIES = 3
RETRY_DELAY = 2


def _kill_all_chrome():
    """Force-kill all Chrome processes."""
    try:
        subprocess.run(
            ["taskkill", "/F", "/IM", "chrome.exe"],
            capture_output=True, timeout=10
        )
    except Exception:
        pass
    time.sleep(2)


def _check_port_free(port: int) -> bool:
    """Check if CDP port is available."""
    try:
        resp = requests.get(f"http://127.0.0.1:{port}/json/version", timeout=1)
        return False  # Port is in use (CDP is responding)
    except Exception:
        return True  # Port is free


class BrowserSession:
    """
    Launches Chrome with user's real profile (copied to temp) + CDP.
    Playwright connects via CDP for full browser control.
    Retries up to 3 times on failure.
    """

    def __init__(self, headless: bool = False, port: int = CDP_PORT):
        self.headless = headless
        self.port = port
        self._playwright = None
        self._browser: Optional[Browser] = None
        self._chrome_proc: Optional[subprocess.Popen] = None
        self._temp_dir: Optional[str] = None
        self._started = False

    async def start(self) -> BrowserContext:
        """
        Copy Chrome profile, launch Chrome with CDP, connect Playwright.
        Returns default BrowserContext with user's cookies/extensions.
        Retries on failure.
        """
        last_error = None
        for attempt in range(MAX_RETRIES):
            try:
                return await self._start_inner(attempt)
            except Exception as e:
                last_error = e
                logger.warning(f"Browser start attempt {attempt + 1} failed: {e}")
                await self._cleanup()
                await asyncio.sleep(RETRY_DELAY * (attempt + 1))

        raise RuntimeError(f"Browser failed after {MAX_RETRIES} attempts: {last_error}")

    async def _start_inner(self, attempt: int) -> BrowserContext:
        """Single start attempt."""
        # 1. Ensure Chrome is dead + port is free
        _kill_all_chrome()
        if not _check_port_free(self.port):
            logger.warning(f"Port {self.port} in use, killing processes")
            _kill_all_chrome()
            time.sleep(1)

        # 2. Copy profile
        self._temp_dir = tempfile.mkdtemp(prefix="va-chrome-")
        self._copy_profile(USER_DATA_SRC, self._temp_dir)
        logger.info(f"Profile copied (attempt {attempt + 1})")

        # 3. Launch Chrome
        args = [
            CHROME_PATH,
            f"--remote-debugging-port={self.port}",
            f"--user-data-dir={self._temp_dir}",
            "--no-first-run",
            "--no-default-browser-check",
            "--disable-background-networking",
            "--disable-sync",
            "--no-pings",
            "about:blank",
        ]
        self._chrome_proc = subprocess.Popen(
            args, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL
        )

        # 4. Wait for CDP
        await self._wait_for_cdp()

        # 5. Connect Playwright
        self._playwright = await async_playwright().start()
        self._browser = await self._playwright.chromium.connect_over_cdp(
            f"http://127.0.0.1:{self.port}"
        )

        self._started = True
        logger.info("Playwright connected")
        return self._browser.contexts[0] if self._browser.contexts else None

    async def _wait_for_cdp(self, timeout: int = 45):
        """Poll until CDP is available. Retry Chrome launch if it crashes."""
        deadline = time.time() + timeout
        while time.time() < deadline:
            # Check if Chrome process is still alive
            if self._chrome_proc and self._chrome_proc.poll() is not None:
                raise RuntimeError(f"Chrome crashed with code {self._chrome_proc.returncode}")

            try:
                resp = requests.get(
                    f"http://127.0.0.1:{self.port}/json/version", timeout=2
                )
                if resp.status_code == 200:
                    return
            except Exception:
                pass
            await asyncio.sleep(0.5)

        raise TimeoutError(f"CDP not available after {timeout}s")

    def _copy_profile(self, src: str, dst: str):
        """Copy essential Chrome profile files. Retries on locked files."""
        ignore = shutil.ignore_patterns(
            "Cache", "Code Cache", "GPUCache", "Service Worker",
            "WebStorage", "ShaderCache", "DawnGraphiteCache",
            "DawnWebGPUCache", "Safe Browsing", "OptimizationHints",
            "History", "Favicons", "Top Sites", "Cookies-journal",
        )
        for item in ["Default", "Local State"]:
            src_path = os.path.join(src, item)
            dst_path = os.path.join(dst, item)
            for retry in range(3):
                try:
                    if os.path.isdir(src_path):
                        shutil.copytree(src_path, dst_path, ignore=ignore)
                    elif os.path.isfile(src_path):
                        shutil.copy2(src_path, dst_path)
                    break
                except (PermissionError, OSError) as e:
                    if retry < 2:
                        time.sleep(1)
                    else:
                        logger.warning(f"Could not copy {item}: {e}")

    async def new_page(self) -> Page:
        """Create a new page."""
        if not self._browser:
            raise RuntimeError("Browser not started")
        ctx = self._browser.contexts[0] if self._browser.contexts else None
        if not ctx:
            ctx = await self._browser.new_context()
        page = await ctx.new_page()
        # Set longer timeout for slow pages
        page.set_default_timeout(30000)
        return page

    @property
    def default_page(self) -> Optional[Page]:
        if not self._browser or not self._browser.contexts:
            return None
        ctx = self._browser.contexts[0]
        return ctx.pages[0] if ctx.pages else None

    async def goto(self, url: str, page: Page = None,
                   wait_until: str = "domcontentloaded") -> Page:
        """Navigate to URL. Creates page if needed.
        Handles Google consent redirects and other navigation interruptions."""
        pg = page or self.default_page or await self.new_page()
        for attempt in range(3):
            try:
                await pg.goto(url, wait_until=wait_until, timeout=30000)
                break
            except Exception as e:
                err = str(e)
                if "interrupted by another navigation" in err or "net::" in err:
                    # Page is navigating — wait for it to settle, then use current page
                    await asyncio.sleep(3)
                    # Check if page landed somewhere useful
                    current_url = pg.url
                    if current_url and current_url != "about:blank":
                        break
                if attempt < 2:
                    await asyncio.sleep(2)
                    # Try gentler approach
                    try:
                        await pg.goto(url, wait_until="commit", timeout=30000)
                        await asyncio.sleep(3)
                        break
                    except Exception:
                        continue
                else:
                    # Last attempt: just try to load whatever page is there
                    await asyncio.sleep(2)
        return pg

    async def get_text(self, page: Page = None) -> str:
        pg = page or self.default_page
        if not pg:
            return ""
        try:
            return await pg.inner_text("body")
        except Exception:
            return ""

    async def get_title(self, page: Page = None) -> str:
        pg = page or self.default_page
        if not pg:
            return ""
        return await pg.title()

    async def screenshot(self, path: str, page: Page = None):
        pg = page or self.default_page
        if pg:
            await pg.screenshot(path=path, full_page=True)

    async def click(self, selector: str, page: Page = None):
        pg = page or self.default_page
        if pg:
            await pg.click(selector, timeout=5000)

    async def type_text(self, selector: str, text: str, page: Page = None):
        pg = page or self.default_page
        if pg:
            await pg.fill(selector, text)

    async def get_cookies(self) -> list:
        if not self._browser or not self._browser.contexts:
            return []
        return await self._browser.contexts[0].cookies()

    async def _cleanup(self):
        """Clean up without raising errors."""
        if self._browser:
            try:
                await self._browser.close()
            except Exception:
                pass
            self._browser = None
        if self._playwright:
            try:
                await self._playwright.stop()
            except Exception:
                pass
            self._playwright = None
        if self._chrome_proc:
            try:
                self._chrome_proc.terminate()
                self._chrome_proc.wait(timeout=5)
            except Exception:
                try:
                    self._chrome_proc.kill()
                except Exception:
                    pass
            self._chrome_proc = None

    async def close(self):
        await self._cleanup()
        if self._temp_dir and os.path.exists(self._temp_dir):
            try:
                shutil.rmtree(self._temp_dir, ignore_errors=True)
            except Exception:
                pass
        self._started = False

    @property
    def is_connected(self) -> bool:
        return self._started and self._browser is not None
