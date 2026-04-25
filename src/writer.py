"""
Phase 1 Excel writer — Rogo-standard 3-statement financial model.

Color conventions:
  Blue  (#0070C0) = hardcoded input (user-editable)
  Black (#000000) = same-tab formula (do not edit directly)
  Green (#375623) = cross-tab link (do not edit directly)

Tab order: Cover → IS → BS → CF → Sources
IS structure: Revenue → COGS → GP → SGA → R&D → EBIT → D&A → EBITDA → interest → EBT → Tax → NI
Projection cells: Excel formulas driven by assumption driver block below IS
"""
from __future__ import annotations
import xlsxwriter
from xlsxwriter.utility import xl_col_to_name
from schemas.financial_data import ModelOutput, VerificationReport

# ─────────────────────────────────────────────────────────────────────────────
# Column layout (0-based)
# ─────────────────────────────────────────────────────────────────────────────
MARGIN = 0   # Col A — narrow gutter
LABEL  = 1   # Col B — row labels
DATA0  = 2   # Col C — period[0]


def _c(row: int, col: int) -> str:
    """0-based (row, col) → Excel address, e.g. 'C10'."""
    return f"{xl_col_to_name(col)}{row + 1}"


def _xr(sheet: str, row: int, col: int) -> str:
    """Cross-tab formula string, e.g. '=IS!C10'."""
    return f"={sheet}!{_c(row, col)}"


# ─────────────────────────────────────────────────────────────────────────────
# Row maps — 0-based row indices for each tab
# ─────────────────────────────────────────────────────────────────────────────
IS_R: dict[str, int] = {
    "title": 2, "subtitle": 4, "units": 5,
    "circ": 7,      # circ switch single cell
    "headers": 9,
    "revenue": 10,  "rev_growth": 11,
    "cogs": 12,
    "gross_profit": 13, "gross_margin": 14,
    "sga": 15, "rd": 16,
    # spacer 17
    "ebit": 18,     "ebit_margin": 19,
    # spacer 20
    "da": 21,
    "ebitda": 22,   "ebitda_margin": 23,
    # spacer 24
    "int_exp": 25,  "int_inc": 26,
    "ebt": 27,
    "tax": 28,      "tax_rate": 29,
    # spacer 30
    "net_income": 31, "net_margin": 32,
    # spacer 33
    "eps_diluted": 34, "eps_basic": 35,
    "shares_diluted": 36, "shares_basic": 37,
    # spacer 38
    "drv_header": 39,
    "drv_rev_g": 40, "drv_gm": 41,
    "drv_sga": 42,   "drv_rd": 43,
    "drv_da": 44,    "drv_tax": 45,
    "drv_int": 46,   "drv_shares": 47,
}

BS_R: dict[str, int] = {
    "title": 2, "subtitle": 4, "units": 5, "headers": 9,
    "assets_hdr": 10,
    "cash": 11, "ar": 12, "inventory": 13,
    "total_cur_assets": 14,
    "ppe_net": 15, "goodwill": 16, "intangibles": 17,
    "total_assets": 18,
    # spacer 19
    "le_hdr": 20,
    "ap": 21, "total_cur_liab": 22,
    "ltd": 23, "total_liab": 24,
    # spacer 25
    "rnci": 26,   # redeemable NCI / mezzanine equity
    # spacer 27
    "equity_hdr": 28,
    "retained_earnings": 29, "total_equity": 30,
    "total_le": 31,   # = total_liab + rnci + total_equity
    "bs_check": 32,
}

CF_R: dict[str, int] = {
    "title": 2, "subtitle": 4, "units": 5, "headers": 9,
    "cfo_hdr": 10,
    "ni": 11, "da": 12, "wc_net": 13, "other_cfo": 14,
    "cfo": 15,
    # spacer 16
    "cfi_hdr": 17,
    "capex": 18, "other_cfi": 19,
    "cfi": 20,
    # spacer 21
    "cff_hdr": 22,
    "dividends": 23, "buybacks": 24, "other_cff": 25,
    "cff": 26,
    # spacer 27
    "net_change": 28, "beg_cash": 29, "ending_cash": 30,
    "fcf": 31,
    # spacer 32
    "chk_ni": 33, "chk_cash": 34,
}


# ─────────────────────────────────────────────────────────────────────────────
# Format palette
# ─────────────────────────────────────────────────────────────────────────────
class _Fmt:
    BLUE  = "#0070C0"   # hardcoded input
    BLACK = "#000000"   # same-tab formula
    GREEN = "#375623"   # cross-tab link
    NAVY  = "#1F4E79"   # headers
    RED   = "#C00000"   # check failure
    LGRAY = "#D9E1F2"   # section divider bg
    SGRAY = "#808080"   # hist separator border

    _D  = "$#,##0.0_);($#,##0.0);\"-\""   # dollar — section totals only
    _N  = "#,##0.0_);(#,##0.0);\"-\""     # plain number
    _P  = "0.0%;(0.0%);\"-\""             # percentage (italic)
    _BL = "\"-\";;\"-\""                   # always shows "–" (for check rows)

    def __init__(self, wb: xlsxwriter.Workbook, font: str = "Calibri", sz: int = 10):
        def mk(**kw):
            return wb.add_format({"font_name": font, "font_size": sz,
                                   "valign": "vcenter", **kw})

        B, Bk, G, N = self.BLUE, self.BLACK, self.GREEN, self.NAVY
        D, P, Pl = self._D, self._P, self._N
        HS = {"right": 2, "right_color": self.SGRAY}   # hist | proj separator

        # ── labels ─────────────────────────────────────────────────────────
        self.lbl      = mk(font_color=Bk, align="left")
        self.lbl_b    = mk(font_color=Bk, align="left", bold=True)
        self.lbl_i    = mk(font_color="#595959", align="left", italic=True, indent=1)
        self.lbl_sec  = mk(font_color=Bk, bold=True, bg_color=self.LGRAY, align="left")
        self.lbl_drv  = mk(font_color="#595959", italic=True, align="left", indent=1)
        self.lbl_chk  = mk(font_color=Bk, align="left", italic=True)

        # ── tab header ─────────────────────────────────────────────────────
        self.hbar     = mk(font_color="#FFFFFF", bold=True, font_size=13,
                           bg_color=N, align="left", valign="vcenter")
        self.hsub     = mk(font_color=N, bold=True, align="left")
        self.hunit    = mk(font_color="#595959", italic=True, align="left")
        self.hcol     = mk(font_color=N, bold=True, align="center",
                           bottom=1, bottom_color=N)
        self.hcol_hs  = mk(font_color=N, bold=True, align="center",
                           bottom=1, bottom_color=N, **HS)

        # ── helpers to build format families ──────────────────────────────
        def hc(**kw):  return mk(font_color=B,  align="right", **kw)
        def nu(**kw):  return mk(font_color=Bk, align="right", **kw)
        def xt(**kw):  return mk(font_color=G,  align="right", **kw)

        # ── hardcoded input (blue) ──────────────────────────────────────────
        self.hc       = hc(num_format=Pl)
        self.hc_d     = hc(num_format=D)
        self.hc_p     = hc(num_format=P,  italic=True)
        self.hc_b     = hc(num_format=Pl, bold=True, top=2)
        self.hc_bd    = hc(num_format=D,  bold=True, top=2)
        self.hc_hs    = hc(num_format=Pl, **HS)
        self.hc_d_hs  = hc(num_format=D,  **HS)
        self.hc_p_hs  = hc(num_format=P,  italic=True, **HS)
        self.hc_b_hs  = hc(num_format=Pl, bold=True, top=2, **HS)
        self.hc_bd_hs = hc(num_format=D,  bold=True, top=2, **HS)

        # ── same-tab formula (black) ────────────────────────────────────────
        self.num      = nu(num_format=Pl)
        self.num_d    = nu(num_format=D)
        self.num_p    = nu(num_format=P,  italic=True)
        self.num_b    = nu(num_format=Pl, bold=True, top=2)
        self.num_bd   = nu(num_format=D,  bold=True, top=2)
        self.num_hs   = nu(num_format=Pl, **HS)
        self.num_d_hs = nu(num_format=D,  **HS)
        self.num_p_hs = nu(num_format=P,  italic=True, **HS)
        self.num_b_hs = nu(num_format=Pl, bold=True, top=2, **HS)
        self.num_bd_hs= nu(num_format=D,  bold=True, top=2, **HS)

        # ── cross-tab link (green) ──────────────────────────────────────────
        self.xt       = xt(num_format=Pl)
        self.xt_b     = xt(num_format=Pl, bold=True, top=2)
        self.xt_hs    = xt(num_format=Pl, **HS)
        self.xt_b_hs  = xt(num_format=Pl, bold=True, top=2, **HS)

        # ── driver block ───────────────────────────────────────────────────
        self.drv      = mk(font_color=B,  num_format=P,  italic=True, align="right")
        self.drv_num  = mk(font_color=B,  num_format=Pl, align="right")
        self.drv_imp  = mk(font_color=Bk, num_format=P,  italic=True, align="right")
        self.drv_imp_hs = mk(font_color=Bk, num_format=P, italic=True,
                              align="right", **HS)

        # ── validation checks ───────────────────────────────────────────────
        self.chk_ok   = mk(font_color=Bk, num_format=self._BL, align="right", italic=True)
        self.chk_fail = mk(font_color="#FFFFFF", num_format=Pl, align="right",
                           bold=True, bg_color=self.RED)

        # ── cover / sources ─────────────────────────────────────────────────
        self.cv_title = mk(font_size=18, bold=True, font_color=N, align="left")
        self.cv_sub   = mk(font_size=11, font_color=Bk, align="left")
        self.cv_lbl   = mk(font_size=10, bold=True, font_color=Bk, align="left")
        self.cv_blue  = mk(font_size=10, bold=True, font_color=B,  align="left")
        self.cv_black = mk(font_size=10, bold=True, font_color=Bk, align="left")
        self.cv_green = mk(font_size=10, bold=True, font_color=G,  align="left")
        self.src_hdr  = mk(font_color=Bk, bold=True)
        self.src_row  = mk(font_color=Bk)
        self.src_low  = mk(font_color=Bk, bg_color="#FFF2CC")


