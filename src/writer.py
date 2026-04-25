import yaml
from pathlib import Path
import xlsxwriter
from schemas.financial_data import ModelOutput, VerificationReport

_BRANDING_PATH = Path(__file__).parent.parent / "config" / "branding.yaml"


def _load_branding() -> dict:
    with open(_BRANDING_PATH) as f:
        return yaml.safe_load(f)


class Formats:
    def __init__(self, wb: xlsxwriter.Workbook, b: dict):
        self.header_bar = wb.add_format({
            "bg_color": b["primary_color"], "font_color": b["white"],
            "bold": True, "font_size": b["font_size_title"],
            "font_name": b["font_display"], "valign": "vcenter",
        })
        self.subheader = wb.add_format({
            "font_color": b["ink_color"], "bold": True,
            "font_size": b["font_size_subheader"], "font_name": b["font_body"],
            "valign": "vcenter",
        })
        self.units_label = wb.add_format({
            "font_color": b["ink_color"], "italic": True,
            "font_size": b["font_size_body"], "font_name": b["font_body"],
        })
        self.year_header = wb.add_format({
            "font_color": b["primary_color"], "bold": True,
            "font_size": b["font_size_body"], "font_name": b["font_body"],
            "align": "center", "bottom": 1, "valign": "vcenter",
        })
        self.number = wb.add_format({
            "font_color": b["ink_color"], "font_size": b["font_size_body"],
            "font_name": b["font_body"], "num_format": "#,##0_);(#,##0);\"-\";@_)",
            "align": "right", "valign": "vcenter",
        })
        self.number_bold = wb.add_format({
            "font_color": b["ink_color"], "bold": True,
            "font_size": b["font_size_body"], "font_name": b["font_body"],
            "num_format": "#,##0_);(#,##0);\"-\";@_)",
            "align": "right", "valign": "vcenter", "top": 1,
        })
        self.number_total = wb.add_format({
            "bg_color": b["primary_color"], "font_color": b["white"], "bold": True,
            "font_size": b["font_size_body"], "font_name": b["font_body"],
            "num_format": "#,##0_);(#,##0);\"-\";@_)",
            "align": "right", "valign": "vcenter",
        })
        self.pct_italic = wb.add_format({
            "font_color": b["ink_color"], "italic": True,
            "font_size": b["font_size_body"], "font_name": b["font_body"],
            "num_format": "0.0%;(0.0%);\"-\";@_)",
            "align": "right", "valign": "vcenter",
        })
        self.label = wb.add_format({
            "font_color": b["ink_color"], "font_size": b["font_size_body"],
            "font_name": b["font_body"], "align": "left", "valign": "vcenter",
        })
        self.label_bold = wb.add_format({
            "font_color": b["ink_color"], "bold": True,
            "font_size": b["font_size_body"], "font_name": b["font_body"],
            "align": "left", "valign": "vcenter",
        })
        self.hardcode = wb.add_format({
            "font_color": b["blue_input"], "font_size": b["font_size_body"],
            "font_name": b["font_body"], "num_format": "#,##0_);(#,##0);\"-\";@_)",
            "align": "right", "valign": "vcenter",
        })
        self.pct_hardcode = wb.add_format({
            "font_color": b["blue_input"], "font_size": b["font_size_body"],
            "font_name": b["font_body"], "num_format": "0.0%;(0.0%);\"-\";@_)",
            "align": "right", "valign": "vcenter",
        })
        self.check_pass = wb.add_format({
            "font_color": "#006400", "bold": True, "font_size": b["font_size_body"],
            "font_name": b["font_body"], "align": "right",
        })
        self.check_fail = wb.add_format({
            "bg_color": b["alert_color"], "font_color": b["white"], "bold": True,
            "font_size": b["font_size_body"], "font_name": b["font_body"], "align": "right",
        })
        self.divider = wb.add_format({
            "bg_color": b["sand_color"], "font_color": b["ink_color"], "bold": True,
            "font_size": b["font_size_body"], "font_name": b["font_body"],
        })
        self.hist_divider_right = wb.add_format({
            "right": 2, "right_color": b["gray_03"],
            "font_color": b["ink_color"], "font_size": b["font_size_body"],
            "font_name": b["font_body"], "num_format": "#,##0_);(#,##0);\"-\";@_)",
            "align": "right", "valign": "vcenter",
        })
        self.sources_low_conf = wb.add_format({
            "bg_color": b["sand_color"], "font_size": b["font_size_body"],
            "font_name": b["font_condensed"], "valign": "vcenter",
        })
        self.sources_normal = wb.add_format({
            "font_size": b["font_size_body"], "font_name": b["font_condensed"],
            "valign": "vcenter",
        })


