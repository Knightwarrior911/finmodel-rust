"""
Virtual Financial Analyst — Top-Level LLM Orchestrator.

Receives natural-language queries, plans steps using Claude, dispatches
to sub-systems as tools, synthesizes results. Every current capability —
research, financial modeling, DCF, comps, EV bridge — is a callable tool.
New features added to the project automatically become available by
registering a tool here.

Entry points:
  run_sync(query, ticker, company)   — blocking, for CLI use
  run(query, ticker, company)        — async, for embedding
"""

import asyncio
import json
import logging
import os
from typing import Optional

import anthropic

# Load .env from project root if present (convenient for local dev)
try:
    from dotenv import load_dotenv
    load_dotenv(os.path.join(os.path.dirname(__file__), "..", ".env"), override=False)
except ImportError:
    pass

logger = logging.getLogger(__name__)

_MODEL = "claude-opus-4-7"

_SYSTEM = """You are a senior investment banking analyst with access to financial data tools.

When given a query, reason about the best approach, use the available tools to gather data, and deliver a precise, source-backed answer.

Tool selection rules:
- US companies → try `search_sec_edgar` first (fast, 1-2s, authoritative XBRL data)
- Non-US companies or missing EDGAR data → use `run_browser_pipeline` (slow, 30-120s, use sparingly)
- News, M&A terms, regulatory approvals → `search_web` then `fetch_page` for the top hit
- Full 3-statement model → `run_financial_model`
- DCF or price target → `run_dcf`
- EV calculation → `run_ev_bridge`
- Peer multiples table → `run_public_comps`
- Run independent tools in the same turn — the harness executes them in parallel.
- Only invoke `run_browser_pipeline` when faster paths fail or for non-US annual report extraction.
"""

