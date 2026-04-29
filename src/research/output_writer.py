"""
Excel output writer for research agent results.
Follows valuation_kit formatting standards:
  SPEC_excel_formatting.md   — layout, colors, number formats
  SPEC_spreadsheet_engineering.md — formula colors, cell comments, formulas over hardcodes

Key rules:
- Hardcoded numbers → Blue #0000FF + cell comment citing source
- Same-sheet formulas → Black #000000 (no comment needed)
- Cross-sheet formulas → Green #008000
- Every blue cell MUST have a comment with source citation
- Computed values ALWAYS use Excel formulas, never pre-calculated numbers
"""

import os
from datetime import datetime
from typing import Optional

import xlsxwriter
from xlsxwriter.utility import xl_col_to_name, xl_rowcol_to_cell

# --- Layout (0-based cols) ---
MARGIN_A = 0
MARGIN_B = 1
LABEL_COL = 2   # width 42
DATA_START = 3   # Col D, width 13

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
               comment: str = "", indent: bool = False):
        """
        Write a hardcoded INPUT cell (BLUE font). REQUIRES a source comment.
        Per SPEC: every blue cell must have a non-empty comment.
        """
        lf = self.fmt["label_i"] if indent else self.fmt["label"]
        self.ws.write(self._row, LABEL_COL, label, lf)

        cell = _c(self._row, DATA_START)
        self.ws.write(self._row, DATA_START, value, self.fmt[fmt_key])

        if comment:
            self.ws.write_comment(self._row, DATA_START, comment,
                                 {"width": COMMENT_W, "height": COMMENT_H,
                                  "x_scale": 2, "y_scale": 2})
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
                          pdf_url: str = "", filename: str = None) -> str:
        """
        Write IFRS 16 conversion bridge to Excel.
        Hardcoded inputs (blue) for ROU Depr + Lease Int.
        Formula (black) for Pre-IFRS EBITDA = Reported - Depr - Int.
        Formula (black) for margins.
        """
        notes = notes or {}
        fname = filename or f"{company.replace(' ', '_')}_IFRS_Bridge.xlsx"
        path = os.path.join(self.output_dir, fname)

        self.wb = xlsxwriter.Workbook(path)
        self._build_formats()

        direction = "IFRS 16 → US GAAP (Pre-IFRS)" if output.direction.name == "IFRS_TO_US_GAAP" else "US GAAP → IFRS 16 (Post-IFRS)"
        self._setup("IFRS Bridge", f"{company} {period} — {direction}",
                    "(in thousands unless noted)")

        arrow = "-" if output.direction.name == "IFRS_TO_US_GAAP" else "+"

        # === EBITDA DERIVATION ===
        self._section("EBITDA Derivation")

        # EBIT from income statement (hardcoded input)
        r_ebit = self._row
        self._input("Reported EBIT (from income statement)", inputs.reported_ebit,
                    comment=f"Source: {notes.get('ebit_src', 'Annual report — income statement: Operating Result')}")

        # D&A from income statement (hardcoded input)
        da = inputs.standard_depreciation + inputs.standard_amortization
        r_da = None
        if da > 0:
            r_da = self._row
            self._input("+  Depreciation & Amortisation", da,
                        comment=f"Source: {notes.get('da_src', 'Annual report — income statement: Depreciation & Amortisation')}")

        self._divider()

        # EBITDA = EBIT + D&A (BLACK FORMULA)
        r_ebitda = self._row
        if r_da:
            ebitda_formula = f"={_c(r_ebit,DATA_START)}+{_c(r_da,DATA_START)}"
        else:
            ebitda_formula = f"={_c(r_ebit,DATA_START)}"
        self._formula("= Reported EBITDA (computed: EBIT + D&A)", ebitda_formula, bold=True)
        self._spacer()

        # === IFRS 16 ADJUSTMENT ===
        self._section("IFRS 16 Adjustment")

        # Reference EBITDA (formula)
        self._formula("Reported EBITDA (Post-IFRS)", f"={_c(r_ebitda,DATA_START)}")

        # ROU Depreciation (hardcoded input)
        r_rou = self._row
        self._input(f"  {arrow} ROU Depreciation", inputs.rou_depreciation,
                    comment=f"Source: {notes.get('rou_depr', 'Lease note — depreciation of right-of-use assets')}")

        # Lease Interest (hardcoded input)
        r_int = self._row
        self._input(f"  {arrow} Interest on Lease Liabilities", inputs.lease_interest,
                    comment=f"Source: {notes.get('lease_int', 'Finance expense note — interest on lease liabilities')}")

        self._divider()

        # Adjusted EBITDA (BLACK FORMULA)
        op = "-" if output.direction.name == "IFRS_TO_US_GAAP" else "+"
        adj_formula = f"={_c(r_ebitda,DATA_START)}{op}{_c(r_rou,DATA_START)}{op}{_c(r_int,DATA_START)}"
        r_adj_ebitda = self._row
        self._formula("Adjusted EBITDA (Pre-IFRS)", adj_formula, bold=True)

        # Revenue (for margins)
        r_rev = None
        if revenue > 0:
            r_rev = self._row
            self._input("Revenue", revenue,
                       comment=f"Source: {notes.get('revenue_src', 'Annual report — income statement: Revenue')}")
            self._spacer()

        # Margins
        if revenue > 0 and r_rev is not None:
            self._formula("  Reported EBITDA Margin",
                         f"={_c(r_ebitda,DATA_START)}/{_c(r_rev,DATA_START)}",
                         fmt_key="fm_pct", indent=True)
            self._formula("  Adjusted EBITDA Margin",
                         f"={_c(r_adj_ebitda,DATA_START)}/{_c(r_rev,DATA_START)}",
                         fmt_key="fm_pct", indent=True)

        self._spacer()

        # === EBIT BRIDGE ===
        self._section("EBIT Bridge")
        r_ebit2 = self._row
        self._input("Reported EBIT (from income statement)", inputs.reported_ebit,
                    comment=f"Source: {notes.get('ebit_src', 'Annual report — income statement: Operating Result')}")

        r_ebit_int = self._row
        self._input(f"  {arrow} Interest on Lease Liabilities", inputs.lease_interest,
                    comment=f"Source: {notes.get('lease_int', 'Finance expense note — interest on lease liabilities')}")

        self._divider()
        ebit_formula = f"={_c(r_ebit2,DATA_START)}{op}{_c(r_ebit_int,DATA_START)}"
        self._formula("Adjusted EBIT", ebit_formula, bold=True)
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
        name = ev_input.company or "Company"
        fname = filename or f"{name.replace(' ', '_')}_EV_Bridge.xlsx"
        path = os.path.join(self.output_dir, fname)

        self.wb = xlsxwriter.Workbook(path)
        self._build_formats()

        curr = ev_input.currency
        units = f"({curr} in millions unless noted)"
        self._setup("EV Bridge", f"{name} – Enterprise Value Bridge", units)

        notes = ev_input.notes_ref or {}
        mc = ev_input.computed_market_cap

        # === EQUITY VALUE ===
        self._section("Equity Value")

        r_price = self._row
        self._input("Share Price", ev_input.share_price or 0, fmt_key="hc_price",
                    comment=f"Source: {notes.get('share_price', 'Primary exchange')}")

        r_shares = self._row
        shares_display = ev_input.shares_outstanding
        self._input("Shares Outstanding (wtd avg basic)", shares_display, fmt_key="hc_shares",
                    comment=f"Source: {notes.get('shares', 'Latest filing — weighted average basic shares (F-001)')}")

        self._divider()

        r_mc = self._row
        mc_formula = f"={_c(r_price,DATA_START)}*{_c(r_shares,DATA_START)}"
        self._formula("Market Cap (Equity Value)", mc_formula, bold=True)
        self._spacer()

        # === ENTERPRISE VALUE BRIDGE ===
        self._section("Enterprise Value Bridge")

        # Market Cap (reference to above)
        self._formula("Market Cap", f"={_c(r_mc,DATA_START)}")
        ev_row_start = self._row  # track for EV formula

        # ADD items
        add_map = [
            (ev_input.total_debt, "Total Debt",
             notes.get('total_debt', 'Balance Sheet')),
            (ev_input.finance_leases, "Finance/Capital Lease Liabilities",
             notes.get('finance_leases', 'ASC 842 / IFRS 16 note')),
            (ev_input.operating_leases, "Operating Lease Liabilities (R-016)",
             notes.get('operating_leases', 'ASC 842 / IFRS 16 lease footnote (R-016)')),
            (ev_input.underfunded_pension, "Underfunded Pension (R-015)",
             notes.get('pension', 'Pension footnote ONLY — NOT balance sheet (R-015)')),
            (ev_input.minority_interest, "Minority Interest (NCI)",
             notes.get('nci', 'Balance Sheet')),
            (ev_input.preferred_stock, "Preferred Stock",
             notes.get('preferred', 'Balance Sheet')),
        ]

        add_rows = []
        for val, label, comment in add_map:
            if val and val > 0:
                r = self._row
                self._input(f"+  {label}", val, comment=comment)
                add_rows.append(r)

        # SUBTRACT items
        sub_map = [
            (ev_input.cash, "Cash & Cash Equivalents",
             notes.get('cash', 'Balance Sheet')),
            (ev_input.short_term_investments, "Short-term Investments",
             notes.get('st_inv', 'Balance Sheet')),
            (ev_input.equity_investments, "Equity Method Investments (R-014)",
             notes.get('equity_inv', 'Balance Sheet — non-operating (R-014)')),
            (ev_input.financial_investments, "Financial Investments (non-operating)",
             notes.get('fin_inv', 'Balance Sheet')),
            (ev_input.assets_held_for_sale, "Assets Held for Sale",
             notes.get('held_sale', 'Balance Sheet')),
            (ev_input.discontinued_ops_assets, "Discontinued Ops Assets",
             notes.get('disc_ops', 'Balance Sheet')),
            (ev_input.nol_dta, "NOL Deferred Tax Assets",
             notes.get('nol', 'Balance Sheet')),
        ]

        sub_rows = []
        for val, label, comment in sub_map:
            if val and val > 0:
                r = self._row
                self._input(f"-  {label}", val, comment=comment)
                sub_rows.append(r)

        self._divider()

        # EV formula: MC + sum(adds) - sum(subs)
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

            # Revenue input (if available)
            r_rev = None
            if ev_input.ltm_revenue:
                r_rev = self._row
                self._input("LTM Revenue", ev_input.ltm_revenue,
                           comment=f"Source: {notes.get('revenue', 'SEC EDGAR / Annual Report')}")

            if ev_input.ltm_ebitda:
                r_ebitda = self._row
                self._input("LTM EBITDA", ev_input.ltm_ebitda,
                           comment=f"Source: {notes.get('ebitda', 'yfinance / Company filing')}")

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
