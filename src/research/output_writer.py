"""
Excel output writer for research agent results.
Follows valuation_kit formatting standards:
  SPEC_excel_formatting.md         — layout, colors, number formats
  SPEC_spreadsheet_engineering.md  — formula colors, cell comments, formulas over hardcodes
  SPEC_excel_layout_decisions.md   — ad-hoc layout decision tree (AdHocExcelWriter)

Key rules:
- Hardcoded numbers → Blue #0000FF + cell comment citing source
- Same-sheet formulas → Black #000000 (no comment needed)
- Cross-sheet formulas → Green #008000
- Every blue cell MUST have a comment with source citation
- Computed values ALWAYS use Excel formulas, never pre-calculated numbers

Writers exposed:
- ResearchExcelWriter — fixed-template writers (write_ifrs_bridge, write_company_profile, ...)
- AdHocExcelWriter    — ad-hoc research output, picks layout per SPEC_excel_layout_decisions
"""

import os
import statistics
from dataclasses import dataclass, field
from datetime import datetime
from typing import Any, Optional

import xlsxwriter
from xlsxwriter.utility import xl_col_to_name, xl_rowcol_to_cell

# --- Layout (0-based cols) ---
MARGIN_A = 0
MARGIN_B = 1
LABEL_COL = 2   # width 42
DATA_START = 3   # Col D, width 13
SOURCE_COL = 4   # Col E, width 18 — clickable PDF hyperlink for each hardcoded input

# --- Colors from SPEC ---
INK        = "#0F1632"
BRAND_BLUE = "#2558B3"
WHITE      = "#FFFFFF"
SAND       = "#EAE0D3"
LIGHT_GRAY = "#E6EBED"
MID_GRAY   = "#D3DADD"

# SPEC formula colors
BLUE_INPUT  = "#0000FF"   # hardcoded number
BLACK_FORM  = "#000000"   # same-sheet formula
GREEN_CROSS = "#008000"   # cross-sheet formula

# --- Number formats from SPEC ---
NF_DOLLAR   = '#,##0_);($#,##0);"-";@'
NF_PLAIN    = '#,##0_);(#,##0);"-";@'
NF_PCT      = '0.0%_);(0.0%);"-";@'
NF_MULT     = '0.0"x";(0.0"x");"-";@'
NF_PRICE    = '#,##0.00_);($#,##0.00);"-";@'
NF_SHARES   = '#,##0.0,,;"-"'

# --- xlsxwriter comment width (approx chars) ---
COMMENT_W = 360
COMMENT_H = 80


def _c(row: int, col: int) -> str:
    """0-based (row, col) -> Excel address, e.g. D8."""
    return xl_rowcol_to_cell(row, col)


