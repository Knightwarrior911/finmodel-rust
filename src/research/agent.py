"""
Main research agent orchestrator.
Combines SEC EDGAR API (direct HTTP) + Playwright browser (real Chrome profile)
to execute IB research queries.

Flow:
1. Detect query type from natural language
2. SEC API first (US companies, instant, no browser)
3. Browser for everything else (real Chrome = zero bot detection)
4. Cross-verify (2+ sources)
5. Synthesize + return
"""

import asyncio
import logging
from datetime import datetime
from typing import Optional

from src.models.company import Company
from src.models.research import (
    ResearchQuery, ResearchResult, QueryType,
    SourceTier, AccessMethod, VerificationResult,
)
from src.research.router import detect_query_type, get_source_chain, detect_listing_type
from src.research.sec_edgar import SECEdgarClient
from src.browser.session import BrowserSession
from src.browser.navigation import BrowserNav
from src.browser.extraction import BrowserExtract
from src.research.filings import GlobalFilingNavigator
from src.research.news import NewsSearcher
from src.research.browser_pipeline import BrowserPipeline
from src.research.market_data import get_market_data
from src.research.deal_synthesis import synthesize_deal as _synthesize_deal
from kb.sectors import detect_sector
from kb.ev_bridge import EVBridgeInput, format_ev_bridge
from src.research.output_writer import ResearchExcelWriter
from src.research.pptx_output import ResearchPPTXWriter

logger = logging.getLogger(__name__)


def _parse_ma_query(user_query: str, company_hint: str = "") -> tuple[str, str]:
    """
    Extract (target, acquirer) from an M&A query string.
    Falls back to company_hint if provided.
    """
    import re as _re

    # Strip conversational preamble
    preamble = _re.compile(
        r'^(?:what\s+(?:is|are|was|were)\s+(?:the\s+)?|'
        r'(?:research|find|analyze|tell\s+me\s+about|look\s+up)\s+(?:the\s+)?|'
        r'(?:deal\s+(?:terms|analysis|details|value)\s+for\s+(?:the\s+)?))',
        _re.IGNORECASE,
    )
    q = preamble.sub("", user_query.strip()).strip()

    # "X acquisition by Y" / "X acquired by Y" / "X merger with Y"
    m = _re.search(
        r'^(.+?)\s+(?:acquisition|acquired?|merger)\s+(?:by|with|of)\s+(.+)',
        q, _re.IGNORECASE,
    )
    if m:
        return m.group(1).strip(), m.group(2).strip().rstrip(".,;?!")

    # "merger of X with Y" / "merger between X and Y"
    m = _re.search(
        r'^merger\s+(?:of|between)\s+(.+?)\s+(?:with|and)\s+(.+)',
        q, _re.IGNORECASE,
    )
    if m:
        return m.group(1).strip(), m.group(2).strip().rstrip(".,;?!")

    # "Y acquires X" / "Y buys X" / "Y to acquire X"
    m = _re.search(
        r'^(.+?)\s+(?:acquires?|buys?|purchased?|(?:to\s+)?acquire[sd]?)\s+(.+)',
        q, _re.IGNORECASE,
    )
    if m:
        return m.group(2).strip().rstrip(".,;?!"), m.group(1).strip()

    # "X / Y deal" or "X / Y acquisition"
    m = _re.search(r'^([^/]+?)\s*/\s*([^/]+?)(?:\s+deal|\s+acquisition|\s+merger|$)', q, _re.IGNORECASE)
    if m:
        return m.group(1).strip(), m.group(2).strip()

    # Fallback: stop before first trigger keyword → that's the target
    triggers = ("acquisition", "acquir", "merger", "takeover", "buyout", "sold", "bought")
    words = q.split()
    for i, w in enumerate(words):
        if any(t in w.lower() for t in triggers):
            target = " ".join(words[:i]).strip()
            return target or company_hint, ""

    return company_hint or q, ""


