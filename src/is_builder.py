"""
Dynamic IS row structure builder.

Builds the IS row list for each sector. Each ISRow defines:
- key: maps to income_statement dict key (or empty for non-data)
- label: display text in Excel
- row_type: 'section_header'|'line_item'|'subtotal'|'driver'|'memo'|'spacer'
- bold/italic/indent: formatting
- driver_key: assumption dict key this driver links to (Assumptions active block)
- driver_format: "pct" or "num"
- hist_numer_key/hist_denom_key: how to compute implied hist ratio

Driver key → Assumptions active block row offset mapping:
  revenue_growth_pct → 0, gross_margin_pct → 1, sga_pct_rev → 2,
  rd_pct_rev → 3, da_pct_rev → 4, capex_pct_rev → 5,
  tax_rate_pct → 6, interest_rate_pct → 7,
  dso_days → 8, dio_days → 9, dpo_days → 10,
  dividend_per_share → 11, terminal_growth_rate → 12, exit_ebitda_multiple → 13
"""
from schemas.financial_data import ISRow

# Maps driver_key → offset in Assumptions ACTIVE block (ASSUMP_R["active_drv0"] + offset)
DRIVER_KEY_TO_ASSUMP_OFFSET: dict[str, int] = {
    "revenue_growth_pct": 0,
    "gross_margin_pct":   1,
    "sga_pct_rev":        2,
    "rd_pct_rev":         3,
    "da_pct_rev":         4,
    "capex_pct_rev":      5,
    "tax_rate_pct":       6,
    "interest_rate_pct":  7,
    "dso_days":           8,
    "dio_days":           9,
    "dpo_days":           10,
    "dividend_per_share": 11,
    "terminal_growth_rate": 12,
    "exit_ebitda_multiple": 13,
}

IS_BODY_START = 10   # 0-based row where IS body begins (row 11 in Excel)


def _li(key: str, label: str, bold: bool = False) -> ISRow:
    """Line item row."""
    return ISRow(key=key, label=label, row_type="line_item", bold=bold)


def _st(key: str, label: str) -> ISRow:
    """Subtotal row (bold, formula)."""
    return ISRow(key=key, label=label, row_type="subtotal", bold=True)


def _sec(label: str) -> ISRow:
    """Section header row."""
    return ISRow(key="", label=label, row_type="section_header", bold=True)


def _drv(label: str, driver_key: str, driver_format: str = "pct",
         hist_numer: str = "", hist_denom: str = "revenue") -> ISRow:
    """Driver row: green link to Assumptions in proj; implied ratio in hist."""
    return ISRow(
        key=f"__drv_{driver_key}",
        label=f"  {label}",
        row_type="driver",
        italic=True,
        indent=1,
        driver_key=driver_key,
        driver_format=driver_format,
        hist_numer_key=hist_numer,
        hist_denom_key=hist_denom,
    )


def _mo(key: str, label: str, numer: str, denom: str = "revenue") -> ISRow:
    """Memo row: computed ratio, black formula all periods."""
    return ISRow(
        key=key, label=label, row_type="memo",
        italic=True,
        hist_numer_key=numer, hist_denom_key=denom,
    )


def _sp() -> ISRow:
    """Spacer row (5px height)."""
    return ISRow(key="", label="", row_type="spacer")


# ─────────────────────────────────────────────────────────────────────────────
# Standard IS (tech, consumer, industrial, healthcare, etc.)
# ─────────────────────────────────────────────────────────────────────────────