class ResearchExcelWriter:
    """Writes research results to IB-standard formatted Excel files."""

    def __init__(self, output_dir: str = None):
        self.output_dir = output_dir or os.path.join(
            os.path.dirname(__file__), "..", "..", "models"
        )
        os.makedirs(self.output_dir, exist_ok=True)
        self.wb: Optional[xlsxwriter.Workbook] = None
        self.ws = None
        self.fmt: dict = {}
        self._row: int = 0
        self._source_pdf: str = ""   # local filing PDF for page-accurate links
        self._source_doc = None      # lazily-opened fitz doc

    def _audit_url(self, value) -> str:
        """If a local source PDF is set, return a finmodelaudit page link for
        `value` (page-accurate when locatable, else doc-level). Else "".

        Lets bridge inputs open the filing at the exact page via the same handler
        as the main model, instead of a plain whole-document URL.
        """
        pdf = self._source_pdf
        if not pdf or not os.path.exists(pdf) or value in (None, 0):
            return ""
        try:
            import fitz
            from ..audit_open import build_uri
            from ..provenance import locate_value_in_pdf
            if self._source_doc is None:
                self._source_doc = fitz.open(pdf)
            page_idx, _bbox, _raw = locate_value_in_pdf(self._source_doc, value)
            page = (page_idx + 1) if page_idx is not None else None
            return build_uri(pdf, page)
        except Exception:
            return ""

    # ── format builders ──────────────────────────────────────────────

    def _build_formats(self):
        """Create all named formats. wb must be set."""
        wb = self.wb

        def mk(**kw): return wb.add_format(kw)

        # Title & structure
        self.fmt["title"] = mk(font_color=WHITE, bold=True, font_size=16,
                               bg_color=BRAND_BLUE, align="left", valign="vcenter")
        self.fmt["section"] = mk(font_color=INK, bold=True, font_size=11,
                                 bg_color=SAND, align="left", bottom=1)
        self.fmt["sub"] = mk(font_color=INK, bold=True, font_size=11)
        self.fmt["units"] = mk(font_color="#595959", italic=True, font_size=10, align="left")

        # Labels
        self.fmt["label"]     = mk(font_color=INK, font_size=10, align="left")
        self.fmt["label_b"]   = mk(font_color=INK, font_size=10, align="left", bold=True)
        self.fmt["label_i"]   = mk(font_color=INK, font_size=10, align="left", indent=1)
        self.fmt["src"]       = mk(font_color="#595959", font_size=9, align="left", italic=True)

        # Hardcoded inputs (BLUE #0000FF per SPEC)
        self.fmt["hc"]        = mk(font_color=BLUE_INPUT, font_size=10, align="right", num_format=NF_DOLLAR)
        self.fmt["hc_plain"]  = mk(font_color=BLUE_INPUT, font_size=10, align="right", num_format=NF_PLAIN)
        self.fmt["hc_price"]  = mk(font_color=BLUE_INPUT, font_size=10, align="right", num_format=NF_PRICE)
        self.fmt["hc_pct"]    = mk(font_color=BLUE_INPUT, font_size=10, align="right", num_format=NF_PCT)
        self.fmt["hc_shares"] = mk(font_color=BLUE_INPUT, font_size=10, align="right", num_format=NF_SHARES)

        # Same-sheet formulas (BLACK #000000 per SPEC)
        self.fmt["fm"]        = mk(font_color=BLACK_FORM, font_size=10, align="right", num_format=NF_DOLLAR)
        self.fmt["fm_plain"]  = mk(font_color=BLACK_FORM, font_size=10, align="right", num_format=NF_PLAIN)
        self.fmt["fm_mult"]   = mk(font_color=BLACK_FORM, font_size=10, align="right", num_format=NF_MULT)
        self.fmt["fm_pct"]    = mk(font_color=BLACK_FORM, font_size=10, align="right", num_format=NF_PCT)
        self.fmt["fm_bold"]   = mk(font_color=BLACK_FORM, font_size=10, align="right", num_format=NF_DOLLAR, bold=True, top=1, bottom=1)

        # Divider
        self.fmt["div"] = mk(bottom=1, bottom_color=MID_GRAY)

        # Footer
        self.fmt["footer"] = mk(font_color="#999999", font_size=8, align="left")

        # Source URL hyperlink (Col E — audit trail)
        self.fmt["source_link"] = mk(font_color=BRAND_BLUE, underline=True, font_size=9, align="left")

    # ── sheet setup ───────────────────────────────────────────────────

    def _setup(self, name: str, title: str, units: str, data_cols: int = 2):
        """Create sheet with standard layout. Returns ws + sets self._row to content start."""
        self.ws = self.wb.add_worksheet(name)
        self.ws.hide_gridlines(2)

        # Column widths
        self.ws.set_column(MARGIN_A, MARGIN_A, 3)
        self.ws.set_column(MARGIN_B, MARGIN_B, 3)
        self.ws.set_column(LABEL_COL, LABEL_COL, 42)
        for c in range(DATA_START, DATA_START + data_cols):
            self.ws.set_column(c, c, 13)
        self.ws.set_column(SOURCE_COL, SOURCE_COL, 18)  # audit trail hyperlinks

        # Row 0-1: spacers
        self.ws.set_row(0, 8)
        self.ws.set_row(1, 8)

        # Row 2: title bar
        self.ws.merge_range(2, LABEL_COL, 2, DATA_START + data_cols - 1,
                           title, self.fmt["title"])
        self.ws.set_row(2, 24)

        # Row 3: spacer
        self.ws.set_row(3, 8)

        # Row 5: units
        self.ws.write(5, LABEL_COL, units, self.fmt["units"])

        # Row 6: spacer
        self.ws.set_row(6, 8)

        self._row = 7  # Content starts row 8 (0-based)

    # ── write helpers ─────────────────────────────────────────────────

    def _section(self, label: str):
        """Write a Sand-background section header."""
        self.ws.merge_range(self._row, LABEL_COL, self._row, DATA_START + 1,
                           f" {label}", self.fmt["section"])
        self._row += 1

    def _input(self, label: str, value: float, fmt_key: str = "hc",
               comment: str = "", indent: bool = False, url: str = ""):
        """
        Write a hardcoded INPUT cell (BLUE font). REQUIRES a source comment.
        Per SPEC: every blue cell must have a non-empty comment.
        url: if provided, writes a clickable hyperlink in SOURCE_COL for full auditability.
        """
        lf = self.fmt["label_i"] if indent else self.fmt["label"]
        self.ws.write(self._row, LABEL_COL, label, lf)

        cell = _c(self._row, DATA_START)
        self.ws.write(self._row, DATA_START, value, self.fmt[fmt_key])

        if comment:
            self.ws.write_comment(self._row, DATA_START, comment,
                                 {"width": COMMENT_W, "height": COMMENT_H,
                                  "x_scale": 2, "y_scale": 2})

        link = self._audit_url(value) or url
        if link:
            self.ws.write_url(self._row, SOURCE_COL, link,
                              self.fmt["source_link"], "Source")
        self._row += 1

    def _formula(self, label: str, formula: str, fmt_key: str = "fm",
                 bold: bool = False, indent: bool = False):
        """
        Write a FORMULA cell (BLACK font). No comment needed.
        Formula is an Excel formula string like '=D8*D9'.
        """
        lf = self.fmt["label_i"] if indent else self.fmt["label"]
        if bold:
            lf = self.fmt["label_b"]
        self.ws.write(self._row, LABEL_COL, label, lf)
        self.ws.write_formula(self._row, DATA_START, formula, self.fmt[fmt_key])
        self._row += 1

    def _line(self, label: str, fmt_key: str = "label", indent: bool = False):
        """Write a text-only line (no data cell)."""
        lf = self.fmt["label_i"] if indent else self.fmt[fmt_key]
        self.ws.write(self._row, LABEL_COL, label, lf)
        self._row += 1

    def _source(self, text: str):
        """Write a gray italic source note on its own row."""
        self.ws.write(self._row, LABEL_COL, text, self.fmt["src"])
        self._row += 1

    def _divider(self):
        for c in range(LABEL_COL, DATA_START + 2):
            self.ws.write(self._row, c, "", self.fmt["div"])
        self._row += 1

    def _spacer(self, n: int = 1):
        self._row += n

    def _footer(self):
        self._spacer()
        ts = datetime.now().strftime('%Y-%m-%d %H:%M')
        self.ws.write(self._row, LABEL_COL,
                     f"Generated: {ts} | Source: SEC EDGAR / yfinance / Company filings",
                     self.fmt["footer"])

    # ── IFRS BRIDGE ──────────────────────────────────────────────────

    def write_ifrs_bridge(self, inputs, output, company: str, period: str,
                          revenue: float = 0, notes: dict = None,
                          pdf_url: str = "", filename: str = None,
                          source_pdf: str = "") -> str:
        """
        Write IFRS 16 conversion bridge to Excel.
        Hardcoded inputs (blue) for ROU Depr + Lease Int.
        Formula (black) for Pre-IFRS EBITDA = Reported - Depr - Int.
        Formula (black) for margins.

        source_pdf: optional local filing PDF; when set, blue inputs link to the
        exact page via finmodelaudit: (else they fall back to pdf_url).
        """
        notes = notes or {}
        self._source_pdf = source_pdf
        self._source_doc = None
        fname = filename or f"{company.replace(' ', '_')}_IFRS_Bridge.xlsx"
        path = os.path.join(self.output_dir, fname)

        self.wb = xlsxwriter.Workbook(path)
        self._build_formats()

        direction = "IFRS 16 → US GAAP (Pre-IFRS)" if output.direction.name == "IFRS_TO_US_GAAP" else "US GAAP → IFRS 16 (Post-IFRS)"
        self._setup("IFRS Bridge", f"{company} {period} — {direction}",
                    "(in thousands unless noted)")

        arrow = "-" if output.direction.name == "IFRS_TO_US_GAAP" else "+"

        # === EBITDA DERIVATION ===
        self._section("EBITDA Derivation — Hierarchy: Adjusted > Reported > Computed")

        # EBIT from income statement (hardcoded input)
        r_ebit = self._row
        self._input("Reported EBIT (from income statement)", inputs.reported_ebit,
                    comment=f"Source: {notes.get('ebit_src', 'Annual report — income statement: Operating Result')}",
                    url=pdf_url)

        # D&A from income statement (hardcoded input)
        da = inputs.standard_depreciation + inputs.standard_amortization
        r_da = None
        if da > 0:
            r_da = self._row
            self._input("+  Depreciation & Amortisation", da,
                        comment=f"Source: {notes.get('da_src', 'Annual report — income statement: Depreciation & Amortisation')}",
                        url=pdf_url)

        self._divider()

        # Computed EBITDA = EBIT + D&A (BLACK FORMULA)
        r_ebitda_computed = self._row
        ebitda_formula = f"={_c(r_ebit,DATA_START)}+{_c(r_da,DATA_START)}" if r_da else f"={_c(r_ebit,DATA_START)}"
        self._formula("EBITDA (computed: EBIT + D&A)", ebitda_formula)

        # Check if Adjusted EBITDA was provided and differs from computed
        ebitda_computed_val = inputs.reported_ebit + da
        has_adjusted = (inputs.reported_ebitda > inputs.reported_ebit and
                        abs(inputs.reported_ebitda - ebitda_computed_val) > ebitda_computed_val * 0.01)

        r_start = r_ebitda_computed  # Which row has the starting EBITDA
        if has_adjusted:
            self._spacer()
            r_adj = self._row
            self._input("Adjusted EBITDA (company-reported, one-offs removed)",
                       inputs.reported_ebitda, fmt_key="hc",
                       comment=f"Source: {notes.get('ebitda_src', 'Annual report — one-off items removed')}",
                       url=pdf_url)
            r_diff = self._row
            diff_formula = f"={_c(r_adj,DATA_START)}-{_c(r_ebitda_computed,DATA_START)}"
            self._formula("  Difference (one-off items)", diff_formula, fmt_key="fm_plain", indent=True)
            self._line("  (Adjusted EBITDA NOT related to IFRS 16 — one-off items only)", indent=True)
            self._divider()
            r_start = r_adj  # Starting EBITDA = Adjusted EBITDA
            start_label = "Starting EBITDA (Adjusted, Post-IFRS)"
        else:
            self._line("  (EBITDA not separately reported; using computed EBIT + D&A)", indent=True)
            start_label = "Starting EBITDA (Post-IFRS)"

        self._spacer()

        # === IFRS 16 ADJUSTMENT ===
        self._section("IFRS 16 Adjustment")
        self._formula(start_label, f"={_c(r_start,DATA_START)}")

        # ROU Depreciation (hardcoded input)
        r_rou = self._row
        self._input(f"  {arrow} ROU Depreciation", inputs.rou_depreciation,
                    comment=f"Source: {notes.get('rou_depr', 'Lease note — depreciation of right-of-use assets')}",
                    url=pdf_url)

        # Lease Interest (hardcoded input)
        r_int = self._row
        self._input(f"  {arrow} Interest on Lease Liabilities", inputs.lease_interest,
                    comment=f"Source: {notes.get('lease_int', 'Finance expense note — interest on lease liabilities')}",
                    url=pdf_url)

        self._divider()

        # Pre-IFRS EBITDA (BLACK FORMULA from starting EBITDA)
        op = "-" if output.direction.name == "IFRS_TO_US_GAAP" else "+"
        adj_formula = f"={_c(r_start,DATA_START)}{op}{_c(r_rou,DATA_START)}{op}{_c(r_int,DATA_START)}"
        r_adj_ebitda = self._row
        self._formula("Pre-IFRS EBITDA", adj_formula, bold=True)

        # Revenue (for margins)
        r_rev = None
        if revenue > 0:
            r_rev = self._row
            self._input("Revenue", revenue,
                       comment=f"Source: {notes.get('revenue_src', 'Annual report — income statement: Revenue')}",
                       url=pdf_url)
            self._spacer()

        # Margins (using starting EBITDA)
        if revenue > 0 and r_rev is not None:
            self._formula("  Starting EBITDA Margin",
                         f"={_c(r_start,DATA_START)}/{_c(r_rev,DATA_START)}",
                         fmt_key="fm_pct", indent=True)
            self._formula("  Pre-IFRS EBITDA Margin",
                         f"={_c(r_adj_ebitda,DATA_START)}/{_c(r_rev,DATA_START)}",
                         fmt_key="fm_pct", indent=True)

        self._spacer()

        # === EBIT BRIDGE ===
        self._section("EBIT Bridge")
        r_ebit2 = self._row
        self._input("Reported EBIT (from income statement)", inputs.reported_ebit,
                    comment=f"Source: {notes.get('ebit_src', 'Annual report — income statement: Operating Result')}",
                    url=pdf_url)

        r_ebit_int = self._row
        self._input(f"  {arrow} Interest on Lease Liabilities", inputs.lease_interest,
                    comment=f"Source: {notes.get('lease_int', 'Finance expense note — interest on lease liabilities')}",
                    url=pdf_url)

        self._divider()
        ebit_formula = f"={_c(r_ebit2,DATA_START)}{op}{_c(r_ebit_int,DATA_START)}"
        self._formula("Adjusted EBIT", ebit_formula, bold=True)
        self._spacer()

        # === EBITA BRIDGE ===
        if inputs.reported_ebita and inputs.reported_ebita != inputs.reported_ebit:
            self._section("EBITA Bridge")
            r_ebita = self._row
            self._input("Reported EBITA (EBIT + amortisation of intangibles)", inputs.reported_ebita,
                        comment="Source: Annual report — EBITA (EBIT + amortisation of acquired intangibles)",
                        url=pdf_url)
            r_ebita_int = self._row
            self._input(f"  {arrow} Interest on Lease Liabilities", inputs.lease_interest,
                        comment=f"Source: {notes.get('lease_int', 'Finance expense note')}",
                        url=pdf_url)
            self._divider()
            ebita_formula = f"={_c(r_ebita,DATA_START)}{op}{_c(r_ebita_int,DATA_START)}"
            self._formula("Adjusted EBITA", ebita_formula, bold=True)
            if revenue > 0 and r_rev is not None:
                self._formula("  EBITA Margin (Post-IFRS)",
                              f"={_c(r_ebita,DATA_START)}/{_c(r_rev,DATA_START)}",
                              fmt_key="fm_pct", indent=True)
                self._formula("  EBITA Margin (Pre-IFRS)",
                              f"={_c(self._row - 1,DATA_START)}/{_c(r_rev,DATA_START)}",
                              fmt_key="fm_pct", indent=True)
            self._spacer()

        # === SOURCES ===
        if pdf_url:
            self._section("Sources")
            self._line(f"  Annual Report: {pdf_url}", indent=True)
            self._spacer()

        # === EXCLUDED ITEMS ===
        self._section("Items Excluded from Adjustment")
        for item in (output.items_excluded or []):
            self._line(f"  X  {item}", indent=True)
        if inputs.short_term_rent > 0:
            self._line(f"  X  Short-term rent: {inputs.short_term_rent:,.0f} (already OPEX in both frameworks)", indent=True)
        self._spacer()

        # === LEASE DATA REFERENCE ===
        self._section("Input Data Sources")
        if pdf_url:
            self._line(f"  Annual Report URL: {pdf_url}", indent=True)
        self._line(f"  ROU Depreciation: {inputs.rou_depreciation:,.0f}", indent=True)
        self._line(f"  Lease Interest: {inputs.lease_interest:,.0f}", indent=True)
        if inputs.short_term_rent > 0:
            self._line(f"  Short-term Rent: {inputs.short_term_rent:,.0f}", indent=True)

        self._footer()
        self.wb.close()
        return path

    # ── EV BRIDGE ────────────────────────────────────────────────────

    def write_ev_bridge(self, ev_input, filename: str = None) -> str:
        """
        Write Enterprise Value bridge to Excel.
        Hardcoded inputs (blue) for price, shares, debt, cash, etc.
        Formulas (black) for Market Cap, EV, multiples.
        """
        from kb.ev_bridge import EVBridgeInput
        self._source_pdf = ""   # EV bridge inputs are market data, not filing pages
        self._source_doc = None
        name = ev_input.company or "Company"
        fname = filename or f"{name.replace(' ', '_')}_EV_Bridge.xlsx"
        path = os.path.join(self.output_dir, fname)

        self.wb = xlsxwriter.Workbook(path)
        self._build_formats()

        curr = ev_input.currency
        units = f"({curr} in millions unless noted)"
        self._setup("EV Bridge", f"{name} – Enterprise Value Bridge", units)

        notes = ev_input.notes_ref or {}
        urls = getattr(ev_input, 'field_urls', {}) or {}
        mc = ev_input.computed_market_cap

        # Convert raw units to millions for display (header says "in millions")
        def _m(val):
            if val is None:
                return 0
            return val / 1_000_000

        # === EQUITY VALUE ===
        self._section("Equity Value")

        r_price = self._row
        self._input("Share Price", ev_input.share_price or 0, fmt_key="hc_price",
                    comment=f"Source: {notes.get('share_price', 'Primary exchange')}")

        r_shares = self._row
        self._input("Shares Outstanding (wtd avg basic)", ev_input.shares_outstanding or 0, fmt_key="hc_shares",
                    comment=f"Source: {notes.get('shares', 'Latest filing — weighted average basic shares (F-001)')}")

        self._divider()

        r_mc = self._row
        # Market cap in millions: price * shares / 1,000,000
        mc_formula = f"={_c(r_price,DATA_START)}*{_c(r_shares,DATA_START)}/1000000"
        self._formula("Market Cap (Equity Value)", mc_formula, bold=True)
        self._spacer()

        # === ENTERPRISE VALUE BRIDGE ===
        self._section("Enterprise Value Bridge")

        # Market Cap (reference to above)
        self._formula("Market Cap", f"={_c(r_mc,DATA_START)}")
        ev_row_start = self._row  # track for EV formula

        # ADD items (converted to millions)
        # Each entry: (value, label, comment_text, field_key_for_url)
        add_map = [
            (ev_input.total_debt, "Total Debt",
             notes.get('total_debt', 'Balance Sheet'), 'total_debt'),
            (ev_input.finance_leases, "Finance/Capital Lease Liabilities",
             notes.get('finance_leases', 'ASC 842 / IFRS 16 note'), 'finance_leases'),
            (ev_input.operating_leases, "Operating Lease Liabilities (R-016)",
             notes.get('operating_leases', 'ASC 842 / IFRS 16 lease footnote (R-016)'), 'operating_leases'),
            (ev_input.underfunded_pension, "Underfunded Pension (R-015)",
             notes.get('pension', 'Pension footnote ONLY — NOT balance sheet (R-015)'), 'underfunded_pension'),
            (ev_input.minority_interest, "Minority Interest (NCI)",
             notes.get('nci', 'Balance Sheet'), 'minority_interest'),
            (ev_input.preferred_stock, "Preferred Stock",
             notes.get('preferred', 'Balance Sheet'), 'preferred_stock'),
        ]

        add_rows = []
        for val, label, comment, url_key in add_map:
            if val and val > 0:
                r = self._row
                self._input(f"+  {label}", _m(val), comment=comment, url=urls.get(url_key, ''))
                add_rows.append(r)

        # SUBTRACT items (converted to millions)
        sub_map = [
            (ev_input.cash, "Cash & Cash Equivalents",
             notes.get('cash', 'Balance Sheet'), 'cash'),
            (ev_input.short_term_investments, "Short-term Investments",
             notes.get('st_inv', 'Balance Sheet'), 'short_term_investments'),
            (ev_input.equity_investments, "Equity Method Investments (R-014)",
             notes.get('equity_inv', 'Balance Sheet — non-operating (R-014)'), 'equity_investments'),
            (ev_input.financial_investments, "Financial Investments (non-operating)",
             notes.get('fin_inv', 'Balance Sheet'), 'financial_investments'),
            (ev_input.assets_held_for_sale, "Assets Held for Sale",
             notes.get('held_sale', 'Balance Sheet'), 'assets_held_for_sale'),
            (ev_input.discontinued_ops_assets, "Discontinued Ops Assets",
             notes.get('disc_ops', 'Balance Sheet'), 'discontinued_ops_assets'),
            (ev_input.nol_dta, "NOL Deferred Tax Assets",
             notes.get('nol', 'Balance Sheet'), 'nol_dta'),
        ]

        sub_rows = []
        for val, label, comment, url_key in sub_map:
            if val and val > 0:
                r = self._row
                self._input(f"-  {label}", _m(val), comment=comment, url=urls.get(url_key, ''))
                sub_rows.append(r)

        self._divider()

        # EV formula: MC + sum(adds) - sum(subs) (all in millions)
        terms = [_c(r_mc, DATA_START)]
        for r in add_rows:
            terms.append(f"+{_c(r, DATA_START)}")
        for r in sub_rows:
            terms.append(f"-{_c(r, DATA_START)}")
        ev_formula = "=" + "".join(terms)
        r_ev = self._row
        self._formula("Enterprise Value", ev_formula, bold=True)
        self._spacer()

        # === VALUATION MULTIPLES ===
        if any([ev_input.ltm_revenue, ev_input.ltm_ebitda, ev_input.ltm_ebit]):
            self._section("Valuation Multiples")

            # Revenue input (if available, in millions)
            r_rev = None
            if ev_input.ltm_revenue:
                r_rev = self._row
                self._input("LTM Revenue", _m(ev_input.ltm_revenue),
                           comment=f"Source: {notes.get('revenue', 'SEC EDGAR / Annual Report')}",
                           url=urls.get('ltm_revenue', ''))

            if ev_input.ltm_ebitda:
                r_ebitda = self._row
                self._input("LTM EBITDA", _m(ev_input.ltm_ebitda),
                           comment=f"Source: {notes.get('ebitda', 'yfinance / Company filing')}",
                           url=urls.get('ltm_ebitda', ''))

            self._spacer()

            if r_rev:
                self._formula("EV / LTM Revenue",
                             f"={_c(r_ev,DATA_START)}/{_c(r_rev,DATA_START)}",
                             fmt_key="fm_mult")
            if r_rev and mc:
                self._formula("Market Cap / LTM Revenue",
                             f"={_c(r_mc,DATA_START)}/{_c(r_rev,DATA_START)}",
                             fmt_key="fm_mult")
            if ev_input.ltm_ebitda:
                r_eb = r_rev + 1 if r_rev else self._row - 1
                self._formula("EV / LTM EBITDA",
                             f"={_c(r_ev,DATA_START)}/{_c(r_eb,DATA_START)}",
                             fmt_key="fm_mult")

        self._spacer()

        # === RULES APPLIED ===
        self._section("Rules Applied")
        rules = [
            "R-009  EV Bridge — checklist, not template",
            "R-014  Goodwill NOT subtracted from EV",
            "R-015  Pension sourced from NOTES section only (not BS XBRL tag)",
            "R-016  Operating leases from ASC 842 / IFRS 16 footnote",
            "F-001  Shares = latest filing weighted average basic",
        ]
        for rule in rules:
            self._line(f"  {rule}", indent=True)

        self._footer()
        self.wb.close()
        return path

    # ── COMPANY PROFILE ──────────────────────────────────────────────

    def write_company_profile(self, company_name: str, data: dict,
                              filename: str = None) -> str:
        """Write company profile to Excel."""
        fname = filename or f"{company_name.replace(' ', '_')}_Profile.xlsx"
        path = os.path.join(self.output_dir, fname)

        self.wb = xlsxwriter.Workbook(path)
        self._build_formats()
        self._setup("Profile", f"{company_name} – Company Profile",
                    "(USD in millions unless noted)")

        self._section("Company Overview")
        for label, value in data.items():
            if isinstance(value, (int, float)):
                comment = f"Source: {data.get('_source', 'yfinance / SEC EDGAR')}"
                self._input(label, value, comment=comment)
            else:
                self._line(f"{label}: {value}")

        self._footer()
        self.wb.close()
        return path


