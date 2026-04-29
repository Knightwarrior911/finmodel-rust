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
from kb.sectors import detect_sector
from kb.ev_bridge import EVBridgeInput, format_ev_bridge

logger = logging.getLogger(__name__)


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
                # M&A deal search
                news = NewsSearcher(self.browser)
                deal_info = await news.find_ma_deal(company)
                if deal_info:
                    result.findings["deal_info"] = deal_info
                source.result = "found" if deal_info else "checked"

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
        Works for any ticker with one command.
        """
        # 1. Get live market data
        md = get_market_data(ticker)
        if not md.current_price:
            return f"Could not get market data for {ticker}"

        # 2. Try SEC EDGAR for balance sheet items
        bs_debt = None
        bs_cash = None
        bs_goodwill = None
        bs_revenue = None
        sec_name = ""
        try:
            if ticker.endswith('.NS') or ticker.endswith('.BO'):
                pass  # Indian stocks — skip SEC
            else:
                company, fin = self.sec.get_company_financials(ticker)
                sec_name = company.name
                bs_debt = fin.total_debt
                bs_cash = fin.cash_and_equivalents
                bs_goodwill = fin.goodwill
                bs_revenue = fin.revenue
        except Exception as e:
            logger.info(f"SEC EDGAR not available for {ticker}: {e}")

        # 3. Build EV bridge input
        ev = EVBridgeInput(
            company=md.company_name or sec_name or ticker,
            period=f"Live as of {md.price_date}",
            currency=md.currency,

            share_price=md.current_price,
            shares_outstanding=md.shares_outstanding,
            market_cap=md.market_cap,

            # From SEC EDGAR balance sheet (if available)
            total_debt=bs_debt,
            operating_leases=None,  # Would need 10-Q/10-K note extraction
            underfunded_pension=None,

            cash=bs_cash or (md.market_cap - md.enterprise_value if md.enterprise_value and md.market_cap else None),
            short_term_investments=None,

            goodwill=bs_goodwill,

            ltm_revenue=bs_revenue or md.revenue,
            ltm_ebitda=md.ebitda,

            notes_ref={
                'share_price': f'{md.exchange} via yfinance ({md.price_date})',
                'shares': 'yfinance shares outstanding (F-001: prefer latest filing weighted avg)',
                'total_debt': 'SEC 10-Q/10-K Balance Sheet' if bs_debt else 'Not available',
                'cash': 'SEC 10-Q/10-K Balance Sheet' if bs_cash else 'yfinance estimate',
            }
        )

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
                        '.MI', '.MC', '.IR', '.CO', '.T', '.HK')
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

            return format_bridge(inputs, out, revenue=revenue or 0,
                                company=sec_company.name or company,
                                period=f"FY{year}",
                                notes_ref=notes)

        # --- PATH B: Non-US Company (browser pipeline) ---
        pipeline = BrowserPipeline()
        try:
            logger.info(f"IFRS analysis (browser): {company} {year}")
            doc, fin = await pipeline.run_full_pipeline(company, year, country)

            if not fin.rou_depreciation or not fin.lease_interest:
                return (f"Could not extract lease data for {company} {year}. "
                        f"ROU depr: {fin.rou_depreciation}, Lease int: {fin.lease_interest}")

            if fin.operating_income and fin.depreciation_total:
                ebitda = fin.operating_income + fin.depreciation_total
            else:
                ebitda = fin.operating_income or 0

            inputs = IFRSAdjustmentInput(
                rou_depreciation=fin.rou_depreciation or 0,
                lease_interest=fin.lease_interest or 0,
                short_term_rent=fin.short_term_rent or 0,
                reported_ebit=fin.operating_income or 0,
                reported_ebitda=ebitda,
                reported_ebita=fin.operating_income or 0,
                accounting_standard=fin.accounting_standard or "IFRS",
            )

            out = convert_ifrs_to_us_gaap(inputs, revenue=fin.revenue or 0)

            notes = {
                'rou_depr': f'Annual Report Note (p.{doc.total_pages} pages)',
                'lease_int': f'Annual Report Note - Finance expense',
                'short_term': f'Annual Report Note - Short-term lease',
            }

            return format_bridge(inputs, out, revenue=fin.revenue or 0,
                                company=company, period=f"FY{year}",
                                notes_ref=notes)

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


def ev_bridge_sync(ticker: str):
    """Synchronous EV bridge entry point."""
    async def _run():
        agent = ResearchAgent()
        bridge = await agent.ev_bridge_analyze(ticker)
        await agent.close()
        return bridge

    return asyncio.run(_run())


def print_result(result: ResearchResult):
    """Pretty-print research result."""
    print(f"\n{'='*60}")
    print(f"RESEARCH: {result.query.user_query}")
    print(f"Type: {result.query.query_type.value}")
    print(f"Status: {result.status}")
    print(f"Sources: {len(result.sources_checked)} checked, {len(result.sources_failed)} failed")
    print(f"{'='*60}")

    if result.findings:
        print("\nFINDINGS:")
        for key, value in result.findings.items():
            if hasattr(value, '__dict__'):
                print(f"  {key}:")
                for k, v in value.__dict__.items():
                    if v is not None:
                        print(f"    {k}: {v}")
            else:
                print(f"  {key}: {value}")

    if result.verification:
        v = result.verification
        print(f"\nVERIFICATION: {'PASSED' if v.passed else 'ISSUES'}")
        for w in v.warnings:
            print(f"  ⚠ {w}")
        for e in v.errors:
            print(f"  ❌ {e}")


if __name__ == "__main__":
    import sys
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