class ResearchAgent:
    """IB research agent. SEC API + real Chrome browser."""

    def __init__(self):
        self.sec = SECEdgarClient()
        self._browser: Optional[BrowserSession] = None

    @property
    def browser(self) -> BrowserSession:
        if self._browser is None:
            self._browser = BrowserSession()
        return self._browser

    async def _ensure_browser(self):
        """Start browser if not already running."""
        if self._browser is None or not self._browser.is_connected:
            self._browser = BrowserSession()
            await self._browser.start()

    # --- Main entry ---

    async def research(self, user_query: str, company_name: str = "",
                       ticker: str = "") -> ResearchResult:
        """Execute a natural-language IB research query."""
        started_at = datetime.now()

        # 1. Classify
        query_type = detect_query_type(user_query)
        source_chain = get_source_chain(query_type)
        listing_type = detect_listing_type(company_name) if company_name else "us"

        query = ResearchQuery(
            user_query=user_query,
            query_type=query_type,
            company_name=company_name,
        )

        result = ResearchResult(
            query=query,
            status="in_progress",
            started_at=started_at,
        )

        # 2. Execute sources by priority
        sources = sorted(source_chain.sources, key=lambda s: s.priority)

        for source in sources:
            try:
                if source.tier == SourceTier.TIER_1_FILINGS and ticker:
                    await self._fetch_sec(source, ticker, result)
                elif source.access_method in (AccessMethod.BROWSER_CDP, AccessMethod.BROWSER_HEADED):
                    await self._fetch_browser(source, result)
            except Exception as e:
                logger.warning(f"Source '{source.name}' failed: {e}")
                source.result = "failed"
                result.sources_failed.append(source)

        # 3. Verify
        result.verification = self._verify(result)

        # 4. Status
        result.status = "complete" if result.verification.passed else "gaps_found"
        if not result.findings:
            result.status = "failed"

        result.completed_at = datetime.now()

        # Cleanup
        if self._browser:
            await self._browser.close()
            self._browser = None

        return result

    async def _fetch_sec(self, source, ticker: str, result: ResearchResult):
        """Fetch from SEC EDGAR XBRL API."""
        company, financials = self.sec.get_company_financials(ticker)
        sector = detect_sector(company.name)

        result.findings["company"] = company
        result.findings["financials"] = financials
        if sector:
            result.findings["sector"] = sector.name

        source.result = "found"
        result.sources_checked.append(source)

    async def _fetch_browser(self, source, result: ResearchResult):
        """Fetch via browser (real Chrome) — actually executes the pipeline."""
        pipeline = BrowserPipeline()
        try:
            company = result.query.company_name
            year = result.query.additional_context.get("year", "2025")
            country = detect_listing_type(company)

            if result.query.query_type == QueryType.EARNINGS_ANALYSIS:
                # Find annual report, extract financials
                fin = await self._run_financial_pipeline(pipeline, company, year, country)
                result.findings["extracted_financials"] = fin
                source.result = "found"

            elif result.query.query_type == QueryType.GENERAL_COMPANY_INTELLIGENCE:
                # Find company IR page
                await self._ensure_browser()
                nav = BrowserNav(self.browser)
                ir_url = await nav.find_company_ir(company)
                if ir_url:
                    result.findings["ir_url"] = ir_url
                source.result = "found" if ir_url else "checked"

            elif result.query.query_type == QueryType.TRANSACTION_TERMS:
                await self._ensure_browser()
                news = NewsSearcher(self.browser)
                target, acquirer = _parse_ma_query(
                    result.query.user_query, company
                )
                deal_info = await news.find_ma_deal(target, acquirer=acquirer)
                news_coverage = await news.search_all(
                    f"{target} {acquirer} acquisition deal".strip(),
                    sources=["reuters", "google_news"],
                )
                deal_info.update(news_coverage)
                deal_summary = _synthesize_deal(deal_info)
                result.findings["deal_info"] = deal_info
                result.findings["deal_summary"] = deal_summary
                source.result = "found" if any(
                    v and "ERROR" not in str(v) for v in deal_info.values()
                ) else "checked"

            else:
                # Generic: try to find IR page
                await self._ensure_browser()
                nav = BrowserNav(self.browser)
                ir_url = await nav.find_company_ir(company)
                source.result = "found" if ir_url else "checked"

        except Exception as e:
            logger.warning(f"Browser pipeline failed: {e}")
            source.result = "failed"
            raise
        finally:
            await pipeline.close()

        result.sources_checked.append(source)

    async def _run_financial_pipeline(self, pipeline: BrowserPipeline,
                                      company: str, year: str,
                                      country: str) -> Optional[object]:
        """Run the browser pipeline to find and extract financial data."""
        doc, fin = await pipeline.run_full_pipeline(company, year, country)
        if fin:
            # Store raw extracted data
            return fin
        return None

    def _verify(self, result: ResearchResult) -> VerificationResult:
        """Cross-verify findings."""
        errors = []
        warnings = []
        gaps = []
        sanity = {}

        verified = [s for s in result.sources_checked if s.result in ("found", "checked")]
        if len(verified) < 2:
            warnings.append("Fewer than 2 sources verified. IB standard: 2+ required.")

        if "financials" in result.findings:
            fin = result.findings["financials"]
            if fin.revenue and fin.market_cap:
                ratio = fin.market_cap / fin.revenue if fin.revenue > 0 else 0
                sanity["market_cap_to_revenue"] = ratio
                if ratio > 50:
                    warnings.append(f"Market cap/Revenue {ratio:.1f}x unusually high.")

        return VerificationResult(
            passed=len(errors) == 0,
            errors=errors,
            warnings=warnings,
            gaps=gaps,
            sanity_checks=sanity,
        )

    # --- Quick methods ---

    async def get_company_snapshot(self, ticker: str) -> dict:
        """Quick: company + financials + sector from SEC."""
        company, financials = self.sec.get_company_financials(ticker)
        sector = detect_sector(company.name)
        return {
            "company": company,
            "financials": financials,
            "sector": sector.name if sector else "unknown",
        }

    async def search_ma_deal(self, target: str, acquirer: str = "") -> dict:
        """Search for M&A deal (uses browser + SEC)."""
        await self._ensure_browser()
        news = NewsSearcher(self.browser)
        return await news.find_ma_deal(target, acquirer)

    async def search_google(self, query: str) -> str:
        """Simple Google search via browser."""
        await self._ensure_browser()
        nav = BrowserNav(self.browser)
        return await nav.google_search(query)

    async def ev_bridge_analyze(self, ticker: str) -> str:
        """
        Auto-build EV bridge combining yfinance (live price) + SEC EDGAR (balance sheet).
        For non-US companies, falls back to browser pipeline to extract annual report data.
        """
        # 1. Get live market data
        md = get_market_data(ticker)
        if not md.current_price:
            return f"Could not get market data for {ticker}"

        # 2. Try SEC EDGAR for balance sheet items (US companies)
        sec_fin = None       # Full Financials object from SEC
        sec_name = ""
        is_us = True
        try:
            intl_suffixes = ('.DE', '.PA', '.AS', '.L', '.SW', '.MI', '.MC',
                           '.IR', '.CO', '.T', '.HK', '.NS', '.BO')
            if any(ticker.endswith(s) for s in intl_suffixes):
                is_us = False
            else:
                company, fin = self.sec.get_company_financials(ticker)
                sec_name = company.name
                sec_fin = fin
        except Exception as e:
            logger.info(f"SEC EDGAR not available for {ticker}: {e}")
            is_us = False

        # 3. For non-US: browser pipeline to extract annual report data
        bp_extracted = None  # Full ExtractedFinancials object from browser
        bp_name = ""
        if not is_us:
            try:
                from src.research.browser_pipeline import BrowserPipeline
                pipeline = BrowserPipeline()
                company_name = md.company_name or ticker
                logger.info(f"Browser pipeline: extracting annual report for {company_name}")
                doc, extracted = await pipeline.run_full_pipeline(company_name, "2025", ticker=ticker)
                bp_extracted = extracted
                bp_name = extracted.company or company_name
                logger.info(
                    f"Browser pipeline extracted: debt={extracted.total_debt}, "
                    f"cash={extracted.cash}, revenue={extracted.revenue}, "
                    f"ebitda={extracted.adjusted_ebitda or extracted.reported_ebitda}, "
                    f"nci={extracted.minority_interest}, pension_pbo={extracted.pension_pbo}"
                )
                await pipeline.close()
            except Exception as e:
                logger.warning(f"Browser pipeline failed for {ticker}: {e}")

        # 3b. Auto-scale browser pipeline values to raw units
        # PDFs report in millions or thousands. Market cap from yfinance is in raw units.
        # Compare in same unit: market cap in millions vs extracted value.
        if bp_extracted and md.market_cap and md.market_cap > 1e9:
            mc = md.market_cap
            mc_millions = mc / 1_000_000
            bp = bp_extracted
            bs_items = [bp.total_debt, bp.cash, bp.goodwill, bp.short_term_investments]
            # If value is < 0.1% of market cap when both in millions, it's too small
            has_tiny = any(
                v and v > 0 and v < mc_millions * 0.001
                for v in bs_items if v
            )
            if has_tiny:
                # Values might be in thousands — scale to raw units (x1,000,000)
                scale = 1_000_000
                logger.info(f"Auto-scaling BP values x{scale:,} "
                           f"(values too small vs market cap {mc:,.0f}, "
                           f"mc_millions={mc_millions:,.0f})")
            else:
                # Values already in millions — scale to raw units (x1,000,000)
                scale = 1_000_000
                logger.info(f"Converting BP values from millions to raw units (x{scale:,})")

            for field in ['total_debt', 'cash', 'goodwill', 'short_term_investments',
                         'minority_interest', 'preferred_stock', 'equity_investments',
                         'financial_investments', 'assets_held_for_sale',
                         'discontinued_ops_assets', 'nol_dta',
                         'pension_pbo', 'pension_plan_assets',
                         'lease_liabilities_current', 'lease_liabilities_noncurrent',
                         'operating_lease_liabilities', 'finance_lease_liabilities',
                         'revenue', 'operating_income', 'net_income',
                         'adjusted_ebitda', 'reported_ebitda',
                         'depreciation_total', 'amortisation_total',
                         'rou_depreciation', 'lease_interest', 'short_term_rent',
                         'rou_assets', 'total_assets', 'total_equity']:
                val = getattr(bp, field, None)
                if val is not None and val > 0:
                    try:
                        setattr(bp, field, val * scale)
                    except Exception:
                        pass

        # 4. Build EV bridge input — merge SEC + browser pipeline + yfinance
        def _best(sec_val, bp_val, yf_val=None):
            """Prefer SEC (XBRL) > browser pipeline (PDF extract) > yfinance (estimate)."""
            if sec_val is not None:
                return sec_val
            if bp_val is not None:
                return bp_val
            return yf_val

        sec = sec_fin
        bp = bp_extracted

        total_debt = _best(
            sec.total_debt if sec else None,
            bp.total_debt if bp else None
        )
        cash = _best(
            sec.cash_and_equivalents if sec else None,
            bp.cash if bp else None,
            md.market_cap - md.enterprise_value if md.enterprise_value and md.market_cap else None
        )
        revenue = _best(
            sec.revenue if sec else None,
            bp.revenue if bp else None,
            md.revenue
        )
        short_term_inv = _best(
            sec.short_term_investments if sec else None,
            bp.short_term_investments if bp else None
        )
        goodwill = _best(
            sec.goodwill if sec else None,
            bp.goodwill if bp else None
        )
        minority_interest = _best(
            sec.minority_interest if sec else None,
            bp.minority_interest if bp else None
        )
        preferred_stock = _best(
            sec.preferred_stock if sec else None,
            bp.preferred_stock if bp else None
        )
        equity_inv = _best(
            None,  # SEC Financials doesn't have equity_investments yet
            bp.equity_investments if bp else None
        )
        financial_inv = _best(
            None,
            bp.financial_investments if bp else None
        )
        assets_hfs = _best(
            None,
            bp.assets_held_for_sale if bp else None
        )
        disc_ops = _best(
            None,
            bp.discontinued_ops_assets if bp else None
        )
        nol_dta = _best(
            None,
            bp.nol_dta if bp else None
        )
        pension_pbo = _best(
            sec.pension_pbo if sec else None,
            bp.pension_pbo if bp else None
        )
        pension_plan_assets = _best(
            sec.pension_plan_assets if sec else None,
            bp.pension_plan_assets if bp else None
        )
        operating_leases = _best(
            (sec.lease_liabilities_noncurrent if sec else None),
            (bp.operating_lease_liabilities if bp else None) or (
                # Fallback: combined lease liabilities as operating leases
                ((bp.lease_liabilities_current or 0) + (bp.lease_liabilities_noncurrent or 0))
                if bp and (bp.lease_liabilities_current or bp.lease_liabilities_noncurrent)
                else None
            )
        )
        finance_leases = _best(
            (sec.lease_liabilities_current if sec else None),  # Approx: current portion as finance
            bp.finance_lease_liabilities if bp else None
        )

        # Underfunded pension (R-015: only from notes, not BS tag)
        underfunded_pension = None
        if pension_pbo is not None and pension_plan_assets is not None:
            net = pension_pbo - pension_plan_assets
            if net > 0:
                underfunded_pension = net
            # If overfunded, leave as None (excluded per R-015)

        # EBITDA hierarchy: browser (adjusted > reported) > yfinance
        if bp:
            ebitda = bp.adjusted_ebitda or bp.reported_ebitda
            if not ebitda and bp.operating_income:
                da = bp.depreciation_total or 0
                ebitda = bp.operating_income + da
        else:
            ebitda = None
        if not ebitda:
            ebitda = md.ebitda

        # Source labels
        debt_src = ('SEC 10-Q/10-K Balance Sheet' if (sec and sec.total_debt)
                    else 'Annual report via browser pipeline' if (bp and bp.total_debt)
                    else 'Not available')
        cash_src = ('SEC 10-Q/10-K Balance Sheet' if (sec and sec.cash_and_equivalents)
                    else 'Annual report via browser pipeline' if (bp and bp.cash)
                    else 'yfinance estimate')
        pension_src = ('Pension footnote (R-015)' if underfunded_pension
                       else 'Pension footnote — fully funded or no DB plan')

        # Build per-field PDF URLs for Excel audit hyperlinks
        _fs = bp_extracted.field_sources if bp_extracted else {}
        field_urls = {
            'total_debt':            _fs.get('total_debt', ''),
            'finance_leases':        _fs.get('finance_lease_liabilities', ''),
            'operating_leases':      _fs.get('operating_lease_liabilities', ''),
            'underfunded_pension':   _fs.get('pension_pbo', ''),
            'minority_interest':     _fs.get('minority_interest', ''),
            'preferred_stock':       _fs.get('preferred_stock', ''),
            'cash':                  _fs.get('cash', ''),
            'short_term_investments':_fs.get('short_term_investments', ''),
            'equity_investments':    _fs.get('equity_investments', ''),
            'financial_investments': _fs.get('financial_investments', ''),
            'assets_held_for_sale':  _fs.get('assets_held_for_sale', ''),
            'discontinued_ops_assets':_fs.get('discontinued_ops_assets', ''),
            'nol_dta':               _fs.get('nol_dta', ''),
            'ltm_revenue':           _fs.get('revenue', ''),
            'ltm_ebitda':            _fs.get('adjusted_ebitda', '') or _fs.get('reported_ebitda', ''),
        }

        ev = EVBridgeInput(
            company=md.company_name or bp_name or sec_name or ticker,
            period=f"Live as of {md.price_date}",
            currency=md.currency,

            share_price=md.current_price,
            shares_outstanding=md.shares_outstanding,
            market_cap=md.market_cap,

            total_debt=total_debt,
            finance_leases=finance_leases,
            operating_leases=operating_leases,
            underfunded_pension=underfunded_pension,
            minority_interest=minority_interest,
            preferred_stock=preferred_stock,

            cash=cash,
            short_term_investments=short_term_inv,
            equity_investments=equity_inv,
            financial_investments=financial_inv,
            assets_held_for_sale=assets_hfs,
            discontinued_ops_assets=disc_ops,
            nol_dta=nol_dta,

            goodwill=goodwill,
            pension_bs_tag=(sec.pension_liability if sec else None),

            ltm_revenue=revenue,
            ltm_ebitda=ebitda,

            notes_ref={
                'share_price': f'{md.exchange} via yfinance ({md.price_date})',
                'shares': 'yfinance shares outstanding (F-001: prefer latest filing weighted avg)',
                'total_debt': debt_src,
                'cash': cash_src,
                'pension': pension_src,
                'leases': ('ASC 842 / IFRS 16 note (R-016)' if operating_leases or finance_leases
                          else 'Not disclosed'),
            },
            field_urls=field_urls,
        )

        # Write Excel + PPT
        self._last_xl_path = None
        try:
            writer = ResearchExcelWriter()
            xl_path = writer.write_ev_bridge(ev)
            logger.info(f"EV bridge Excel: {xl_path}")
            self._last_xl_path = xl_path
        except Exception as e:
            logger.warning(f"Excel write failed: {e}")
        try:
            pptx_writer = ResearchPPTXWriter()
            pptx_writer.write_ev_bridge_deck(ev)
        except Exception as e:
            logger.warning(f"PPT deck write failed: {e}")

        return format_ev_bridge(ev)

    async def close(self):
        if self._browser:
            await self._browser.close()
            self._browser = None

    async def ifrs_analyze(self, company: str, year: str = "2025",
                           country: str = "", ticker: str = "") -> str:
        """
        Full IFRS 16 analysis. Works for BOTH US and non-US companies.
        US companies (ticker): direct HTTP to 10-K, ASC 842 extraction.
        Non-US companies: browser pipeline to find annual report.
        """
        from kb.ifrs import IFRSAdjustmentInput, convert_ifrs_to_us_gaap, format_bridge

        # --- PATH A: US Company (ticker provided, not international suffix) ---
        intl_suffixes = ('.NS', '.BO', '.DE', '.PA', '.AS', '.L', '.SW',
                        '.MI', '.MC', '.IR', '.CO', '.T', '.HK',
                        '.ST', '.OL', '.HE', '.CPH')
        is_intl = ticker and any(ticker.endswith(s) for s in intl_suffixes)

        if ticker and not is_intl:
            from src.research.us_gaap_leases import extract_asc842_from_10k
            from src.research.market_data import get_market_data

            logger.info(f"IFRS analysis (US 10-K): {ticker}")
            lease_data = extract_asc842_from_10k(ticker)

            if not lease_data.estimated_rou_depreciation or not lease_data.estimated_lease_interest:
                return (f"Could not extract ASC 842 lease data for {ticker}. "
                        f"Confidence: {lease_data.extraction_confidence}.")

            # Get financials from SEC EDGAR
            sec_company, sec_fin = self.sec.get_company_financials(ticker)
            md = get_market_data(ticker)

            revenue = sec_fin.revenue or md.revenue
            ebit = sec_fin.ebit or 0
            ebitda = md.ebitda if md.ebitda and md.ebitda > (ebit or 0) else ebit

            inputs = IFRSAdjustmentInput(
                rou_depreciation=lease_data.estimated_rou_depreciation or 0,
                lease_interest=lease_data.estimated_lease_interest or 0,
                short_term_rent=lease_data.short_term_lease_cost or 0,
                reported_ebit=ebit,
                reported_ebitda=ebitda,
                reported_ebita=ebit,
                standard_depreciation=lease_data.estimated_rou_depreciation or 0,
                accounting_standard="US GAAP",
            )

            from kb.ifrs import convert_us_gaap_to_ifrs
            out = convert_us_gaap_to_ifrs(inputs, revenue=revenue or 0)

            notes = {
                'rou_depr': (f'Est: Lease cost ({lease_data.operating_lease_cost/1e9:.1f}B) '
                            f'- Liab ({lease_data.operating_lease_liability/1e9:.1f}B) '
                            f'x Rate ({lease_data.weighted_avg_discount_rate}%)'),
                'lease_int': f'Est: Liab x Disc rate = {lease_data.estimated_lease_interest/1e6:.0f}M',
                'short_term': f'ASC 842 Note, 10-K filing',
            }

            bridge_text = format_bridge(inputs, out, revenue=revenue or 0,
                                company=sec_company.name or company,
                                period=f"FY{year}",
                                notes_ref=notes)

            # Write Excel + PPT
            try:
                writer = ResearchExcelWriter()
                writer.write_ifrs_bridge(inputs, out, sec_company.name or company,
                                        f"FY{year}", revenue=revenue or 0, notes=notes)
            except Exception as e:
                logger.warning(f"Excel write failed: {e}")
            try:
                pptx_writer = ResearchPPTXWriter()
                pptx_writer.write_ifrs_bridge_deck(
                    inputs, out, sec_company.name or company,
                    f"FY{year}", revenue=revenue or 0,
                )
            except Exception as e:
                logger.warning(f"PPT deck write failed: {e}")

            return bridge_text

        # --- PATH B: Non-US Company (browser pipeline) ---
        pipeline = BrowserPipeline()
        try:
            logger.info(f"IFRS analysis (browser): {company} {year}")
            doc, fin = await pipeline.run_full_pipeline(company, year, country, ticker=ticker)

            # Overlay extraction cache for P&L fields the browser pipeline may miss
            if ticker:
                try:
                    import json as _json, os as _os
                    _cache_path = _os.path.join("extraction_cache", f"{ticker.replace('.', '_')}.json")
                    if _os.path.exists(_cache_path):
                        _c = _json.load(open(_cache_path))
                        _is = _c.get("income_statement", {})
                        _n  = len(_is.get("revenue", []))
                        def _last(key):
                            vals = _is.get(key, [])
                            return next((v for v in reversed(vals) if v is not None), None)
                        if not fin.operating_income: fin.operating_income = _last("ebit")
                        if not fin.revenue:          fin.revenue          = _last("revenue")
                        if not fin.depreciation_total: fin.depreciation_total = _last("da")
                        if not fin.ebita:            fin.ebita            = _last("ebita")
                        # Always prefer cache for IFRS lease items — regex picks wrong year column
                        _li = _last("lease_interest");   fin.lease_interest   = _li if _li is not None else fin.lease_interest
                        _rd = _last("rou_depreciation"); fin.rou_depreciation = _rd if _rd is not None else fin.rou_depreciation
                        if not fin.reported_ebitda:
                            _ebit = _last("ebit"); _da = _last("da")
                            if _ebit and _da: fin.reported_ebitda = _ebit + _da
                        logger.info(f"Cache overlay: EBIT={fin.operating_income}, DA={fin.depreciation_total}, "
                                    f"ROU={fin.rou_depreciation}, LeaseInt={fin.lease_interest}")
                except Exception as _e:
                    logger.warning(f"Cache overlay failed: {_e}")

            if not fin.rou_depreciation and not fin.lease_interest:
                return (f"Could not extract lease data for {company} {year}. "
                        f"ROU depr: {fin.rou_depreciation}, Lease int: {fin.lease_interest}")

            # EBITDA hierarchy: Adjusted > Reported > Computed
            da_total = fin.depreciation_total or 0
            if fin.adjusted_ebitda:
                ebitda = fin.adjusted_ebitda
                ebitda_source = "Adjusted EBITDA (company-reported, one-off items removed)"
            elif fin.reported_ebitda:
                ebitda = fin.reported_ebitda
                ebitda_source = "Reported EBITDA (from annual report)"
            else:
                ebit_v = fin.operating_income or 0
                ebitda = ebit_v + da_total
                ebitda_source = f"Computed: EBIT ({ebit_v:,.0f}) + D&A ({da_total:,.0f})"

            ebit_v    = fin.operating_income or 0
            rou_depr  = fin.rou_depreciation or 0
            lease_int = fin.lease_interest   or 0
            short_r   = fin.short_term_rent  or 0
            ebita     = fin.ebita or ebit_v
            inputs = IFRSAdjustmentInput(
                rou_depreciation=rou_depr,
                lease_interest=lease_int,
                short_term_rent=short_r,
                reported_ebit=ebit_v,
                reported_ebitda=ebitda,
                reported_ebita=ebita,
                standard_depreciation=da_total,
                accounting_standard=fin.accounting_standard or "IFRS",
            )

            out = convert_ifrs_to_us_gaap(inputs, revenue=fin.revenue or 0)

            notes = {
                'ebit_src': f'Annual Report — Operating Result = {ebit_v:,.0f}',
                'da_src': f'Annual Report — Depreciation & Amortisation = {da_total:,.0f}',
                'ebitda_src': ebitda_source,
                'rou_depr': f'Annual Report — ROU Depreciation = {rou_depr:,.0f}',
                'lease_int': f'Annual Report — Interest on Lease Liabilities = {lease_int:,.0f}',
                'short_term': f'Annual Report — Short-term rent = {short_r:,.0f} (excluded)',
                'revenue_src': f'Annual Report — Revenue = {fin.revenue or 0:,.0f}',
            }

            bridge_text = format_bridge(inputs, out, revenue=fin.revenue or 0,
                                company=company, period=f"FY{year}",
                                notes_ref=notes)

            # Write Excel + PPT
            try:
                writer = ResearchExcelWriter()
                writer.write_ifrs_bridge(inputs, out, company, f"FY{year}",
                                        revenue=fin.revenue or 0, notes=notes,
                                        pdf_url=doc.pdf_url or "")
            except Exception as e:
                logger.warning(f"Excel write failed: {e}")
            try:
                pptx_writer = ResearchPPTXWriter()
                pptx_writer.write_ifrs_bridge_deck(
                    inputs, out, company, f"FY{year}", revenue=fin.revenue or 0,
                )
            except Exception as e:
                logger.warning(f"PPT deck write failed: {e}")

            return bridge_text

        finally:
            await pipeline.close()


