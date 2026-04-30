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
        """Search DDG HTML via requests (no browser, no CAPTCHA), return page text."""
        import requests
        from bs4 import BeautifulSoup as _BS
        try:
            headers = {
                "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
                "Accept": "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
                "Accept-Language": "en-US,en;q=0.5",
            }
            resp = requests.post(
                "https://html.duckduckgo.com/html/",
                data={"q": query, "kl": "us-en"},
                headers=headers, timeout=15,
            )
            return _BS(resp.text, "lxml").get_text(separator=" ", strip=True)
        except Exception as e:
            logger.warning(f"DDG search failed: {e}")
            return ""

    def search_urls(self, query: str) -> list[str]:
        """Search DDG HTML, return direct result URLs. Uses requests (no browser needed)."""
        import requests
        from bs4 import BeautifulSoup as _BS
        skip = {
            "duckduckgo.com", "google.com", "bing.com",
            "youtube.com", "facebook.com", "twitter.com", "instagram.com",
            "reddit.com", "quora.com", "wikipedia.org",
        }
        try:
            headers = {
                "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
                "Accept": "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
                "Accept-Language": "en-US,en;q=0.5",
            }
            resp = requests.post(
                "https://html.duckduckgo.com/html/",
                data={"q": query, "kl": "us-en"},
                headers=headers, timeout=15,
            )
            soup = _BS(resp.text, "lxml")
            urls = [
                a["href"] for a in soup.select("a.result__a")
                if a.get("href", "").startswith("http")
                and not any(s in a["href"] for s in skip)
            ]
            return list(dict.fromkeys(urls))  # dedupe preserving order
        except Exception as e:
            logger.warning(f"DDG search_urls failed: {e}")
            return []

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

    async def get_links(self) -> list[str]:
        """Extract all external links from current page via JS.
        Returns JSON string from JS so nodriver doesn't wrap values as node dicts.
        """
        tab = self.session.default_page
        if not tab:
            return []
        try:
            evaluate = getattr(tab, "evaluate", None)
            if not evaluate:
                return []
            # DuckDuckGo HTML (html.duckduckgo.com) serves static HTML with direct links.
            # Result links are in <a class="result__a" href="https://..."> — no tracking wrappers.
            # Fallback to generic a[href] for any other search engine page.
            result = await evaluate("""(() => {
                const skip = ['duckduckgo.com', 'gstatic.', 'googleapis.', 'youtube.',
                               'facebook.', 'twitter.', 'instagram.', 'accounts.',
                               'webcache.', 'translate.', 'google.co.',
                               'google.com/search', 'google.com/webhp', 'google.com/intl',
                               'google.com/preferences', 'google.com/sorry',
                               'bing.com/images', 'bing.com/videos', 'bing.com/maps',
                               'bing.com/search', 'bing.com/?', 'bing.com/ck'];

                const isDDG = window.location.hostname.includes('duckduckgo.com');

                let urls = [];
                if (isDDG) {
                    // DDG HTML result links
                    const els = Array.from(document.querySelectorAll('a.result__a'));
                    for (const a of els) {
                        const h = a.href;
                        if (h && h.startsWith('http') && !skip.some(s => h.includes(s)))
                            urls.push(h);
                    }
                } else {
                    urls = Array.from(document.querySelectorAll('a[href]'))
                        .map(a => a.href)
                        .filter(h => h && h.startsWith('http') && h.length > 25
                                  && !skip.some(s => h.includes(s)));
                }

                const seen = new Set();
                const deduped = [];
                for (const u of urls) {
                    if (!seen.has(u)) { seen.add(u); deduped.push(u); }
                }
                return JSON.stringify(deduped);
            })()""")
            if not result:
                return []
            import json
            # nodriver may wrap the string result as {'type': 'string', 'value': '...'}
            if isinstance(result, dict):
                result = result.get("value", "[]")
            return json.loads(result) if isinstance(result, str) else []
        except Exception:
            return []

    async def fetch_article(self, url: str) -> str:
        """Navigate to URL, dismiss cookie popup, return full page text."""
        try:
            tab = await self.session.goto(url)
            await self.handle_cookies(tab)
            await asyncio.sleep(1.5)
            return await self.session.get_text(tab)
        except Exception as e:
            logger.warning(f"fetch_article failed {url[:60]}: {e}")
            return ""

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
            # nodriver may wrap strings as {'type': 'string', 'value': '...'}
            val = result.get("value") if isinstance(result, dict) else result
            if val:
                logger.info(f"Cookie accepted: {val}")
                await asyncio.sleep(0.5)
                return True
        except Exception:
            pass

        return False
