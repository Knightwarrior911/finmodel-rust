"""
M&A deal research — 3-phase cascade.

Phase 1: Multi-strategy direct fetch (no search engine CAPTCHA):
         Google News RSS + company newsroom + trade press native search.
Phase 2: Actionbook extension mode — user's real Chrome with authenticated
         Bloomberg + Reuters sessions. Full article access, no paywall.
Phase 3: browser-use LLM agent — nuclear fallback, handles any page structure.

Stealth-browser MCP is available for in-conversation (Claude Code) research
but cannot be called from the Python pipeline directly.
"""

import asyncio
import logging
import re
import time
from urllib.parse import quote

from src.browser.session import BrowserSession
from src.browser.navigation import BrowserNav
from src.research.deal_synthesis import synthesize_deal, is_sufficient

logger = logging.getLogger(__name__)

# ── URL classifiers ──────────────────────────────────────────────────────────

_SKIP_DOMAINS = {
    "gstatic.com", "googleapis.com", "youtube.com",
    "facebook.com", "twitter.com", "instagram.com",
    "maps.google", "translate.google", "accounts.google",
    "google.com/search", "google.com/webhp", "google.com/intl",
    "google.com/sorry", "google.com/preferences",
    "reddit.com", "quora.com", "wikipedia.org",
    "bing.com/images", "bing.com/videos", "bing.com/maps",
    "bing.com/news?", "bing.com/search", "bing.com/ck",
    "duckduckgo.com",
}

_PRIORITY_DOMAINS = {
    "businesswire.com", "prnewswire.com", "globenewswire.com",
    "reuters.com", "bloomberg.com", "ft.com", "wsj.com",
    "sec.gov", "aircargonews.net", "freightwaves.com",
    "logisticsmgmt.com", "supplychaindive.com", "joc.com",
    "pitchbook.com", "preqin.com", "mergermarket.com",
    "yahoo.com", "marketwatch.com",
}

_DEAL_KW = {
    "acquisition", "acquired", "merger", "stake", "deal",
    "transaction", "equity", "buyout", "partnership", "announced",
}


def _rank_urls(urls: list[str]) -> list[str]:
    filtered = [u for u in urls if not any(d in u for d in _SKIP_DOMAINS)]
    priority = [u for u in filtered if any(d in u for d in _PRIORITY_DOMAINS)]
    rest = [u for u in filtered if u not in priority]
    return priority + rest


def _has_deal_content(text: str) -> bool:
    t = text.lower()
    return sum(1 for kw in _DEAL_KW if kw in t) >= 2


def _is_sufficient(summary: dict) -> bool:
    """Phase result is good enough to stop the cascade."""
    has_date = bool(summary.get("announced"))
    has_value = summary.get("deal_value", "undisclosed") != "undisclosed"
    has_acquirer = bool(summary.get("acquirer"))
    has_rationale = bool(summary.get("strategic_rationale"))
    sources_count = len(summary.get("sources", "").split(",")) if summary.get("sources") else 0
    return (has_date and (has_value or (has_acquirer and has_rationale))) or sources_count >= 3


# ── Main searcher ────────────────────────────────────────────────────────────