def _build_standard_is(has_cogs: bool, has_rd: bool, has_sga: bool,
                       revenue_segments: list[dict] | None = None,
                       opex_items: list[dict] | None = None,
                       cogs_detail: list[dict] | None = None) -> list[ISRow]:
    rows: list[ISRow] = []

    # ── Revenue ────────────────────────────────────────────────────────
    if revenue_segments:
        for seg in revenue_segments:
            dk = f"{seg['key']}_growth_pct"
            rows += [
                _li(seg["key"], f"  {seg['label']}"),
                _drv(f"{seg['label']} Growth %", dk,
                     hist_numer="__growth", hist_denom=seg["key"]),
            ]
        rows.append(_sp())
        rows.append(_st("revenue", "Total Revenue"))
        rows.append(_sp())
    else:
        rows += [
            _li("revenue", "Revenue", bold=True),
            _drv("Revenue Growth %", "revenue_growth_pct",
                 hist_numer="__growth", hist_denom="revenue"),
            _sp(),
        ]

    # ── Dynamic COGS / OpEx from actual XBRL disclosure ─────────────────
    if opex_items:
        cogs_items = [o for o in opex_items if o["category"] == "cogs"]
        rd_items   = [o for o in opex_items if o["category"] == "opex_rd"]
        other_oe   = [o for o in opex_items if o["category"] == "opex"]

        # COST OF REVENUES — actual COGS line items
        if cogs_items:
            rows.append(_sec("COST OF REVENUES"))
            if cogs_detail:
                # Detailed breakdown from R-file (e.g. subscription vs professional services)
                for cd in cogs_detail:
                    rows.append(_li(cd["key"], f"  {cd['label']}"))
                rows.append(_st("cogs", "  Total Cost of Revenues"))
            else:
                for ci in cogs_items:
                    rows.append(_li("cogs", f"  {ci['label']}"))
            rows.append(_st("gross_profit", "Gross Profit"))
            rows.append(_drv("Gross Margin %", "gross_margin_pct",
                             hist_numer="gross_profit", hist_denom="revenue"))
            rows.append(_sp())

        # OPERATING EXPENSES — R&D + other actual opex line items
        if rd_items or other_oe:
            rows.append(_sec("OPERATING EXPENSES"))

            # R&D items — map first to rd key, assign driver
            for idx, ri in enumerate(rd_items):
                key = "rd" if idx == 0 else ri["key"]
                rows.append(_li(key, f"  {ri['label']}"))
                if idx == 0:
                    rows.append(_drv("R&D % of Revenue", "rd_pct_rev",
                                     hist_numer="rd", hist_denom="revenue"))

            # Other opex — map first to sga key with driver; rest flat
            for idx, oi in enumerate(other_oe):
                key = "sga" if idx == 0 else oi["key"]
                rows.append(_li(key, f"  {oi['label']}"))
                if idx == 0:
                    label = f"{oi['label']} % of Revenue"
                    rows.append(_drv(label, "sga_pct_rev",
                                     hist_numer="sga", hist_denom="revenue"))

        # Register extra opex item data keys as flat-projection — no driver
        extra_opex_keys: list[str] = []
        for oi in opex_items:
            if oi["category"] == "opex_rd":
                for idx, ri in enumerate(rd_items):
                    if idx > 0 and ri["key"] == oi["key"]:
                        extra_opex_keys.append(ri["key"])
            elif oi["category"] == "opex":
                for idx, oi2 in enumerate(other_oe):
                    if idx > 0 and oi2["key"] == oi["key"]:
                        extra_opex_keys.append(oi2["key"])
        # Driver offset mapping for extra opex — use sga slot (shared)
        for ek in extra_opex_keys:
            dk = f"{ek}_pct_rev"
            if dk not in DRIVER_KEY_TO_ASSUMP_OFFSET:
                DRIVER_KEY_TO_ASSUMP_OFFSET[dk] = 2  # sga_pct_rev slot

    else:
        # Fallback: current archetype-based structure
        if has_cogs:
            rows += [
                _sec("COST OF REVENUES"),
                _li("cogs", "  Cost of Revenue"),
                _st("gross_profit", "Gross Profit"),
                _drv("Gross Margin %", "gross_margin_pct",
                     hist_numer="gross_profit", hist_denom="revenue"),
                _sp(),
            ]

        rows.append(_sec("OPERATING EXPENSES"))

        if has_rd:
            rows += [
                _li("rd", "  Research & Development"),
                _drv("R&D % of Revenue", "rd_pct_rev",
                     hist_numer="rd", hist_denom="revenue"),
            ]

        if has_sga:
            rows += [
                _li("sga", "  Selling, General & Administrative"),
                _drv("SG&A % of Revenue", "sga_pct_rev",
                     hist_numer="sga", hist_denom="revenue"),
            ]

    # ── EBIT / EBITDA / Non-op / Per share — common tail ────────────────
    rows += [
        _sp(),
        _st("ebit", "Operating Income (EBIT)"),
        _mo("ebit_margin", "  EBIT Margin %", "ebit", "revenue"),
        _sp(),
        _li("da", "  (+) Depreciation & Amortization", bold=False),
        _drv("D&A % of Revenue", "da_pct_rev",
             hist_numer="da", hist_denom="revenue"),
        _st("ebitda", "EBITDA"),
        _mo("ebitda_margin", "  EBITDA Margin %", "ebitda", "revenue"),
        _sp(),
        _sec("OTHER INCOME / EXPENSE"),
        _li("interest_expense", "  Interest Expense"),
        _drv("Interest Rate %", "interest_rate_pct",
             hist_numer="", hist_denom=""),
        _li("interest_income", "  Interest Income"),
        _st("ebt", "EBT"),
        _sp(),
        _li("income_tax", "  Income Tax"),
        _drv("Effective Tax Rate %", "tax_rate_pct",
             hist_numer="income_tax", hist_denom="ebt"),
        _st("net_income", "Net Income"),
        _mo("net_margin", "  Net Margin %", "net_income", "revenue"),
        _li("nci_income_loss", "  Less: Net Income to NCI"),
        _st("ni_common", "Net Income to Common"),
        _mo("ni_common_margin", "  Net Margin % (to Common)", "ni_common", "revenue"),
        _sp(),
        _sec("PER SHARE DATA"),
        _li("eps_diluted", "  EPS — Diluted"),
        _li("eps_basic", "  EPS — Basic"),
        _li("shares_diluted", "  Shares — Diluted (wtd avg)"),
        _li("shares_basic", "  Shares — Basic (wtd avg)"),
    ]
    return rows