_TOOLS = [
    {
        "name": "search_sec_edgar",
        "description": (
            "Fetch financial data directly from SEC EDGAR XBRL API. Fast (1-2s). "
            "Use first for any US public company. Returns revenue, EBIT, net income, "
            "total assets, debt, cash, shares, and filing metadata."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "ticker": {"type": "string", "description": "US stock ticker (e.g. AAPL, MSFT)"},
                "form_type": {
                    "type": "string",
                    "enum": ["10-K", "10-Q", "8-K"],
                    "description": "Filing type. Omit for latest annual data.",
                },
            },
            "required": ["ticker"],
            "additionalProperties": False,
        },
    },
    {
        "name": "search_web",
        "description": (
            "Search DuckDuckGo and return the top result URLs + snippets. "
            "No browser — pure HTTP (instant). Use for news, M&A deals, "
            "non-US company info, regulatory approvals, ownership data."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query. Use operators: site:, filetype:, after: when useful.",
                },
            },
            "required": ["query"],
            "additionalProperties": False,
        },
    },
    {
        "name": "fetch_page",
        "description": (
            "Fetch a specific URL via direct HTTP and return the page text. "
            "Fast, no browser. Use after search_web to read a specific article, "
            "press release, IR page, or regulatory filing."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "url": {"type": "string", "description": "Full URL to fetch"},
            },
            "required": ["url"],
            "additionalProperties": False,
        },
    },
    {
        "name": "run_browser_pipeline",
        "description": (
            "Full browser pipeline: finds the company's annual report PDF, downloads it, "
            "extracts 40+ financial fields (P&L, balance sheet, IFRS 16 leases, EV bridge items, "
            "pension, D&A). SLOW (30-120s). Use only for non-US companies or when EDGAR has no data."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "company": {"type": "string", "description": "Company name"},
                "year": {"type": "string", "description": "Fiscal year (e.g. '2024')"},
                "ticker": {"type": "string", "description": "Ticker symbol if known"},
                "country": {"type": "string", "description": "Country of listing if known"},
            },
            "required": ["company", "year"],
            "additionalProperties": False,
        },
    },
    {
        "name": "run_financial_model",
        "description": (
            "Build a full 3-statement financial model (IS, BS, CF) and export to Excel. "
            "Takes 30-90s. Use when the user explicitly wants a model or Excel file."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "ticker": {"type": "string", "description": "Stock ticker"},
                "periods_historical": {
                    "type": "integer",
                    "description": "Historical periods. Default 3.",
                    "default": 3,
                },
                "periods_projected": {
                    "type": "integer",
                    "description": "Projected periods. Default 5.",
                    "default": 5,
                },
                "output_path": {
                    "type": "string",
                    "description": "Optional path for the output .xlsx file.",
                },
            },
            "required": ["ticker"],
            "additionalProperties": False,
        },
    },
    {
        "name": "run_dcf",
        "description": (
            "Run a DCF valuation (WACC, terminal value, implied EV and equity value) "
            "for a US-listed company. Returns key valuation metrics."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "ticker": {"type": "string", "description": "Stock ticker"},
            },
            "required": ["ticker"],
            "additionalProperties": False,
        },
    },
    {
        "name": "run_ev_bridge",
        "description": (
            "Compute Enterprise Value bridge: Market Cap → EV via net debt, minority interest, "
            "pension obligations, lease liabilities. Returns all EV bridge components."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "ticker": {"type": "string", "description": "Stock ticker"},
            },
            "required": ["ticker"],
            "additionalProperties": False,
        },
    },
    {
        "name": "run_public_comps",
        "description": (
            "Build a trading comps table for a company and its sector peers. "
            "Returns EV/EBITDA, P/E, EV/Revenue multiples for target + peers."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "ticker": {"type": "string", "description": "Target company ticker"},
                "peers": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Optional peer tickers. If omitted, uses curated peer list.",
                },
            },
            "required": ["ticker"],
            "additionalProperties": False,
        },
    },
    {
        "name": "build_deck",
        "description": (
            "Build an IB-style PowerPoint deck from structured slide specs. "
            "Each slide spec is a dict with 'type' (cover|section|comparison|"
            "scorecard|quote_wall|timeline|process|strategy|bar_chart|"
            "football_field|line_chart|waterfall|stacked_bar|pie|pros_cons|"
            "quad_page|org_chart|tombstone_page|team_page|table_of_contents) "
            "and type-specific keys. Returns saved deck path. "
            "Optional brand_pdf to clone visual style from a sample firm deck. "
            "Pass 'markdown' (multi-doc YAML stream) instead of 'slides' to "
            "build from a flat text spec."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "filename": {
                    "type": "string",
                    "description": "Output filename without extension"
                },
                "firm": {"type": "string", "description": "Firm name in footer"},
                "project": {"type": "string", "description": "Project name"},
                "confidentiality": {
                    "type": "string",
                    "description": "CONFIDENTIAL / DRAFT / empty to suppress"
                },
                "brand_pdf": {
                    "type": "string",
                    "description": "Optional path/URL to a sample firm deck PDF "
                                   "(BrandProfile.from_pdf), or 'pitchpres' for "
                                   "the Citi A4 PitchPres preset."
                },
                "headline_bold": {
                    "type": "boolean",
                    "description": "Bold action titles (default true)"
                },
                "slides": {
                    "type": "array",
                    "items": {"type": "object"},
                    "description": (
                        "List of slide specs. Each MUST have 'type' key. "
                        "Mutually exclusive with 'markdown'. Examples: "
                        "{type:'cover', title:'...', subtitle:'...', date:'...'}; "
                        "{type:'section', section_num:'I', title:'Setup'}; "
                        "{type:'comparison', action_title:'...', entities:[...], "
                        "metrics:[...], values:[[...]], target_label:'...', source:'...'}; "
                        "{type:'line_chart', action_title:'...', x_labels:[...], "
                        "series:[{label,values}], target_series:'...', y_format:'...', source:'...'}; "
                        "{type:'waterfall', action_title:'...', "
                        "segments:[{label,value,kind}], source:'...'}; "
                        "{type:'stacked_bar', action_title:'...', categories:[...], "
                        "series:[{label,values,color?}], target_category:'...', source:'...'}; "
                        "{type:'pie', action_title:'...', "
                        "slices:[{label,value,color?}], target_label:'...', source:'...'}"
                    ),
                },
                "markdown": {
                    "type": "string",
                    "description": (
                        "Multi-doc YAML stream (separated by '---'). Each doc is "
                        "one slide spec with 'type' and type-specific keys. "
                        "Mutually exclusive with 'slides'."
                    ),
                },
            },
            "required": ["filename"],
            "additionalProperties": False,
        },
    },
]


# ---------------------------------------------------------------------------
# Tool implementations
# ---------------------------------------------------------------------------