class NewsSearcher:
    """M&A deal research via 3-phase browser cascade."""

    def __init__(self, session: BrowserSession):
        self.session = session
        self.nav = BrowserNav(session)

    # ── Public API ───────────────────────────────────────────────────────────

    async def find_ma_deal(self, target: str, acquirer: str = "",
                           date_range: str = "2024..2026") -> dict:
        """
        Full M&A deal research across all available browser tools.

        Cascade:
          Phase 1 — Multi-strategy direct fetch: Google News RSS, company
                    newsrooms, trade press native search. No CAPTCHA.
          Phase 2 — Actionbook extension (real Chrome): Bloomberg + Reuters
                    full articles via authenticated browser session.
          Phase 3 — browser-use LLM: navigates to company newsroom and primary
                    sources intelligently when phases 1-2 miss key facts.
        """
        combined: dict = {}

        # Phase 1 ─ multi-strategy direct fetch
        logger.info("[deal] Phase 1: multi-strategy direct fetch")
        p1 = await self._phase1_nodriver(target, acquirer, date_range)
        combined.update(p1)
        if is_sufficient(synthesize_deal(combined)):
            logger.info("[deal] Phase 1 sufficient, stopping")
            return combined

        # Phase 2 ─ Actionbook (Bloomberg + Reuters full access)
        logger.info("[deal] Phase 2: Actionbook (Bloomberg/Reuters)")
        p2 = await self._phase2_actionbook(target, acquirer)
        combined.update(p2)
        if is_sufficient(synthesize_deal(combined)):
            logger.info("[deal] Phase 2 sufficient, stopping")
            return combined

        # Phase 3 ─ browser-use LLM fallback
        logger.info("[deal] Phase 3: browser-use LLM")
        p3 = await self._phase3_browser_use(target, acquirer)
        combined.update(p3)

        return combined

    async def search_all(self, query: str, sources: list[str] = None) -> dict:
        if sources is None:
            sources = ["reuters", "bloomberg", "ft"]
        results = {}
        for src in sources:
            try:
                if src == "bloomberg":
                    url = f"https://www.bloomberg.com/search?query={quote(query)}"
                    tab = await self.session.goto(url)
                    await asyncio.sleep(2)
                    results[src] = await self.session.get_text(tab)
                elif src == "reuters":
                    results[src] = await self.nav.google_search_operators(
                        query, site="reuters.com")
                elif src == "ft":
                    results[src] = await self.nav.google_search_operators(
                        query, site="ft.com")
                elif src == "google_news":
                    url = f"https://news.google.com/search?q={quote(query)}"
                    page = await self.session.goto(url)
                    results[src] = await self.session.get_text(page)
            except Exception as e:
                logger.warning(f"{src} search failed: {e}")
                results[src] = f"ERROR: {e}"
        return results

    async def research_company_profile(self, company_name: str) -> dict:
        results = {}
        for key, query in [
            ("sec_10k",       f'site:sec.gov "{company_name}" 10-K "Item 1" business'),
            ("company_ir",    f'"{company_name}" investor relations about company'),
            ("investor_deck", f'"{company_name}" investor presentation filetype:pdf'),
        ]:
            try:
                results[key] = await self.nav.google_search(query)
            except Exception as e:
                results[key] = f"ERROR: {e}"
        return results

    # ── Phase 1: Multi-strategy direct fetch (no search engine CAPTCHA) ──────

    async def _phase1_nodriver(self, target: str, acquirer: str,
                               date_range: str) -> dict:
        """
        3-strategy cascade that avoids search engine CAPTCHA entirely:
          1. Google News RSS → discover sources → fetch via nodriver
          2. Company / acquirer newsroom direct navigation
          3. Priority trade press native search (WordPress ?s=)
        """
        results: dict = {}

        # Strategy 1: Google News RSS (no CAPTCHA — plain HTTP RSS feed)
        logger.info("[deal p1] Strategy 1: Google News RSS")
        rss_results = await self._fetch_via_gnews_rss(target, acquirer)
        results.update(rss_results)
        if any(_has_deal_content(v) for v in results.values() if v):
            logger.info("[deal p1] RSS strategy sufficient")
            return results

        # Strategy 2: Direct company / acquirer newsroom navigation
        logger.info("[deal p1] Strategy 2: company newsroom direct nav")
        newsroom_results = await self._fetch_company_newsrooms(target, acquirer)
        results.update(newsroom_results)
        if any(_has_deal_content(v) for v in results.values() if v):
            logger.info("[deal p1] Newsroom strategy sufficient")
            return results

        # Strategy 3: Priority trade press native search
        logger.info("[deal p1] Strategy 3: trade press native search")
        trade_results = await self._fetch_trade_press(target, acquirer)
        results.update(trade_results)

        return results

    def _gnews_rss_source_map(self, target: str, acquirer: str) -> list[tuple]:
        """Parse Google News RSS, return list of (domain, native_search_url) pairs."""
        import xml.etree.ElementTree as ET
        import requests as _req

        acq = f" {acquirer}" if acquirer else ""
        q = quote(f'"{target}"{acq} acquisition')
        rss_url = f"https://news.google.com/rss/search?q={q}&hl=en-US&gl=US&ceid=US:en"
        try:
            resp = _req.get(rss_url, timeout=10,
                            headers={"User-Agent": "Mozilla/5.0"})
            root = ET.fromstring(resp.text)
            items = root.findall(".//item")

            source_domains: dict = {
                "Air Cargo News": "aircargonews.net",
                "FreightWaves": "freightwaves.com",
                "DC Velocity": "dcvelocity.com",
                "Transport Topics": "ttnews.com",
                "Logistics Management": "logisticsmgmt.com",
                "Supply Chain Dive": "supplychaindive.com",
                "JOC": "joc.com",
                "Reuters": "reuters.com",
                "Bloomberg": "bloomberg.com",
                "Wall Street Journal": "wsj.com",
                "Financial Times": "ft.com",
                "Yahoo Finance": "finance.yahoo.com",
                "PR Newswire": "prnewswire.com",
                "Business Wire": "businesswire.com",
                "Globe Newswire": "globenewswire.com",
            }

            seen_domains: set = set()
            results = []
            sq = quote(f"{target} {acquirer}".strip())
            for item in items[:10]:
                src = item.findtext("source", "").strip()
                for src_key, domain in source_domains.items():
                    if src_key.lower() in src.lower() and domain not in seen_domains:
                        seen_domains.add(domain)
                        results.append((domain, f"https://www.{domain}/?s={sq}"))
                        break

            logger.info(f"[deal p1] RSS: {len(items)} items, {len(results)} sources")
            return results
        except Exception as e:
            logger.warning(f"[deal p1] RSS failed: {e}")
            return []

    @staticmethod
    def _matches_target(url: str, target_word: str, acquirer: str) -> bool:
        """URL likely covers target — requires word-boundary match in path, not substring."""
        u = url.lower()
        t = target_word.lower()
        acq = acquirer.split()[0].lower() if acquirer else ""
        # Target appears at path word boundary: /ait-... or -ait- or /ait/
        target_hit = (f"/{t}-" in u or f"/{t}/" in u or f"-{t}-" in u or f"-{t}." in u)
        # Or acquirer appears similarly
        acq_hit = acq and (f"/{acq}-" in u or f"/{acq}/" in u or f"-{acq}-" in u)
        return target_hit or acq_hit

    async def _fetch_via_gnews_rss(self, target: str, acquirer: str) -> dict:
        """For each source found in RSS, search on their site and fetch article text."""
        results: dict = {}
        target_word = target.split()[0].lower()

        for domain, search_url in self._gnews_rss_source_map(target, acquirer)[:4]:
            try:
                text = await self.nav.fetch_article(search_url)
                if len(text) < 200:
                    continue
                links = await self.nav.get_links()
                deal_links = [
                    l for l in links
                    if self._matches_target(l, target_word, acquirer)
                    and domain in l
                    and "?s=" not in l and "?q=" not in l
                    and l.count("/") >= 4
                    and not any(d in l for d in _SKIP_DOMAINS)
                ][:2]
                logger.info(f"[deal p1] {domain}: {len(links)} links, {len(deal_links)} deal links")
                for url in deal_links:
                    art = await self.nav.fetch_article(url)
                    if art and _has_deal_content(art):
                        key = domain.split(".")[0]
                        results[key] = art[:5000]
                        logger.info(f"[deal p1] {key}: hit {url[:70]}")
                        break
                await asyncio.sleep(1)
            except Exception as e:
                logger.warning(f"[deal p1] {domain}: {e}")

        return results

    async def _fetch_company_newsrooms(self, target: str, acquirer: str) -> dict:
        """
        Navigate directly to company / acquirer newsrooms.
        Tries common domain patterns without needing a search engine.
        """
        import re as _re
        results: dict = {}

        def _candidate_domains(name: str) -> list[str]:
            # Only strip true legal-entity suffixes, not brand words like "worldwide"
            stop = {"inc", "inc.", "corp", "corp.", "llc", "ltd", "plc",
                    "group", "holdings", "partners", "capital", "equity",
                    "ventures", "corporation", "incorporated", "company", "co"}
            raw_words = [w.lower() for w in _re.split(r"[\s&,]+", name) if len(w) > 1]
            sig_words = [w for w in raw_words if w not in stop]
            if not sig_words:
                return []
            candidates = [sig_words[0] + ".com"]
            if len(sig_words) >= 2:
                candidates.append(sig_words[0] + sig_words[1] + ".com")
            # Also try first two raw words (e.g. "ait" + "worldwide" = "aitworldwide.com")
            if len(raw_words) >= 2 and raw_words[1] not in stop:
                combined = raw_words[0] + raw_words[1] + ".com"
                if combined not in candidates:
                    candidates.append(combined)
            return candidates

        newsroom_paths = ["/newsroom/", "/news/", "/press-releases/", "/media/"]

        for entity_name, entity_key in [(target, "company_announcement"),
                                        (acquirer, "acquirer_announcement")]:
            if not entity_name:
                continue
            for domain in _candidate_domains(entity_name)[:2]:
                for path in newsroom_paths[:3]:
                    url = f"https://www.{domain}{path}"
                    try:
                        text = await self.nav.fetch_article(url)
                        if len(text) < 300:
                            continue
                        search_term = (acquirer.lower() if acquirer and entity_key == "company_announcement"
                                       else target.split()[0].lower())
                        has_mention = search_term in text.lower()
                        if has_mention and _has_deal_content(text):
                            logger.info(f"[deal p1] newsroom listing hit: {url}")
                            links = await self.nav.get_links()
                            article_link = next(
                                (l for l in links
                                 if search_term in l.lower() and domain in l),
                                None
                            )
                            if article_link:
                                art = await self.nav.fetch_article(article_link)
                                if art and _has_deal_content(art):
                                    results[entity_key] = art[:5000]
                                    logger.info(f"[deal p1] {entity_key}: hit {article_link[:70]}")
                                    break
                            else:
                                results[entity_key] = text[:5000]
                                break
                    except Exception:
                        pass
                if entity_key in results:
                    break
                await asyncio.sleep(0.5)

        return results

    async def _fetch_trade_press(self, target: str, acquirer: str) -> dict:
        """Search priority trade press sites using their native WordPress ?s= search."""
        results: dict = {}
        sq = quote(f"{target} {acquirer}".strip())
        target_word = target.split()[0].lower()

        trade_sites = [
            ("aircargonews",    f"https://aircargonews.net/?s={sq}"),
            ("freightwaves",    f"https://www.freightwaves.com/?s={sq}"),
            ("joc",             f"https://www.joc.com/?s={sq}"),
            ("supplychaindive", f"https://www.supplychaindive.com/search/?q={sq}"),
            ("logisticsmgmt",   f"https://www.logisticsmgmt.com/?s={sq}"),
        ]
        for key, search_url in trade_sites:
            domain = search_url.split("/")[2].lstrip("www.")
            try:
                text = await self.nav.fetch_article(search_url)
                if len(text) < 200:
                    continue
                links = await self.nav.get_links()
                deal_links = [
                    l for l in links
                    if self._matches_target(l, target_word, acquirer)
                    and domain in l          # must be on the same domain
                    and "?s=" not in l       # exclude search URLs
                    and "?q=" not in l
                    and "/search" not in l.lower()
                    and l.count("/") >= 4    # proper article path has 4+ slashes
                    and not any(d in l for d in _SKIP_DOMAINS)
                ][:2]
                logger.info(f"[deal p1] {key}: {len(links)} links, {len(deal_links)} deal links")
                for url in deal_links:
                    art = await self.nav.fetch_article(url)
                    if art and _has_deal_content(art):
                        results[key] = art[:5000]
                        logger.info(f"[deal p1] {key}: hit {url[:70]}")
                        break
                if key in results:
                    break
                await asyncio.sleep(1)
            except Exception as e:
                logger.warning(f"[deal p1] trade {key}: {e}")

        return results

    # ── Phase 2: Actionbook (Bloomberg + Reuters full text) ──────────────────

    async def _phase2_actionbook(self, target: str, acquirer: str) -> dict:
        """
        Actionbook extension mode — user's real Chrome profile.
        Has authenticated Bloomberg and Reuters sessions → full article text.
        Opens 3 tabs concurrently: bloomberg / reuters / ft.
        """
        acq = f' "{acquirer}"' if acquirer else ""
        session_id = f"deal_{int(time.time())}"

        import shutil
        _ab_bin = (
            shutil.which("actionbook")
            or shutil.which("actionbook.cmd")
            or r"C:\Users\vinit\AppData\Roaming\npm\actionbook.cmd"
        )

        async def ab(*args) -> str:
            proc = await asyncio.create_subprocess_exec(
                _ab_bin, *args,
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.PIPE,
            )
            out, _ = await proc.communicate()
            return out.decode(errors="replace")

        def search_url(q: str) -> str:
            return f"https://www.google.com/search?q={quote(q)}"

        bloomberg_q = f'site:bloomberg.com "{target}"{acq} acquisition deal'
        reuters_q   = f'site:reuters.com "{target}"{acq} acquisition deal'
        ft_q        = f'site:ft.com "{target}"{acq} acquisition deal'

        results: dict = {}
        try:
            # Start browser — extension mode needs --open-url to create t1
            await ab("browser", "start",
                     "--set-session-id", session_id,
                     "--mode", "extension",
                     "--open-url", search_url(bloomberg_q))

            # Open t2 (reuters) and t3 (ft) concurrently
            await asyncio.gather(
                ab("browser", "goto", search_url(reuters_q),
                   "--session", session_id, "--tab", "t2"),
                ab("browser", "goto", search_url(ft_q),
                   "--session", session_id, "--tab", "t3"),
            )
            await asyncio.sleep(4)

            # Snapshot all 3 — snapshot YAML preserves href: attributes
            snaps = await asyncio.gather(
                ab("browser", "snapshot", "--session", session_id, "--tab", "t1"),
                ab("browser", "snapshot", "--session", session_id, "--tab", "t2"),
                ab("browser", "snapshot", "--session", session_id, "--tab", "t3"),
            )

            source_labels = ["bloomberg", "reuters", "ft"]
            article_domains = {
                "bloomberg": "bloomberg.com/news",
                "reuters":   "reuters.com/",
                "ft":        "ft.com/content",
            }
            href_re = re.compile(r'href:\s*(https?://[^\s\n]+)', re.IGNORECASE)

            # For each tab: find best article URL → navigate → get text
            nav_tasks = []
            tab_ids   = ["t1", "t2", "t3"]
            for snap, label, tab_id in zip(snaps, source_labels, tab_ids):
                domain_hint = article_domains[label]
                hrefs = href_re.findall(snap)
                logger.info(f"[deal p2] {label}: snapshot hrefs={len(hrefs)}, domain_hint={domain_hint}")
                article_urls = [
                    u for u in hrefs
                    if domain_hint in u and "search" not in u
                ][:3]
                if article_urls:
                    logger.info(f"[deal p2] {label}: article URLs found: {article_urls}")
                    nav_tasks.append((label, tab_id, article_urls[0]))
                else:
                    # Fallback: any deal-relevant href on that domain
                    fallback = [u for u in hrefs if label.split(".")[0] in u][:1]
                    logger.info(f"[deal p2] {label}: no article URLs, fallback={fallback}")
                    if fallback:
                        nav_tasks.append((label, tab_id, fallback[0]))

            # Navigate to article URLs concurrently
            if nav_tasks:
                await asyncio.gather(*[
                    ab("browser", "goto", url,
                       "--session", session_id, "--tab", tab_id)
                    for _, tab_id, url in nav_tasks
                ])
                await asyncio.sleep(3)

                # Get text from each article tab
                texts = await asyncio.gather(*[
                    ab("browser", "text",
                       "--session", session_id, "--tab", tab_id)
                    for _, tab_id, _ in nav_tasks
                ])
                for (label, _, url), text in zip(nav_tasks, texts):
                    if text and _has_deal_content(text):
                        logger.info(f"[deal p2] {label}: {url[:70]}")
                        results[f"actionbook_{label}"] = text[:5000]
                    else:
                        results[f"actionbook_{label}"] = text[:2000]

        except RuntimeError:
            logger.info("[deal p2] actionbook not available, skipping")
        except Exception as e:
            logger.warning(f"[deal p2] Actionbook failed: {e}")
        finally:
            try:
                await ab("browser", "close", "--session", session_id)
            except Exception:
                pass

        return results

    # ── Phase 3: browser-use LLM ─────────────────────────────────────────────

    async def _phase3_browser_use(self, target: str, acquirer: str) -> dict:
        """
        LLM-driven navigation. Finds the press release / announcement article
        and returns its full text. Handles any page structure.
        """
        results: dict = {}
        acq_phrase = f" by {acquirer}" if acquirer else ""
        try:
            import os
            from browser_use import Agent
            from browser_use.llm.anthropic.chat import ChatAnthropic as BUChatAnthropic

            api_key = os.environ.get("ANTHROPIC_API_KEY", "")
            if not api_key:
                logger.info("[deal p3] ANTHROPIC_API_KEY not set, skipping browser-use")
                return results
            llm = BUChatAnthropic(model="claude-haiku-4-5-20251001", temperature=0,
                                  api_key=api_key)
            task = (
                f"Research the acquisition{acq_phrase} of {target}.\n"
                f"Steps:\n"
                f"1. Search Google for: \"{target}\"{acq_phrase} acquisition announcement 2025 OR 2026\n"
                f"2. Navigate to the official press release (prefer businesswire.com, "
                f"prnewswire.com, or the company's own newsroom)\n"
                f"3. If not found, try reuters.com or bloomberg.com\n"
                f"4. Return the FULL TEXT of the most relevant article or press release.\n"
                f"Include: announcement date, deal value, stake acquired, "
                f"strategic rationale, advisors if mentioned."
            )
            agent = Agent(task=task, llm=llm, use_vision=False,
                          max_failures=3, max_actions_per_step=5)
            history = await agent.run(max_steps=20)
            final = history.final_result() if history else None
            if final and len(final) > 100:
                logger.info(f"[deal p3] browser-use returned {len(final)} chars")
                results["browser_use"] = final[:6000]
        except ImportError:
            logger.info("[deal p3] browser-use not installed, skipping")
        except Exception as e:
            logger.warning(f"[deal p3] browser-use failed: {e}")
        return results
