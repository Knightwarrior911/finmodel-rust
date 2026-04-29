"""
Excel output writer for research agent results.
Follows valuation_kit formatting standards (SPEC_excel_formatting.md).

Outputs: IFRS bridge, EV bridge, company profile, earnings summary.
Each result gets a properly formatted .xlsx with IB-standard styling.
"""

import os
from datetime import datetime
from typing import Optional

import xlsxwriter
from xlsxwriter.utility import xl_col_to_name

# --- Layout constants (from SPEC_excel_formatting.md) ---
MARGIN_A = 0   # Col A: width 3
MARGIN_B = 1   # Col B: width 3
LABEL_COL = 2  # Col C: width 42
DATA_START = 3  # Col D+: width 13

# --- Colors (from SPEC) ---
INK = "#0F1632"
BRAND_BLUE = "#2558B3"
RED = "#FF3C28"
WHITE = "#FFFFFF"
LIGHT_GRAY = "#E6EBED"
MID_GRAY = "#D3DADD"
SAND = "#EAE0D3"
BLUE_INPUT = "#0000FF"
BLACK_FORMULA = "#000000"
GREEN_CROSS = "#008000"

# --- Number formats (from SPEC) ---
FMT_DOLLAR_M = '#,##0_);($#,##0);"-";@'
FMT_PLAIN = '#,##0_);(#,##0);"-";@'
FMT_PCT = '0.0%_);(0.0%);"-";@'
FMT_MULTIPLE = '0.0"x";(0.0"x");"-";@'
FMT_PRICE = '#,##0.00_);($#,##0.00);"-";@'

# --- Standard formats ---
FMT_TITLE_BAR = {
    "bg_color": BRAND_BLUE,
    "font_color": WHITE,
    "bold": True,
    "font_size": 16,
    "align": "left",
    "valign": "vcenter",
}
FMT_SUBHEADER = {
    "font_color": INK,
    "bold": True,
    "font_size": 11,
}
FMT_UNITS = {
    "font_color": INK,
    "italic": True,
    "font_size": 10,
}
FMT_SECTION_HEADER = {
    "bg_color": SAND,
    "font_color": INK,
    "bold": True,
    "font_size": 11,
    "bottom": 1,
}
FMT_LABEL = {
    "font_color": INK,
    "font_size": 10,
    "indent": 0,
}
FMT_LABEL_INDENT = {
    "font_color": INK,
    "font_size": 10,
    "indent": 1,
}
FMT_LABEL_SOURCE = {
    "font_color": "#666666",
    "font_size": 9,
    "italic": True,
    "indent": 1,
}
FMT_DATA = {
    "font_color": INK,
    "font_size": 10,
    "align": "right",
    "num_format": FMT_DOLLAR_M,
}
FMT_DATA_BOLD = {
    "font_color": INK,
    "font_size": 10,
    "bold": True,
    "align": "right",
    "num_format": FMT_DOLLAR_M,
    "top": 1,
    "bottom": 1,
}
FMT_TOTAL = {
    "font_color": INK,
    "font_size": 10,
    "bold": True,
    "align": "right",
    "num_format": FMT_DOLLAR_M,
    "top": 1,
    "bottom": 6,
}