# ══════════════════════════════════════════════════════════════════════
# Ad-Hoc Excel Layout — per SPEC_excel_layout_decisions.md
# ══════════════════════════════════════════════════════════════════════

LAYOUT_WIDE        = "wide"         # Section 3.1 — one row per company
LAYOUT_LONG        = "long"         # Section 3.2 — one row per data point
LAYOUT_TIME_SERIES = "time_series"  # Section 3.3 — one row per period
LAYOUT_EVENT_LOG   = "event_log"    # Section 3.4 — one row per event
LAYOUT_DASHBOARD   = "dashboard"    # Section 3.5 — multi-tab

_GRAIN_TO_LAYOUT = {
    "company":    LAYOUT_WIDE,
    "data_point": LAYOUT_LONG,
    "period":     LAYOUT_TIME_SERIES,
    "event":      LAYOUT_EVENT_LOG,
    "mixed":      LAYOUT_DASHBOARD,
}


@dataclass
class LayoutDecision:
    """Output of pick_adhoc_layout() — encodes Q1-Q5 answers from Section 1."""
    layout: str
    multi_tab: bool
    freeze_first_col: bool
    use_autofilter: bool
    summary_stats: bool
    section_dividers: bool
    qualitative_handling: str
    rationale: list = field(default_factory=list)