# --- Sync CLI wrapper ---

def research_sync(user_query: str, ticker: str = "", company: str = ""):
    """Synchronous CLI entry point."""
    async def _run():
        agent = ResearchAgent()
        result = await agent.research(user_query, company_name=company, ticker=ticker)
        await agent.close()
        return result

    return asyncio.run(_run())


def ifrs_analyze_sync(company: str, year: str = "2025", country: str = "",
                      ticker: str = ""):
    """Synchronous IFRS analysis entry point."""
    async def _run():
        agent = ResearchAgent()
        bridge = await agent.ifrs_analyze(company, year, country, ticker=ticker)
        await agent.close()
        return bridge

    return asyncio.run(_run())


def ev_bridge_sync(ticker: str) -> str:
    """Synchronous EV bridge entry point. Prints Excel path if written."""
    async def _run():
        agent = ResearchAgent()
        bridge = await agent.ev_bridge_analyze(ticker)
        xl = getattr(agent, "_last_xl_path", None)
        await agent.close()
        return bridge, xl

    result, xl_path = asyncio.run(_run())
    if xl_path:
        print(f"\nExcel: {xl_path}")
    return result


def print_result(result: ResearchResult):
    """Pretty-print research result."""
    print(f"\n{'='*60}")
    print(f"RESEARCH: {result.query.user_query}")
    print(f"Type: {result.query.query_type.value}")
    print(f"Status: {result.status}")
    print(f"Sources: {len(result.sources_checked)} checked, {len(result.sources_failed)} failed")
    print(f"{'='*60}")

    if result.findings:
        # Deal summary first if present
        if "deal_summary" in result.findings:
            print("\nDEAL SUMMARY:")
            for k, v in result.findings["deal_summary"].items():
                print(f"  {k}: {v}")

        print("\nFINDINGS:")
        for key, value in result.findings.items():
            if key == "deal_summary":
                continue  # already printed above
            if hasattr(value, '__dict__'):
                print(f"  {key}:")
                for k, v in value.__dict__.items():
                    if v is not None:
                        print(f"    {k}: {v}")
            elif isinstance(value, dict):
                print(f"  {key}:")
                for k, v in value.items():
                    v_str = str(v)
                    # For deal_info, show source name + char count rather than raw article
                    if key == "deal_info" and len(v_str) > 200:
                        v_str = f"[{len(v_str)} chars retrieved]"
                    elif len(v_str) > 300:
                        v_str = v_str[:300] + "..."
                    print(f"    {k}: {v_str}")
            else:
                v_str = str(value)[:300] + "..." if len(str(value)) > 300 else str(value)
                print(f"  {key}: {v_str}")

    if result.verification:
        v = result.verification
        print(f"\nVERIFICATION: {'PASSED' if v.passed else 'ISSUES'}")
        for w in v.warnings:
            print(f"  ⚠ {w}")
        for e in v.errors:
            print(f"  ❌ {e}")