def _tool_search_sec_edgar(ticker: str, form_type: str = "10-K") -> str:
    try:
        from src.research.sec_edgar import SECEdgarClient
        client = SECEdgarClient()
        company, financials = client.get_company_financials(ticker)
        result = {
            "company": company.name,
            "ticker": ticker,
            "currency": getattr(financials, "currency", "USD"),
            "revenue": getattr(financials, "revenue", None),
            "operating_income": getattr(financials, "operating_income", None),
            "net_income": getattr(financials, "net_income", None),
            "total_assets": getattr(financials, "total_assets", None),
            "total_debt": getattr(financials, "total_debt", None),
            "cash": getattr(financials, "cash", None),
            "shares_outstanding": getattr(financials, "shares_outstanding", None),
        }
        return json.dumps({k: v for k, v in result.items() if v is not None}, indent=2)
    except Exception as e:
        return f"SEC EDGAR error for {ticker}: {e}"


def _tool_search_web(query: str) -> str:
    try:
        from src.browser.navigation import BrowserNav
        from src.browser.session import BrowserSession
        nav = BrowserNav(BrowserSession())
        urls = nav.search_urls(query)
        if not urls:
            return "No results found."
        lines = [f"{i+1}. {u}" for i, u in enumerate(urls[:8])]
        return "\n".join(lines)
    except Exception as e:
        return f"Web search error: {e}"


def _tool_fetch_page(url: str) -> str:
    try:
        import requests
        from bs4 import BeautifulSoup
        headers = {
            "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
            "Accept": "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
        }
        resp = requests.get(url, headers=headers, timeout=15)
        text = BeautifulSoup(resp.text, "lxml").get_text(separator=" ", strip=True)
        return text[:8000]
    except Exception as e:
        return f"Fetch error for {url[:80]}: {e}"


async def _tool_run_browser_pipeline(
    company: str, year: str, ticker: str = "", country: str = ""
) -> str:
    try:
        from src.research.browser_pipeline import BrowserPipeline
        pipeline = BrowserPipeline()
        try:
            doc, fin = await pipeline.run_full_pipeline(
                company=company, year=year, country=country, ticker=ticker
            )
            result = {k: v for k, v in fin.__dict__.items()
                      if v is not None and not k.startswith("_")
                      and k not in ("source_sections", "extraction_confidence",
                                    "raw_snippets", "field_sources")}
            result["pdf_url"] = doc.pdf_url
            result["source"] = doc.source
            return json.dumps(result, indent=2, default=str)
        finally:
            await pipeline.close()
    except Exception as e:
        return f"Browser pipeline error for {company}: {e}"


def _tool_run_financial_model(
    ticker: str,
    periods_historical: int = 3,
    periods_projected: int = 5,
    output_path: Optional[str] = None,
) -> str:
    try:
        import subprocess, sys
        out = output_path or f"{ticker.replace('.', '_')}_model.xlsx"
        cmd = [
            sys.executable, "-m", "src.cli",
            "--ticker", ticker,
            "--periods-historical", str(periods_historical),
            "--periods-projected", str(periods_projected),
            "--output", out,
            "--direct",
        ]
        result = subprocess.run(
            cmd,
            capture_output=True, text=True,
            cwd="C:/Users/vinit/Documents/financial_model",
            timeout=180,
        )
        if result.returncode == 0:
            return f"Financial model built: {out}\n{result.stdout[-2000:]}"
        return f"Model build failed:\n{result.stderr[-2000:]}"
    except Exception as e:
        return f"Financial model error: {e}"


def _tool_run_dcf(ticker: str) -> str:
    try:
        from src.research.sec_edgar import SECEdgarClient
        from src.dcf import compute_dcf
        from src.wacc import compute_wacc
        client = SECEdgarClient()
        company, financials = client.get_company_financials(ticker)
        wacc_result = compute_wacc(ticker, financials)
        dcf_result = compute_dcf(financials, wacc_result)
        return json.dumps({
            "company": company.name,
            "wacc": getattr(wacc_result, "wacc", None),
            "terminal_value": getattr(dcf_result, "terminal_value", None),
            "enterprise_value": getattr(dcf_result, "enterprise_value", None),
            "equity_value": getattr(dcf_result, "equity_value", None),
            "implied_price": getattr(dcf_result, "implied_price", None),
        }, indent=2, default=str)
    except Exception as e:
        return f"DCF error for {ticker}: {e}"


def _tool_run_ev_bridge(ticker: str) -> str:
    try:
        from src.research.agent import ev_bridge_sync
        return ev_bridge_sync(ticker)
    except Exception as e:
        return f"EV bridge error for {ticker}: {e}"


