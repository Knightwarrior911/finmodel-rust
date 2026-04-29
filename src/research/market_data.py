"""
Live market data via yfinance.
Share price, market cap, basic metrics. Free, no API key needed.
"""

import logging
from dataclasses import dataclass
from datetime import date, datetime
from typing import Optional

import yfinance as yf

logger = logging.getLogger(__name__)


@dataclass
class MarketData:
    ticker: str
    company_name: str = ""
    exchange: str = ""
    currency: str = "USD"

    # Price
    current_price: Optional[float] = None
    prev_close: Optional[float] = None
    open_price: Optional[float] = None
    day_high: Optional[float] = None
    day_low: Optional[float] = None
    fifty_two_week_high: Optional[float] = None
    fifty_two_week_low: Optional[float] = None

    # Volume
    volume: Optional[int] = None
    avg_volume: Optional[int] = None

    # Valuation
    market_cap: Optional[float] = None
    enterprise_value: Optional[float] = None
    shares_outstanding: Optional[float] = None
    float_shares: Optional[float] = None

    # Multiples
    pe_ratio: Optional[float] = None
    forward_pe: Optional[float] = None
    peg_ratio: Optional[float] = None
    price_to_book: Optional[float] = None
    price_to_sales: Optional[float] = None

    # Fundamentals (TTM)
    revenue: Optional[float] = None
    ebitda: Optional[float] = None
    net_income: Optional[float] = None
    eps: Optional[float] = None
    book_value: Optional[float] = None

    # Dividends
    dividend_yield: Optional[float] = None
    dividend_rate: Optional[float] = None
    payout_ratio: Optional[float] = None

    # Metadata
    sector: str = ""
    industry: str = ""
    description: str = ""
    website: str = ""
    price_date: Optional[date] = None
    price_source: str = ""

    @property
    def market_cap_formatted(self) -> str:
        if not self.market_cap:
            return "N/A"
        v = self.market_cap
        if v >= 1e12:
            return f"${v/1e12:.2f}T"
        elif v >= 1e9:
            return f"${v/1e9:.1f}B"
        elif v >= 1e6:
            return f"${v/1e6:.1f}M"
        return f"${v:,.0f}"


def get_market_data(ticker: str) -> MarketData:
    """
    Fetch live market data for a ticker via yfinance.
    Handles US stocks, ADRs, international tickers with exchange suffix.
    Examples: 'AAPL', 'JPM', 'BAMNB.AS' (Euronext Amsterdam), 'SIE.DE' (Xetra)
    """
    try:
        stock = yf.Ticker(ticker)
        info = stock.info or {}

        md = MarketData(
            ticker=ticker.upper(),
            company_name=info.get("longName") or info.get("shortName", ""),
            exchange=info.get("exchange", ""),
            currency=info.get("currency", "USD"),

            # Price
            current_price=info.get("currentPrice") or info.get("regularMarketPrice"),
            prev_close=info.get("previousClose") or info.get("regularMarketPreviousClose"),
            open_price=info.get("open") or info.get("regularMarketOpen"),
            day_high=info.get("dayHigh") or info.get("regularMarketDayHigh"),
            day_low=info.get("dayLow") or info.get("regularMarketDayLow"),
            fifty_two_week_high=info.get("fiftyTwoWeekHigh"),
            fifty_two_week_low=info.get("fiftyTwoWeekLow"),

            # Volume
            volume=info.get("volume") or info.get("regularMarketVolume"),
            avg_volume=info.get("averageVolume") or info.get("averageDailyVolume10Day"),

            # Valuation
            market_cap=info.get("marketCap"),
            enterprise_value=info.get("enterpriseValue"),
            shares_outstanding=info.get("sharesOutstanding"),
            float_shares=info.get("floatShares"),

            # Multiples
            pe_ratio=info.get("trailingPE"),
            forward_pe=info.get("forwardPE"),
            peg_ratio=info.get("pegRatio"),
            price_to_book=info.get("priceToBook"),
            price_to_sales=info.get("priceToSales"),

            # Fundamentals
            revenue=info.get("totalRevenue"),
            ebitda=info.get("ebitda"),
            net_income=info.get("netIncomeToCommon"),
            eps=info.get("trailingEps"),
            book_value=info.get("bookValue"),

            # Dividends
            dividend_yield=info.get("dividendYield"),
            dividend_rate=info.get("dividendRate"),
            payout_ratio=info.get("payoutRatio"),

            # Metadata
            sector=info.get("sector", ""),
            industry=info.get("industry", ""),
            description=(info.get("longBusinessSummary") or "")[:500],
            website=info.get("website", ""),
            price_date=date.today(),
            price_source=f"yfinance ({info.get('exchange', 'primary')})",
        )

        return md

    except Exception as e:
        logger.error(f"yfinance fetch failed for {ticker}: {e}")
        return MarketData(ticker=ticker.upper(), price_source="ERROR")


def get_share_price(ticker: str) -> Optional[float]:
    """Quick: get current share price only."""
    data = get_market_data(ticker)
    return data.current_price


def get_company_info(ticker: str) -> dict:
    """Get basic company info: name, sector, industry, market cap."""
    data = get_market_data(ticker)
    return {
        "ticker": data.ticker,
        "name": data.company_name,
        "sector": data.sector,
        "industry": data.industry,
        "market_cap": data.market_cap,
        "current_price": data.current_price,
        "currency": data.currency,
        "exchange": data.exchange,
        "pe_ratio": data.pe_ratio,
        "ev_ebitda": data.enterprise_value / data.ebitda if data.enterprise_value and data.ebitda else None,
    }


# --- EV Bridge integration ---

def build_ev_bridge_from_market(ticker: str, extra_debt: float = 0.0,
                                extra_leases: float = 0.0) -> dict:
    """
    Quick EV bridge using yfinance data.
    For US companies, combine with SEC EDGAR for accuracy.
    """
    md = get_market_data(ticker)
    if not md.current_price:
        return {"error": f"Could not get price for {ticker}"}

    mc = md.market_cap
    if not mc and md.shares_outstanding:
        mc = md.current_price * md.shares_outstanding

    ev = mc or 0

    # Use yfinance's EV calculation as reference
    yf_ev = md.enterprise_value

    return {
        "ticker": ticker,
        "company": md.company_name,
        "price": md.current_price,
        "price_date": str(md.price_date),
        "shares_outstanding": md.shares_outstanding,
        "market_cap": mc,
        "yf_enterprise_value": yf_ev,
        "cash": None,  # Need SEC EDGAR for accurate BS items
        "debt": None,
        "yf_ev_ebitda": md.enterprise_value / md.ebitda if md.enterprise_value and md.ebitda else None,
    }