# ─────────────────────────────────────────────────────────────────────────────
# Utility IS (NEE, Duke, Dominion, etc.)
# Slot repurposing: gross_margin_pct→O&M%, sga_pct_rev→taxes_other%,
#                   rd_pct_rev→other_opex%
# ─────────────────────────────────────────────────────────────────────────────

def _build_utility_is() -> list[ISRow]:
    return [
        _li("revenue", "Operating Revenues", bold=True),
        _drv("Revenue Growth %", "revenue_growth_pct",
             hist_numer="__growth", hist_denom="revenue"),
        _sp(),
        _sec("OPERATING EXPENSES"),
        _li("utility_om", "  Operation & Maintenance"),
        _drv("O&M % of Revenue", "gross_margin_pct",
             hist_numer="utility_om", hist_denom="revenue"),
        _li("da", "  Depreciation & Amortization"),
        _drv("D&A % of Revenue", "da_pct_rev",
             hist_numer="da", hist_denom="revenue"),
        _li("utility_taxes_other", "  Taxes other than income taxes"),
        _drv("Taxes other % of Revenue", "sga_pct_rev",
             hist_numer="utility_taxes_other", hist_denom="revenue"),
        _li("utility_other", "  Other operating expenses"),
        _drv("Other OpEx % of Revenue", "rd_pct_rev",
             hist_numer="utility_other", hist_denom="revenue"),
        _st("utility_total_opex", "Total Operating Expenses"),
        _sp(),
        _st("ebit", "Operating Income (EBIT)"),
        _mo("ebit_margin", "  EBIT Margin %", "ebit", "revenue"),
        _sp(),
        _st("ebitda", "EBITDA  (EBIT + D&A)"),
        _mo("ebitda_margin", "  EBITDA Margin %", "ebitda", "revenue"),
        _sp(),
        _sec("OTHER INCOME / EXPENSE"),
        _li("interest_expense", "  Interest Expense"),
        _drv("Interest Rate %", "interest_rate_pct",
             hist_numer="", hist_denom=""),
        _li("interest_income", "  Interest Income"),
        _st("ebt", "EBT"),
        _sp(),
        _li("income_tax", "  Income Tax"),
        _drv("Effective Tax Rate %", "tax_rate_pct",
             hist_numer="income_tax", hist_denom="ebt"),
        _st("net_income", "Net Income"),
        _mo("net_margin", "  Net Margin %", "net_income", "revenue"),
        _li("nci_income_loss", "  Less: Net Income to NCI"),
        _st("ni_common", "Net Income to Common"),
        _mo("ni_common_margin", "  Net Margin % (to Common)", "ni_common", "revenue"),
        _sp(),
        _sec("PER SHARE DATA"),
        _li("eps_diluted", "  EPS — Diluted"),
        _li("eps_basic", "  EPS — Basic"),
        _li("shares_diluted", "  Shares — Diluted (wtd avg)"),
        _li("shares_basic", "  Shares — Basic (wtd avg)"),
    ]