def _write_tab_header(ws, fmt: Formats, title: str, subtitle: str, currency: str):
    ws.set_row(0, 10)
    ws.set_row(1, 10)
    ws.set_row(2, 22)
    ws.set_row(3, 10)
    ws.set_row(4, 16)
    ws.set_row(5, 14)
    ws.set_row(6, 10)
    ws.write(2, 2, title, fmt.header_bar)
    ws.write(4, 2, subtitle, fmt.subheader)
    ws.write(5, 2, f"({currency} $ in millions)", fmt.units_label)
    ws.set_column(0, 0, 2)   # col A gutter
    ws.set_column(1, 1, 2)   # col B gutter


def _write_year_headers(ws, fmt: Formats, periods: list[str], hist_count: int,
                        start_col: int = 3, label_col: int = 2):
    ws.set_column(label_col, label_col, 32)
    for j, period in enumerate(periods):
        col = start_col + j
        ws.write(7, col, period, fmt.year_header)
        ws.set_column(col, col, 12)


class ExcelWriter:
    def __init__(self, output: ModelOutput, report: VerificationReport,
                 company_name: str, out_path: str, sources: dict | None = None):
        self.output = output
        self.report = report
        self.company_name = company_name
        self.out_path = out_path
        self.sources = sources or {}
        self.b = _load_branding()
        self.hist_count = sum(1 for p in output.periods if p.endswith("A"))

    def write(self):
        wb = xlsxwriter.Workbook(self.out_path)
        fmt = Formats(wb, self.b)

        tabs = [
            ("IS", self.b["gray_04"]),
            ("BS", self.b["gray_04"]),
            ("CF", self.b["gray_04"]),
            ("Assumptions", self.b["gray_04"]),
            ("Schedules", self.b["gray_03"]),
            ("Sources", self.b["gray_03"]),
        ]
        sheets = {}
        for name, color in tabs:
            ws = wb.add_worksheet(name)
            ws.set_tab_color(color)
            sheets[name] = ws

        try:
            self._write_is(wb, sheets["IS"], fmt)
            self._write_bs(wb, sheets["BS"], fmt)
            self._write_cf(wb, sheets["CF"], fmt)
            self._write_assumptions(wb, sheets["Assumptions"], fmt)
            self._write_schedules(wb, sheets["Schedules"], fmt)
            self._write_sources(wb, sheets["Sources"], fmt)
        finally:
            wb.close()

    def _vals(self, section: dict, key: str) -> list:
        return section.get(key, [None] * len(self.output.periods))

    def _write_is(self, wb, ws, fmt: Formats):
        o = self.output
        _write_tab_header(ws, fmt, self.company_name, "Income Statement", "USD")
        _write_year_headers(ws, fmt, o.periods, self.hist_count)

        START = 8
        LABEL_COL = 2
        DATA_START_COL = 3

        rev = self._vals(o.income_statement, "revenue")
        cogs = self._vals(o.income_statement, "cogs")
        gross = self._vals(o.income_statement, "gross_profit")
        sga = self._vals(o.income_statement, "sga")
        rd = self._vals(o.income_statement, "rd")
        da = self._vals(o.income_statement, "da")
        ebit = self._vals(o.income_statement, "ebit")
        int_exp = self._vals(o.income_statement, "interest_expense")
        int_inc = self._vals(o.income_statement, "interest_income")
        tax = self._vals(o.income_statement, "income_tax")
        ni = self._vals(o.income_statement, "net_income")
        eps_d = self._vals(o.income_statement, "eps_diluted")
        eps_b = self._vals(o.income_statement, "eps_basic")
        sh_d = self._vals(o.income_statement, "shares_diluted")
        sh_b = self._vals(o.income_statement, "shares_basic")

        def write_row(row, label, values, row_fmt, label_fmt=None):
            ws.write(row, LABEL_COL, label, label_fmt or fmt.label)
            for j, v in enumerate(values):
                col = DATA_START_COL + j
                f = fmt.hist_divider_right if j == self.hist_count - 1 else row_fmt
                ws.write(row, col, v, f)

        def write_pct_row(row, label, numerator, denominator):
            ws.write(row, LABEL_COL, label, fmt.label)
            for j in range(len(o.periods)):
                col = DATA_START_COL + j
                n_val = numerator[j] if numerator and j < len(numerator) else None
                d_val = denominator[j] if denominator and j < len(denominator) else None
                val = (n_val / d_val) if n_val is not None and d_val is not None and d_val != 0 else None
                f = fmt.hist_divider_right if j == self.hist_count - 1 else fmt.pct_italic
                ws.write(row, col, val, f)

        r = START
        write_row(r, "Revenue", rev, fmt.number, fmt.label_bold); r += 1
        write_pct_row(r, "  YoY Growth %", [
            ((rev[j] / rev[j-1] - 1) if j > 0 and rev[j-1] is not None and rev[j-1] != 0 else None) for j in range(len(rev))
        ], [1] * len(rev)); r += 1

        write_row(r, "Cost of Revenue", cogs, fmt.number); r += 1
        write_row(r, "Gross Profit", gross, fmt.number_bold, fmt.label_bold); r += 1
        write_pct_row(r, "  Gross Margin %", gross, rev); r += 1

        write_row(r, "SG&A", sga, fmt.number); r += 1
        write_row(r, "R&D", rd, fmt.number); r += 1
        ws.set_row(r, 5); r += 1  # spacer

        write_row(r, "EBITDA", [
            (e + d) if e is not None and d is not None else None
            for e, d in zip(ebit, da)
        ], fmt.number_bold, fmt.label_bold); r += 1
        write_pct_row(r, "  EBITDA Margin %", [
            (e + d) if e is not None and d is not None else None
            for e, d in zip(ebit, da)
        ], rev); r += 1

        write_row(r, "D&A", da, fmt.number); r += 1
        write_row(r, "EBIT", ebit, fmt.number_bold, fmt.label_bold); r += 1
        write_pct_row(r, "  EBIT Margin %", ebit, rev); r += 1
        ws.set_row(r, 5); r += 1

        write_row(r, "Interest Expense", int_exp, fmt.number); r += 1
        write_row(r, "Interest Income", int_inc, fmt.number); r += 1
        write_row(r, "EBT", [
            ((e or 0) - (ie or 0) + (ii or 0))
            for e, ie, ii in zip(ebit, int_exp, int_inc)
        ], fmt.number_bold, fmt.label_bold); r += 1
        write_row(r, "Income Tax", tax, fmt.number); r += 1
        write_pct_row(r, "  Effective Tax Rate %", tax, [
            ((e or 0) - (ie or 0) + (ii or 0))
            for e, ie, ii in zip(ebit, int_exp, int_inc)
        ]); r += 1
        ws.set_row(r, 5); r += 1

        write_row(r, "Net Income", ni, fmt.number_total, fmt.label_bold); r += 1
        write_pct_row(r, "  Net Margin %", ni, rev); r += 1
        ws.set_row(r, 5); r += 1

        write_row(r, "EPS — Diluted", eps_d, fmt.number); r += 1
        write_row(r, "EPS — Basic", eps_b, fmt.number); r += 1
        write_row(r, "Shares — Diluted (wtd avg)", sh_d, fmt.number); r += 1
        write_row(r, "Shares — Basic (wtd avg)", sh_b, fmt.number); r += 1

        ws.set_print_scale(90)
        ws.set_landscape()
        ws.fit_to_pages(1, 0)
        ws.set_footer("&L IS &R Page &P")

    def _write_bs(self, wb, ws, fmt: Formats):
        o = self.output
        _write_tab_header(ws, fmt, self.company_name, "Balance Sheet", "USD")
        _write_year_headers(ws, fmt, o.periods, self.hist_count)

        START = 8
        LABEL_COL = 2
        DATA_START_COL = 3

        def write_row(row, label, key, row_fmt, label_fmt=None):
            vals = self._vals(o.balance_sheet, key)
            ws.write(row, LABEL_COL, label, label_fmt or fmt.label)
            for j, v in enumerate(vals):
                col = DATA_START_COL + j
                f = fmt.hist_divider_right if j == self.hist_count - 1 else row_fmt
                ws.write(row, col, v, f)

        def write_check_row(row, label):
            ws.write(row, LABEL_COL, label, fmt.label_bold)
            assets = self._vals(o.balance_sheet, "total_assets")
            liab = self._vals(o.balance_sheet, "total_liabilities")
            equity = self._vals(o.balance_sheet, "total_equity")
            for j in range(len(o.periods)):
                col = DATA_START_COL + j
                a = assets[j] if j < len(assets) else 0
                le = (liab[j] if j < len(liab) else 0) + (equity[j] if j < len(equity) else 0)
                diff = (a or 0) - (le or 0)
                f = fmt.check_pass if abs(diff) <= 1 else fmt.check_fail
                ws.write(row, col, round(diff, 2), f)

        r = START
        ws.write(r, LABEL_COL, "ASSETS", fmt.divider); r += 1
        write_row(r, "  Cash & Equivalents", "cash", fmt.number); r += 1
        write_row(r, "  Accounts Receivable", "accounts_receivable", fmt.number); r += 1
        write_row(r, "  Inventory", "inventory", fmt.number); r += 1
        write_row(r, "  Total Current Assets", "total_current_assets", fmt.number_bold, fmt.label_bold); r += 1
        write_row(r, "  PP&E, net", "ppe_net", fmt.number); r += 1
        write_row(r, "  Goodwill", "goodwill", fmt.number); r += 1
        write_row(r, "  Intangibles, net", "intangibles_net", fmt.number); r += 1
        write_row(r, "Total Assets", "total_assets", fmt.number_total, fmt.label_bold); r += 1
        write_check_row(r, "  Check: Assets − (L+E)"); r += 2

        ws.write(r, LABEL_COL, "LIABILITIES & EQUITY", fmt.divider); r += 1
        write_row(r, "  Accounts Payable", "accounts_payable", fmt.number); r += 1
        write_row(r, "  Total Current Liabilities", "total_current_liabilities", fmt.number_bold, fmt.label_bold); r += 1
        write_row(r, "  Long-Term Debt", "long_term_debt", fmt.number); r += 1
        write_row(r, "Total Liabilities", "total_liabilities", fmt.number_bold, fmt.label_bold); r += 1
        write_row(r, "  Retained Earnings", "retained_earnings", fmt.number); r += 1
        write_row(r, "Total Equity", "total_equity", fmt.number_bold, fmt.label_bold); r += 1
        ws.write(r, LABEL_COL, "Total L+E", fmt.label_bold)
        liab_vals = self._vals(o.balance_sheet, "total_liabilities")
        equity_vals = self._vals(o.balance_sheet, "total_equity")
        for j in range(len(o.periods)):
            col = DATA_START_COL + j
            lv = liab_vals[j] if j < len(liab_vals) else 0
            ev = equity_vals[j] if j < len(equity_vals) else 0
            le_total = (lv or 0) + (ev or 0)
            f = fmt.hist_divider_right if j == self.hist_count - 1 else fmt.number_total
            ws.write(r, col, le_total, f)

        ws.set_landscape()
        ws.fit_to_pages(1, 0)
        ws.set_footer("&L BS &R Page &P")

    def _write_cf(self, wb, ws, fmt: Formats):
        o = self.output
        _write_tab_header(ws, fmt, self.company_name, "Cash Flow Statement", "USD")
        _write_year_headers(ws, fmt, o.periods, self.hist_count)

        START = 8
        LABEL_COL = 2
        DATA_START_COL = 3

        def write_row(row, label, key, section_dict, row_fmt, label_fmt=None):
            vals = self._vals(section_dict, key)
            ws.write(row, LABEL_COL, label, label_fmt or fmt.label)
            for j, v in enumerate(vals):
                col = DATA_START_COL + j
                f = fmt.hist_divider_right if j == self.hist_count - 1 else row_fmt
                ws.write(row, col, v, f)

        r = START
        ws.write(r, LABEL_COL, "OPERATING ACTIVITIES", fmt.divider); r += 1
        write_row(r, "  Net Income", "net_income", o.income_statement, fmt.number); r += 1
        write_row(r, "  D&A", "da", o.income_statement, fmt.number); r += 1
        write_row(r, "Cash from Operations", "cfo", o.cash_flow_statement, fmt.number_total, fmt.label_bold); r += 2

        ws.write(r, LABEL_COL, "INVESTING ACTIVITIES", fmt.divider); r += 1
        write_row(r, "  Capital Expenditures", "capex", o.cash_flow_statement, fmt.number); r += 1
        write_row(r, "Cash from Investing", "cfi", o.cash_flow_statement, fmt.number_total, fmt.label_bold); r += 2

        ws.write(r, LABEL_COL, "FINANCING ACTIVITIES", fmt.divider); r += 1
        write_row(r, "  Dividends Paid", "dividends_paid", o.cash_flow_statement, fmt.number); r += 1
        write_row(r, "  Share Buybacks", "buybacks", o.cash_flow_statement, fmt.number); r += 1
        write_row(r, "Cash from Financing", "cff", o.cash_flow_statement, fmt.number_total, fmt.label_bold); r += 2

        write_row(r, "Net Change in Cash", "net_change_cash", o.cash_flow_statement, fmt.number_bold, fmt.label_bold); r += 1
        ws.write(r, LABEL_COL, "Beginning Cash", fmt.label)
        bs_cash_beg = self._vals(o.balance_sheet, "cash")
        for j in range(len(o.periods)):
            col = DATA_START_COL + j
            beg = bs_cash_beg[j - 1] if j > 0 else None
            f = fmt.hist_divider_right if j == self.hist_count - 1 else fmt.number
            ws.write(r, col, beg, f)
        r += 1

        ws.write(r, LABEL_COL, "Ending Cash", fmt.label_bold)
        bs_cash = self._vals(o.balance_sheet, "cash")
        cfs_nc = self._vals(o.cash_flow_statement, "net_change_cash")
        for j in range(len(o.periods)):
            col = DATA_START_COL + j
            ending = bs_cash[j] if j < len(bs_cash) else None
            net_chg = cfs_nc[j] if j < len(cfs_nc) else None
            beg = bs_cash[j - 1] if j > 0 and j - 1 < len(bs_cash) else None
            computed = (beg + net_chg) if beg is not None and net_chg is not None else None
            passes = computed is None or ending is None or abs((computed or 0) - (ending or 0)) <= 1
            f = fmt.check_pass if passes else fmt.check_fail
            ws.write(r, col, ending, f)

        ws.set_landscape()
        ws.fit_to_pages(1, 0)
        ws.set_footer("&L CF &R Page &P")

    def _write_assumptions(self, wb, ws, fmt: Formats):
        o = self.output
        _write_tab_header(ws, fmt, self.company_name, "Projection Assumptions", "USD")

        proj_periods = [p for p in o.periods if p.endswith("E")]
        _write_year_headers(ws, fmt, proj_periods, 0)

        START = 8
        LABEL_COL = 2
        DATA_START_COL = 3

        assumption_rows = [
            ("Revenue Growth %", "revenue_growth_pct", "pct"),
            ("Gross Margin %", "gross_margin_pct", "pct"),
            ("SG&A % Revenue", "sga_pct_rev", "pct"),
            ("R&D % Revenue", "rd_pct_rev", "pct"),
            ("D&A % Revenue", "da_pct_rev", "pct"),
            ("CapEx % Revenue", "capex_pct_rev", "pct"),
            ("Effective Tax Rate %", "tax_rate_pct", "pct"),
            ("Interest Rate %", "interest_rate_pct", "pct"),
            ("DSO (days)", "dso_days", "num"),
            ("DPO (days)", "dpo_days", "num"),
            ("DIO (days)", "dio_days", "num"),
            ("Diluted Shares (000s)", "shares_diluted", "num"),
            ("Dividend per Share", "dividend_per_share", "num"),
        ]

        r = START
        for label, key, fmt_type in assumption_rows:
            val = o.assumptions.get(key)
            ws.write(r, LABEL_COL, label, fmt.label)
            for j in range(len(proj_periods)):
                col = DATA_START_COL + j
                f = fmt.pct_hardcode if fmt_type == "pct" else fmt.hardcode
                ws.write(r, col, val, f)
            r += 1

        ws.set_landscape()
        ws.fit_to_pages(1, 0)
        ws.set_footer("&L Assumptions &R Page &P")

    def _write_schedules(self, wb, ws, fmt: Formats):
        o = self.output
        _write_tab_header(ws, fmt, self.company_name, "Supporting Schedules", "USD")
        _write_year_headers(ws, fmt, o.periods, self.hist_count)

        START = 8
        LABEL_COL = 2
        DATA_START_COL = 3
        r = START

        ws.write(r, LABEL_COL, "PP&E ROLLFORWARD", fmt.divider); r += 1
        ppe_rows = [
            ("Opening PP&E", "opening"),
            ("CapEx", "capex"),
            ("D&A", "da"),
            ("Closing PP&E", "closing"),
        ]
        for label, field in ppe_rows:
            ws.write(r, LABEL_COL, f"  {label}", fmt.label_bold if field == "closing" else fmt.label)
            for j, sched in enumerate(o.schedules.get("ppe_rollforward", [])):
                col = DATA_START_COL + j
                base_f = fmt.number_bold if field == "closing" else fmt.number
                f = fmt.hist_divider_right if j == self.hist_count - 1 else base_f
                ws.write(r, col, sched.get(field), f)
            r += 1

        ws.set_landscape()
        ws.fit_to_pages(1, 0)
        ws.set_footer("&L Schedules &R Page &P")

    def _write_sources(self, wb, ws, fmt: Formats):
        o = self.output
        _write_tab_header(ws, fmt, self.company_name, "Sources & Audit Trail", "USD")

        headers = ["Line Item", "Period", "Value ($M)", "Filing", "XBRL Tag / Page", "Confidence", "Notes"]
        for j, h in enumerate(headers):
            ws.write(7, 2 + j, h, fmt.subheader)

        r = 8
        for item, cit_list in (self.sources or {}).items():
            if not isinstance(cit_list, list):
                continue
            for cit in cit_list:
                row_vals = [
                    item, "", "",
                    getattr(cit, "filing", ""),
                    getattr(cit, "xbrl_tag", "") or f"p.{getattr(cit, 'page', '')}",
                    getattr(cit, "confidence", ""),
                    ""
                ]
                low_conf = getattr(cit, "confidence", 1.0) < 0.75
                for j, v in enumerate(row_vals):
                    ws.write(r, 2 + j, v, fmt.sources_low_conf if low_conf else fmt.sources_normal)
                r += 1

        r += 2
        ws.write(r, 2, "VERIFICATION REPORT", fmt.divider); r += 1
        ws.write(r, 2, f"Status: {'PASSED' if self.report.passed else 'FAILED'}", fmt.label_bold); r += 1

        if self.report.critical_failures:
            ws.write(r, 2, "Critical Failures:", fmt.label_bold); r += 1
            for cf in self.report.critical_failures:
                ws.write(r, 3, cf, fmt.check_fail); r += 1

        if self.report.warnings:
            ws.write(r, 2, "Warnings:", fmt.label_bold); r += 1
            for w in self.report.warnings:
                ws.write(r, 3, w, fmt.label); r += 1

        if self.report.notes:
            ws.write(r, 2, "Notes:", fmt.label_bold); r += 1
            for n in self.report.notes:
                ws.write(r, 3, n, fmt.label); r += 1

        ws.set_landscape()
        ws.fit_to_pages(1, 0)
        ws.set_footer("&L Sources &R Page &P")