def pick_adhoc_layout(
    *,
    grain: str,
    n_metrics: int,
    n_entities: int,
    qualitative_max_chars: int = 0,
    needs_sort_filter: bool = False,
    is_comparative: bool = False,
) -> LayoutDecision:
    """
    Apply Section 1 decision tree to pick a layout.
    Returns LayoutDecision with rationale strings explaining each Q.
    """
    rationale = []
    layout = _GRAIN_TO_LAYOUT.get(grain, LAYOUT_LONG)
    rationale.append(f"Q1 grain={grain} -> {layout}")

    if n_metrics >= 20 and layout != LAYOUT_LONG:
        layout = LAYOUT_DASHBOARD
        rationale.append(f"Q2 metrics={n_metrics} >=20 -> escalate to multi-tab")
    freeze_first_col = 9 <= n_metrics <= 20
    if freeze_first_col:
        rationale.append(f"Q2 metrics={n_metrics} in [9,20] -> freeze first col")

    if qualitative_max_chars == 0:
        qual = "none"
    elif qualitative_max_chars < 50:
        qual = "in_cell"
    elif qualitative_max_chars < 200:
        qual = "in_comment"
    else:
        qual = "separate_column"
    rationale.append(f"Q3 max_text={qualitative_max_chars} -> {qual}")

    section_dividers = 15 < n_entities <= 50
    if n_entities > 50 and layout == LAYOUT_WIDE:
        layout = LAYOUT_DASHBOARD
        rationale.append(f"Q4 entities={n_entities} >50 -> split to multi-tab")

    use_autofilter = needs_sort_filter
    if use_autofilter:
        rationale.append("Q5 -> AutoFilter on")

    summary_stats = is_comparative and layout in (LAYOUT_WIDE, LAYOUT_TIME_SERIES)
    if summary_stats:
        rationale.append("S7 comparative -> summary stats row")

    return LayoutDecision(
        layout=layout,
        multi_tab=(layout == LAYOUT_DASHBOARD),
        freeze_first_col=freeze_first_col,
        use_autofilter=use_autofilter,
        summary_stats=summary_stats,
        section_dividers=section_dividers,
        qualitative_handling=qual,
        rationale=rationale,
    )


