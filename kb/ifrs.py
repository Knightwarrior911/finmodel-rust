"""
IFRS 16 conversion rules ported from ifrs_rules.json.
Converts between IFRS 16 (Post-IFRS) and US GAAP (Pre-IFRS) for EBIT/EBITDA/EBITA.

CRITICAL: Only use ROU Depreciation + Lease Interest as adjustment items.
Short-term rent = already OPEX in both frameworks = NOT an adjustment item.
"""

from dataclasses import dataclass
from enum import Enum
from typing import Optional


class AdjustmentDirection(Enum):
    IFRS_TO_US_GAAP = "ifrs_to_us_gaap"    # Strip IFRS 16 lease capitalization
    US_GAAP_TO_IFRS = "us_gaap_to_ifrs"    # Add IFRS 16 lease capitalization


@dataclass
class IFRSAdjustmentInput:
    """Inputs extracted from lease note (Phase 3 of IFRS workflow)."""
    rou_depreciation: float           # Depreciation of right-of-use assets
    lease_interest: float             # Interest expense on lease liabilities
    short_term_rent: float = 0.0      # Short-term lease exemption — DO NOT USE in adjustment

    # Starting values (as reported)
    reported_ebit: float = 0.0
    reported_ebitda: float = 0.0
    reported_ebita: float = 0.0
    standard_depreciation: float = 0.0  # PPE depreciation (NOT ROU depreciation)
    standard_amortization: float = 0.0  # Intangible amortization

    # Metadata
    accounting_standard: str = "IFRS"  # IFRS or US GAAP
    weighted_discount_rate: Optional[float] = None
    weighted_lease_term: Optional[float] = None

    @property
    def total_cash_rental_expense(self) -> float:
        """Total cash rental = ROU Depreciation + Lease Interest."""
        return self.rou_depreciation + self.lease_interest


@dataclass
class IFRSAdjustmentOutput:
    direction: AdjustmentDirection
    adjusted_ebit: float
    adjusted_ebitda: float
    adjusted_ebita: float

    # Margins
    reported_ebit_margin: float = 0.0
    adjusted_ebit_margin: float = 0.0
    reported_ebitda_margin: float = 0.0
    adjusted_ebitda_margin: float = 0.0
    reported_ebita_margin: float = 0.0
    adjusted_ebita_margin: float = 0.0

    # Deltas
    ebit_delta: float = 0.0
    ebitda_delta: float = 0.0
    ebita_delta: float = 0.0

    # Validation
    adjustment_items_used: list[str] = None
    items_excluded: list[str] = None


def convert_ifrs_to_us_gaap(inputs: IFRSAdjustmentInput,
                             revenue: float = 0.0) -> IFRSAdjustmentOutput:
    """
    IFRS 16 → US GAAP (Pre-IFRS).
    Strips out IFRS 16 lease capitalization.

    Formulas:
      Pre-IFRS EBIT   = Post-IFRS EBIT - Lease Interest
      Pre-IFRS EBITDA = Post-IFRS EBITDA - Lease Interest - ROU Depreciation
      Pre-IFRS EBITA  = Post-IFRS EBITA - Lease Interest
    """
    adj_ebit = inputs.reported_ebit - inputs.lease_interest
    adj_ebitda = inputs.reported_ebitda - inputs.lease_interest - inputs.rou_depreciation
    adj_ebita = inputs.reported_ebita - inputs.lease_interest

    out = IFRSAdjustmentOutput(
        direction=AdjustmentDirection.IFRS_TO_US_GAAP,
        adjusted_ebit=adj_ebit,
        adjusted_ebitda=adj_ebitda,
        adjusted_ebita=adj_ebita,
        ebit_delta=adj_ebit - inputs.reported_ebit,
        ebitda_delta=adj_ebitda - inputs.reported_ebitda,
        ebita_delta=adj_ebita - inputs.reported_ebita,
        adjustment_items_used=["ROU Depreciation", "Lease Interest"],
        items_excluded=["Short-term rent (already OPEX in both frameworks)"],
    )

    if revenue > 0:
        out.reported_ebit_margin = inputs.reported_ebit / revenue * 100
        out.adjusted_ebit_margin = adj_ebit / revenue * 100
        out.reported_ebitda_margin = inputs.reported_ebitda / revenue * 100
        out.adjusted_ebitda_margin = adj_ebitda / revenue * 100
        out.reported_ebita_margin = inputs.reported_ebita / revenue * 100
        out.adjusted_ebita_margin = adj_ebita / revenue * 100

    return out