def _tool_run_public_comps(ticker: str, peers: Optional[list] = None) -> str:
    try:
        from src.public_comps import build_public_comps
        result = build_public_comps(ticker, peer_tickers=peers)
        return str(result)
    except Exception as e:
        return f"Comps error for {ticker}: {e}"


def _tool_build_deck(
    filename: str,
    slides: Optional[list] = None,
    markdown: str = "",
    firm: str = "",
    project: str = "Confidential",
    confidentiality: str = "CONFIDENTIAL",
    brand_pdf: str = "",
    headline_bold: bool = True,
) -> str:
    """
    Build a PowerPoint deck from structured slide specs or a multi-doc YAML
    markdown stream. Dispatches each slide spec to the matching PPTXDeckWriter
    method.
    """
    try:
        from src.research.pptx_writer import (
            PPTXDeckWriter, BrandProfile, make_pitchpres_profile,
            ScorecardTile, Quote, TimelineEvent, ProcessBox, ProcessArrow,
            FrameworkSection, OrgBox, TombstoneTile, TeamMember, TocEntry,
            parse_deck_markdown, verify,
        )
    except Exception as e:
        return f"Deck builder import error: {e}"

    if markdown and slides:
        return "build_deck error: pass either 'slides' or 'markdown', not both"
    if markdown:
        try:
            slides = parse_deck_markdown(markdown)
        except Exception as e:
            return f"Markdown parse error: {e}"
    if not slides:
        return "build_deck error: need 'slides' or 'markdown'"

    brand = None
    if brand_pdf:
        if brand_pdf.lower() == "pitchpres":
            brand = make_pitchpres_profile()
        else:
            try:
                brand = BrandProfile.from_pdf(brand_pdf)
            except Exception as e:
                return f"Failed to extract brand from {brand_pdf}: {e}"

    deck = PPTXDeckWriter(
        firm=firm, project=project, confidentiality=confidentiality,
        brand=brand, headline_bold=headline_bold,
    )

    type_handlers = {
        "cover": lambda s: deck.add_cover(
            s["title"], subtitle=s.get("subtitle", ""),
            deck_date=s.get("date") or s.get("deck_date"),
        ),
        "section": lambda s: deck.add_section_divider(
            s.get("section_num", "I"), s["title"],
        ),
        "comparison": lambda s: deck.add_comparison_matrix(
            action_title=s["action_title"],
            entities=s["entities"], metrics=s["metrics"], values=s["values"],
            target_label=s.get("target_label", ""),
            source=s.get("source", ""), notes=s.get("notes", ""),
            summary_stats=s.get("summary_stats", True),
            skip_source=s.get("skip_source", False),
        ),
        "scorecard": lambda s: deck.add_scorecard(
            action_title=s["action_title"],
            tiles=[ScorecardTile(**t) for t in s["tiles"]],
            source=s.get("source", ""), notes=s.get("notes", ""),
            skip_source=s.get("skip_source", False),
        ),
        "quote_wall": lambda s: deck.add_quote_wall(
            action_title=s["action_title"],
            quotes=[Quote(**q) for q in s["quotes"]],
            source=s.get("source", ""), notes=s.get("notes", ""),
            skip_source=s.get("skip_source", False),
        ),
        "timeline": lambda s: deck.add_timeline(
            action_title=s["action_title"],
            events=[TimelineEvent(**e) for e in s["events"]],
            source=s.get("source", ""), notes=s.get("notes", ""),
            skip_source=s.get("skip_source", False),
        ),
        "process": lambda s: deck.add_process_diagram(
            action_title=s["action_title"],
            boxes=[ProcessBox(**b) for b in s["boxes"]],
            arrows=[ProcessArrow(**a) for a in s.get("arrows", [])],
            direction=s.get("direction", "ltr"),
            source=s.get("source", ""), notes=s.get("notes", ""),
            skip_source=s.get("skip_source", False),
        ),
        "strategy": lambda s: deck.add_strategy_framework(
            action_title=s["action_title"],
            sections=[FrameworkSection(**sec) for sec in s["sections"]],
            vision=s.get("vision", ""),
            vision_label=s.get("vision_label", "OUR VISION"),
            framework_label=s.get("framework_label", ""),
            source=s.get("source", ""), notes=s.get("notes", ""),
            skip_source=s.get("skip_source", False),
        ),
        "bar_chart": lambda s: deck.add_bar_chart(
            action_title=s["action_title"],
            labels=s["labels"], values=s["values"],
            value_format=s.get("value_format", "{:,.1f}"),
            target_label=s.get("target_label", ""),
            x_label=s.get("x_label", ""),
            source=s.get("source", ""), notes=s.get("notes", ""),
            skip_source=s.get("skip_source", False),
        ),
        "football_field": lambda s: deck.add_football_field(
            action_title=s["action_title"], methods=s["methods"],
            target_value=s.get("target_value"),
            target_label=s.get("target_label", "Current"),
            value_format=s.get("value_format", "${:,.0f}"),
            source=s.get("source", ""), notes=s.get("notes", ""),
            skip_source=s.get("skip_source", False),
        ),
        "line_chart": lambda s: deck.add_line_chart(
            action_title=s["action_title"],
            x_labels=s["x_labels"], series=s["series"],
            target_series=s.get("target_series", ""),
            y_format=s.get("y_format", "{:,.0f}"),
            y_label=s.get("y_label", ""),
            source=s.get("source", ""), notes=s.get("notes", ""),
            skip_source=s.get("skip_source", False),
        ),
        "waterfall": lambda s: deck.add_waterfall(
            action_title=s["action_title"], segments=s["segments"],
            value_format=s.get("value_format", "{:+,.0f}"),
            y_label=s.get("y_label", ""),
            source=s.get("source", ""), notes=s.get("notes", ""),
            skip_source=s.get("skip_source", False),
        ),
        "stacked_bar": lambda s: deck.add_stacked_bar(
            action_title=s["action_title"],
            categories=s["categories"], series=s["series"],
            target_category=s.get("target_category", ""),
            value_format=s.get("value_format", "{:,.0f}"),
            show_totals=s.get("show_totals", True),
            source=s.get("source", ""), notes=s.get("notes", ""),
            skip_source=s.get("skip_source", False),
        ),
        "pie": lambda s: deck.add_pie(
            action_title=s["action_title"], slices=s["slices"],
            target_label=s.get("target_label", ""),
            show_pct=s.get("show_pct", True),
            source=s.get("source", ""), notes=s.get("notes", ""),
            skip_source=s.get("skip_source", False),
        ),
        "pros_cons": lambda s: deck.add_pros_cons(
            action_title=s["action_title"],
            pros=s["pros"], cons=s["cons"], neutral=s.get("neutral"),
            source=s.get("source", ""), notes=s.get("notes", ""),
            skip_source=s.get("skip_source", False),
        ),
        "quad_page": lambda s: deck.add_quad_page(
            action_title=s["action_title"], panels=s["panels"],
            source=s.get("source", ""), notes=s.get("notes", ""),
            skip_source=s.get("skip_source", False),
        ),
        "org_chart": lambda s: deck.add_org_chart(
            action_title=s["action_title"],
            boxes=[OrgBox(**b) for b in s["boxes"]],
            source=s.get("source", ""), notes=s.get("notes", ""),
            skip_source=s.get("skip_source", False),
        ),
        "tombstone_page": lambda s: deck.add_tombstone_page(
            action_title=s["action_title"],
            tiles=[TombstoneTile(**t) for t in s["tiles"]],
            cols=s.get("cols", 7),
            source=s.get("source", ""), notes=s.get("notes", ""),
            skip_source=s.get("skip_source", False),
        ),
        "team_page": lambda s: deck.add_team_page(
            action_title=s["action_title"],
            team_name=s["team_name"],
            members=[TeamMember(**m) for m in s["members"]],
            cols=s.get("cols", 3),
            source=s.get("source", ""), notes=s.get("notes", ""),
            skip_source=s.get("skip_source", False),
        ),
        "table_of_contents": lambda s: deck.add_table_of_contents(
            action_title=s["action_title"],
            entries=[TocEntry(**e) for e in s["entries"]],
            source=s.get("source", ""), notes=s.get("notes", ""),
            skip_source=s.get("skip_source", True),
        ),
    }

    errors = []
    for i, spec in enumerate(slides):
        st = spec.get("type")
        h = type_handlers.get(st)
        if h is None:
            errors.append(f"slide {i+1}: unknown type '{st}'")
            continue
        try:
            h(spec)
        except Exception as e:
            errors.append(f"slide {i+1} ({st}): {e}")

    path = deck.save(filename)
    qa = verify(path)
    summary = (
        f"Deck saved: {path}\n"
        f"Slides built: {qa['passed']}\n"
        f"QA - critical: {len(qa['critical'])}, minor: {len(qa['minor'])}"
    )
    if errors:
        summary += "\nBuild errors:\n  " + "\n  ".join(errors)
    if qa["critical"]:
        summary += "\nCritical issues:\n  " + "\n  ".join(qa["critical"][:5])
    if qa["minor"]:
        summary += "\nMinor issues (first 5):\n  " + "\n  ".join(qa["minor"][:5])
    return summary