_KIND_TO_HC_FMT = {
    "number":   "hc_plain",
    "dollar":   "hc",
    "percent":  "hc_pct",
    "multiple": "hc_plain",
    "price":    "hc_price",
    "shares":   "hc_shares",
}
_KIND_TO_NF = {
    "number":   NF_PLAIN,
    "dollar":   NF_DOLLAR,
    "percent":  NF_PCT,
    "multiple": NF_MULT,
    "price":    NF_PRICE,
    "shares":   NF_SHARES,
}


@dataclass
class ColumnSpec:
    """One column in an ad-hoc Excel table."""
    key: str
    header: str
    kind: str = "text"        # "text"|"number"|"dollar"|"percent"|"multiple"|"price"|"shares"|"date"|"url"
    width: int = 13
    units: str = ""
    group: str = ""
    definition: str = ""
    is_label: bool = False


class AdHocExcelWriter(ResearchExcelWriter):
    """
    Writes ad-hoc research output to Excel by picking layout via SPEC_excel_layout_decisions.

    Usage:
        rows = [{"ticker": "AAPL", "revenue": 383285, "ebitda_margin": 0.32, ...}, ...]
        cols = [
            ColumnSpec("ticker", "Ticker", "text", width=10, is_label=True),
            ColumnSpec("revenue", "Revenue", "dollar", width=14, units="USD millions",
                       group="Financial Metrics"),
            ColumnSpec("ebitda_margin", "EBITDA Margin", "percent", width=12,
                       group="Financial Metrics"),
        ]
        path = AdHocExcelWriter().write_research(
            title="Industrials Peers - Margin Comparison",
            rows=rows, columns=cols,
            grain="company", is_comparative=True,
            sources={("AAPL", "revenue"): "AAPL 10-K FY2024 p.31"},
        )
    """

    def write_research(
        self,
        title: str,
        rows: list,
        columns: list,
        *,
        grain: str = "company",
        units: str = "",
        is_comparative: bool = False,
        needs_sort_filter: bool = True,
        sources: Optional[dict] = None,
        layout_override: Optional[str] = None,
        filename: Optional[str] = None,
    ) -> str:
        """
        Build a research Excel file. Layout chosen per SPEC_excel_layout_decisions.

        Args:
            rows: list[dict]; one per entity/data-point/period/event.
            columns: list[ColumnSpec] in display order. Exactly one with is_label=True.
            grain: what each row represents (Q1 of decision tree).
            sources: dict keyed by (row_label_value, column_key) -> citation string.
            layout_override: skip auto-pick and force a layout.
            filename: defaults to '{title}.xlsx' under self.output_dir.
        """
        if not rows:
            raise ValueError("rows must be non-empty")
        if sum(1 for c in columns if c.is_label) != 1:
            raise ValueError("exactly one ColumnSpec must have is_label=True")

        sources = sources or {}
        n_metrics = sum(1 for c in columns if not c.is_label)

        max_text = 0
        for r in rows:
            for c in columns:
                if c.kind == "text":
                    v = r.get(c.key)
                    if v is not None:
                        max_text = max(max_text, len(str(v)))

        decision = pick_adhoc_layout(
            grain=grain,
            n_metrics=n_metrics,
            n_entities=len(rows),
            qualitative_max_chars=max_text,
            needs_sort_filter=needs_sort_filter,
            is_comparative=is_comparative,
        )
        if layout_override:
            decision.layout = layout_override
            decision.rationale.append(f"override -> {layout_override}")

        # Multi-tab DASHBOARD not yet implemented — fall back
        if decision.layout == LAYOUT_DASHBOARD:
            decision.layout = LAYOUT_LONG if grain == "data_point" else LAYOUT_WIDE
            decision.rationale.append(
                "DASHBOARD not yet implemented - falling back to single-tab"
            )

        fname = filename or f"{title.replace(' ', '_').replace('/', '_')}.xlsx"
        path = os.path.join(self.output_dir, fname)
        self.wb = xlsxwriter.Workbook(path)
        self._build_formats()
        self._build_adhoc_formats()

        sheet_name = {
            LAYOUT_WIDE: "Comparison",
            LAYOUT_LONG: "Findings",
            LAYOUT_TIME_SERIES: "Time Series",
            LAYOUT_EVENT_LOG: "Events",
        }[decision.layout]
        self._setup_table(sheet_name, title,
                          units or self._default_units(decision), columns)
        self._render_table(rows, columns, decision, sources)
        self._render_decision_footer(decision)
        self.wb.close()
        return path

    def _build_adhoc_formats(self):
        """Add formats specific to ad-hoc layouts."""
        wb = self.wb
        def mk(**kw): return wb.add_format(kw)

        self.fmt["adhoc_header"] = mk(
            font_color=WHITE, bg_color=INK, bold=True, font_size=10,
            align="center", valign="vcenter", border=1, border_color=MID_GRAY,
        )
        self.fmt["adhoc_group"] = mk(
            font_color=INK, bg_color=SAND, bold=True, font_size=10,
            align="center", valign="vcenter", italic=True,
        )
        self.fmt["adhoc_label"] = mk(font_color=INK, font_size=10,
                                     align="left", valign="top")
        self.fmt["adhoc_text"]  = mk(font_color=INK, font_size=10,
                                     align="left", valign="top", text_wrap=True)
        self.fmt["adhoc_date"]  = mk(font_color=BLUE_INPUT, font_size=10,
                                     align="center", num_format="yyyy-mm-dd")
        self.fmt["adhoc_url"]   = mk(font_color=BRAND_BLUE, underline=True,
                                     font_size=9, align="left")
        self.fmt["adhoc_summary_lbl"] = mk(font_color=INK, bg_color=LIGHT_GRAY,
                                           bold=True, font_size=10, align="left",
                                           italic=True, top=1)

    @staticmethod
    def _default_units(decision: LayoutDecision) -> str:
        if decision.layout == LAYOUT_WIDE:
            return "(comparable peers - units per column header)"
        if decision.layout == LAYOUT_TIME_SERIES:
            return "(per-period values - units per column header)"
        if decision.layout == LAYOUT_EVENT_LOG:
            return "(events sorted by date - most recent first)"
        return "(research findings - one row per data point)"

    def _setup_table(self, sheet_name: str, title: str, units: str,
                     columns: list):
        self.ws = self.wb.add_worksheet(sheet_name)
        self.ws.hide_gridlines(2)
        self.ws.set_column(MARGIN_A, MARGIN_A, 3)
        self.ws.set_column(MARGIN_B, MARGIN_B, 3)
        for i, col in enumerate(columns):
            self.ws.set_column(LABEL_COL + i, LABEL_COL + i, col.width)
        last_col = LABEL_COL + len(columns) - 1
        self.ws.set_row(0, 8); self.ws.set_row(1, 8)
        self.ws.merge_range(2, LABEL_COL, 2, last_col, title, self.fmt["title"])
        self.ws.set_row(2, 24)
        self.ws.set_row(3, 8)
        self.ws.write(5, LABEL_COL, units, self.fmt["units"])
        self.ws.set_row(6, 8)
        self._row = 7

    def _render_table(self, rows: list, columns: list,
                      decision: LayoutDecision, sources: dict):
        if any(c.group for c in columns):
            self._render_group_banner(columns)

        header_row = self._row
        for i, col in enumerate(columns):
            self.ws.write(header_row, LABEL_COL + i, col.header,
                          self.fmt["adhoc_header"])
            comment_parts = []
            if col.definition:
                comment_parts.append(col.definition)
            if col.units:
                comment_parts.append(f"Units: {col.units}")
            if comment_parts:
                self.ws.write_comment(header_row, LABEL_COL + i,
                                      "\n".join(comment_parts),
                                      {"width": COMMENT_W, "height": COMMENT_H})
        self.ws.set_row(header_row, 22)
        self._row += 1
        data_start_row = self._row

        label_offset = next(i for i, c in enumerate(columns) if c.is_label)

        for r in rows:
            label_value = r.get(columns[label_offset].key)
            for i, col in enumerate(columns):
                v = r.get(col.key)
                src = sources.get((label_value, col.key))
                self._write_value_cell(self._row, LABEL_COL + i, v, col, src)
            self._row += 1
        data_end_row = self._row - 1

        if decision.use_autofilter and data_end_row >= data_start_row:
            self.ws.autofilter(header_row, LABEL_COL,
                               data_end_row, LABEL_COL + len(columns) - 1)
        if decision.freeze_first_col:
            self.ws.freeze_panes(data_start_row, LABEL_COL + 1)

        if decision.summary_stats and data_end_row >= data_start_row:
            self._render_summary_stats(columns, data_start_row,
                                       data_end_row, label_offset)

    def _render_group_banner(self, columns: list):
        row = self._row
        i = 0
        while i < len(columns):
            grp = columns[i].group
            j = i
            while j < len(columns) and columns[j].group == grp:
                j += 1
            if grp and (j - i) > 1:
                self.ws.merge_range(row, LABEL_COL + i, row, LABEL_COL + j - 1,
                                    grp.upper(), self.fmt["adhoc_group"])
            elif grp:
                self.ws.write(row, LABEL_COL + i, grp.upper(), self.fmt["adhoc_group"])
            i = j
        self._row += 1

    def _write_value_cell(self, row: int, col: int, value, spec: ColumnSpec,
                          source: Optional[str]):
        if value is None or (isinstance(value, str) and not value):
            self.ws.write_blank(row, col, None, self.fmt["adhoc_text"])
            return

        if spec.is_label:
            self.ws.write(row, col, value, self.fmt["adhoc_label"])
            return

        if spec.kind == "url":
            self.ws.write_url(row, col, str(value), self.fmt["adhoc_url"], "link")
            return

        if spec.kind == "date":
            self.ws.write(row, col, value, self.fmt["adhoc_date"])
            if source: self._add_source_comment(row, col, source)
            return

        if spec.kind == "text":
            self.ws.write(row, col, str(value), self.fmt["adhoc_text"])
            if source: self._add_source_comment(row, col, source)
            return

        fmt_key = _KIND_TO_HC_FMT.get(spec.kind, "hc_plain")
        self.ws.write(row, col, value, self.fmt[fmt_key])
        if source: self._add_source_comment(row, col, source)

    def _add_source_comment(self, row: int, col: int, source: str):
        self.ws.write_comment(row, col, f"Source: {source}",
                              {"width": COMMENT_W, "height": COMMENT_H})

    def _render_summary_stats(self, columns: list,
                              data_start: int, data_end: int, label_offset: int):
        self._row += 1
        for stat_label, fn in (("Median", "MEDIAN"), ("Mean", "AVERAGE"),
                               ("Min", "MIN"), ("Max", "MAX")):
            stat_row = self._row
            self.ws.write(stat_row, LABEL_COL + label_offset, stat_label,
                          self.fmt["adhoc_summary_lbl"])
            for i, col in enumerate(columns):
                if col.is_label or col.kind in ("text", "url", "date"):
                    continue
                cell_col = LABEL_COL + i
                rng = f"{_c(data_start, cell_col)}:{_c(data_end, cell_col)}"
                fmt = self.wb.add_format({
                    "font_color": INK, "bg_color": LIGHT_GRAY, "bold": True,
                    "font_size": 10, "align": "right", "top": 1,
                    "num_format": _KIND_TO_NF.get(col.kind, NF_PLAIN),
                })
                self.ws.write_formula(stat_row, cell_col, f"={fn}({rng})", fmt)
            self._row += 1

    def _render_decision_footer(self, decision: LayoutDecision):
        self._row += 2
        self.ws.write(self._row, LABEL_COL, f"Layout: {decision.layout}",
                      self.fmt["footer"])
        self._row += 1
        for line in decision.rationale:
            self.ws.write(self._row, LABEL_COL, f"  - {line}", self.fmt["footer"])
            self._row += 1
        self._footer()