# ─────────────────────────────────────────────────────────────────────────────
# Main writer class
# ─────────────────────────────────────────────────────────────────────────────
class ExcelWriter:
    def __init__(
        self,
        output: ModelOutput,
        report: VerificationReport,
        company_name: str,
        out_path: str,
        sources: dict | None = None,
        currency: str = "USD",
    ) -> None:
        self.o    = output
        self.rpt  = report
        self.co   = company_name
        self.path = out_path
        self.srcs = sources or {}
        self.ccy  = currency
        self.n_h  = sum(1 for p in output.periods if p.endswith("A"))
        self.n_p  = sum(1 for p in output.periods if p.endswith("E"))
        self.n    = len(output.periods)

    # ── public ───────────────────────────────────────────────────────────────

    def write(self) -> None:
        wb = xlsxwriter.Workbook(self.path)
        wb.set_calc_mode("auto")
        fmt = _Fmt(wb)
        tabs = [
            ("Cover",   "#D9E1F2"),
            ("IS",      "#D9E1F2"),
            ("BS",      "#D9E1F2"),
            ("CF",      "#D9E1F2"),
            ("Sources", "#F2F2F2"),
        ]
        sheets = {}
        for name, color in tabs:
            sheets[name] = wb.add_worksheet(name)
            sheets[name].set_tab_color(color)
        try:
            self._write_cover(wb, sheets["Cover"], fmt)
            self._write_is(wb, sheets["IS"], fmt)
            self._write_bs(wb, sheets["BS"], fmt)
            self._write_cf(wb, sheets["CF"], fmt)
            self._write_sources(wb, sheets["Sources"], fmt)
        finally:
            wb.close()

    # ── internal helpers ─────────────────────────────────────────────────────

    def _col(self, j: int) -> int:
        return DATA0 + j

    def _cell(self, row: int, j: int) -> str:
        return _c(row, self._col(j))

    def _hs(self, j: int) -> bool:
        """True when j is the last historical period."""
        return j == self.n_h - 1

    def _hv(self, sec: dict, key: str) -> list:
        v = sec.get(key) or []
        return (list(v) + [None] * self.n)[:self.n_h]

    def _av(self, sec: dict, key: str) -> list:
        v = sec.get(key) or []
        return (list(v) + [None] * self.n)[:self.n]

    def _pv(self, sec: dict, key: str) -> list:
        return self._av(sec, key)[self.n_h:]

    # ── shared layout helpers ────────────────────────────────────────────────

    def _tab_header(self, ws, title: str, subtitle: str, fmt: _Fmt) -> None:
        ws.set_row(0, 4); ws.set_row(1, 4); ws.set_row(2, 26)
        ws.set_row(3, 4); ws.set_row(4, 18); ws.set_row(5, 14); ws.set_row(6, 8)
        ws.set_column(MARGIN, MARGIN, 2)
        ws.set_column(LABEL, LABEL, 33)
        ws.hide_gridlines(2)
        last_col = self._col(self.n - 1)
        ws.merge_range(2, LABEL, 2, last_col, title, fmt.hbar)
        ws.write(4, LABEL, subtitle, fmt.hsub)
        ws.write(5, LABEL, f"({self.ccy} $ in millions, unless noted)", fmt.hunit)

    def _col_headers(self, ws, row: int, fmt: _Fmt, col_w: float = 11.5) -> None:
        ws.set_row(row, 16)
        for j, period in enumerate(self.o.periods):
            col = self._col(j)
            ws.set_column(col, col, col_w)
            ws.write(row, col, period, fmt.hcol_hs if self._hs(j) else fmt.hcol)

    def _sp(self, ws, row: int, h: int = 5) -> None:
        ws.set_row(row, h)

    # ── helpers to write a value (blue hardcoded) ────────────────────────────

    def _hc(self, ws, row: int, j: int, val, f_normal, f_hs) -> None:
        col = self._col(j)
        f   = f_hs if self._hs(j) else f_normal
        if val is not None:
            ws.write(row, col, val, f)
        else:
            ws.write_blank(row, col, f)

    def _fmla(self, ws, row: int, j: int, fmla: str, f, cache=None) -> None:
        ws.write_formula(row, self._col(j), fmla, f, cache)

    # ── apply check conditional formatting ───────────────────────────────────

    def _apply_check_cf(self, wb, ws, row: int) -> None:
        fail_fmt = wb.add_format({"bg_color": _Fmt.RED, "font_color": "#FFFFFF",
                                   "bold": True, "align": "right",
                                   "num_format": "#,##0.0_);(#,##0.0)"})
        ws.conditional_format(row, DATA0, row, self._col(self.n - 1), {
            "type": "cell", "criteria": "!=", "value": 0, "format": fail_fmt
        })

    # ─────────────────────────────────────────────────────────────────────────
    # Cover tab
    # ─────────────────────────────────────────────────────────────────────────

    def _write_cover(self, wb, ws, fmt: _Fmt) -> None:
        ws.hide_gridlines(2)
        ws.set_column(MARGIN, MARGIN, 2)
        ws.set_column(LABEL, LABEL, 35)
        ws.set_column(DATA0, DATA0 + 1, 25)
        ws.set_row(2, 40); ws.set_row(3, 8)

        last_col = self._col(self.n - 1)
        ws.merge_range(2, LABEL, 2, last_col, self.co, fmt.hbar)
        ws.write(4, LABEL, "3-Statement Financial Model", fmt.hsub)
        ws.write(5, LABEL, f"({self.ccy} $ in millions)", fmt.hunit)

        ws.set_row(7, 20)
        ws.write(7, LABEL, "Model Overview", fmt.lbl_b)
        periods = self.o.periods
        hist = [p for p in periods if p.endswith("A")]
        proj = [p for p in periods if p.endswith("E")]
        ws.write(9,  LABEL, "Historical periods", fmt.lbl)
        ws.write(9,  DATA0, f"{hist[0]} – {hist[-1]}" if hist else "—", fmt.cv_sub)
        ws.write(10, LABEL, "Projection periods", fmt.lbl)
        ws.write(10, DATA0, f"{proj[0]} – {proj[-1]}" if proj else "—", fmt.cv_sub)
        ws.write(11, LABEL, "Currency", fmt.lbl)
        ws.write(11, DATA0, self.ccy, fmt.cv_sub)

        ws.set_row(14, 18)
        ws.write(14, LABEL, "Tab Guide", fmt.lbl_b)
        for i, (tab, desc) in enumerate([
            ("IS",      "Income Statement with projection driver block"),
            ("BS",      "Balance Sheet (PP&E / WC / Debt schedules added in Phase 2)"),
            ("CF",      "Cash Flow Statement — NI and D&A linked from IS"),
            ("Sources", "XBRL sources, derivations, and verification report"),
        ]):
            ws.write(15 + i, LABEL, tab, fmt.lbl_b)
            ws.write(15 + i, DATA0, desc, fmt.cv_sub)

        ws.set_row(21, 18)
        ws.write(21, LABEL, "Color Conventions", fmt.lbl_b)
        ws.write(22, LABEL, "Blue text",  fmt.cv_blue)
        ws.write(22, DATA0, "Hardcoded input — safe to edit", fmt.cv_sub)
        ws.write(23, LABEL, "Black text", fmt.cv_black)
        ws.write(23, DATA0, "Same-tab formula — do not edit directly", fmt.cv_sub)
        ws.write(24, LABEL, "Green text", fmt.cv_green)
        ws.write(24, DATA0, "Cross-tab link formula — do not edit directly", fmt.cv_sub)

        ws.set_landscape()
        ws.fit_to_pages(1, 0)
        ws.set_footer("&L Cover &R Page &P")

    # ─────────────────────────────────────────────────────────────────────────
    # IS tab
    # ─────────────────────────────────────────────────────────────────────────

    def _write_is(self, wb, ws, fmt: _Fmt) -> None:
        o = self.o
        n_h, n_p = self.n_h, self.n_p
        R = IS_R
        asmp = o.assumptions

        self._tab_header(ws, self.co, "Income Statement", fmt)
        self._col_headers(ws, R["headers"], fmt)

        # spacer rows
        for r in (17, 20, 24, 30, 33, 38):
            self._sp(ws, r)

        # circ switch — single blue input cell at C8 (DATA0, row circ)
        ws.write(R["circ"], LABEL, "Circ Switch  (0 = off | 1 = on)", fmt.lbl_drv)
        ws.write(R["circ"], DATA0, 0, fmt.hc)

        # pull historical
        h_rev  = self._hv(o.income_statement, "revenue")
        h_cogs = self._hv(o.income_statement, "cogs")
        h_sga  = self._hv(o.income_statement, "sga")
        h_rd   = self._hv(o.income_statement, "rd")
        h_da   = self._hv(o.income_statement, "da")
        h_ebit = self._hv(o.income_statement, "ebit")
        h_ie   = self._hv(o.income_statement, "interest_expense")
        h_ii   = self._hv(o.income_statement, "interest_income")
        h_tax  = self._hv(o.income_statement, "income_tax")
        h_ni   = self._hv(o.income_statement, "net_income")
        h_epsd = self._hv(o.income_statement, "eps_diluted")
        h_epsb = self._hv(o.income_statement, "eps_basic")
        h_shd  = self._hv(o.income_statement, "shares_diluted")
        h_shb  = self._hv(o.income_statement, "shares_basic")

        # pull projection engine values (used only as formula cache hints)
        p_rev  = self._pv(o.income_statement, "revenue")
        p_cogs = self._pv(o.income_statement, "cogs")
        p_gp   = self._pv(o.income_statement, "gross_profit")
        p_sga  = self._pv(o.income_statement, "sga")
        p_rd   = self._pv(o.income_statement, "rd")
        p_da   = self._pv(o.income_statement, "da")
        p_ebit = self._pv(o.income_statement, "ebit")
        p_ie   = self._pv(o.income_statement, "interest_expense")
        p_ii   = self._pv(o.income_statement, "interest_income")
        p_tax  = self._pv(o.income_statement, "income_tax")
        p_ni   = self._pv(o.income_statement, "net_income")
        p_epsd = self._pv(o.income_statement, "eps_diluted")
        p_shd  = self._pv(o.income_statement, "shares_diluted")

        # ── Revenue ──────────────────────────────────────────────────────────
        r = R["revenue"]
        ws.write(r, LABEL, "Revenue", fmt.lbl_b)
        for j in range(n_h):
            self._hc(ws, r, j, h_rev[j], fmt.hc_d, fmt.hc_d_hs)
        for j in range(n_p):
            prev_c = self._cell(r, n_h + j - 1)
            drv_c  = self._cell(R["drv_rev_g"], n_h + j)
            self._fmla(ws, r, n_h + j, f"={prev_c}*(1+{drv_c})", fmt.num_d, p_rev[j])

        # ── YoY Revenue Growth % ─────────────────────────────────────────────
        r = R["rev_growth"]
        ws.write(r, LABEL, "  YoY Growth %", fmt.lbl_i)
        for j in range(self.n):
            f = fmt.num_p_hs if self._hs(j) else fmt.num_p
            if j == 0:
                ws.write_blank(r, self._col(j), f)
            else:
                cur  = self._cell(R["revenue"], j)
                prev = self._cell(R["revenue"], j - 1)
                self._fmla(ws, r, j, f"=IF({prev}<>0,{cur}/{prev}-1,\"\")", f)

        # ── COGS ─────────────────────────────────────────────────────────────
        r = R["cogs"]
        ws.write(r, LABEL, "  Cost of Revenue", fmt.lbl)
        for j in range(n_h):
            self._hc(ws, r, j, h_cogs[j], fmt.hc, fmt.hc_hs)
        for j in range(n_p):
            rev_c = self._cell(R["revenue"], n_h + j)
            gp_c  = self._cell(R["gross_profit"], n_h + j)
            self._fmla(ws, r, n_h + j, f"={rev_c}-{gp_c}", fmt.num, p_cogs[j])

        # ── Gross Profit (formula all periods: Rev − COGS) ───────────────────
        r = R["gross_profit"]
        ws.write(r, LABEL, "Gross Profit", fmt.lbl_b)
        for j in range(self.n):
            rev_c  = self._cell(R["revenue"], j)
            cogs_c = self._cell(R["cogs"], j)
            f = fmt.num_bd_hs if self._hs(j) else (fmt.num_bd if j < n_h else fmt.num_bd)
            cache = (self._av(o.income_statement, "gross_profit"))[j]
            self._fmla(ws, r, j, f"={rev_c}-{cogs_c}", f, cache)

        # ── Gross Margin % ───────────────────────────────────────────────────
        self._pct_row(ws, R["gross_margin"], "  Gross Margin %",
                      R["gross_profit"], R["revenue"], fmt)

        # ── SG&A ─────────────────────────────────────────────────────────────
        r = R["sga"]
        ws.write(r, LABEL, "  SG&A", fmt.lbl)
        for j in range(n_h):
            self._hc(ws, r, j, h_sga[j], fmt.hc, fmt.hc_hs)
        for j in range(n_p):
            rev_c = self._cell(R["revenue"], n_h + j)
            drv_c = self._cell(R["drv_sga"], n_h + j)
            self._fmla(ws, r, n_h + j, f"={rev_c}*{drv_c}", fmt.num, p_sga[j])

        # ── R&D ──────────────────────────────────────────────────────────────
        r = R["rd"]
        ws.write(r, LABEL, "  R&D", fmt.lbl)
        for j in range(n_h):
            self._hc(ws, r, j, h_rd[j], fmt.hc, fmt.hc_hs)
        for j in range(n_p):
            rev_c = self._cell(R["revenue"], n_h + j)
            drv_c = self._cell(R["drv_rd"], n_h + j)
            self._fmla(ws, r, n_h + j, f"={rev_c}*{drv_c}", fmt.num, p_rd[j])

        # ── EBIT (hist = blue hardcoded operating income from XBRL) ──────────
        r = R["ebit"]
        ws.write(r, LABEL, "EBIT", fmt.lbl_b)
        for j in range(n_h):
            self._hc(ws, r, j, h_ebit[j], fmt.hc_b, fmt.hc_b_hs)
        for j in range(n_p):
            gp_c  = self._cell(R["gross_profit"], n_h + j)
            sga_c = self._cell(R["sga"], n_h + j)
            rd_c  = self._cell(R["rd"], n_h + j)
            # D&A is embedded in COGS/SGA in XBRL; project EBIT = GP − SGA − R&D
            # D&A row below is a memo add-back to bridge EBIT → EBITDA
            self._fmla(ws, r, n_h + j, f"={gp_c}-{sga_c}-{rd_c}",
                       fmt.num_b, p_ebit[j])

        # ── EBIT Margin % ────────────────────────────────────────────────────
        self._pct_row(ws, R["ebit_margin"], "  EBIT Margin %",
                      R["ebit"], R["revenue"], fmt)

        # ── D&A ──────────────────────────────────────────────────────────────
        r = R["da"]
        ws.write(r, LABEL, "  D&A", fmt.lbl_i)
        for j in range(n_h):
            self._hc(ws, r, j, h_da[j], fmt.hc, fmt.hc_hs)
        for j in range(n_p):
            rev_c = self._cell(R["revenue"], n_h + j)
            drv_c = self._cell(R["drv_da"], n_h + j)
            self._fmla(ws, r, n_h + j, f"={rev_c}*{drv_c}", fmt.num, p_da[j])

        # ── EBITDA (formula all periods: EBIT + D&A) ─────────────────────────
        r = R["ebitda"]
        ws.write(r, LABEL, "EBITDA", fmt.lbl_b)
        all_ebit = self._av(o.income_statement, "ebit")
        all_da   = self._av(o.income_statement, "da")
        for j in range(self.n):
            ebit_c = self._cell(R["ebit"], j)
            da_c   = self._cell(R["da"], j)
            f = fmt.num_b_hs if self._hs(j) else fmt.num_b
            cache  = ((all_ebit[j] or 0) + (all_da[j] or 0)) if (
                all_ebit[j] is not None or all_da[j] is not None) else None
            self._fmla(ws, r, j, f"={ebit_c}+{da_c}", f, cache)

        # ── EBITDA Margin % ──────────────────────────────────────────────────
        self._pct_row(ws, R["ebitda_margin"], "  EBITDA Margin %",
                      R["ebitda"], R["revenue"], fmt)

        # ── Interest Expense (Phase 1: blue hardcoded all periods) ───────────
        # Phase 2: replace with =IF(circ,AVG(beg_LTD,end_LTD),beg_LTD)*rate
        r = R["int_exp"]
        ws.write(r, LABEL, "  Interest Expense", fmt.lbl)
        all_ie = self._av(o.income_statement, "interest_expense")
        for j in range(self.n):
            self._hc(ws, r, j, all_ie[j], fmt.hc, fmt.hc_hs)

        # ── Interest Income (Phase 1: blue hardcoded all periods) ────────────
        r = R["int_inc"]
        ws.write(r, LABEL, "  Interest Income", fmt.lbl)
        all_ii = self._av(o.income_statement, "interest_income")
        for j in range(self.n):
            self._hc(ws, r, j, all_ii[j], fmt.hc, fmt.hc_hs)

        # ── EBT (formula all periods: EBIT − IE + II) ────────────────────────
        r = R["ebt"]
        ws.write(r, LABEL, "EBT", fmt.lbl_b)
        for j in range(self.n):
            ebit_c = self._cell(R["ebit"], j)
            ie_c   = self._cell(R["int_exp"], j)
            ii_c   = self._cell(R["int_inc"], j)
            f = fmt.num_b_hs if self._hs(j) else fmt.num_b
            cache = ((all_ebit[j] or 0) - (all_ie[j] or 0) + (all_ii[j] or 0)
                     ) if all_ebit[j] is not None else None
            self._fmla(ws, r, j, f"={ebit_c}-{ie_c}+{ii_c}", f, cache)

        # ── Income Tax (hist = blue; proj = formula MAX(0, EBT×rate)) ────────
        r = R["tax"]
        ws.write(r, LABEL, "  Income Tax", fmt.lbl)
        all_tax = self._av(o.income_statement, "income_tax")
        for j in range(n_h):
            self._hc(ws, r, j, all_tax[j], fmt.hc, fmt.hc_hs)
        for j in range(n_p):
            ebt_c = self._cell(R["ebt"], n_h + j)
            drv_c = self._cell(R["drv_tax"], n_h + j)
            self._fmla(ws, r, n_h + j, f"=MAX(0,{ebt_c}*{drv_c})", fmt.num, p_tax[j])

        # ── Effective Tax Rate % ─────────────────────────────────────────────
        self._pct_row(ws, R["tax_rate"], "  Effective Tax Rate %",
                      R["tax"], R["ebt"], fmt)

        # ── Net Income (hist = blue XBRL; proj = formula EBT − Tax) ──────────
        r = R["net_income"]
        ws.write(r, LABEL, "Net Income", fmt.lbl_b)
        for j in range(n_h):
            self._hc(ws, r, j, h_ni[j], fmt.hc_b, fmt.hc_b_hs)
        for j in range(n_p):
            ebt_c = self._cell(R["ebt"], n_h + j)
            tax_c = self._cell(R["tax"], n_h + j)
            self._fmla(ws, r, n_h + j, f"={ebt_c}-{tax_c}", fmt.num_b, p_ni[j])

        # ── Net Margin % ─────────────────────────────────────────────────────
        self._pct_row(ws, R["net_margin"], "  Net Margin %",
                      R["net_income"], R["revenue"], fmt)

        # ── EPS — Diluted ────────────────────────────────────────────────────
        r = R["eps_diluted"]
        ws.write(r, LABEL, "  EPS — Diluted", fmt.lbl)
        for j in range(n_h):
            self._hc(ws, r, j, h_epsd[j], fmt.hc, fmt.hc_hs)
        for j in range(n_p):
            ni_c = self._cell(R["net_income"], n_h + j)
            sh_c = self._cell(R["shares_diluted"], n_h + j)
            self._fmla(ws, r, n_h + j,
                       f"=IF({sh_c}<>0,{ni_c}/{sh_c},\"\")", fmt.num,
                       p_epsd[j] if j < len(p_epsd) else None)

        # ── EPS — Basic ──────────────────────────────────────────────────────
        r = R["eps_basic"]
        ws.write(r, LABEL, "  EPS — Basic", fmt.lbl)
        for j in range(n_h):
            self._hc(ws, r, j, h_epsb[j], fmt.hc, fmt.hc_hs)
        for j in range(n_p):
            ni_c = self._cell(R["net_income"], n_h + j)
            sh_c = self._cell(R["shares_basic"], n_h + j)
            self._fmla(ws, r, n_h + j,
                       f"=IF({sh_c}<>0,{ni_c}/{sh_c},\"\")", fmt.num)

        # ── Shares — Diluted (Phase 1: blue all periods) ─────────────────────
        r = R["shares_diluted"]
        ws.write(r, LABEL, "  Shares — Diluted (wtd avg)", fmt.lbl)
        all_shd = self._av(o.income_statement, "shares_diluted")
        proj_sh = asmp.get("shares_diluted")
        for j in range(n_h):
            self._hc(ws, r, j, all_shd[j], fmt.hc, fmt.hc_hs)
        for j in range(n_p):
            val = (all_shd[n_h + j] if (n_h + j) < len(all_shd) else None) or proj_sh
            self._hc(ws, r, n_h + j, val, fmt.hc, fmt.hc_hs)

        # ── Shares — Basic ───────────────────────────────────────────────────
        r = R["shares_basic"]
        ws.write(r, LABEL, "  Shares — Basic (wtd avg)", fmt.lbl)
        all_shb = self._av(o.income_statement, "shares_basic")
        for j in range(n_h):
            self._hc(ws, r, j, all_shb[j], fmt.hc, fmt.hc_hs)
        for j in range(n_p):
            val = (all_shb[n_h + j] if (n_h + j) < len(all_shb) else None) or proj_sh
            self._hc(ws, r, n_h + j, val, fmt.hc, fmt.hc_hs)

        # ── PROJECTION DRIVER BLOCK ──────────────────────────────────────────
        ws.set_row(R["drv_header"], 16)
        ws.write(R["drv_header"], LABEL, "PROJECTION DRIVERS", fmt.lbl_sec)

        def _drv(row: int, label: str, hist_fn, proj_val, pct: bool = True) -> None:
            """Driver row: implied historical formula | blue projection assumption."""
            ws.write(row, LABEL, label, fmt.lbl_drv)
            for j in range(n_h):
                fmla = hist_fn(j)
                f = fmt.drv_imp_hs if self._hs(j) else fmt.drv_imp
                if fmla:
                    ws.write_formula(row, self._col(j), fmla, f)
                else:
                    ws.write_blank(row, self._col(j), f)
            for j in range(n_p):
                f = fmt.drv if pct else fmt.drv_num
                if proj_val is not None:
                    ws.write(row, self._col(n_h + j), proj_val, f)
                else:
                    ws.write_blank(row, self._col(n_h + j), f)

        def _rev_g(j):
            if j == 0: return None
            return f"=IF({self._cell(R['revenue'],j-1)}<>0,{self._cell(R['revenue'],j)}/{self._cell(R['revenue'],j-1)}-1,\"\")"

        def _ratio(num_r, den_r):
            def fn(j):
                n_c = self._cell(num_r, j)
                d_c = self._cell(den_r, j)
                return f"=IF({d_c}<>0,{n_c}/{d_c},\"\")"
            return fn

        _drv(R["drv_rev_g"], "Revenue Growth %",
             _rev_g, asmp.get("revenue_growth_pct"))
        _drv(R["drv_gm"],    "Gross Margin %",
             _ratio(R["gross_profit"], R["revenue"]), asmp.get("gross_margin_pct"))
        _drv(R["drv_sga"],   "SG&A % Revenue",
             _ratio(R["sga"], R["revenue"]), asmp.get("sga_pct_rev"))
        _drv(R["drv_rd"],    "R&D % Revenue",
             _ratio(R["rd"],  R["revenue"]), asmp.get("rd_pct_rev"))
        _drv(R["drv_da"],    "D&A % Revenue",
             _ratio(R["da"],  R["revenue"]), asmp.get("da_pct_rev"))
        _drv(R["drv_tax"],   "Effective Tax Rate %",
             _ratio(R["tax"], R["ebt"]),     asmp.get("tax_rate_pct"))
        _drv(R["drv_int"],   "Interest Rate % (Phase 2: → debt schedule)",
             lambda j: None, asmp.get("interest_rate_pct"))
        _drv(R["drv_shares"],"Diluted Shares ('000s)",
             lambda j: None, asmp.get("shares_diluted"), pct=False)

        ws.set_landscape(); ws.fit_to_pages(1, 0)
        ws.set_footer(f"&L IS &C {self.co} &R Page &P")
        ws.set_print_scale(85)

    # ─────────────────────────────────────────────────────────────────────────
    # Pct margin row helper (formula all periods)
    # ─────────────────────────────────────────────────────────────────────────

    def _pct_row(self, ws, row: int, label: str,
                 num_r: int, den_r: int, fmt: _Fmt) -> None:
        ws.write(row, LABEL, label, fmt.lbl_i)
        for j in range(self.n):
            n_c = self._cell(num_r, j)
            d_c = self._cell(den_r, j)
            f = fmt.num_p_hs if self._hs(j) else fmt.num_p
            ws.write_formula(row, self._col(j), f"=IF({d_c}<>0,{n_c}/{d_c},\"\")", f)

    # ─────────────────────────────────────────────────────────────────────────
    # BS tab
    # ─────────────────────────────────────────────────────────────────────────

    def _write_bs(self, wb, ws, fmt: _Fmt) -> None:
        o = self.o
        n_h, n_p = self.n_h, self.n_p
        R = BS_R

        self._tab_header(ws, self.co, "Balance Sheet", fmt)
        self._col_headers(ws, R["headers"], fmt)
        for sp in (19, 25, 27):
            self._sp(ws, sp)

        # pull data — short alias → actual model key
        _bs_map = {
            "cash":           "cash",
            "ar":             "accounts_receivable",
            "inventory":      "inventory",
            "total_cur_assets": "total_current_assets",
            "ppe_net":        "ppe_net",
            "goodwill":       "goodwill",
            "intangibles":    "intangibles",
            "total_assets":   "total_assets",
            "ap":             "accounts_payable",
            "total_cur_liab": "total_current_liabilities",
            "ltd":            "long_term_debt",
            "total_liab":     "total_liabilities",
            "redeemable_nci": "redeemable_nci",
            "retained_earnings": "retained_earnings",
            "total_equity":   "total_equity",
        }
        h     = {alias: self._hv(o.balance_sheet, actual) for alias, actual in _bs_map.items()}
        all_v = {alias: self._av(o.balance_sheet, actual) for alias, actual in _bs_map.items()}
        p_cash_is = self._pv(o.balance_sheet, "cash")   # engine-computed proj cash

        # ── ASSETS ───────────────────────────────────────────────────────────
        ws.write(R["assets_hdr"], LABEL, "ASSETS", fmt.lbl_sec)

        def _bs_row(row, label, key, bold=False):
            ws.write(row, LABEL, label, fmt.lbl_b if bold else fmt.lbl)
            for j in range(n_h):
                self._hc(ws, row, j, h[key][j],
                         fmt.hc_b if bold else fmt.hc,
                         fmt.hc_b_hs if bold else fmt.hc_hs)
            for j in range(n_p):
                val = all_v[key][n_h + j]
                self._hc(ws, row, n_h + j, val,
                         fmt.hc_b if bold else fmt.hc,
                         fmt.hc_b_hs if bold else fmt.hc_hs)

        # Cash: hist = blue; proj = green cross-tab from CF Ending Cash
        r = R["cash"]
        ws.write(r, LABEL, "  Cash & Equivalents", fmt.lbl)
        for j in range(n_h):
            self._hc(ws, r, j, h["cash"][j], fmt.hc, fmt.hc_hs)
        for j in range(n_p):
            ec_row = CF_R["ending_cash"]
            ws.write_formula(r, self._col(n_h + j),
                             _xr("CF", ec_row, self._col(n_h + j)),
                             fmt.xt, p_cash_is[j])

        _bs_row(R["ar"],        "  Accounts Receivable",  "ar")
        _bs_row(R["inventory"], "  Inventory",             "inventory")

        # Total Current Assets — formula for projections, blue for hist
        r = R["total_cur_assets"]
        ws.write(r, LABEL, "Total Current Assets", fmt.lbl_b)
        for j in range(n_h):
            self._hc(ws, r, j, h["total_cur_assets"][j], fmt.hc_b, fmt.hc_b_hs)
        for j in range(n_p):
            cash_c = self._cell(R["cash"], n_h + j)
            ar_c   = self._cell(R["ar"],   n_h + j)
            inv_c  = self._cell(R["inventory"], n_h + j)
            cache  = all_v["total_cur_assets"][n_h + j]
            self._fmla(ws, r, n_h + j, f"={cash_c}+{ar_c}+{inv_c}", fmt.num_b, cache)

        _bs_row(R["ppe_net"],    "  PP&E, net",        "ppe_net")
        _bs_row(R["goodwill"],   "  Goodwill",          "goodwill")
        _bs_row(R["intangibles"],"  Intangibles, net",  "intangibles")

        # Total Assets — formula for projections
        r = R["total_assets"]
        ws.write(r, LABEL, "Total Assets", fmt.lbl_b)
        for j in range(n_h):
            self._hc(ws, r, j, h["total_assets"][j], fmt.hc_bd, fmt.hc_bd_hs)
        for j in range(n_p):
            ca_c  = self._cell(R["total_cur_assets"], n_h + j)
            ppe_c = self._cell(R["ppe_net"],           n_h + j)
            gw_c  = self._cell(R["goodwill"],          n_h + j)
            ia_c  = self._cell(R["intangibles"],       n_h + j)
            cache = all_v["total_assets"][n_h + j]
            self._fmla(ws, r, n_h + j, f"={ca_c}+{ppe_c}+{gw_c}+{ia_c}",
                       fmt.num_bd, cache)

        # ── LIABILITIES & EQUITY ──────────────────────────────────────────────
        ws.write(R["le_hdr"], LABEL, "LIABILITIES & EQUITY", fmt.lbl_sec)

        _bs_row(R["ap"],           "  Accounts Payable",        "ap")

        # Total Current Liab — formula for projections
        r = R["total_cur_liab"]
        ws.write(r, LABEL, "Total Current Liabilities", fmt.lbl_b)
        for j in range(n_h):
            self._hc(ws, r, j, h["total_cur_liab"][j], fmt.hc_b, fmt.hc_b_hs)
        for j in range(n_p):
            ap_c  = self._cell(R["ap"], n_h + j)
            cache = all_v["total_cur_liab"][n_h + j]
            self._fmla(ws, r, n_h + j, f"={ap_c}", fmt.num_b, cache)

        _bs_row(R["ltd"],        "  Long-Term Debt",       "ltd")
        _bs_row(R["total_liab"], "Total Liabilities",      "total_liab", bold=True)

        # RNCI / Mezzanine
        r = R["rnci"]
        ws.write(r, LABEL, "  Redeemable NCI (Mezzanine)", fmt.lbl)
        for j in range(n_h):
            self._hc(ws, r, j, h["redeemable_nci"][j], fmt.hc, fmt.hc_hs)
        for j in range(n_p):
            self._hc(ws, r, n_h + j, all_v["redeemable_nci"][n_h + j],
                     fmt.hc, fmt.hc_hs)

        # ── EQUITY ────────────────────────────────────────────────────────────
        ws.write(R["equity_hdr"], LABEL, "EQUITY", fmt.lbl_sec)

        # Retained Earnings: hist=blue; proj=formula: prev_RE + IS!NI
        r = R["retained_earnings"]
        ws.write(r, LABEL, "  Retained Earnings", fmt.lbl)
        for j in range(n_h):
            self._hc(ws, r, j, h["retained_earnings"][j], fmt.hc, fmt.hc_hs)
        for j in range(n_p):
            prev_j = n_h + j - 1
            prev_re = self._cell(r, prev_j)
            ni_c    = _xr("IS", IS_R["net_income"], self._col(n_h + j))
            cache   = all_v["retained_earnings"][n_h + j]
            self._fmla(ws, r, n_h + j, f"={prev_re}+{ni_c[1:]}", fmt.num, cache)

        _bs_row(R["total_equity"], "Total Equity", "total_equity", bold=True)

        # Total L + Mezzanine + E (formula all periods)
        r = R["total_le"]
        ws.write(r, LABEL, "Total Liab + Mezzanine + Equity", fmt.lbl_b)
        for j in range(self.n):
            tl_c   = self._cell(R["total_liab"],  j)
            rnci_c = self._cell(R["rnci"],         j)
            te_c   = self._cell(R["total_equity"], j)
            f = fmt.num_bd_hs if self._hs(j) else fmt.num_bd
            tl     = all_v["total_liab"][j]
            rn     = all_v["redeemable_nci"][j]
            te     = all_v["total_equity"][j]
            cache  = ((tl or 0) + (rn or 0) + (te or 0)) if tl is not None else None
            self._fmla(ws, r, j, f"={tl_c}+{rnci_c}+{te_c}", f, cache)

        # BS Check: Total Assets − Total L+M+E  (should = 0)
        r = R["bs_check"]
        ws.write(r, LABEL, "  BS Check  (Assets − L+M+E)", fmt.lbl_chk)
        for j in range(self.n):
            ta_c  = self._cell(R["total_assets"], j)
            tle_c = self._cell(R["total_le"],     j)
            ta    = all_v["total_assets"][j]
            tle   = ((all_v["total_liab"][j] or 0) +
                     (all_v["redeemable_nci"][j] or 0) +
                     (all_v["total_equity"][j] or 0)) if all_v["total_assets"][j] else None
            cache = round((ta or 0) - (tle or 0), 2) if ta is not None else None
            self._fmla(ws, r, j, f"={ta_c}-{tle_c}", fmt.chk_ok, cache)
        self._apply_check_cf(wb, ws, r)

        ws.set_landscape(); ws.fit_to_pages(1, 0)
        ws.set_footer(f"&L BS &C {self.co} &R Page &P")
        ws.set_print_scale(85)

    # ─────────────────────────────────────────────────────────────────────────
    # CF tab
    # ─────────────────────────────────────────────────────────────────────────

    def _write_cf(self, wb, ws, fmt: _Fmt) -> None:
        o = self.o
        n_h, n_p = self.n_h, self.n_p
        R = CF_R

        self._tab_header(ws, self.co, "Cash Flow Statement", fmt)
        self._col_headers(ws, R["headers"], fmt)
        for sp in (16, 21, 27, 32):
            self._sp(ws, sp)

        # pull XBRL data
        h_cfo  = self._hv(o.cash_flow_statement, "cfo")
        h_ni   = self._hv(o.income_statement,    "net_income")
        h_da   = self._hv(o.income_statement,    "da")
        h_cap  = self._hv(o.cash_flow_statement, "capex")
        h_cfi  = self._hv(o.cash_flow_statement, "cfi")
        h_div  = self._hv(o.cash_flow_statement, "dividends_paid")
        h_bb   = self._hv(o.cash_flow_statement, "buybacks")
        h_cff  = self._hv(o.cash_flow_statement, "cff")
        h_nc   = self._hv(o.cash_flow_statement, "net_change_cash")
        h_cash = self._hv(o.balance_sheet,        "cash")

        # projection engine values (formula caches)
        p_cfo  = self._pv(o.cash_flow_statement, "cfo")
        p_cap  = self._pv(o.cash_flow_statement, "capex")
        p_cfi  = self._pv(o.cash_flow_statement, "cfi")
        p_cff  = self._pv(o.cash_flow_statement, "cff")
        p_nc   = self._pv(o.cash_flow_statement, "net_change_cash")
        p_cash = self._pv(o.balance_sheet,        "cash")

        # residuals: WC = CFO − NI − DA (Phase 1 placeholder; Phase 2 → WC schedule)
        def _res(total, a, b):
            return [round((t or 0) - (x or 0) - (y or 0), 2)
                    for t, x, y in zip(total, a, b)]

        h_wc_res = _res(h_cfo, h_ni, h_da)
        h_other_cfi = [round((cfi or 0) - (cap or 0), 2)
                       for cfi, cap in zip(h_cfi, h_cap)]
        h_other_cff = [round((cff or 0) - (div or 0) - (bb or 0), 2)
                       for cff, div, bb in zip(h_cff, h_div, h_bb)]

        # Beginning cash[j] = ending_cash[j] - net_change[j]
        h_beg = [round((c or 0) - (nc or 0), 2) if c is not None else None
                 for c, nc in zip(h_cash, h_nc)]

        # ── CFO ──────────────────────────────────────────────────────────────
        ws.write(R["cfo_hdr"], LABEL, "OPERATING ACTIVITIES", fmt.lbl_sec)

        # Net Income: green cross-tab link to IS (all periods)
        r = R["ni"]
        ws.write(r, LABEL, "  Net Income", fmt.lbl)
        all_ni = self._av(o.income_statement, "net_income")
        for j in range(self.n):
            f = fmt.xt_hs if self._hs(j) else fmt.xt
            self._fmla(ws, r, j, _xr("IS", IS_R["net_income"], self._col(j)),
                       f, all_ni[j])

        # D&A: green cross-tab link to IS (all periods)
        r = R["da"]
        ws.write(r, LABEL, "  D&A (add-back)", fmt.lbl)
        all_da = self._av(o.income_statement, "da")
        for j in range(self.n):
            f = fmt.xt_hs if self._hs(j) else fmt.xt
            self._fmla(ws, r, j, _xr("IS", IS_R["da"], self._col(j)), f, all_da[j])

        # Working Capital, net (Phase 1: blue hardcoded residual)
        r = R["wc_net"]
        ws.write(r, LABEL, "  Working Capital, net  (Phase 2: → WC schedule)", fmt.lbl)
        p_wc = _res(p_cfo, [all_ni[n_h + j] for j in range(n_p)],
                            [all_da[n_h + j] for j in range(n_p)])
        for j in range(n_h):
            self._hc(ws, r, j, h_wc_res[j], fmt.hc, fmt.hc_hs)
        for j in range(n_p):
            self._hc(ws, r, n_h + j, p_wc[j], fmt.hc, fmt.hc_hs)

        # Other / misc CFO (blue residual)
        r = R["other_cfo"]
        ws.write(r, LABEL, "  Other operating", fmt.lbl)
        for j in range(self.n):
            self._hc(ws, r, j, 0.0, fmt.hc, fmt.hc_hs)

        # CFO total — formula = SUM of NI + DA + WC + Other
        r = R["cfo"]
        ws.write(r, LABEL, "Cash from Operations", fmt.lbl_b)
        for j in range(self.n):
            ni_c  = self._cell(R["ni"],        j)
            da_c  = self._cell(R["da"],        j)
            wc_c  = self._cell(R["wc_net"],    j)
            ot_c  = self._cell(R["other_cfo"], j)
            f = fmt.num_b_hs if self._hs(j) else fmt.num_b
            cache = (all_ni[j] or 0) + (all_da[j] or 0) + (
                h_wc_res[j] if j < n_h else p_wc[j - n_h] or 0)
            self._fmla(ws, r, j, f"={ni_c}+{da_c}+{wc_c}+{ot_c}", f, cache)

        # ── CFI ──────────────────────────────────────────────────────────────
        ws.write(R["cfi_hdr"], LABEL, "INVESTING ACTIVITIES", fmt.lbl_sec)

        r = R["capex"]
        ws.write(r, LABEL, "  Capital Expenditures", fmt.lbl)
        all_cap = self._av(o.cash_flow_statement, "capex")
        for j in range(self.n):
            self._hc(ws, r, j, all_cap[j], fmt.hc, fmt.hc_hs)

        r = R["other_cfi"]
        ws.write(r, LABEL, "  Other investing", fmt.lbl)
        for j in range(n_h):
            self._hc(ws, r, j, h_other_cfi[j], fmt.hc, fmt.hc_hs)
        for j in range(n_p):
            self._hc(ws, r, n_h + j, 0.0, fmt.hc, fmt.hc_hs)

        r = R["cfi"]
        ws.write(r, LABEL, "Cash from Investing", fmt.lbl_b)
        for j in range(self.n):
            cap_c = self._cell(R["capex"],     j)
            ot_c  = self._cell(R["other_cfi"], j)
            f = fmt.num_b_hs if self._hs(j) else fmt.num_b
            cache = all_cap[j]
            self._fmla(ws, r, j, f"={cap_c}+{ot_c}", f, cache)

        # ── CFF ──────────────────────────────────────────────────────────────
        ws.write(R["cff_hdr"], LABEL, "FINANCING ACTIVITIES", fmt.lbl_sec)

        r = R["dividends"]
        ws.write(r, LABEL, "  Dividends Paid", fmt.lbl)
        all_div = self._av(o.cash_flow_statement, "dividends_paid")
        for j in range(self.n):
            self._hc(ws, r, j, all_div[j], fmt.hc, fmt.hc_hs)

        r = R["buybacks"]
        ws.write(r, LABEL, "  Share Buybacks", fmt.lbl)
        all_bb = self._av(o.cash_flow_statement, "buybacks")
        for j in range(self.n):
            self._hc(ws, r, j, all_bb[j], fmt.hc, fmt.hc_hs)

        r = R["other_cff"]
        ws.write(r, LABEL, "  Other financing  (debt ±, issuances)", fmt.lbl)
        for j in range(n_h):
            self._hc(ws, r, j, h_other_cff[j], fmt.hc, fmt.hc_hs)
        for j in range(n_p):
            self._hc(ws, r, n_h + j, 0.0, fmt.hc, fmt.hc_hs)

        r = R["cff"]
        ws.write(r, LABEL, "Cash from Financing", fmt.lbl_b)
        for j in range(self.n):
            div_c = self._cell(R["dividends"],  j)
            bb_c  = self._cell(R["buybacks"],   j)
            ot_c  = self._cell(R["other_cff"],  j)
            f = fmt.num_b_hs if self._hs(j) else fmt.num_b
            all_cff = self._av(o.cash_flow_statement, "cff")
            cache = all_cff[j]
            self._fmla(ws, r, j, f"={div_c}+{bb_c}+{ot_c}", f, cache)

        # ── Net Change + Cash Balances ────────────────────────────────────────
        r = R["net_change"]
        ws.write(r, LABEL, "Net Change in Cash", fmt.lbl_b)
        all_nc = self._av(o.cash_flow_statement, "net_change_cash")
        for j in range(self.n):
            cfo_c = self._cell(R["cfo"], j)
            cfi_c = self._cell(R["cfi"], j)
            cff_c = self._cell(R["cff"], j)
            f = fmt.num_b_hs if self._hs(j) else fmt.num_b
            self._fmla(ws, r, j, f"={cfo_c}+{cfi_c}+{cff_c}", f, all_nc[j])

        r = R["beg_cash"]
        ws.write(r, LABEL, "Beginning Cash", fmt.lbl)
        for j in range(self.n):
            f = fmt.hc_hs if self._hs(j) else (fmt.hc if j == 0 else fmt.num)
            if j == 0:
                # first period beg cash = ending − net change (blue hardcoded)
                ws.write(r, self._col(j), h_beg[0], fmt.hc)
            else:
                prev_ec = self._cell(R["ending_cash"], j - 1)
                ws.write_formula(r, self._col(j), f"={prev_ec}", f)

        r = R["ending_cash"]
        ws.write(r, LABEL, "Ending Cash", fmt.lbl_b)
        all_cash = self._av(o.balance_sheet, "cash")
        for j in range(self.n):
            bc_c = self._cell(R["beg_cash"],   j)
            nc_c = self._cell(R["net_change"], j)
            f = fmt.num_bd_hs if self._hs(j) else fmt.num_bd
            self._fmla(ws, r, j, f"={bc_c}+{nc_c}", f, all_cash[j])

        # FCF = CFO − CapEx
        r = R["fcf"]
        ws.write(r, LABEL, "  Free Cash Flow  (CFO − CapEx)", fmt.lbl_i)
        for j in range(self.n):
            cfo_c = self._cell(R["cfo"],   j)
            cap_c = self._cell(R["capex"], j)
            f = fmt.num_p_hs if self._hs(j) else fmt.num
            all_cfo = self._av(o.cash_flow_statement, "cfo")
            cache   = (all_cfo[j] or 0) - abs(all_cap[j] or 0) if all_cfo[j] is not None else None
            self._fmla(ws, r, j, f"={cfo_c}-ABS({cap_c})", f, cache)

        # ── Validation Checks ─────────────────────────────────────────────────
        r = R["chk_ni"]
        ws.write(r, LABEL, "  Check: CF NI = IS NI  (should = 0)", fmt.lbl_chk)
        for j in range(self.n):
            ni_cf = self._cell(R["ni"], j)
            ni_is = _c(IS_R["net_income"], self._col(j))
            self._fmla(ws, r, j, f"={ni_cf}-IS!{ni_is}", fmt.chk_ok, 0.0)
        self._apply_check_cf(wb, ws, r)

        r = R["chk_cash"]
        ws.write(r, LABEL, "  Check: CF Ending Cash = BS Cash  (should = 0)", fmt.lbl_chk)
        for j in range(self.n):
            ec_c  = self._cell(R["ending_cash"], j)
            bs_c  = _c(BS_R["cash"], self._col(j))
            self._fmla(ws, r, j, f"={ec_c}-BS!{bs_c}", fmt.chk_ok, 0.0)
        self._apply_check_cf(wb, ws, r)

        ws.set_landscape(); ws.fit_to_pages(1, 0)
        ws.set_footer(f"&L CF &C {self.co} &R Page &P")
        ws.set_print_scale(85)

    # ─────────────────────────────────────────────────────────────────────────
    # Sources tab
    # ─────────────────────────────────────────────────────────────────────────

    def _write_sources(self, wb, ws, fmt: _Fmt) -> None:
        ws.hide_gridlines(2)
        ws.set_column(MARGIN, MARGIN, 2)
        ws.set_column(LABEL, LABEL, 28)
        for col, w in [(DATA0, 12), (DATA0+1, 14), (DATA0+2, 14),
                       (DATA0+3, 30), (DATA0+4, 10)]:
            ws.set_column(col, col, w)

        ws.merge_range(2, LABEL, 2, DATA0 + 5, self.co, fmt.hbar)
        ws.write(4, LABEL, "Sources & Audit Trail", fmt.hsub)
        ws.write(5, LABEL, f"({self.ccy} $ in millions)", fmt.hunit)

        headers = ["Line Item", "Period", "Value ($M)", "Filing / XBRL Tag", "Confidence"]
        for j, h in enumerate(headers):
            ws.write(7, LABEL + j, h, fmt.src_hdr)
        ws.set_row(7, 16)

        r = 8
        for item, cit_list in (self.srcs or {}).items():
            if not isinstance(cit_list, list):
                continue
            for cit in cit_list:
                low = getattr(cit, "confidence", 1.0) < 0.80
                f = fmt.src_low if low else fmt.src_row
                vals = [
                    item,
                    getattr(cit, "filing", ""),
                    "",
                    getattr(cit, "xbrl_tag", "") or "",
                    getattr(cit, "confidence", ""),
                ]
                for j, v in enumerate(vals):
                    ws.write(r, LABEL + j, v, f)
                r += 1

        r += 2
        ws.write(r, LABEL, "VERIFICATION REPORT", fmt.lbl_sec); r += 1
        status = "PASSED ✓" if self.rpt.passed else "FAILED ✗"
        ws.write(r, LABEL, f"Status: {status}", fmt.lbl_b); r += 1

        if self.rpt.critical_failures:
            ws.write(r, LABEL, "Critical Failures:", fmt.lbl_b); r += 1
            for cf in self.rpt.critical_failures:
                ws.write(r, DATA0, cf, fmt.chk_fail); r += 1
        if self.rpt.warnings:
            ws.write(r, LABEL, "Warnings:", fmt.lbl_b); r += 1
            for w in self.rpt.warnings:
                ws.write(r, DATA0, w, fmt.src_row); r += 1
        if self.rpt.notes:
            ws.write(r, LABEL, "Notes:", fmt.lbl_b); r += 1
            for n in self.rpt.notes:
                ws.write(r, DATA0, n, fmt.src_row); r += 1
        if self.o.plug_used:
            ws.write(r, DATA0, "⚠ Plug was used to balance BS — review assumptions", fmt.src_low)

        ws.set_landscape(); ws.fit_to_pages(1, 0)
        ws.set_footer(f"&L Sources &C {self.co} &R Page &P")