# ─────────────────────────────────────────────────────────────────────────────
# Bank IS (JPM, BAC, WFC, etc.) — per SPEC_methodology §7.1
#
# Bank P&L structure: Interest Income + Non-Interest Income = Total Revenue.
# Net Interest Income = Interest Income − Interest Expense.
# Pre-Tax Income = NII + Non-Interest Income − Non-Interest Expense − Provision.
#
# Slot mapping (reuses standard driver slots with bank semantics):
#   revenue_growth_pct → Interest Income Growth %
#   gross_margin_pct    → Net Interest Margin % (NII / Interest Income)
#   sga_pct_rev         → Efficiency Ratio (Non-Interest Exp / Revenue)
#   rd_pct_rev          → Credit Cost % (Provision / Revenue)
# ─────────────────────────────────────────────────────────────────────────────

def _build_bank_is() -> list[ISRow]:
    return [
        _sec("INTEREST INCOME"),
        _li("revenue", "Interest & Fee Income", bold=True),
        _drv("Interest Income Growth %", "revenue_growth_pct",
             hist_numer="__growth", hist_denom="revenue"),
        _sp(),
        _sec("INTEREST EXPENSE"),
        _li("cogs", "  Interest Expense"),
        _st("gross_profit", "Net Interest Income"),
        _drv("Net Interest Margin (NIM) %", "gross_margin_pct",
             hist_numer="gross_profit", hist_denom="revenue"),
        _sp(),
        _sec("NON-INTEREST INCOME / EXPENSE"),
        _li("sga", "  Non-Interest Expense"),
        _drv("Efficiency Ratio % of Revenue", "sga_pct_rev",
             hist_numer="sga", hist_denom="revenue"),
        _li("rd", "  Provision for Credit Losses"),
        _drv("Credit Cost % of Revenue", "rd_pct_rev",
             hist_numer="rd", hist_denom="revenue"),
        _sp(),
        _st("ebit", "Pre-Tax, Pre-Provision Income"),
        _mo("ebit_margin", "  PTPP Margin %", "ebit", "revenue"),
        _sp(),
        _li("da", "  (+) D&A"),
        _drv("D&A % of Revenue", "da_pct_rev",
             hist_numer="da", hist_denom="revenue"),
        _st("ebitda", "Pre-Tax, Pre-Provision Income + D&A"),
        _mo("ebitda_margin", "  EBITDA Margin %", "ebitda", "revenue"),
        _sp(),
        _sec("BELOW THE LINE"),
        _li("interest_income", "  Other Interest Income"),
        _li("interest_expense", "  Long-Term Debt Interest"),
        _drv("Long-Term Debt Interest Rate %", "interest_rate_pct",
             hist_numer="", hist_denom=""),
        _st("ebt", "Pre-Tax Income"),
        _sp(),
        _li("income_tax", "  Income Tax"),
        _drv("Effective Tax Rate %", "tax_rate_pct",
             hist_numer="income_tax", hist_denom="ebt"),
        _st("net_income", "Net Income"),
        _mo("net_margin", "  Net Margin %", "net_income", "revenue"),
        _li("nci_income_loss", "  Less: Net Income to NCI"),
        _st("ni_common", "Net Income to Common"),
        _mo("ni_common_margin", "  Net Margin % (to Common)", "ni_common", "revenue"),
        _sp(),
        _sec("PER SHARE DATA"),
        _li("eps_diluted", "  EPS — Diluted"),
        _li("eps_basic", "  EPS — Basic"),
        _li("shares_diluted", "  Shares — Diluted (wtd avg)"),
        _li("shares_basic", "  Shares — Basic (wtd avg)"),
    ]


# ─────────────────────────────────────────────────────────────────────────────
# Insurance IS — per SPEC_methodology §7.2
#
# Insurance P&L: Premiums Earned + Net Investment Income = Total Revenue.
# Benefits / Losses + LAE + Acquisition costs = Total Benefits & Expenses.
# Underwriting Income = Premiums − Benefits − Opex.
# Combined Ratio = (Benefits + Opex) / Premiums.
#
# Slot mapping:
#   revenue_growth_pct → Premium Growth %
#   gross_margin_pct    → Combined Ratio % (inverted: 1-CR x Prem = UW income)
#   sga_pct_rev         → G&A % of Premiums
#   rd_pct_rev          → Acquisition Cost % of Premiums
# ─────────────────────────────────────────────────────────────────────────────

