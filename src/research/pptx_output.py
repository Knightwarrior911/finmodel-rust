"""
PPT output writer for research agent results.
Mirrors ResearchExcelWriter pattern — outputs IB-standard PowerPoint decks.
"""
from __future__ import annotations

import logging
import os

logger = logging.getLogger(__name__)


def _fmt_v(v: float, currency: str = "USD") -> str:
    if v is None:
        return "—"
    if abs(v) >= 1e9:
        return f"{currency} {v/1e9:.1f}B"
    if abs(v) >= 1e6:
        return f"{currency} {v/1e6:,.0f}M"
    return f"{currency} {v:,.0f}"


class ResearchPPTXWriter:
    """Writes research results to IB-standard PowerPoint decks."""

    def __init__(self, output_dir: str = None):
        self.output_dir = output_dir or os.path.join(
            os.path.dirname(__file__), "..", "..", "decks"
        )
        os.makedirs(self.output_dir, exist_ok=True)

    def write_ev_bridge_deck(self, ev_input, filename: str = None) -> str:
        """3-slide EV bridge deck: cover + waterfall + scorecard."""
        from src.research.pptx_writer import PPTXDeckWriter, ScorecardTile, verify

        company = ev_input.company or "Company"
        period = ev_input.period or "LTM"
        currency = ev_input.currency or "USD"
        mc = ev_input.computed_market_cap or 0

        # Waterfall: Market Cap → +debt/leases/pension → -cash/investments → EV
        segments = [{"label": "Market Cap", "value": mc, "kind": "start"}]
        for value, label in [
            (ev_input.total_debt,           "Total Debt"),
            (ev_input.operating_leases,     "Operating Leases"),
            (ev_input.finance_leases,       "Finance Leases"),
            (ev_input.underfunded_pension,  "Underfunded Pension"),
            (ev_input.minority_interest,    "Minority Interest"),
            (ev_input.preferred_stock,      "Preferred Stock"),
        ]:
            if value and value > 0:
                segments.append({"label": label, "value": value, "kind": "plus"})
        for value, label in [
            (ev_input.cash,                    "Cash & Equivalents"),
            (ev_input.short_term_investments,  "Short-term Investments"),
            (ev_input.equity_investments,      "Equity Investments"),
        ]:
            if value and value > 0:
                segments.append({"label": label, "value": -value, "kind": "minus"})

        # EV = running sum
        running = mc
        for s in segments[1:]:
            running += s["value"]
        ev = running
        segments.append({"label": "Enterprise Value", "value": ev, "kind": "total"})

        if len(segments) < 3:
            # No bridge components extracted — just cover + scorecard, skip waterfall
            segments = None

        # Scorecard tiles
        tiles = []
        if mc:
            tiles.append(ScorecardTile(metric="Market Cap", value=_fmt_v(mc, currency)))
        if ev:
            tiles.append(ScorecardTile(metric="Enterprise Value", value=_fmt_v(ev, currency)))
        net_debt = (
            (ev_input.total_debt or 0)
            + (ev_input.operating_leases or 0)
            - (ev_input.cash or 0)
        )
        tiles.append(ScorecardTile(metric="Net Debt", value=_fmt_v(net_debt, currency),
                                   sub="Debt + Leases − Cash"))
        if ev_input.ltm_ebitda and ev_input.ltm_ebitda > 0:
            mult = ev / ev_input.ltm_ebitda
            tiles.append(ScorecardTile(
                metric="EV/EBITDA", value=f"{mult:.1f}x",
                sub=f"{period} EBITDA = {_fmt_v(ev_input.ltm_ebitda, currency)}",
            ))
        if ev_input.ltm_revenue and ev_input.ltm_revenue > 0:
            mult = ev / ev_input.ltm_revenue
            tiles.append(ScorecardTile(
                metric="EV/Revenue", value=f"{mult:.1f}x",
                sub=f"{period} Revenue = {_fmt_v(ev_input.ltm_revenue, currency)}",
            ))

        deck = PPTXDeckWriter(
            firm="Virtual Analyst",
            project=f"{company} EV Bridge",
            output_dir=self.output_dir,
        )
        deck.add_cover(
            f"{company} — Enterprise Value Bridge",
            subtitle=f"{period} | {currency}",
        )
        if segments:
            scale = "millions" if max(abs(mc), abs(ev)) < 1e9 else "billions"
            deck.add_waterfall(
                action_title=(
                    f"{company} EV bridge: {_fmt_v(mc, currency)} market cap → "
                    f"{_fmt_v(ev, currency)} enterprise value"
                ),
                segments=segments,
                value_format="{:+,.0f}",
                y_label=f"{currency} ({scale})",
                source="Bloomberg, company filings, SEC EDGAR",
                broken_axis=True,
            )
        if tiles:
            deck.add_scorecard(
                action_title=f"{company} key valuation metrics ({period})",
                tiles=tiles,
                source="Bloomberg, SEC EDGAR",
            )

        fname = filename or f"{company.replace(' ', '_')}_EV_Bridge"
        path = deck.save(fname)
        qa = verify(path)
        logger.info(
            "EV bridge deck: %s | slides=%d crit=%d",
            path, qa["passed"], len(qa["critical"]),
        )
        return path

    def write_ifrs_bridge_deck(
        self,
        inputs,
        out,
        company: str,
        period: str,
        revenue: float = 0,
        filename: str = None,
    ) -> str:
        """3-slide IFRS 16 deck: cover + waterfall + scorecard."""
        from src.research.pptx_writer import PPTXDeckWriter, ScorecardTile, verify

        is_ifrs_to_gaap = getattr(inputs, "accounting_standard", "IFRS") == "IFRS"
        reported = inputs.reported_ebitda or 0
        adjusted = (out.adjusted_ebitda if out else reported) or reported
        rou = inputs.rou_depreciation or 0
        lease_int = inputs.lease_interest or 0
        short_r = inputs.short_term_rent or 0

        # Waterfall: reported EBITDA → adjustments → adjusted EBITDA
        segments = [{"label": "Reported EBITDA", "value": reported, "kind": "start"}]
        if is_ifrs_to_gaap:
            if rou > 0:
                segments.append({"label": "Less: ROU Depreciation", "value": -rou,      "kind": "minus"})
            if lease_int > 0:
                segments.append({"label": "Less: Lease Interest",   "value": -lease_int, "kind": "minus"})
            if short_r > 0:
                segments.append({"label": "Less: Short-term Rent",  "value": -short_r,  "kind": "minus"})
        else:
            if rou > 0:
                segments.append({"label": "Add: ROU Depreciation",  "value": rou,       "kind": "plus"})
            if lease_int > 0:
                segments.append({"label": "Add: Lease Interest",    "value": lease_int, "kind": "plus"})
            if short_r > 0:
                segments.append({"label": "Less: Cash Rent",        "value": -short_r,  "kind": "minus"})
        segments.append({"label": "Adj. EBITDA", "value": adjusted, "kind": "total"})

        tiles = []
        if reported:
            tiles.append(ScorecardTile(metric="Reported EBITDA",
                                       value=f"{reported/1e6:,.0f}M", sub=period))
        if adjusted and adjusted != reported:
            delta = adjusted - reported
            tiles.append(ScorecardTile(
                metric="Adj. EBITDA",
                value=f"{adjusted/1e6:,.0f}M",
                sub=f"Δ = {delta/1e6:+,.0f}M",
            ))
        if revenue and adjusted:
            tiles.append(ScorecardTile(
                metric="Adj. EBITDA Margin",
                value=f"{adjusted / revenue:.1%}",
                sub=f"Revenue = {revenue/1e6:,.0f}M",
            ))
        if rou:
            tiles.append(ScorecardTile(metric="ROU Depreciation",
                                       value=f"{rou/1e6:,.0f}M",
                                       sub="Annual report lease note"))
        if lease_int:
            tiles.append(ScorecardTile(metric="Lease Interest",
                                       value=f"{lease_int/1e6:,.0f}M",
                                       sub="Annual report lease note"))

        standard = getattr(inputs, "accounting_standard", "IFRS") or "IFRS"
        deck = PPTXDeckWriter(
            firm="Virtual Analyst",
            project=f"{company} IFRS 16 Analysis",
            output_dir=self.output_dir,
        )
        deck.add_cover(
            f"{company} — IFRS 16 Lease Adjustment",
            subtitle=f"{period} | {standard} Bridge",
        )
        if len(segments) >= 3:
            deck.add_waterfall(
                action_title=(
                    f"{company} EBITDA bridge: reported {reported/1e6:,.0f}M → "
                    f"adjusted {adjusted/1e6:,.0f}M ({period})"
                ),
                segments=segments,
                value_format="{:+,.0f}",
                y_label=standard,
                source="Annual report — lease note (IFRS 16 / ASC 842)",
            )
        if tiles:
            deck.add_scorecard(
                action_title=f"{company} IFRS 16 adjustment summary ({period})",
                tiles=tiles,
                source="Annual report, company filings",
            )

        fname = filename or f"{company.replace(' ', '_')}_IFRS_Bridge"
        path = deck.save(fname)
        qa = verify(path)
        logger.info(
            "IFRS bridge deck: %s | slides=%d crit=%d",
            path, qa["passed"], len(qa["critical"]),
        )
        return path