def convert_us_gaap_to_ifrs(inputs: IFRSAdjustmentInput,
                             revenue: float = 0.0) -> IFRSAdjustmentOutput:
    """
    US GAAP → IFRS 16 (Post-IFRS).
    Adds IFRS 16 lease capitalization.

    Formulas:
      Post-IFRS EBIT   = Pre-IFRS EBIT + Lease Interest
      Post-IFRS EBITDA = Pre-IFRS EBITDA + Lease Interest + ROU Depreciation
      Post-IFRS EBITA  = Pre-IFRS EBITA + Lease Interest
    """
    adj_ebit = inputs.reported_ebit + inputs.lease_interest
    adj_ebitda = inputs.reported_ebitda + inputs.lease_interest + inputs.rou_depreciation
    adj_ebita = inputs.reported_ebita + inputs.lease_interest

    out = IFRSAdjustmentOutput(
        direction=AdjustmentDirection.US_GAAP_TO_IFRS,
        adjusted_ebit=adj_ebit,
        adjusted_ebitda=adj_ebitda,
        adjusted_ebita=adj_ebita,
        ebit_delta=adj_ebit - inputs.reported_ebit,
        ebitda_delta=adj_ebitda - inputs.reported_ebitda,
        ebita_delta=adj_ebita - inputs.reported_ebita,
        adjustment_items_used=["ROU Depreciation", "Lease Interest"],
        items_excluded=["Short-term rent (already OPEX in both frameworks)"],
    )

    if revenue > 0:
        out.reported_ebit_margin = inputs.reported_ebit / revenue * 100
        out.adjusted_ebit_margin = adj_ebit / revenue * 100
        out.reported_ebitda_margin = inputs.reported_ebitda / revenue * 100
        out.adjusted_ebitda_margin = adj_ebitda / revenue * 100
        out.reported_ebita_margin = inputs.reported_ebita / revenue * 100
        out.adjusted_ebita_margin = adj_ebita / revenue * 100

    return out