def _build_insurance_is() -> list[ISRow]:
    return [
        _sec("REVENUES"),
        _li("revenue", "Premiums Earned", bold=True),
        _drv("Premium Growth %", "revenue_growth_pct",
             hist_numer="__growth", hist_denom="revenue"),
        _sp(),
        _sec("BENEFITS & EXPENSES"),
        _li("cogs", "  Benefits / Losses & LAE Incurred"),
        _li("rd", "  Acquisition & Underwriting Expenses"),
        _drv("Acquisition Cost % of Premiums", "rd_pct_rev",
             hist_numer="rd", hist_denom="revenue"),
        _li("sga", "  General & Administrative Expenses"),
        _drv("G&A % of Premiums", "sga_pct_rev",
             hist_numer="sga", hist_denom="revenue"),
        _st("gross_profit", "Total Benefits & Expenses"),
        _drv("Combined Ratio %", "gross_margin_pct",
             hist_numer="gross_profit", hist_denom="revenue"),
        _sp(),
        _st("ebit", "Underwriting Income"),
        _mo("ebit_margin", "  Underwriting Margin %", "ebit", "revenue"),
        _sp(),
        _li("da", "  (+) D&A"),
        _drv("D&A % of Premiums", "da_pct_rev",
             hist_numer="da", hist_denom="revenue"),
        _st("ebitda", "EBITDA (adj.)"),
        _mo("ebitda_margin", "  EBITDA Margin %", "ebitda", "revenue"),
        _sp(),
        _sec("NON-UNDERWRITING INCOME / EXPENSE"),
        _li("interest_income", "  Net Investment Income"),
        _li("interest_expense", "  Interest Expense"),
        _drv("Interest Rate %", "interest_rate_pct",
             hist_numer="", hist_denom=""),
        _st("ebt", "Pre-Tax Income"),
        _sp(),
        _li("income_tax", "  Income Tax"),
        _drv("Effective Tax Rate %", "tax_rate_pct",
             hist_numer="income_tax", hist_denom="ebt"),
        _st("net_income", "Net Income"),
        _mo("net_margin", "  Net Margin %", "net_income", "revenue"),
        _li("nci_income_loss", "  Less: Net Income to NCI"),
        _st("ni_common", "Net Income to Common"),
        _mo("ni_common_margin", "  Net Margin % (to Common)", "ni_common", "revenue"),
        _sp(),
        _sec("PER SHARE DATA"),
        _li("eps_diluted", "  EPS — Diluted"),
        _li("eps_basic", "  EPS — Basic"),
        _li("shares_diluted", "  Shares — Diluted (wtd avg)"),
        _li("shares_basic", "  Shares — Basic (wtd avg)"),
    ]


# ─────────────────────────────────────────────────────────────────────────────
# REIT IS — per SPEC_methodology §7.3
#
# REIT P&L: Rental Revenue − Property OpEx = Net Operating Income (NOI).
# G&A and D&A below NOI. FFO = Net Income + D&A. AFFO = FFO − Maint CapEx.
#
# Slot mapping:
#   revenue_growth_pct → Rental Revenue Growth %
#   gross_margin_pct    → NOI Margin % (NOI / Revenue)
#   sga_pct_rev         → G&A % of Revenue
#   rd_pct_rev          → Other OpEx % of Revenue
# ─────────────────────────────────────────────────────────────────────────────

def _build_reit_is() -> list[ISRow]:
    return [
        _sec("REVENUES"),
        _li("revenue", "Rental & Property Revenue", bold=True),
        _drv("Revenue Growth %", "revenue_growth_pct",
             hist_numer="__growth", hist_denom="revenue"),
        _sp(),
        _sec("PROPERTY OPERATING EXPENSES"),
        _li("cogs", "  Property Operating Expenses"),
        _st("gross_profit", "Net Operating Income (NOI)"),
        _drv("NOI Margin %", "gross_margin_pct",
             hist_numer="gross_profit", hist_denom="revenue"),
        _sp(),
        _sec("CORPORATE EXPENSES"),
        _li("sga", "  General & Administrative"),
        _drv("G&A % of Revenue", "sga_pct_rev",
             hist_numer="sga", hist_denom="revenue"),
        _li("rd", "  Other Operating Expenses"),
        _drv("Other OpEx % of Revenue", "rd_pct_rev",
             hist_numer="rd", hist_denom="revenue"),
        _sp(),
        _li("da", "  Depreciation & Amortization"),
        _drv("D&A % of Revenue", "da_pct_rev",
             hist_numer="da", hist_denom="revenue"),
        _st("ebit", "Operating Income (EBIT)"),
        _mo("ebit_margin", "  EBIT Margin %", "ebit", "revenue"),
        _sp(),
        _st("ebitda", "EBITDA"),
        _mo("ebitda_margin", "  EBITDA Margin %", "ebitda", "revenue"),
        _sp(),
        _sec("FINANCING COSTS"),
        _li("interest_expense", "  Interest Expense"),
        _drv("Interest Rate %", "interest_rate_pct",
             hist_numer="", hist_denom=""),
        _li("interest_income", "  Interest Income"),
        _st("ebt", "EBT"),
        _sp(),
        _li("income_tax", "  Income Tax"),
        _drv("Effective Tax Rate %", "tax_rate_pct",
             hist_numer="income_tax", hist_denom="ebt"),
        _st("net_income", "Net Income"),
        _mo("net_margin", "  Net Margin %", "net_income", "revenue"),
        _li("nci_income_loss", "  Less: Net Income to NCI"),
        _st("ni_common", "Net Income to Common"),
        _mo("ni_common_margin", "  Net Margin % (to Common)", "ni_common", "revenue"),
        _sp(),
        _sec("FFO / AFFO  (supplemental REIT metrics)"),
        _li("ffo", "  FFO  (Net Income + D&A)", bold=True),
        _li("affo", "  AFFO  (FFO − Recurring CapEx, approx.)"),
        _sp(),
        _sec("PER SHARE DATA"),
        _li("eps_diluted", "  EPS — Diluted"),
        _li("eps_basic", "  EPS — Basic"),
        _li("shares_diluted", "  Shares — Diluted (wtd avg)"),
        _li("shares_basic", "  Shares — Basic (wtd avg)"),
    ]