if __name__ == "__main__":
    import sys
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    if len(sys.argv) > 1:
        ticker = ""
        company = ""
        mode = "research"
        year = "2025"

        args = sys.argv[1:]
        i = 0
        while i < len(args):
            if args[i] == "--ticker" and i + 1 < len(args):
                ticker = args[i + 1]; i += 2
            elif args[i] == "--company" and i + 1 < len(args):
                company = args[i + 1]; i += 2
            elif args[i] == "--year" and i + 1 < len(args):
                year = args[i + 1]; i += 2
            elif args[i] == "--ifrs":
                mode = "ifrs"; i += 1
            elif args[i] == "--ev-bridge":
                mode = "ev_bridge"; i += 1
            else:
                i += 1

        if mode == "ifrs":
            if ticker:
                print(ifrs_analyze_sync(company or ticker, year, ticker=ticker))
            elif company:
                print(ifrs_analyze_sync(company, year))
        elif mode == "ev_bridge" and ticker:
            print(ev_bridge_sync(ticker))
        else:
            # Default research mode
            query = " ".join(a for a in args if not a.startswith("--"))
            result = research_sync(query, ticker=ticker, company=company)
            print_result(result)
    else:
        print("Usage:")
        print("  Research:  python -m src.research.agent <query> --ticker TICKER")
        print("  IFRS:      python -m src.research.agent --ifrs --company 'Company' --year 2025")
        print("  EV Bridge: python -m src.research.agent --ev-bridge --ticker AAPL")
        print("  Examples:")
        print("    python -m src.research.agent --ev-bridge --ticker AAPL")
        print("    python -m src.research.agent --ifrs --company 'Royal BAM Group' --year 2025")
