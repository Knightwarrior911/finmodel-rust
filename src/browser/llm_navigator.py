"""
LLM-powered decision layer for browser pipeline.
Claude makes the 4 judgment calls that pure Python heuristics get wrong:
  1. Cookie popup detection  — visual screenshot analysis
  2. Annual report link pick — from all candidates on an IR page
  3. Document verification   — correct company + correct filing type
  4. Reports section finder  — when keyword scanning misses sub-nav
"""

import base64
import json
import logging
from typing import Optional

import anthropic

logger = logging.getLogger(__name__)

# Haiku: fast + cheap for navigation micro-decisions (~$0.001 per pipeline run total)
_MODEL = "claude-haiku-4-5-20251001"


def _parse_json(raw: str) -> dict:
    """Strip markdown fences if present, then parse JSON."""
    raw = raw.strip()
    if "```" in raw:
        parts = raw.split("```")
        raw = parts[1].strip()
        if raw.startswith("json"):
            raw = raw[4:].strip()
    return json.loads(raw)


class LLMNavigator:
    """Claude-backed decision layer for browser pipeline ambiguities."""

    def __init__(self):
        self._client: Optional[anthropic.AsyncAnthropic] = None

    @property
    def client(self) -> anthropic.AsyncAnthropic:
        if self._client is None:
            self._client = anthropic.AsyncAnthropic()
        return self._client

    # ------------------------------------------------------------------
    # 1. Cookie popup — visual screenshot detection
    # ------------------------------------------------------------------

    async def decide_cookie_action(self, screenshot_path: str) -> Optional[str]:
        """
        Look at a screenshot and return the exact text of the cookie accept button.
        Returns None if no popup is visible or LLM call fails.
        """
        try:
            with open(screenshot_path, "rb") as f:
                img_b64 = base64.standard_b64encode(f.read()).decode("utf-8")

            resp = await self.client.messages.create(
                model=_MODEL,
                max_tokens=80,
                messages=[{
                    "role": "user",
                    "content": [
                        {
                            "type": "image",
                            "source": {
                                "type": "base64",
                                "media_type": "image/png",
                                "data": img_b64,
                            },
                        },
                        {
                            "type": "text",
                            "text": (
                                "Is there any popup, modal, banner, or overlay blocking the main page content? "
                                "This includes: cookie consent popups, geo-location redirects ('you are in country X'), "
                                "age verification, newsletter signups, or any other overlay. "
                                "Reply with JSON only — no other text.\n"
                                'If blocking popup present: {"has_popup": true, "button_text": "<exact text of the button/link to DISMISS or STAY on current page>"}\n'
                                'If no popup:               {"has_popup": false}'
                            ),
                        },
                    ],
                }],
            )
            result = _parse_json(resp.content[0].text)
            if result.get("has_popup") and result.get("button_text"):
                btn = result["button_text"]
                logger.info(f"LLM cookie: popup found, button='{btn}'")
                return btn
        except Exception as e:
            logger.debug(f"LLM cookie check failed: {e}")
        return None

    # ------------------------------------------------------------------
    # 2. Annual report link — pick best from candidate list
    # ------------------------------------------------------------------

    async def select_annual_report_link(
        self, links: list[dict], company: str, year: str
    ) -> Optional[str]:
        """
        From {text, href} candidates on an IR/publications page, return the href
        of the full consolidated IFRS/annual group report for company+year.
        Rejects: subsidiary reports, HGB standalone, half-year, sustainability.
        Returns None if no suitable link found or LLM fails.
        """
        if not links:
            return None
        try:
            numbered = "\n".join(
                f"{i+1}. Text: \"{l.get('text','')[:80]}\"  URL: {l.get('href','')[:120]}"
                for i, l in enumerate(links)
            )
            resp = await self.client.messages.create(
                model=_MODEL,
                max_tokens=120,
                messages=[{
                    "role": "user",
                    "content": (
                        f"I need the FULL CONSOLIDATED annual report for {company}, fiscal year {year}.\n"
                        f"DO NOT pick: subsidiary entity reports, HGB/statutory standalone accounts, "
                        f"half-year reports, sustainability/ESG reports, press releases, earnings releases, "
                        f"proxy statements, or individual chapter PDFs.\n"
                        f"DO pick: the single complete consolidated group annual report PDF.\n\n"
                        f"Candidates:\n{numbered}\n\n"
                        f"Reply with JSON only:\n"
                        f'  {{"index": <1-based integer>, "reason": "<brief>"}}\n'
                        f'  or {{"index": null, "reason": "none found"}}'
                    ),
                }],
            )
            result = _parse_json(resp.content[0].text)
            idx = result.get("index")
            reason = result.get("reason", "")
            if idx and 1 <= int(idx) <= len(links):
                href = links[int(idx) - 1].get("href", "")
                logger.info(
                    f"LLM link pick [{idx}]: '{links[int(idx)-1].get('text','')[:60]}' — {reason}"
                )
                return href
            logger.info(f"LLM link pick: none found — {reason}")
        except Exception as e:
            logger.debug(f"LLM link selection failed: {e}")
        return None

    # ------------------------------------------------------------------
    # 3. Document verification — correct company + correct filing type
    # ------------------------------------------------------------------

    async def verify_document(
        self, first_page_text: str, company: str, year: str
    ) -> tuple[bool, str]:
        """
        Verify the downloaded PDF is the full consolidated group annual report.
        Returns (is_correct, reason).
        On LLM failure returns (True, "LLM unavailable") — fail open to preserve pipeline.
        """
        try:
            resp = await self.client.messages.create(
                model=_MODEL,
                max_tokens=120,
                messages=[{
                    "role": "user",
                    "content": (
                        f"We downloaded a PDF expecting the FULL CONSOLIDATED IFRS annual report "
                        f"for {company}, fiscal year {year}.\n\n"
                        f"First page text of the downloaded PDF:\n\"\"\"\n{first_page_text[:700]}\n\"\"\"\n\n"
                        f"Is this the correct document? Reject if: it is a subsidiary entity's report, "
                        f"HGB/statutory standalone accounts, half-year, sustainability, or wrong year.\n"
                        f"Reply with JSON only:\n"
                        f'  {{"correct": true,  "reason": "<brief>"}}\n'
                        f'  {{"correct": false, "reason": "<brief>"}}'
                    ),
                }],
            )
            result = _parse_json(resp.content[0].text)
            correct = bool(result.get("correct"))
            reason = result.get("reason", "")
            logger.info(f"LLM doc verify: correct={correct} — {reason}")
            return correct, reason
        except Exception as e:
            logger.debug(f"LLM document verification failed: {e}")
            return True, "LLM unavailable — heuristic fallback"

    # ------------------------------------------------------------------
    # 3b. Latest filing selector — prefers most recent quarterly/interim
    # ------------------------------------------------------------------

    async def select_latest_filing(
        self, links: list[dict], company: str, year: int
    ) -> Optional[str]:
        """
        From PDF candidates, pick the most recent quarterly or semi-annual report.
        Prefers: Q1/Q2/Q3 reports, H1 reports, interim reports.
        Rejects: annual reports (handled separately), press releases, sustainability.
        Returns href or None.
        """
        if not links:
            return None
        try:
            numbered = "\n".join(
                f"{i+1}. Text: \"{l.get('text','')[:80]}\"  URL: {l.get('href','')[:120]}"
                for i, l in enumerate(links)
            )
            resp = await self.client.messages.create(
                model=_MODEL,
                max_tokens=120,
                messages=[{
                    "role": "user",
                    "content": (
                        f"I want the MOST RECENT quarterly or semi-annual (half-year) financial report "
                        f"for {company} — specifically one published in {year} or {year+1} that includes "
                        f"a balance sheet (total assets, debt, cash).\n"
                        f"Prefer: Q1, Q2/H1, Q3, interim reports with balance sheet.\n"
                        f"Reject: full annual reports, press releases, presentations, sustainability reports.\n\n"
                        f"Candidates:\n{numbered}\n\n"
                        f"Reply with JSON only:\n"
                        f'  {{"index": <1-based integer>, "reason": "<brief>"}}\n'
                        f'  or {{"index": null, "reason": "none found"}}'
                    ),
                }],
            )
            result = _parse_json(resp.content[0].text)
            idx = result.get("index")
            reason = result.get("reason", "")
            if idx and 1 <= int(idx) <= len(links):
                href = links[int(idx) - 1].get("href", "")
                logger.info(f"LLM latest filing [{idx}]: '{links[int(idx)-1].get('text','')[:60]}' — {reason}")
                return href
            logger.info(f"LLM latest filing: none found — {reason}")
        except Exception as e:
            logger.debug(f"LLM latest filing selection failed: {e}")
        return None

    # ------------------------------------------------------------------
    # 4. Reports section finder — sub-nav fallback
    # ------------------------------------------------------------------

    async def find_reports_section(
        self, links: list[dict], company: str, year: str
    ) -> Optional[str]:
        """
        From all navigation links on an IR page, find the URL of the annual
        reports / publications section. Used when keyword matching fails.
        Returns href or None.
        """
        if not links:
            return None
        try:
            # Cap at 30 links to keep tokens low
            subset = links[:30]
            numbered = "\n".join(
                f"{i+1}. \"{l.get('text','')[:60]}\" → {l.get('href','')[:100]}"
                for i, l in enumerate(subset)
            )
            resp = await self.client.messages.create(
                model=_MODEL,
                max_tokens=100,
                messages=[{
                    "role": "user",
                    "content": (
                        f"I am on the investor relations page of {company}. "
                        f"I want to navigate to the section that lists annual reports or publications "
                        f"so I can find the {year} annual report PDF.\n\n"
                        f"Navigation links on this page:\n{numbered}\n\n"
                        f"Which link number leads to annual reports / publications / financial reports? "
                        f"Reply with JSON only:\n"
                        f'  {{"index": <number>, "reason": "<brief>"}}\n'
                        f'  or {{"index": null}} if none is suitable.'
                    ),
                }],
            )
            result = _parse_json(resp.content[0].text)
            idx = result.get("index")
            if idx and 1 <= int(idx) <= len(subset):
                href = subset[int(idx) - 1].get("href", "")
                logger.info(
                    f"LLM nav section [{idx}]: '{subset[int(idx)-1].get('text','')[:50]}' → {href[:80]}"
                )
                return href
        except Exception as e:
            logger.debug(f"LLM reports section failed: {e}")
        return None