class ResearchExcelWriter:
    """Writes research results to IB-standard formatted Excel files."""

    def __init__(self, output_dir: str = None):
        self.output_dir = output_dir or os.path.join(
            os.path.dirname(__file__), "..", "..", "models"
        )
        os.makedirs(self.output_dir, exist_ok=True)
        self.workbook: Optional[xlsxwriter.Workbook] = None
        self.formats: dict = {}
        self._row: int = 0   # Current row tracker

    def _c(self, row: int, col: int) -> str:
        return f"{xl_col_to_name(col)}{row + 1}"

    def _setup_formats(self):
        """Create all named formats."""
        wb = self.workbook

        # Base formats
        self.formats["title_bar"] = wb.add_format(FMT_TITLE_BAR)
        self.formats["subheader"] = wb.add_format(FMT_SUBHEADER)
        self.formats["units"] = wb.add_format(FMT_UNITS)
        self.formats["section"] = wb.add_format(FMT_SECTION_HEADER)

        # Label formats
        self.formats["label"] = wb.add_format(FMT_LABEL)
        self.formats["label_indent"] = wb.add_format(FMT_LABEL_INDENT)
        self.formats["label_source"] = wb.add_format(FMT_LABEL_SOURCE)
        self.formats["label_bold"] = wb.add_format({**FMT_LABEL, "bold": True})

        # Data formats
        self.formats["data"] = wb.add_format(FMT_DATA)
        self.formats["data_bold"] = wb.add_format(FMT_DATA_BOLD)
        self.formats["data_total"] = wb.add_format(FMT_TOTAL)
        self.formats["data_multiples"] = wb.add_format({**FMT_DATA, "num_format": FMT_MULTIPLE})
        self.formats["data_pct"] = wb.add_format({**FMT_DATA, "num_format": FMT_PCT})

        # Divider
        self.formats["divider"] = wb.add_format({
            "bottom": 1, "bottom_color": MID_GRAY,
        })

    def _setup_sheet(self, sheet_name: str, title: str, units: str = "(USD $ in millions)"):
        """Standard sheet setup: columns, title bar, units row."""
        ws = self.workbook.add_worksheet(sheet_name)
        ws.hide_gridlines(2)

        # Column widths
        ws.set_column(MARGIN_A, MARGIN_A, 3)
        ws.set_column(MARGIN_B, MARGIN_B, 3)
        ws.set_column(LABEL_COL, LABEL_COL, 42)
        ws.set_column(DATA_START, DATA_START + 3, 13)

        # Row heights
        ws.set_row(0, 8)   # spacer
        ws.set_row(1, 8)   # spacer
        ws.set_row(3, 8)   # spacer

        # Title bar (row 3, 0-based = row 2)
        ws.merge_range(2, LABEL_COL, 2, DATA_START + 1, title, self.formats["title_bar"])
        ws.set_row(2, 24)

        # Units (row 6, 0-based = row 5)
        ws.write(5, LABEL_COL, units, self.formats["units"])

        # Row 7 spacer
        ws.set_row(6, 8)

        self._row = 7  # Content starts at row 8 (0-based)

    def _write_section(self, ws, label: str):
        """Write a section header."""
        ws.merge_range(self._row, LABEL_COL, self._row, DATA_START + 1,
                       label, self.formats["section"])
        self._row += 1

    def _write_line(self, ws, label: str, value=None,
                    source: str = "", indent: bool = False,
                    fmt_key: str = "data", label_fmt: str = "label",
                    is_total: bool = False):
        """Write a data line with optional value and source."""
        lf = self.formats["label_indent"] if indent else self.formats[label_fmt]
        ws.write(self._row, LABEL_COL, label, lf)

        if value is not None:
            df = self.formats[fmt_key]
            ws.write(self._row, DATA_START, value, df)

        if source:
            self._row += 1
            ws.write(self._row, LABEL_COL, f"  {source}", self.formats["label_source"])

        self._row += 1

    def _write_divider(self, ws):
        """Write a thin dividing line row."""
        for col in range(LABEL_COL, DATA_START + 2):
            ws.write(self._row, col, "", self.formats["divider"])
        self._row += 1

    # --- Public methods ---

    def write_ev_bridge(self, ev_input, filename: str = None) -> str:
        """Write Enterprise Value bridge to Excel."""
        from kb.ev_bridge import EVBridgeInput
        name = ev_input.company or "Company"
        fname = filename or f"{name.replace(' ', '_')}_EV_Bridge.xlsx"
        path = os.path.join(self.output_dir, fname)

        self.workbook = xlsxwriter.Workbook(path)
        self._setup_formats()
        ws = self._setup_worksheet("EV Bridge", f"{name} – Enterprise Value Bridge",
                                   f"({ev_input.currency} in millions)")

        # Equity Value section
        self._write_section(ws, "Equity Value")
        if ev_input.share_price and ev_input.shares_outstanding:
            self._write_line(ws, "Share Price", ev_input.share_price,
                           fmt_key="data", label_fmt="label")
            self._write_line(ws, "Shares Outstanding (wtd avg basic)",
                           ev_input.shares_outstanding, indent=True,
                           fmt_key="label", label_fmt="label_indent")
        mc = ev_input.computed_market_cap
        self._write_line(ws, "Market Cap (Equity Value)", mc,
                        is_total=True, fmt_key="data_bold")

        self._row += 1

        # EV Bridge
        self._write_section(ws, "Enterprise Value Bridge")
        self._write_line(ws, "Market Cap", mc, fmt_key="data")

        ev = mc or 0
        add_items = [
            (ev_input.total_debt, "Total Debt"),
            (ev_input.finance_leases, "Finance/Capital Lease Liabilities"),
            (ev_input.operating_leases, "Operating Lease Liabilities (R-016)"),
            (ev_input.underfunded_pension, "Underfunded Pension (R-015)"),
            (ev_input.minority_interest, "Minority Interest (NCI)"),
            (ev_input.preferred_stock, "Preferred Stock"),
        ]
        for val, label in add_items:
            if val and val > 0:
                self._write_line(ws, f"+  {label}", val, indent=True)
                ev += val

        sub_items = [
            (ev_input.cash, "Cash & Cash Equivalents"),
            (ev_input.short_term_investments, "Short-term Investments"),
            (ev_input.equity_investments, "Equity Method Investments (R-014)"),
            (ev_input.financial_investments, "Financial Investments"),
            (ev_input.assets_held_for_sale, "Assets Held for Sale"),
            (ev_input.discontinued_ops_assets, "Discontinued Ops Assets"),
            (ev_input.nol_dta, "NOL Deferred Tax Assets"),
        ]
        for val, label in sub_items:
            if val and val > 0:
                self._write_line(ws, f"-  {label}", val, indent=True)
                ev -= val

        self._write_divider(ws)
        self._write_line(ws, "Enterprise Value", ev, is_total=True, fmt_key="data_total")
        self._row += 1

        # Multiples
        if any([ev_input.ltm_revenue, ev_input.ltm_ebitda]):
            self._write_section(ws, "Valuation Multiples")
            if ev_input.ltm_revenue and ev_input.ltm_revenue > 0:
                self._write_line(ws, "EV / LTM Revenue",
                               ev / ev_input.ltm_revenue, fmt_key="data_multiples")
            if ev_input.ltm_ebitda and ev_input.ltm_ebitda > 0:
                self._write_line(ws, "EV / LTM EBITDA",
                               ev / ev_input.ltm_ebitda, fmt_key="data_multiples")
            if mc and ev_input.ltm_revenue and ev_input.ltm_revenue > 0:
                self._write_line(ws, "Market Cap / LTM Revenue",
                               mc / ev_input.ltm_revenue, fmt_key="data_multiples")
            self._row += 1

        # Rules
        self._write_section(ws, "Rules Applied")
        self._write_line(ws, "R-009 EV Bridge checklist", indent=True)
        self._write_line(ws, "R-014 Goodwill NOT subtracted", indent=True)
        self._write_line(ws, "R-015 Pension from notes section only", indent=True)
        self._write_line(ws, "R-016 Leases from ASC 842 / IFRS 16 note", indent=True)
        self._write_line(ws, "F-001 Shares = latest filing weighted avg basic", indent=True)

        # Footer
        self._row += 1
        ws.write(self._row, LABEL_COL, f"Generated: {datetime.now().strftime('%Y-%m-%d %H:%M')}",
                self.formats["label_source"])

        self.workbook.close()
        return path

    def write_ifrs_bridge(self, inputs, output, company: str, period: str,
                          revenue: float = 0, notes: dict = None,
                          filename: str = None) -> str:
        """Write IFRS 16 conversion bridge to Excel."""
        fname = filename or f"{company.replace(' ', '_')}_IFRS_Bridge.xlsx"
        path = os.path.join(self.output_dir, fname)

        self.workbook = xlsxwriter.Workbook(path)
        self._setup_formats()

        direction = "IFRS 16 to US GAAP" if output.direction.name == "IFRS_TO_US_GAAP" else "US GAAP to IFRS 16"
        ws = self._setup_worksheet("IFRS Bridge", f"{company} {period} – {direction}",
                                   "(in thousands)")

        # EBITDA Bridge
        self._write_section(ws, "EBITDA Bridge")
        self._write_line(ws, "Reported EBITDA",
                       inputs.reported_ebitda, fmt_key="data")

        notes = notes or {}
        arrow = "−" if output.direction.name == "IFRS_TO_US_GAAP" else "+"

        self._write_line(ws, f"{arrow}  ROU Depreciation",
                       inputs.rou_depreciation, indent=True)
        self._write_line(ws, "", source=notes.get("rou_depr", "Lease note"))

        self._write_line(ws, f"{arrow}  Lease Interest",
                       inputs.lease_interest, indent=True)
        self._write_line(ws, "", source=notes.get("lease_int", "Finance expense note"))

        self._write_divider(ws)
        self._write_line(ws, "Adjusted EBITDA", output.adjusted_ebitda,
                        is_total=True, fmt_key="data_total")

        if revenue > 0:
            self._write_line(ws, "  Reported EBITDA Margin",
                           inputs.reported_ebitda / revenue if revenue else 0,
                           indent=True, fmt_key="data_pct")
            self._write_line(ws, "  Adjusted EBITDA Margin",
                           output.adjusted_ebitda / revenue if revenue else 0,
                           indent=True, fmt_key="data_pct")
            self._write_line(ws, f"EBITDA Delta: {output.ebitda_delta:+,.0f}")

        self._row += 1

        # EBIT Bridge
        self._write_section(ws, "EBIT Bridge")
        self._write_line(ws, "Reported EBIT", inputs.reported_ebit, fmt_key="data")
        self._write_line(ws, f"{arrow}  Lease Interest", inputs.lease_interest, indent=True)
        self._write_divider(ws)
        self._write_line(ws, "Adjusted EBIT", output.adjusted_ebit,
                        is_total=True, fmt_key="data_total")
        self._row += 1

        # Excluded Items
        self._write_section(ws, "Items Excluded from Adjustment")
        for item in (output.items_excluded or []):
            self._write_line(ws, f"X  {item}", indent=True)
        if inputs.short_term_rent > 0:
            self._write_line(ws,
                f"X  Short-term rent: {inputs.short_term_rent:,.0f} (already OPEX in both)",
                indent=True)

        # Footer
        self._row += 1
        ws.write(self._row, LABEL_COL, f"Generated: {datetime.now().strftime('%Y-%m-%d %H:%M')}",
                self.formats["label_source"])

        self.workbook.close()
        return path

    def write_company_profile(self, company_name: str, data: dict,
                              filename: str = None) -> str:
        """Write company profile / research summary to Excel."""
        fname = filename or f"{company_name.replace(' ', '_')}_Profile.xlsx"
        path = os.path.join(self.output_dir, fname)

        self.workbook = xlsxwriter.Workbook(path)
        self._setup_formats()
        ws = self._setup_worksheet("Profile", f"{company_name} – Company Profile",
                                   "(USD $ in millions unless noted)")

        # Key facts
        self._write_section(ws, "Key Facts")
        for label, value in data.items():
            if isinstance(value, (int, float)):
                self._write_line(ws, label, value)
            else:
                self._write_line(ws, label, fmt_key="label")

        # Footer
        self._row += 2
        ws.write(self._row, LABEL_COL, f"Generated: {datetime.now().strftime('%Y-%m-%d %H:%M')}",
                self.formats["label_source"])

        self.workbook.close()
        return path

    def _setup_worksheet(self, sheet_name: str, title: str, units: str):
        """Internal: create sheet with standard layout. Returns worksheet."""
        ws = self.workbook.add_worksheet(sheet_name)
        ws.hide_gridlines(2)

        ws.set_column(MARGIN_A, MARGIN_A, 3)
        ws.set_column(MARGIN_B, MARGIN_B, 3)
        ws.set_column(LABEL_COL, LABEL_COL, 42)
        ws.set_column(DATA_START, DATA_START + 3, 13)

        ws.set_row(0, 8)
        ws.set_row(1, 8)
        ws.set_row(3, 8)

        ws.merge_range(2, LABEL_COL, 2, DATA_START + 1, title, self.formats["title_bar"])
        ws.set_row(2, 24)

        ws.write(5, LABEL_COL, units, self.formats["units"])
        ws.set_row(6, 8)

        self._row = 7
        return ws