def format_bridge(inputs: IFRSAdjustmentInput, out: IFRSAdjustmentOutput,
                  revenue: float = 0.0, company: str = "", period: str = "",
                  notes_ref: dict = None) -> str:
    """Format IFRS conversion as a detailed bridge with all line items, values, and sources.

    Returns a multi-line string showing the step-by-step conversion from starting
    metric through each adjustment to the final result, with sources cited.
    """
    if notes_ref is None:
        notes_ref = {}

    direction_label = "IFRS 16 -> US GAAP (Pre-IFRS)" if out.direction.name == "IFRS_TO_US_GAAP" else "US GAAP -> IFRS 16 (Post-IFRS)"
    op = "-" if out.direction.name == "IFRS_TO_US_GAAP" else "+"
    arrow = "->" if out.direction.name == "IFRS_TO_US_GAAP" else "<-"

    lines = []
    lines.append("=" * 70)
    if company:
        lines.append(f"{company} {period} -- {direction_label}")
    else:
        lines.append(f"IFRS 16 Conversion Bridge -- {direction_label}")
    lines.append("=" * 70)

    # --- EBITDA HIERARCHY ---
    da = inputs.standard_depreciation + inputs.standard_amortization
    ebitda_computed = inputs.reported_ebit + da

    # Determine which EBITDA to use (Adjusted > Reported > Computed)
    ebitda_start = inputs.reported_ebitda if inputs.reported_ebitda > inputs.reported_ebit else ebitda_computed
    # If reported_ebitda was explicitly provided AND differs from computed, use it
    use_adjusted = (inputs.reported_ebitda > 0 and
                    abs(inputs.reported_ebitda - ebitda_computed) > ebitda_computed * 0.01)

    lines.append("")
    lines.append("EBITDA DERIVATION")
    lines.append("-" * 50)
    lines.append(f"  {'Reported EBIT (from income statement)':<40} {inputs.reported_ebit:>12,.0f}")
    lines.append(f"    Source: {notes_ref.get('ebit_src', 'income statement — operating result')}")
    if da > 0:
        lines.append(f"  + Depreciation & Amortisation                {da:>12,.0f}")
        lines.append(f"    Source: {notes_ref.get('da_src', 'income statement — D&A line')}")
    lines.append(f"  {'-' * 50}")
    lines.append(f"  {'= EBITDA (computed: EBIT + D&A)':<40} {ebitda_computed:>12,.0f}")

    if use_adjusted:
        lines.append("")
        lines.append(f"  Adjusted EBITDA (company-reported)           {inputs.reported_ebitda:>12,.0f}")
        lines.append(f"    Source: {notes_ref.get('ebitda_src', 'annual report — one-off items removed')}")
        lines.append(f"    Difference vs computed: {inputs.reported_ebitda - ebitda_computed:+,.0f}")
        lines.append(f"    (Adjusted EBITDA removes one-off items; has NOTHING to do with IFRS 16)")
        lines.append(f"  {'-' * 50}")
        ebitda_start = inputs.reported_ebitda
    else:
        lines.append(f"    (EBITDA not separately reported; computed from EBIT + D&A)")
        ebitda_start = ebitda_computed
    lines.append("")

    # Compute pre-IFRS EBITDA from the starting EBITDA
    if out.direction.name == "IFRS_TO_US_GAAP":
        pre_ifrs_ebitda = ebitda_start - inputs.rou_depreciation - inputs.lease_interest
        pre_ifrs_ebit = inputs.reported_ebit - inputs.lease_interest
    else:
        pre_ifrs_ebitda = ebitda_start + inputs.rou_depreciation + inputs.lease_interest
        pre_ifrs_ebit = inputs.reported_ebit + inputs.lease_interest

    ebitda_delta = pre_ifrs_ebitda - ebitda_start

    lines.append("IFRS 16 ADJUSTMENT")
    lines.append("-" * 50)
    lines.append(f"  {'Starting EBITDA (Post-IFRS)':<40} {ebitda_start:>12,.0f}")
    lines.append(f"  {op} ROU Depreciation                          {inputs.rou_depreciation:>12,.0f}")
    lines.append(f"    Source: {notes_ref.get('rou_depr', 'lease note — depreciation of ROU assets')}")
    lines.append(f"  {op} Interest on lease liabilities            {inputs.lease_interest:>12,.0f}")
    lines.append(f"    Source: {notes_ref.get('lease_int', 'finance expense note — interest on lease liab')}")
    lines.append(f"  {'-' * 50}")
    lines.append(f"  {'= Pre-IFRS EBITDA':<40} {pre_ifrs_ebitda:>12,.0f}")
    if ebitda_start > 0:
        lines.append(f"  EBITDA Delta: {ebitda_delta:+,.0f}  ({abs(ebitda_delta)/ebitda_start*100:.1f}% of starting EBITDA)")
    if revenue > 0:
        lines.append(f"  Margin: {ebitda_start/revenue*100:.1f}% {arrow} {pre_ifrs_ebitda/revenue*100:.1f}%")

    # --- EBIT Bridge ---
    lines.append("")
    lines.append("EBIT BRIDGE")
    lines.append("-" * 50)
    lines.append(f"  {'Reported EBIT (from income statement)':<40} {inputs.reported_ebit:>12,.0f}")
    lines.append(f"    Source: {notes_ref.get('ebit_src', 'income statement — operating result')}")
    lines.append(f"  {op} Interest on lease liabilities            {inputs.lease_interest:>12,.0f}")
    lines.append(f"    Source: {notes_ref.get('lease_int', 'finance expense note')}")
    lines.append(f"  {'-' * 50}")
    lines.append(f"  {'= Pre-IFRS EBIT':<40} {pre_ifrs_ebit:>12,.0f}")
    if revenue > 0:
        lines.append(f"  Margin: {inputs.reported_ebit/revenue*100:.1f}% {arrow} {pre_ifrs_ebit/revenue*100:.1f}%")

    # --- EBITA Bridge ---
    lines.append("")
    lines.append("EBITA BRIDGE")
    lines.append("-" * 50)
    lines.append(f"  {'Post-IFRS EBITA':<40} {inputs.reported_ebita:>12,.0f}")
    lines.append(f"  {op} Interest on lease liabilities            {inputs.lease_interest:>12,.0f}")
    lines.append(f"  {'-' * 50}")
    pre_ifrs_ebita = inputs.reported_ebita - inputs.lease_interest if out.direction.name == "IFRS_TO_US_GAAP" else inputs.reported_ebita + inputs.lease_interest
    lines.append(f"  {'= Pre-IFRS EBITA':<40} {pre_ifrs_ebita:>12,.0f}")

    # --- Items explicitly excluded ---
    lines.append("")
    lines.append("ITEMS EXCLUDED FROM ADJUSTMENT")
    lines.append("-" * 50)
    excluded_items = out.items_excluded or []
    for item in excluded_items:
        lines.append(f"  X  {item}")
    if inputs.short_term_rent > 0:
        lines.append(f"  X  Short-term rent: {inputs.short_term_rent:,.0f} (already OPEX in both IFRS and US GAAP)")
        lines.append(f"     Source: {notes_ref.get('short_term', 'lease note (practical expedient disclosure)')}")

    # --- Adjustment items used ---
    lines.append("")
    lines.append("ADJUSTMENT ITEMS USED")
    lines.append("-" * 50)
    for item in (out.adjustment_items_used or []):
        lines.append(f"  V  {item}")
    lines.append(f"  Total deduction: {abs(ebitda_delta):,.0f}")

    # --- Balance sheet impact ---
    lines.append("")
    lines.append("BALANCE SHEET NOTE")
    lines.append("-" * 50)
    if out.direction.name == "IFRS_TO_US_GAAP":
        lines.append("  Post-IFRS: Add lease liabilities to Enterprise Value")
        lines.append("  Pre-IFRS:  Do NOT add lease liabilities to Enterprise Value")
        lines.append("  (Lease liabilities are an IFRS 16 construct; pre-IFRS EBITDA")
        lines.append("   already reflects the full cash lease expense)")

    lines.append("")
    lines.append("=" * 70)
    return "\n".join(lines)


def auto_convert(inputs: IFRSAdjustmentInput, revenue: float = 0.0) -> IFRSAdjustmentOutput:
    """Auto-detect direction from accounting standard and convert."""
    if inputs.accounting_standard.upper() == "IFRS":
        return convert_ifrs_to_us_gaap(inputs, revenue)
    else:
        return convert_us_gaap_to_ifrs(inputs, revenue)