# ─────────────────────────────────────────────────────────────────────────────
# Public API
# ─────────────────────────────────────────────────────────────────────────────

def _apply_filing_labels(rows: list[ISRow], filing_labels: dict[str, str]) -> list[ISRow]:
    """Override hardcoded IS labels with actual XBRL concept labels from filing."""
    if not filing_labels:
        return rows
    # Keys where the XBRL taxonomy label is worse than the hardcoded one
    _SKIP_LABEL_OVERRIDE = frozenset({"da", "ebitda", "ebit", "gross_profit", "net_income"})
    for isr in rows:
        if (isr.key and isr.key in filing_labels and isr.row_type == "line_item"
                and isr.key not in _SKIP_LABEL_OVERRIDE):
            xl = filing_labels[isr.key]
            ws = isr.label[:len(isr.label) - len(isr.label.lstrip())]
            isr.label = ws + xl
    return rows


def build_is_structure(
    sector: str,
    has_cogs: bool = True,
    has_rd: bool = True,
    has_sga: bool = True,
    revenue_segments: list[dict] | None = None,
    opex_items: list[dict] | None = None,
    filing_labels: dict[str, str] | None = None,
    cogs_detail: list[dict] | None = None,
) -> list[ISRow]:
    """Return the IS row list for the given sector and detected field flags.

    When revenue_segments is non-empty, inserts one ISRow per segment
    (with its own growth driver) before Total Revenue, and converts
    Total Revenue to a subtotal row (sum of segments).

    When opex_items is non-empty, builds COST OF REVENUES and OPERATING
    EXPENSES sections from the company's actual XBRL disclosure line items,
    with their XBRL concept labels. Falls back to archetype when empty.

    When filing_labels is provided, overrides hardcoded IS labels with
    the company's actual XBRL concept labels (Phase 4).
    """
    if revenue_segments:
        for seg in revenue_segments:
            dk = f"{seg['key']}_growth_pct"
            if dk not in DRIVER_KEY_TO_ASSUMP_OFFSET:
                DRIVER_KEY_TO_ASSUMP_OFFSET[dk] = 0

    if sector == "utility":
        rows = _build_utility_is()
    elif sector == "bank":
        rows = _build_bank_is()
    elif sector == "insurance":
        rows = _build_insurance_is()
    elif sector == "reit":
        rows = _build_reit_is()
    else:
        rows = _build_standard_is(
            has_cogs=has_cogs, has_rd=has_rd, has_sga=has_sga,
            revenue_segments=revenue_segments,
            opex_items=opex_items,
            cogs_detail=cogs_detail,
        )

    return _apply_filing_labels(rows, filing_labels or {})


def compute_is_row_map(rows: list[ISRow], start_row: int = IS_BODY_START) -> dict[str, int]:
    """Map each key → 0-based row number. Empty keys and duplicate keys map to first occurrence."""
    row_map: dict[str, int] = {}
    for i, isr in enumerate(rows):
        if isr.key and isr.key not in row_map:
            row_map[isr.key] = start_row + i
    return row_map