# ---------------------------------------------------------------------------
# Tool dispatch
# ---------------------------------------------------------------------------

async def _execute_tool(tool_name: str, tool_input: dict) -> str:
    """Dispatch a tool call to its implementation."""
    dispatch = {
        "search_sec_edgar": lambda i: _tool_search_sec_edgar(**i),
        "search_web": lambda i: _tool_search_web(**i),
        "fetch_page": lambda i: _tool_fetch_page(**i),
        "run_browser_pipeline": lambda i: _tool_run_browser_pipeline(**i),
        "run_financial_model": lambda i: _tool_run_financial_model(**i),
        "run_dcf": lambda i: _tool_run_dcf(**i),
        "run_ev_bridge": lambda i: _tool_run_ev_bridge(**i),
        "run_public_comps": lambda i: _tool_run_public_comps(**i),
        "build_deck": lambda i: _tool_build_deck(**i),
    }
    try:
        fn = dispatch.get(tool_name)
        if fn is None:
            return f"Unknown tool: {tool_name}"
        result = fn(tool_input)
        if asyncio.iscoroutine(result):
            return await result
        return result
    except Exception as e:
        logger.error(f"Tool dispatch error [{tool_name}]: {e}")
        return f"Tool error: {e}"


# ---------------------------------------------------------------------------
# Orchestrator
# ---------------------------------------------------------------------------

class VirtualAnalystOrchestrator:
    """LLM-brain orchestrator. Understands intent, plans steps, calls tools."""

    def __init__(self):
        self._client = anthropic.AsyncAnthropic()

    async def run(
        self,
        query: str,
        ticker: str = "",
        company: str = "",
        max_iterations: int = 10,
    ) -> str:
        """
        Process a natural-language query.
        Returns the final analyst response as a string.
        """
        # Build initial message with context hints
        context_parts = []
        if ticker:
            context_parts.append(f"Ticker: {ticker}")
        if company:
            context_parts.append(f"Company: {company}")
        if context_parts:
            user_content = "\n".join(context_parts) + "\n\n" + query
        else:
            user_content = query

        messages = [{"role": "user", "content": user_content}]
        iterations = 0

        while iterations < max_iterations:
            iterations += 1

            response = await self._client.messages.create(
                model=_MODEL,
                max_tokens=16000,
                thinking={"type": "adaptive"},
                system=_SYSTEM,
                tools=_TOOLS,
                messages=messages,
            )

            if response.stop_reason == "end_turn":
                return next(
                    (b.text for b in response.content if b.type == "text"),
                    "Analysis complete.",
                )

            if response.stop_reason != "tool_use":
                break

            # Append assistant turn (must include tool_use blocks)
            messages.append({"role": "assistant", "content": response.content})

            # Collect all tool calls
            tool_calls = [b for b in response.content if b.type == "tool_use"]
            if not tool_calls:
                break

            # Execute all tools in parallel
            tool_results = await asyncio.gather(*[
                _execute_tool(tc.name, tc.input) for tc in tool_calls
            ])

            logger.info(
                "Tools executed: %s",
                ", ".join(tc.name for tc in tool_calls),
            )

            # Append tool results as user turn
            messages.append({
                "role": "user",
                "content": [
                    {
                        "type": "tool_result",
                        "tool_use_id": tc.id,
                        "content": str(result),
                    }
                    for tc, result in zip(tool_calls, tool_results)
                ],
            })

        # Fallback: return last text block if loop exits without end_turn
        for block in reversed(response.content):
            if block.type == "text":
                return block.text
        return "Analysis complete."


# ---------------------------------------------------------------------------
# Convenience sync wrapper (CLI / model.py integration)
# ---------------------------------------------------------------------------

def run_sync(
    query: str,
    ticker: str = "",
    company: str = "",
    max_iterations: int = 10,
) -> str:
    """Blocking wrapper for use from CLI or synchronous code."""
    orchestrator = VirtualAnalystOrchestrator()
    return asyncio.run(
        orchestrator.run(query, ticker=ticker, company=company,
                         max_iterations=max_iterations)
    )
