"""
Phase 1 Excel writer — Rogo-standard 3-statement financial model.

Color conventions:
  Blue  (#0000FF) = hardcoded input (user-editable)
  Black (#0F1632) = same-tab formula (do not edit directly)
  Green (#008000) = cross-tab link (do not edit directly)

Tab order: IS → BS → CF → Sources
IS structure: Revenue → COGS → GP → SGA → R&D → EBIT → D&A → EBITDA → interest → EBT → Tax → NI (consol) → NCI → NI to Common
Projection cells: Excel formulas driven by assumption driver block below IS
"""
from __future__ import annotations
import xlsxwriter
from xlsxwriter.utility import xl_col_to_name
from schemas.financial_data import (
    ModelOutput, VerificationReport, AssumptionsBlock, ScenarioInputs,
    WACCOutput, PeerSet, PublicCompsOutput,
)

# ─────────────────────────────────────────────────────────────────────────────
# Column layout (0-based)
# ─────────────────────────────────────────────────────────────────────────────
MARGIN  = 0   # Col A — left gutter (per SPEC_excel_formatting Section 1.2)
MARGIN2 = 1   # Col B — second gutter
LABEL   = 2   # Col C — row labels
DATA0   = 3   # Col D — period[0]


def _c(row: int, col: int) -> str:
    """0-based (row, col) → Excel address, e.g. 'C10'."""
    return f"{xl_col_to_name(col)}{row + 1}"


def _xr(sheet: str, row: int, col: int) -> str:
    """Cross-tab formula string, e.g. '=IS!C10'."""
    return f"={sheet}!{_c(row, col)}"


# ─────────────────────────────────────────────────────────────────────────────
# Row maps — 0-based row indices for each tab
# ─────────────────────────────────────────────────────────────────────────────
# Utility IS reuses the same row slots as standard IS (keeps EBIT at row 18
# and everything below unchanged, so CF cross-sheet refs stay valid).
# Rows 12-16 get utility-specific labels and data instead of COGS/GP/SGA/RD.
IS_UTILITY_ROW_LABELS: dict[str, str] = {
    "cogs":         "  O&M",                              # Operations & Maintenance
    "gross_profit": "  D&A",                              # Depreciation & Amortization (as opex)
    "gross_margin": "  Taxes other than income taxes",    # franchise / property taxes
    "sga":          "  Other operating expenses",         # residual (fuel + other not in XBRL)
    "rd":           "Total Operating Expenses",           # bold subtotal
}

IS_R: dict[str, int] = {
    "title": 2, "subtitle": 4, "units": 5,
    "circ": 7,      # circ switch single cell
    "headers": 9,
    "revenue": 10,  "rev_growth": 11,
    "cogs": 12,
    "gross_profit": 13, "gross_margin": 14,
    "sga": 15, "rd": 16,
    # spacer 17
    "ebit": 18,     "ebit_margin": 19,   # EBIT before EBITDA (standard order)
    # spacer 20
    "da": 21,
    "ebitda": 22,   "ebitda_margin": 23, # EBITDA = EBIT + D&A add-back
    # spacer 24
    "int_exp": 25,  "int_inc": 26,
    "ebt": 27,
    "tax": 28,      "tax_rate": 29,
    # spacer 30
    "net_income": 31, "net_margin": 32,
    "nci_income": 33,      # Less: Net Income to NCI
    "ni_common": 34,       # Net Income to Common Stockholders
    "ni_common_margin": 35,
    # spacer 36
    "eps_diluted": 37, "eps_basic": 38,
    "shares_diluted": 39, "shares_basic": 40,
    # spacer 41
    "drv_header": 42,
    "case_input": 43,  # active scenario selector (blue input)
    "case_label": 44,  # CHOOSE formula display
    "drv_rev_g": 45, "drv_gm": 46,
    "drv_sga": 47,   "drv_rd": 48,
    "drv_da": 49,    "drv_tax": 50,
    "drv_int": 51,   "drv_shares": 52,
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
    "deferred_rev_current": 23,  # Deferred Revenue (current portion)
    "ltd": 24,
    "deferred_rev_lt": 25,       # Deferred Revenue (non-current)
    "total_liab": 26,
    # spacer 27
    "rnci": 28,   # redeemable NCI / mezzanine equity
    # spacer 29
    "equity_hdr": 30,
    "retained_earnings": 31, "total_equity": 32,
    "total_le": 33,   # = total_liab + rnci + total_equity
    "bs_check": 34,
}

CF_R: dict[str, int] = {
    "title": 2, "subtitle": 4, "units": 5, "headers": 9,
    "cfo_hdr": 10,
    "ni": 11, "da": 12,
    "wc_ar": 13, "wc_inv": 14, "wc_ap": 15,
    "wc_def_rev": 16,   # Δ Deferred Revenue (operating WC)
    "wc_other": 17,
    "other_cfo": 18, "cfo": 19,
    # spacer 20
    "cfi_hdr": 21,
    "capex": 22,
    "capex_drv": 23,    # CapEx % of Revenue — two-hop restate (green link to Assumptions)
    "investments_net": 24,
    "other_cfi": 25,
    "cfi": 26,
    # spacer 27
    "cff_hdr": 28,
    "dividends": 29, "dividend_drv": 30,  # two-hop restate for dividend per share
    "buybacks": 31, "other_cff": 32,
    "cff": 33,
    "fx_other": 34,   # FX & Other Adjustments — bridges CFO+CFI+CFF to actual net change
    "net_change": 35, "beg_cash": 36, "ending_cash": 37,
    "fcf": 38,
    # spacer 39
    "chk_ni": 40, "chk_cash": 41,
}

# Supporting schedule rows on the BS tab (0-based)
BS_SCHED_R: dict[str, int] = {
    # spacer 35, 36 after bs_check (shifted +2 from deferred_rev rows in BS_R)
    "sched_title": 37,
    # spacer 38
    "ppe_hdr":    39,
    "ppe_beg":    40,
    "ppe_capex":  41,
    "ppe_da":     42,
    "ppe_other":  43,
    "ppe_end":    44,
    # spacer 45
    "wc_hdr":     46,
    "wc_ar_days": 47,
    "wc_ar":      48,
    "wc_inv_days":49,
    "wc_inv":     50,
    "wc_ap_days": 51,
    "wc_ap":      52,
    # spacer 53
    "wc_net_chg": 54,
    # spacer 55
    "debt_hdr":   56,
    "debt_rate":  57,
    "debt_beg":   58,
    "debt_new":   59,
    "debt_repaid":60,
    "debt_end":   61,
    "debt_int":   62,
    # spacer 63
    "re_hdr":     64,
    "re_beg":     65,
    "re_ni":      66,
    "re_div":     67,
    "re_bb":      68,
    "re_end":     69,
}


# Scenario assumption rows on IS tab (0-based)
IS_SCEN_R: dict[str, int] = {
    # spacer 53
    "hdr":    54,
    "labels": 55,   # Base | Upside | Downside sub-headers
    "rev_g":  56,
    "gm":     57,
    "sga":    58,
    "rd":     59,
    "da":     60,
    "tax":    61,
    "int":    62,
    "shares": 63,
}

# DCF tab row map (0-based)
DCF_R: dict[str, int] = {
    "title": 2, "subtitle": 4, "units": 5,
    # WACC build-up
    "wacc_hdr":   8,
    # spacer 9
    "beta": 10, "rf": 11, "erp": 12, "ke": 13,
    # spacer 14
    "kd_pre": 15, "tax_shield": 16, "kd": 17,
    # spacer 18
    "eq_wt": 19, "d_wt": 20, "wacc": 21,
    # spacer 22, 23
    # FCF projection
    "fcf_hdr":     24,
    "fcf_headers": 25,
    "fcf_ebit":    26,
    "fcf_nopat":   27,
    "fcf_da":      28,
    "fcf_capex":   29,
    "fcf_dwc":     30,
    "fcf_fcff":    31,
    "fcf_t":       32,
    "fcf_factor":  33,
    "fcf_pv":      34,
    # spacer 35, 36
    "pv_fcfs":     37,
    # spacer 38, 39
    # Terminal value
    "tv_hdr":      40,
    "tv_method":   41,
    "tv1_lbl":     42, "tv1_mult": 43, "tv1_ebitda": 44, "tv1_tv": 45,
    "tv2_lbl":     46, "tv2_g":    47, "tv2_fcf":    48, "tv2_tv": 49,
    # spacer 50
    "tv_selected": 51,
    "tv_pv":       52,
    # spacer 53
    # EV bridge
    "ev_hdr":      54,
    "ev_pvfcfs":   55, "ev_pvtv":  56, "ev_total":   57,
    # spacer 58
    "ev_debt":     59, "ev_cash":  60, "ev_net_debt": 61,
    "ev_equity":   62, "ev_shares": 63, "ev_price":   64,
    # spacer 65, 66
    # Sensitivity
    "sens_hdr":    67,
    "sens1_lbl":   68, "sens1_col_hdr": 69,
    # sens1 data rows 70-74  (5 WACC values)
    # spacer 75
    "sens2_lbl":   76, "sens2_col_hdr": 77,
    # sens2 data rows 78-82
    # Cross-check block
    "xc_hdr":       84,
    "xc_tv_pct":    85,
    "xc_wacc_g":    86,
    "xc_imp_mult":  87,
    "xc_imp_g":     88,
    "xc_upside":    89,
    "xc_current":   90,
}

# ─────────────────────────────────────────────────────────────────────────────
# Assumptions tab row map (0-based)
# Layout: 14 driver rows per scenario block; 4 blocks (Active, Base, Up, Down).
# Drivers 0-11 are per-period (5 proj year columns); drivers 12-13 are scalar
# (single value in first proj-year column, others blank).
# ─────────────────────────────────────────────────────────────────────────────
ASSUMP_DATA0 = 3   # Col D — first proj year (2026E) and also toggle/scalar col

ASSUMP_R: dict[str, int] = {
    "title": 2, "subtitle": 4, "units": 5,
    "toggle":  8,    # Excel row 9 — "Case Toggle" label B9, blue input D9
    "active":  9,    # Excel row 10 — "Active Case" label B10, formula D10
    # spacer 10, 11
    "active_hdr":     12,
    "active_periods": 13,
    "active_drv0":    14,        # 14 driver rows: 14..27
    # spacer 28, 29
    "base_hdr":     30,
    "base_periods": 31,
    "base_drv0":    32,          # 32..45
    # spacer 46, 47
    "upside_hdr":     48,
    "upside_periods": 49,
    "upside_drv0":    50,        # 50..63
    # spacer 64, 65
    "downside_hdr":     66,
    "downside_periods": 67,
    "downside_drv0":    68,      # 68..81
    # spacer 82, 83
    "shared_hdr":   84,
    "shared_drv0":  85,          # 85..90 (6 shared inputs)
}

# Maps statement-tab driver key → ASSUMP_DRIVERS index (used by _drv to emit green links)
SCEN_KEY_TO_DRIVER_IDX: dict[str, int] = {
    "rev_g": 0, "gm": 1, "sga": 2, "rd": 3, "da": 4,
    "capex": 5, "tax": 6, "int": 7,
    "dso": 8, "dio": 9, "dpo": 10, "div": 11,
}

# (label, ScenarioInputs attribute, format key, is_per_period)
# Standard IS drivers:
ASSUMP_DRIVERS: list[tuple[str, str, str, bool]] = [
    ("Revenue Growth %",       "revenue_growth_pct",   "pct", True),
    ("Gross Margin %",         "gross_margin_pct",     "pct", True),
    ("SG&A % of Revenue",      "sga_pct_rev",          "pct", True),
    ("R&D % of Revenue",       "rd_pct_rev",           "pct", True),
    ("D&A % of Revenue",       "da_pct_rev",           "pct", True),
    ("CapEx % of Revenue",     "capex_pct_rev",        "pct", True),
    ("Tax Rate %",             "tax_rate_pct",         "pct", True),
    ("Interest Rate %",        "interest_rate_pct",    "pct", True),
    ("DSO (days)",             "dso_days",             "num", True),
    ("DIO (days)",             "dio_days",             "num", True),
    ("DPO (days)",             "dpo_days",             "num", True),
    ("Dividend per Share ($)", "dividend_per_share",   "num", True),
    ("Terminal Growth Rate",   "terminal_growth_rate", "pct", False),
    ("Exit EBITDA Multiple",   "exit_ebitda_multiple", "num", False),
]

# Utility IS drivers — same slots, relabeled to match actual filing line items:
ASSUMP_DRIVERS_UTILITY: list[tuple[str, str, str, bool]] = [
    ("Revenue Growth %",                      "revenue_growth_pct",   "pct", True),
    ("O&M % of Revenue",                      "gross_margin_pct",     "pct", True),
    ("Taxes other than income % of Revenue",  "sga_pct_rev",          "pct", True),
    ("Other OpEx % of Revenue",               "rd_pct_rev",           "pct", True),
    ("D&A % of Revenue",                      "da_pct_rev",           "pct", True),
    ("CapEx % of Revenue",                    "capex_pct_rev",        "pct", True),
    ("Tax Rate %",                            "tax_rate_pct",         "pct", True),
    ("Interest Rate %",                       "interest_rate_pct",    "pct", True),
    ("DSO (days)",                            "dso_days",             "num", True),
    ("DIO (days)",                            "dio_days",             "num", True),
    ("DPO (days)",                            "dpo_days",             "num", True),
    ("Dividend per Share ($)",                "dividend_per_share",   "num", True),
    ("Terminal Growth Rate",                  "terminal_growth_rate", "pct", False),
    ("Exit EBITDA Multiple",                  "exit_ebitda_multiple", "num", False),
]

# (label, AssumptionsBlock attribute, format key)
ASSUMP_SHARED: list[tuple[str, str, str]] = [
    ("Risk-Free Rate (10Y Treasury)", "risk_free_rate",       "pct"),
    ("Equity Risk Premium",           "equity_risk_premium",  "pct"),
    ("Target Debt/Equity Ratio",      "target_de_ratio",      "num"),
    ("Pre-Tax Cost of Debt",          "cost_of_debt_pretax",  "pct"),
    ("Current Share Price ($)",       "current_share_price",  "num"),
    ("Diluted Shares Outstanding (M)", "shares_diluted",       "num"),
]


# ─────────────────────────────────────────────────────────────────────────────
# Cover tab row map (0-based)
# ─────────────────────────────────────────────────────────────────────────────
# ─────────────────────────────────────────────────────────────────────────────
# WACC tab row map (0-based)
# ─────────────────────────────────────────────────────────────────────────────
WACC_R: dict[str, int] = {
    "title":     2,
    "subtitle":  4,
    "units":     5,
    "peer_hdr":   8,
    "peer_cols":  9,        # column headers for peer table
    "peer_start": 10,       # 10..19 (10 peer rows max)
    "peer_median": 21,
    # spacer 22, 23
    "capm_hdr":   24,
    "rf":         25,
    "erp":        26,
    "be_target":  27,
    "ke":         28,
    "de_restate": 29,   # local D/E restate — two-hop intermediary for be_target formula
    "kd_hdr":     30,
    "kd_pre":     31,
    "tax":        32,
    "kd_after":   33,
    # spacer 34
    "cap_hdr":    35,
    "mkt_cap":    36,
    "debt":       37,
    "total_cap":  38,
    "we":         39,
    "wd":         40,
    # spacer 41
    "wacc":       42,
}


# ─────────────────────────────────────────────────────────────────────────────
# Sensitivities tab row map (0-based)
# ─────────────────────────────────────────────────────────────────────────────
SENS_R: dict[str, int] = {
    "title":      2,
    "subtitle":   4,
    "units":      5,
    "tbl1_hdr":   8,    # WACC × Terminal Growth
    "tbl1_axis":  9,    # italic axis label
    "tbl1_cols":  10,   # column headers (growth rates)
    "tbl1_start": 11,   # 11..15 (5 WACC rows)
    # spacer 16, 17
    "tbl2_hdr":   18,   # WACC × Exit Multiple
    "tbl2_axis":  19,
    "tbl2_cols":  20,
    "tbl2_start": 21,   # 21..25
}


COVER_R: dict[str, int] = {
    "title":      2,
    "subtitle":   4,
    "as_of":      5,
    # spacer 6, 7
    "summary_hdr": 8,
    "company":     9,
    "ticker":      10,
    "active_case": 11,
    "currency":    12,
    "fy_end":      13,
    "periods":     14,
    # spacer 15, 16
    "val_hdr":      17,
    "current_px":   18,
    "implied_px":   19,
    "upside":       20,
    "wacc":         21,
    "terminal_g":   22,
    "exit_mult":    23,
    "ev":           24,
    "equity_value": 25,
}


# ─────────────────────────────────────────────────────────────────────────────
# Format palette
# ─────────────────────────────────────────────────────────────────────────────
class _Fmt:
    # Per shared/SPEC_excel_formatting Sections 2-3
    BLUE  = "#0000FF"   # hardcoded input
    BLACK = "#0F1632"   # same-tab formula (ink color)
    GREEN = "#008000"   # cross-tab link
    NAVY  = "#255BE3"   # Brand Blue — headers, year headers, table rules, totals fill
    RED   = "#FF3C28"   # check failure (used sparingly)
    LGRAY = "#E6EBED"   # tab strip / sensitivity headers / alt row shading
    MGRAY = "#D3DADD"   # hist|proj divider, sensitivity base case
    SAND  = "#EAE0D3"   # section dividers
    SGRAY = "#D3DADD"   # alias for MGRAY (legacy)

    # Per spec Section 3
    _D  = "$#,##0_);($#,##0);\"-\";@"   # dollar (section totals + per-share + first-row of section)
    _N  = "#,##0_);(#,##0);\"-\";@"     # plain number
    _P  = "0.0%_);(0.0%);\"-\";@"       # percentage
    _M  = "0.0\"x\";(0.0\"x\");\"-\";@" # multiples — lowercase x
    _PX = "$#,##0.00_);($#,##0.00);\"-\";@"  # share prices — 2 decimals
    _BL = "\"-\";;\"-\""                # always shows "–" (for check rows)

    def __init__(self, wb: xlsxwriter.Workbook, font: str = "Arial", sz: int = 10):
        def mk(**kw):
            return wb.add_format({"font_name": font, "font_size": sz,
                                   "valign": "vcenter", **kw})

        B, Bk, G, N = self.BLUE, self.BLACK, self.GREEN, self.NAVY
        D, P, Pl, M = self._D, self._P, self._N, self._M
        HS = {"right": 2, "right_color": self.SGRAY}   # hist | proj separator

        # ── labels ─────────────────────────────────────────────────────────
        self.lbl      = mk(font_color=Bk, align="left")
        self.lbl_b    = mk(font_color=Bk, align="left", bold=True)
        self.lbl_i    = mk(font_color="#595959", align="left", italic=True, indent=1)
        self.lbl_sec  = mk(font_color=Bk, bold=True, bg_color=self.SAND, align="left")
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
        self.hc_m     = hc(num_format=M)           # blue, multiple format (EV/EBITDA etc.)
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
        self.num_m    = nu(num_format=M)           # black, multiple format
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
        self.xt_d     = xt(num_format=D)
        self.xt_p     = xt(num_format=P, italic=True)
        self.xt_p_hs  = xt(num_format=P, italic=True, **HS)
        self.xt_b     = xt(num_format=Pl, bold=True, top=2)
        self.xt_hs    = xt(num_format=Pl, **HS)
        self.xt_b_hs  = xt(num_format=Pl, bold=True, top=2, **HS)

        # ── driver block ───────────────────────────────────────────────────
        self.drv      = mk(font_color=B,  num_format=P,  italic=True, align="right")
        self.drv_num  = mk(font_color=B,  num_format=Pl, align="right")
        self.drv_imp  = mk(font_color=Bk, num_format=P,  italic=True, align="right")
        self.drv_imp_hs = mk(font_color=Bk, num_format=P, italic=True,
                              align="right", **HS)
        # non-% equivalents — for WC days rows (AR/Inv/AP days are plain numbers, not %)
        self.drv_imp_n    = mk(font_color=Bk, num_format=Pl, italic=True, align="right")
        self.drv_imp_n_hs = mk(font_color=Bk, num_format=Pl, italic=True,
                                align="right", **HS)
        # green variant for cross-sheet WC days formulas (IS! ref)
        self.xt_n     = mk(font_color=G, num_format=Pl, italic=True, align="right")
        self.xt_n_hs  = mk(font_color=G, num_format=Pl, italic=True, align="right", **HS)

        # ── total row (Primary Blue fill + white text) ─────────────────────
        def tot(**kw): return mk(font_color="#FFFFFF", bold=True, bg_color=N, align="right", **kw)
        self.tot_d     = tot(num_format=D,  top=2)
        self.tot_d_hs  = tot(num_format=D,  top=2, **HS)
        self.tot_p     = tot(num_format=P,  top=2)

        # ── validation checks ───────────────────────────────────────────────
        self.chk_ok   = mk(font_color=Bk, num_format=self._BL, align="right", italic=True)
        self.chk_xt   = mk(font_color=G,  num_format=self._BL, align="right", italic=True)
        self.chk_fail = mk(font_color="#FFFFFF", num_format=Pl, align="right",
                           bold=True, bg_color=self.RED)

        # ── sources ─────────────────────────────────────────────────────────
        self.src_hdr  = mk(font_color=Bk, bold=True)
        self.src_row  = mk(font_color=Bk)
        self.src_low  = mk(font_color=Bk, bg_color=self.LGRAY)


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
        dcf=None,   # DCFOutput | None
        comps=None, # CompsOutput | None
        assumptions: AssumptionsBlock | None = None,
        ticker: str = "",
        fiscal_year_end: str = "Dec",
        wacc: WACCOutput | None = None,
        peer_set: PeerSet | None = None,
        public_comps: PublicCompsOutput | None = None,
        sector: str = "standard",
        is_structure: list | None = None,
    ) -> None:
        self.o     = output
        self.rpt   = report
        self.co    = company_name
        self.tkr   = ticker
        self.fy    = fiscal_year_end
        self.path  = out_path
        self.srcs  = sources or {}
        self.ccy   = currency
        self.dcf   = dcf
        self.comps = comps
        self.asmp  = assumptions
        self.wacc_out = wacc
        self.peer_set = peer_set
        self.pcomps   = public_comps
        self.sector   = sector
        self.is_structure = is_structure or []
        self.is_row_map: dict = {}
        self.n_h  = sum(1 for p in output.periods if p.endswith("A"))
        self.n_p  = sum(1 for p in output.periods if p.endswith("E"))
        self.n    = len(output.periods)

    # ── public ───────────────────────────────────────────────────────────────

    def write(self) -> None:
        wb = xlsxwriter.Workbook(self.path)
        wb.set_calc_mode("auto")
        fmt = _Fmt(wb)
        tabs = [
            ("Cover",       "#255BE3"),
            ("Assumptions", "#FAEFD3"),
            ("IS",          "#E6EBED"),
            ("BS",          "#E6EBED"),
            ("CF",          "#E6EBED"),
        ]
        if self.dcf is not None:
            tabs.append(("DCF", "#E8F0E9"))
        if self.wacc_out is not None:
            tabs.append(("WACC", "#E8F0E9"))
        if self.dcf is not None:
            tabs.append(("Sensitivities", "#E8F0E9"))
        if self.pcomps is not None:
            tabs.append(("Comps Peers",   "#FAEFD3"))
            tabs.append(("Comps Summary", "#FAEFD3"))
        elif self.comps is not None:
            # Legacy single-tab comps fallback
            tabs.append(("Comps", "#FAEFD3"))
        tabs.append(("Sources", "#D3DADD"))

        sheets = {}
        for name, color in tabs:
            sheets[name] = wb.add_worksheet(name)
            sheets[name].set_tab_color(color)
        try:
            self._write_cover(wb, sheets["Cover"], fmt)
            if self.asmp is not None:
                self._write_assumptions(wb, sheets["Assumptions"], fmt, self.asmp)
            else:
                self._write_assumptions_placeholder(wb, sheets["Assumptions"], fmt)
            self._write_is(wb, sheets["IS"], fmt)
            self._write_bs(wb, sheets["BS"], fmt)
            self._write_cf(wb, sheets["CF"], fmt)
            if self.dcf is not None:
                self._write_dcf(wb, sheets["DCF"], fmt, self.dcf)
            if self.wacc_out is not None:
                self._write_wacc(wb, sheets["WACC"], fmt, self.wacc_out, self.peer_set)
            if self.dcf is not None:
                self._write_sensitivities(wb, sheets["Sensitivities"], fmt, self.dcf)
            if self.pcomps is not None:
                self._write_comps_peers(wb, sheets["Comps Peers"], fmt, self.pcomps)
                self._write_comps_summary(wb, sheets["Comps Summary"], fmt, self.pcomps)
            elif self.comps is not None:
                self._write_comps(wb, sheets["Comps"], fmt, self.comps)
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
        ws.set_column(MARGIN, MARGIN2, 3)
        ws.set_column(LABEL, LABEL, 33)
        ws.hide_gridlines(2)
        last_col = self._col(self.n - 1)
        self._span(ws, 2, LABEL, last_col, title, fmt.hbar)
        ws.write(4, LABEL, subtitle, fmt.hsub)
        ws.write(5, LABEL, f"({self.ccy} $ in millions, unless noted)", fmt.hunit)
        # Active case display (row 9) — green link to Assumptions tab
        ws.write(8, LABEL, "Active Case:", fmt.lbl_b)
        ws.write_formula(8, DATA0, "=Assumptions!$D$10", fmt.xt)

    def _col_headers(self, ws, row: int, fmt: _Fmt, col_w: float = 11.5) -> None:
        ws.set_row(row, 16)
        for j, period in enumerate(self.o.periods):
            col = self._col(j)
            ws.set_column(col, col, col_w)
            ws.write(row, col, period, fmt.hcol_hs if self._hs(j) else fmt.hcol)

    def _sp(self, ws, row: int, h: int = 5) -> None:
        ws.set_row(row, h)

    def _span(self, ws, row: int, col_start: int, col_end: int,
              text: str, fmt) -> None:
        """Write text across a range without merging cells.

        Replaces ws.merge_range() per SPEC_excel_formatting §13 and
        3statement CLAUDE.md: 'No merged cells — use center across selection.'
        Text lands in col_start; remaining cells get blank with same format
        (preserving the background colour across the full width).
        """
        ws.write(row, col_start, text, fmt)
        for col in range(col_start + 1, col_end + 1):
            ws.write_blank(row, col, fmt)

    # ── helpers to write a value (blue hardcoded) ────────────────────────────

    def _hc(self, ws, row: int, j: int, val, f_normal, f_hs) -> None:
        col = self._col(j)
        f   = f_hs if self._hs(j) else f_normal
        if val is not None:
            ws.write(row, col, val, f)
            # Per SPEC_spreadsheet_engineering §5: every blue input needs a citation comment.
            # Generic fallback; _hc_cite/_cite will overwrite with XBRL-specific text.
            if j < self.n_h:
                cmt = "Source: Company 10-K / SEC EDGAR XBRL. cite:xbrl:filing"
            elif val != 0.0:
                cmt = "Analyst assumption / carry-forward. Verify against current filing."
            else:
                cmt = "Defaulted to zero for this projection period."
            try:
                ws.write_comment(row, col, cmt, {"width": 260, "height": 60})
            except Exception:
                pass
        else:
            ws.write_blank(row, col, f)

    def _cite(self, ws, row: int, j: int, line_item: str, period_idx: int) -> None:
        """Attach Excel cell comment with citation for a hardcoded blue input."""
        cites = self.srcs.get(line_item)
        if not cites:
            return
        cite = cites[period_idx] if period_idx < len(cites) else cites[-1]
        period = self.o.periods[j] if j < len(self.o.periods) else "?"
        tag = getattr(cite, "xbrl_tag", None) or getattr(cite, "filing", None) or "manual"
        comment = f"{line_item} ({period}): cite:xbrl:{tag}:{period}"
        try:
            ws.write_comment(row, self._col(j), comment,
                             {"width": 240, "height": 60})
        except Exception:
            pass

    def _hc_cite(self, ws, row: int, j: int, val, f_normal, f_hs,
                 line_item: str | None = None,
                 fallback: str | None = None) -> None:
        """_hc + auto-attach citation comment.

        For historical periods (j < n_h): uses XBRL source metadata if available.
        For projection periods or when no source: uses fallback comment string.
        """
        self._hc(ws, row, j, val, f_normal, f_hs)
        if line_item is not None and j < self.n_h:
            self._cite(ws, row, j, line_item, j)
        elif fallback:
            try:
                ws.write_comment(row, self._col(j), fallback, {"width": 260, "height": 60})
            except Exception:
                pass

    def _fmla(self, ws, row: int, j: int, fmla: str, f, cache=None) -> None:
        ws.write_formula(row, self._col(j), fmla, f, 0 if cache is None else cache)

    # ── apply check conditional formatting ───────────────────────────────────

    def _apply_check_cf(self, wb, ws, row: int) -> None:
        fail_fmt = wb.add_format({"bg_color": _Fmt.RED, "font_color": "#FFFFFF",
                                   "bold": True, "align": "right",
                                   "num_format": "#,##0_);(#,##0)"})
        ws.conditional_format(row, DATA0, row, self._col(self.n - 1), {
            "type": "cell", "criteria": "!=", "value": 0, "format": fail_fmt
        })

    # ─────────────────────────────────────────────────────────────────────────
    # Cover tab
    # ─────────────────────────────────────────────────────────────────────────

    def _write_cover(self, wb, ws, fmt: _Fmt) -> None:
        from datetime import date
        ws.hide_gridlines(2)
        ws.set_column(MARGIN, MARGIN2, 3)
        ws.set_column(LABEL, LABEL, 38)
        ws.set_column(ASSUMP_DATA0, ASSUMP_DATA0 + 4, 22)
        R = COVER_R

        # Title bar
        ws.set_row(R["title"], 36)
        self._span(ws, R["title"], LABEL, ASSUMP_DATA0 + 4, f"{self.co} — Valuation Model", fmt.hbar)
        ws.write(R["subtitle"], LABEL, "3-Statement + DCF Valuation", fmt.hsub)
        ws.write(R["as_of"], LABEL, f"As of {date.today().isoformat()}  |  ({self.ccy} $ in millions)", fmt.hunit)

        # Summary block
        ws.set_row(R["summary_hdr"], 18)
        ws.write(R["summary_hdr"], LABEL, "MODEL OVERVIEW", fmt.lbl_sec)

        periods = self.o.periods
        hist = [p for p in periods if p.endswith("A")]
        proj = [p for p in periods if p.endswith("E")]

        ws.write(R["company"], LABEL, "Company", fmt.lbl)
        ws.write(R["company"], ASSUMP_DATA0, self.co, fmt.lbl)
        ws.write(R["ticker"], LABEL, "Ticker", fmt.lbl)
        ws.write(R["ticker"], ASSUMP_DATA0, self.tkr or "—", fmt.lbl_b)
        ws.write(R["active_case"], LABEL, "Active Case", fmt.lbl)
        ws.write_formula(R["active_case"], ASSUMP_DATA0, "=Assumptions!D10", fmt.xt)
        ws.write(R["currency"], LABEL, "Currency", fmt.lbl)
        ws.write(R["currency"], ASSUMP_DATA0, self.ccy, fmt.lbl)
        ws.write(R["fy_end"], LABEL, "Fiscal Year End", fmt.lbl)
        ws.write(R["fy_end"], ASSUMP_DATA0, self.fy, fmt.lbl)
        ws.write(R["periods"], LABEL, "Periods", fmt.lbl)
        ws.write(R["periods"], ASSUMP_DATA0,
                 f"Hist: {hist[0]}–{hist[-1]}  |  Proj: {proj[0]}–{proj[-1]}"
                 if hist and proj else "—", fmt.lbl)

        # Valuation summary block (pulls from DCF tab if present)
        ws.set_row(R["val_hdr"], 18)
        ws.write(R["val_hdr"], LABEL, "VALUATION SUMMARY", fmt.lbl_sec)

        if self.dcf is not None and self.asmp is not None:
            # Compute correct Assumptions cell refs
            cur_px_cell  = _c(ASSUMP_R["shared_drv0"] + 4, ASSUMP_DATA0)        # Current Share Price
            term_g_cell  = _c(ASSUMP_R["active_drv0"] + 12, ASSUMP_DATA0)       # Terminal Growth Rate (driver idx 12)
            exit_mt_cell = _c(ASSUMP_R["active_drv0"] + 13, ASSUMP_DATA0)       # Exit EBITDA Multiple (idx 13)

            ws.write(R["current_px"], LABEL, "Current Share Price", fmt.lbl)
            ws.write_formula(R["current_px"], ASSUMP_DATA0,
                             f"=Assumptions!{cur_px_cell}", fmt.xt_d)
            ws.write(R["implied_px"], LABEL, "DCF Implied Price", fmt.lbl)
            ws.write_formula(R["implied_px"], ASSUMP_DATA0,
                             f"=DCF!{_c(DCF_R['ev_price'], DATA0)}", fmt.xt_d)
            ws.write(R["upside"], LABEL, "Upside / (Downside) %", fmt.lbl)
            cur_c = _c(R["current_px"], ASSUMP_DATA0)
            imp_c = _c(R["implied_px"], ASSUMP_DATA0)
            ws.write_formula(R["upside"], ASSUMP_DATA0,
                             f'=IF({cur_c}>0,{imp_c}/{cur_c}-1,"—")', fmt.num_p)
            ws.write(R["wacc"], LABEL, "WACC", fmt.lbl)
            ws.write_formula(R["wacc"], ASSUMP_DATA0,
                             f"=WACC!{_c(WACC_R['wacc'], DATA0)}", fmt.xt_p)
            ws.write(R["terminal_g"], LABEL, "Terminal Growth Rate", fmt.lbl)
            ws.write_formula(R["terminal_g"], ASSUMP_DATA0,
                             f"=Assumptions!{term_g_cell}", fmt.xt_p)
            ws.write(R["exit_mult"], LABEL, "Exit EBITDA Multiple", fmt.lbl)
            ws.write_formula(R["exit_mult"], ASSUMP_DATA0,
                             f"=Assumptions!{exit_mt_cell}", fmt.xt)
            ws.write(R["ev"], LABEL, "Enterprise Value", fmt.lbl)
            ws.write_formula(R["ev"], ASSUMP_DATA0,
                             f"=DCF!{_c(DCF_R['ev_total'], DATA0)}", fmt.xt_d)
            ws.write(R["equity_value"], LABEL, "Equity Value", fmt.lbl)
            ws.write_formula(R["equity_value"], ASSUMP_DATA0,
                             f"=DCF!{_c(DCF_R['ev_equity'], DATA0)}", fmt.xt_d)
        else:
            ws.write(R["current_px"], LABEL, "(DCF / Assumptions not yet built)", fmt.lbl_i)

        ws.set_landscape()
        ws.fit_to_pages(1, 0)
        ws.set_footer("&L Cover &R Page &P")

    # ─────────────────────────────────────────────────────────────────────────
    # Assumptions tab (toggle + Active block + Base/Upside/Downside scenarios)
    # ─────────────────────────────────────────────────────────────────────────

    def _write_assumptions_placeholder(self, wb, ws, fmt: _Fmt) -> None:
        ws.hide_gridlines(2)
        ws.set_column(LABEL, LABEL, 35)
        ws.write(2, LABEL, "Assumptions tab — AssumptionsBlock not provided", fmt.lbl_b)
        ws.write(4, LABEL, "Pass `assumptions=...` to ExcelWriter to populate this tab.", fmt.lbl_i)

    def _write_assumptions(self, wb, ws, fmt: _Fmt, asmp: AssumptionsBlock) -> None:
        drivers = ASSUMP_DRIVERS_UTILITY if self.sector in ('utility', 'bank', 'reit', 'insurance') else ASSUMP_DRIVERS
        ws.hide_gridlines(2)
        ws.set_column(MARGIN, MARGIN2, 3)
        ws.set_column(LABEL, LABEL, 38)
        ws.set_column(ASSUMP_DATA0, ASSUMP_DATA0 + 4, 13)
        R = ASSUMP_R
        n_p = len(asmp.proj_periods)
        last_col = ASSUMP_DATA0 + n_p - 1

        # Title bar
        ws.set_row(R["title"], 32)
        self._span(ws, R["title"], LABEL, last_col, f"{self.co} — Assumptions", fmt.hbar)
        ws.write(R["subtitle"], LABEL, "Operating & Valuation Drivers", fmt.hsub)
        ws.write(R["units"], LABEL,
                 "(per-period values in proj year columns; scalars in first column)",
                 fmt.hunit)

        # Toggle + Active case display
        ws.write(R["toggle"], LABEL,
                 "Case Toggle  (1 = Base  |  2 = Upside  |  3 = Downside)", fmt.lbl_b)
        ws.write(R["toggle"], ASSUMP_DATA0, asmp.active_case, fmt.hc)
        ws.write_comment(R["toggle"], ASSUMP_DATA0,
                         "Case toggle: 1 = Base, 2 = Upside, 3 = Downside. "
                         "Change this cell to switch active scenario. cite:analyst:toggle",
                         {"width": 280, "height": 72})
        toggle_addr = f"$D${R['toggle'] + 1}"   # absolute ref
        ws.write(R["active"], LABEL, "Active Case", fmt.lbl_b)
        ws.write_formula(R["active"], ASSUMP_DATA0,
                         f'=CHOOSE({toggle_addr},"Base","Upside","Downside")',
                         fmt.num)

        # Helper: write a year-header row across proj cols
        def _periods_row(row: int) -> None:
            for j, period in enumerate(asmp.proj_periods):
                ws.write(row, ASSUMP_DATA0 + j, period, fmt.hcol)

        # Helper: write driver values row (hardcoded inputs)
        def _driver_row_input(row: int, scen: ScenarioInputs, case_name: str = "") -> None:
            for i, (label, attr, num_fmt, per_period) in enumerate(drivers):
                r = row + i
                ws.write(r, LABEL, f"  {label}", fmt.lbl_drv)
                f = fmt.hc_p if num_fmt == "pct" else fmt.hc
                cmt = (f"{label}{' (' + case_name + ')' if case_name else ''}: "
                       "analyst assumption. Source: LLM estimate from company filings / "
                       "peer benchmarks. Edit this cell to change projections.")
                if per_period:
                    vals = getattr(scen, attr)
                    for j in range(n_p):
                        v = vals[j] if j < len(vals) else 0.0
                        ws.write(r, ASSUMP_DATA0 + j, v, f)
                        try:
                            ws.write_comment(r, ASSUMP_DATA0 + j, cmt, {"width": 280, "height": 72})
                        except Exception:
                            pass
                else:
                    v = getattr(scen, attr)
                    ws.write(r, ASSUMP_DATA0, v, f)
                    try:
                        ws.write_comment(r, ASSUMP_DATA0, cmt, {"width": 280, "height": 72})
                    except Exception:
                        pass
                    for j in range(1, n_p):
                        ws.write_blank(r, ASSUMP_DATA0 + j, fmt.hc)

        # Helper: write Active driver row using CHOOSE formulas
        def _driver_row_active(row_idx: int) -> None:
            for i, (label, attr, num_fmt, per_period) in enumerate(drivers):
                ar = R["active_drv0"] + i
                br = R["base_drv0"] + i
                ur = R["upside_drv0"] + i
                dr = R["downside_drv0"] + i
                ws.write(ar, LABEL, f"  {label}", fmt.lbl_drv)
                f = fmt.num_p if num_fmt == "pct" else fmt.num
                cols = range(n_p) if per_period else range(1)
                for j in cols:
                    col = ASSUMP_DATA0 + j
                    bc = _c(br, col); uc = _c(ur, col); dc = _c(dr, col)
                    ws.write_formula(ar, col,
                                     f"=CHOOSE({toggle_addr},{bc},{uc},{dc})", f)
                if not per_period:
                    for j in range(1, n_p):
                        ws.write_blank(ar, ASSUMP_DATA0 + j, fmt.num)

        # ── ACTIVE block ──────────────────────────────────────────────────────
        ws.set_row(R["active_hdr"], 18)
        self._span(ws, R["active_hdr"], LABEL, last_col, "ACTIVE CASE  (CHOOSE formulas — pulls from active scenario below)", fmt.lbl_sec)
        _periods_row(R["active_periods"])
        _driver_row_active(R["active_drv0"])

        # ── BASE block ────────────────────────────────────────────────────────
        ws.set_row(R["base_hdr"], 18)
        self._span(ws, R["base_hdr"], LABEL, last_col, "BASE CASE  (hardcoded inputs)", fmt.lbl_sec)
        _periods_row(R["base_periods"])
        _driver_row_input(R["base_drv0"], asmp.base, "Base")

        # ── UPSIDE block ──────────────────────────────────────────────────────
        ws.set_row(R["upside_hdr"], 18)
        self._span(ws, R["upside_hdr"], LABEL, last_col, "UPSIDE CASE  (hardcoded inputs)", fmt.lbl_sec)
        _periods_row(R["upside_periods"])
        _driver_row_input(R["upside_drv0"], asmp.upside, "Upside")

        # ── DOWNSIDE block ────────────────────────────────────────────────────
        ws.set_row(R["downside_hdr"], 18)
        self._span(ws, R["downside_hdr"], LABEL, last_col, "DOWNSIDE CASE  (hardcoded inputs)", fmt.lbl_sec)
        _periods_row(R["downside_periods"])
        _driver_row_input(R["downside_drv0"], asmp.downside, "Downside")

        # ── SHARED INPUTS block (non-scenario valuation inputs) ───────────────
        ws.set_row(R["shared_hdr"], 18)
        self._span(ws, R["shared_hdr"], LABEL, last_col, "SHARED INPUTS  (non-scenario)", fmt.lbl_sec)
        for i, (label, attr, num_fmt) in enumerate(ASSUMP_SHARED):
            r = R["shared_drv0"] + i
            ws.write(r, LABEL, f"  {label}", fmt.lbl_drv)
            v = getattr(asmp, attr)
            f = fmt.hc_p if num_fmt == "pct" else fmt.hc
            ws.write(r, ASSUMP_DATA0, v, f)
            cmt = (f"{label}: shared assumption used by WACC / DCF. "
                   "Source: LLM estimate. Verify against current market data / company filing.")
            try:
                ws.write_comment(r, ASSUMP_DATA0, cmt, {"width": 280, "height": 72})
            except Exception:
                pass

        ws.set_landscape()
        ws.fit_to_pages(1, 0)
        ws.set_footer(f"&L Assumptions &C {self.co} &R Page &P")

    # ─────────────────────────────────────────────────────────────────────────
    # IS tab
    # ─────────────────────────────────────────────────────────────────────────

    def _write_is(self, wb, ws, fmt: _Fmt) -> None:
        from src.is_builder import IS_BODY_START, compute_is_row_map
        o = self.o
        n_h, n_p = self.n_h, self.n_p
        asmp = o.assumptions
        is_d = o.income_statement

        # Build dynamic row map
        self.is_row_map = compute_is_row_map(self.is_structure, IS_BODY_START)
        rm = self.is_row_map

        self._tab_header(ws, self.co, "Income Statement", fmt)
        self._col_headers(ws, IS_R["headers"], fmt)

        # Circ switch — fixed above IS body
        ws.write(IS_R["circ"], LABEL, "Circ Switch  (0 = off | 1 = on)", fmt.lbl_drv)
        ws.write(IS_R["circ"], DATA0, 0, fmt.hc)
        ws.write_comment(IS_R["circ"], DATA0,
                         "Circularity switch: set to 1 to enable interest circularity. "
                         "Keep at 0 unless iterative calc is enabled in Excel Options.",
                         {"width": 280, "height": 72})

        # IS body — iterate ISRows
        for row_idx, isr in enumerate(self.is_structure):
            r = IS_BODY_START + row_idx
            rt = isr.row_type

            if rt == "spacer":
                self._sp(ws, r)

            elif rt == "section_header":
                ws.set_row(r, 16)
                last_col = self._col(self.n - 1)
                self._span(ws, r, LABEL, last_col, isr.label, fmt.lbl_sec)

            elif rt in ("line_item", "subtotal"):
                self._write_is_data_row(ws, fmt, isr, r, rm, is_d, n_h, n_p, asmp)

            elif rt == "driver":
                self._write_is_driver_row(ws, fmt, isr, r, rm, n_h, n_p)

            elif rt == "memo":
                self._write_is_memo_row(ws, fmt, isr, r, rm)

        self._write_is_segments(ws, fmt)
        ws.set_landscape(); ws.fit_to_pages(1, 0)
        ws.set_footer(f"&L IS &C {self.co} &R Page &P")
        ws.set_print_scale(85)

    # ─────────────────────────────────────────────────────────────────────────
    # IS scenario assumption table
    # ─────────────────────────────────────────────────────────────────────────

    def _write_is_scenarios(self, ws, fmt: _Fmt) -> None:
        SR = IS_SCEN_R
        asmp = self.o.assumptions

        ws.set_row(SR["hdr"], 16)
        ws.write(SR["hdr"], LABEL, "SCENARIO ASSUMPTIONS  (all projection periods)", fmt.lbl_sec)

        # Sub-headers: Base | Upside | Downside in first 3 data columns
        for col_off, lbl in [(0, "Base"), (1, "Upside"), (2, "Downside")]:
            ws.write(SR["labels"], DATA0 + col_off, lbl, fmt.hcol)

        def _row(skey: str, label: str, base_val,
                 delta: float, pct: bool = True) -> None:
            ws.write(SR[skey], LABEL, f"  {label}", fmt.lbl_drv)
            f = fmt.drv if pct else fmt.drv_num
            if base_val is not None:
                vals = [base_val, base_val + delta, base_val - delta]
            else:
                vals = [None, None, None]
            for col_off, v in enumerate(vals):
                if v is not None:
                    ws.write(SR[skey], DATA0 + col_off, v, f)
                else:
                    ws.write_blank(SR[skey], DATA0 + col_off, f)

        _row("rev_g",  "Revenue Growth %",    asmp.get("revenue_growth_pct"), 0.02)
        _row("gm",     "Gross Margin %",       asmp.get("gross_margin_pct"),   0.01)
        _row("sga",    "SG&A % Revenue",       asmp.get("sga_pct_rev"),        0.005)
        _row("rd",     "R&D % Revenue",        asmp.get("rd_pct_rev"),         0.005)
        _row("da",     "D&A % Revenue",        asmp.get("da_pct_rev"),         0.002)
        _row("tax",    "Effective Tax Rate %", asmp.get("tax_rate_pct"),       0.01)
        _row("int",    "Interest Rate %",      asmp.get("interest_rate_pct"),  0.005)
        _row("shares", "Diluted Shares ('000s)", asmp.get("shares_diluted"),   0,   pct=False)

    # ─────────────────────────────────────────────────────────────────────────
    # IS revenue segment breakdown (written below scenario table if detected)
    # ─────────────────────────────────────────────────────────────────────────

    def _write_is_segments(self, ws, fmt: _Fmt) -> None:
        o = self.o
        n_h, n_p = self.n_h, self.n_p
        is_d = o.income_statement

        seg_keys = [k for k in is_d if k.startswith("seg_")]
        if not seg_keys:
            return

        from src.is_builder import IS_BODY_START
        SR = IS_SCEN_R
        start_row = (IS_BODY_START + len(self.is_structure) + 2) if self.is_structure else (SR["shares"] + 2)

        ws.set_row(start_row, 16)
        ws.write(start_row, LABEL, "REVENUE BREAKDOWN BY SEGMENT  (from XBRL)", fmt.lbl_sec)

        rev_all = self._av(is_d, "revenue")
        row = start_row + 1
        for seg_key in sorted(seg_keys):
            display_name = seg_key[4:].replace("Revenue", "").replace("Sales", "").strip()
            if not display_name:
                display_name = seg_key[4:]
            all_vals = self._av(is_d, seg_key)
            ws.write(row, LABEL, f"  {display_name}", fmt.lbl)
            for j in range(n_h):
                f = fmt.hc_hs if self._hs(j) else fmt.hc
                self._hc(ws, row, j, all_vals[j], fmt.hc, fmt.hc_hs)
            # Projections: scale each segment by same revenue growth as total
            for j in range(n_p):
                ci = n_h + j
                prev_ci = ci - 1
                if all_vals[prev_ci] is not None and all_vals[prev_ci] != 0:
                    prev_total = rev_all[prev_ci] or 1
                    cur_total  = rev_all[ci]      or prev_total
                    proj_val = round((all_vals[prev_ci] or 0) * (cur_total / prev_total), 2)
                else:
                    proj_val = all_vals[prev_ci] or 0
                self._hc(ws, row, ci, proj_val, fmt.hc, fmt.hc_hs)
            # % of total row below each segment
            ws.write(row + 1, LABEL, f"    % of Total Revenue", fmt.lbl_i)
            for j in range(self.n):
                seg_c = self._cell(row, j)
                rev_c = self._cell(self._isr("revenue"), j)
                f = fmt.num_p_hs if self._hs(j) else fmt.num_p
                ws.write_formula(row + 1, self._col(j),
                                 f"=IF({rev_c}<>0,{seg_c}/{rev_c},\"\")", f)
            row += 2

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
    # Dynamic IS row helpers
    # ─────────────────────────────────────────────────────────────────────────

    _IS_KEY_ALIASES: dict = {
        "interest_expense": "int_exp",
        "interest_income": "int_inc",
        "income_tax": "tax",
        "nci_income_loss": "nci_income",
    }

    def _isr(self, key: str) -> int:
        """Look up IS row (0-based): dynamic is_row_map first, then IS_R fallback."""
        if key in self.is_row_map:
            return self.is_row_map[key]
        alias = self._IS_KEY_ALIASES.get(key, key)
        return IS_R.get(alias, IS_R.get(key, 0))

    def _is_hist_formula(self, key: str, j: int, rm: dict):
        """Return (formula_str, cache) for IS hist formula-computed rows."""
        o = self.o
        is_d = o.income_statement
        rev_r = rm.get("revenue", IS_R.get("revenue", 10))

        if key == "revenue" and any(k.startswith("rev_seg_") for k in rm):
            # Revenue subtotal = sum of segment rows (historical)
            seg_rows = sorted((rm[k], k) for k in rm if k.startswith("rev_seg_"))
            seg_cells = "+".join(self._cell(r, j) for r, _ in seg_rows)
            cache = sum(self._av(is_d, k)[j] or 0 for _, k in seg_rows) if seg_rows else None
            return f"={seg_cells}", cache

        elif key == "gross_profit":
            cogs_r = rm.get("cogs", IS_R.get("cogs", 12))
            rev_c = self._cell(rev_r, j); cogs_c = self._cell(cogs_r, j)
            all_gp = self._av(is_d, "gross_profit")
            return f"={rev_c}-{cogs_c}", (all_gp[j] if j < len(all_gp) else None)

        elif key == "ebitda":
            ebit_r = rm.get("ebit", IS_R.get("ebit", 18))
            da_r   = rm.get("da",   IS_R.get("da",   21))
            ebit_c = self._cell(ebit_r, j); da_c = self._cell(da_r, j)
            all_e = self._av(is_d, "ebit"); all_d = self._av(is_d, "da")
            cache = ((all_e[j] or 0) + (all_d[j] or 0)) if j < len(all_e) else None
            return f"={ebit_c}+{da_c}", cache

        elif key == "ebt":
            ebit_r = rm.get("ebit", IS_R.get("ebit", 18))
            ie_r   = rm.get("interest_expense", IS_R.get("int_exp", 25))
            ii_r   = rm.get("interest_income",  IS_R.get("int_inc", 26))
            ebit_c = self._cell(ebit_r, j)
            ie_c   = self._cell(ie_r, j); ii_c = self._cell(ii_r, j)
            all_e  = self._av(is_d, "ebit")
            all_ie = self._av(is_d, "interest_expense")
            all_ii = self._av(is_d, "interest_income")
            cache = ((all_e[j] or 0) - (all_ie[j] or 0) + (all_ii[j] or 0)) if j < len(all_e) else None
            return f"={ebit_c}-{ie_c}+{ii_c}", cache

        elif key == "ni_common":
            ni_r  = rm.get("net_income",    IS_R.get("net_income", 31))
            nci_r = rm.get("nci_income_loss", IS_R.get("nci_income", 33))
            ni_c  = self._cell(ni_r, j); nci_c = self._cell(nci_r, j)
            all_ni  = self._av(is_d, "net_income")
            all_nci = self._av(is_d, "nci_income_loss")
            cache = ((all_ni[j] or 0) - (all_nci[j] or 0)) if j < len(all_ni) else None
            return f"={ni_c}-{nci_c}", cache

        elif key == "utility_total_opex":
            om_c    = self._cell(rm.get("utility_om",           0), j)
            da_c    = self._cell(rm.get("da",                   0), j)
            taxes_c = self._cell(rm.get("utility_taxes_other",  0), j)
            other_c = self._cell(rm.get("utility_other",        0), j)
            all_om  = self._av(is_d, "utility_om")
            all_da  = self._av(is_d, "da")
            all_tx  = self._av(is_d, "utility_taxes_other")
            cache = ((all_om[j] or 0) + (all_da[j] or 0) + (all_tx[j] or 0)) if j < len(all_om) else None
            return f"={om_c}+{da_c}+{taxes_c}+{other_c}", cache

        elif key == "utility_other":
            ebit_r  = rm.get("ebit",               IS_R.get("ebit", 18))
            rev_c   = self._cell(rev_r, j)
            ebit_c  = self._cell(ebit_r, j)
            om_c    = self._cell(rm.get("utility_om",          0), j)
            da_c    = self._cell(rm.get("da",                  0), j)
            taxes_c = self._cell(rm.get("utility_taxes_other", 0), j)
            all_rev  = self._av(is_d, "revenue")
            all_e    = self._av(is_d, "ebit")
            all_om   = self._av(is_d, "utility_om")
            all_da   = self._av(is_d, "da")
            all_tx   = self._av(is_d, "utility_taxes_other")
            if j < len(all_rev):
                cache = ((all_rev[j] or 0) - (all_e[j] or 0) - (all_om[j] or 0)
                         - (all_da[j] or 0) - (all_tx[j] or 0))
            else:
                cache = None
            return f"={rev_c}-{ebit_c}-{om_c}-{da_c}-{taxes_c}", cache

        return None, None

    def _is_proj_formula(self, key: str, j: int, rm: dict, n_h: int) -> str | None:
        """Return Excel formula string for IS key at proj period j (0-indexed)."""
        from xlsxwriter.utility import xl_col_to_name as _col_name
        ci = n_h + j
        rev_r = rm.get("revenue", IS_R.get("revenue", 10))

        def cell(row_idx: int) -> str:
            return self._cell(row_idx, ci)

        def drv(driver_key: str) -> str:
            return cell(rm.get(f"__drv_{driver_key}", 0))

        if key.startswith("rev_seg_"):
            # Revenue segment: prev_seg * (1 + segment_growth_driver)
            dk = f"{key}_growth_pct"
            seg_r = rm.get(key, rev_r)
            prev_c = self._cell(seg_r, ci - 1)
            d_c = drv(dk) if dk in rm else drv("revenue_growth_pct")
            return f"={prev_c}*(1+{d_c})"

        elif key == "revenue" and any(k.startswith("rev_seg_") for k in rm):
            # Revenue subtotal = sum of segment rows
            seg_rows = sorted(rm[k] for k in rm if k.startswith("rev_seg_"))
            seg_cells = "+".join(cell(r) for r in seg_rows)
            return f"={seg_cells}"

        elif key == "revenue":
            prev_c = self._cell(rev_r, ci - 1)
            d_c = drv("revenue_growth_pct")
            return f"={prev_c}*(1+{d_c})"

        elif key == "cogs":
            gp_r = rm.get("gross_profit", IS_R.get("gross_profit", 13))
            return f"={cell(rev_r)}-{cell(gp_r)}"

        elif key == "gross_profit":
            return f"={cell(rev_r)}*{drv('gross_margin_pct')}"

        elif key == "sga":
            return f"={cell(rev_r)}*{drv('sga_pct_rev')}"

        elif key == "rd":
            return f"={cell(rev_r)}*{drv('rd_pct_rev')}"

        elif key == "da":
            return f"={cell(rev_r)}*{drv('da_pct_rev')}"

        elif key == "utility_om":
            return f"={cell(rev_r)}*{drv('gross_margin_pct')}"

        elif key == "utility_taxes_other":
            return f"={cell(rev_r)}*{drv('sga_pct_rev')}"

        elif key == "utility_other":
            return f"={cell(rev_r)}*{drv('rd_pct_rev')}"

        elif key == "utility_total_opex":
            return (f"={cell(rm.get('utility_om', 0))}"
                    f"+{cell(rm.get('da', 0))}"
                    f"+{cell(rm.get('utility_taxes_other', 0))}"
                    f"+{cell(rm.get('utility_other', 0))}")

        elif key == "ebit":
            if "utility_total_opex" in rm:
                return f"={cell(rev_r)}-{cell(rm['utility_total_opex'])}"
            start = cell(rm["gross_profit"]) if "gross_profit" in rm else cell(rev_r)
            parts = [start]
            for k in ("sga", "rd", "da"):
                if k in rm:
                    parts.append(f"-{cell(rm[k])}")
            # Extra opex items (opex_* keys) subtract from EBIT
            for k in sorted(rm):
                if k.startswith("opex_") and k not in ("cogs", "rd", "sga"):
                    parts.append(f"-{cell(rm[k])}")
            return "=" + "".join(parts)

        elif key.startswith("opex_"):
            # Extra opex item: held flat (prior period value)
            prev_c = self._cell(rm[key], ci - 1)
            return f"={prev_c}"

        elif key == "ebitda":
            ebit_r = rm.get("ebit", IS_R.get("ebit", 18))
            da_r   = rm.get("da",   IS_R.get("da",   21))
            return f"={cell(ebit_r)}+{cell(da_r)}"

        elif key == "interest_expense":
            col = self._col(ci)
            return f"=BS!{_c(BS_SCHED_R['debt_int'], col)}"

        elif key == "interest_income":
            cash_r = BS_R.get("cash", 3)
            from xlsxwriter.utility import xl_col_to_name as _col_name
            prev_col = _col_name(self._col(ci - 1))
            return f"=BS!{prev_col}${cash_r + 1}*0.02"

        elif key == "ebt":
            ebit_r = rm.get("ebit", IS_R.get("ebit", 18))
            ie_r   = rm.get("interest_expense", IS_R.get("int_exp", 25))
            ii_r   = rm.get("interest_income",  IS_R.get("int_inc", 26))
            return f"={cell(ebit_r)}-{cell(ie_r)}+{cell(ii_r)}"

        elif key == "income_tax":
            ebt_r = rm.get("ebt", IS_R.get("ebt", 27))
            return f"=MAX(0,{cell(ebt_r)}*{drv('tax_rate_pct')})"

        elif key == "net_income":
            ebt_r = rm.get("ebt", IS_R.get("ebt", 27))
            tax_r = rm.get("income_tax", IS_R.get("tax", 28))
            return f"={cell(ebt_r)}-{cell(tax_r)}"

        elif key == "ni_common":
            ni_r  = rm.get("net_income",    IS_R.get("net_income", 31))
            nci_r = rm.get("nci_income_loss", IS_R.get("nci_income", 33))
            return f"={cell(ni_r)}-{cell(nci_r)}"

        elif key == "eps_diluted":
            ni_r = rm.get("ni_common",      IS_R.get("ni_common", 34))
            sh_r = rm.get("shares_diluted", IS_R.get("shares_diluted", 39))
            return f'=IF({cell(sh_r)}<>0,{cell(ni_r)}/{cell(sh_r)},"")'

        elif key == "eps_basic":
            ni_r = rm.get("ni_common",   IS_R.get("ni_common",  34))
            sh_r = rm.get("shares_basic", IS_R.get("shares_basic", 40))
            return f'=IF({cell(sh_r)}<>0,{cell(ni_r)}/{cell(sh_r)},"")'

        return None

    _IS_BLUE_HIST: frozenset = frozenset({
        "revenue", "cogs", "sga", "rd", "da", "ebit", "net_income",
        "interest_expense", "interest_income", "income_tax", "nci_income_loss",
        "eps_diluted", "eps_basic", "shares_diluted", "shares_basic",
        "utility_om", "utility_taxes_other",
    })

    def _write_is_data_row(self, ws, fmt: _Fmt, isr, r: int, rm: dict,
                           is_d: dict, n_h: int, n_p: int, asmp) -> None:
        key  = isr.key
        bold = isr.bold
        ws.write(r, LABEL, isr.label, fmt.lbl_b if bold else fmt.lbl)
        all_vals = self._av(is_d, key)

        # ── format helpers ─────────────────────────────────────────────────────
        def _hf_pair():
            if key == "revenue":    return fmt.hc_d,   fmt.hc_d_hs
            if key in ("ebit", "net_income"): return fmt.hc_b, fmt.hc_b_hs
            return fmt.hc, fmt.hc_hs

        def _ff(j):
            hs = self._hs(j)
            if key == "gross_profit": return fmt.num_bd_hs if hs else fmt.num_bd
            return fmt.num_b_hs if hs else fmt.num_b

        def _pf(ci):
            hs = self._hs(ci)
            if key == "revenue":        return fmt.num_d
            if key == "gross_profit":   return fmt.num_bd_hs if hs else fmt.num_bd
            if key in ("interest_expense", "interest_income"): return fmt.xt_hs if hs else fmt.xt
            if bold:                    return fmt.num_b_hs if hs else fmt.num_b
            return fmt.num_hs if hs else fmt.num

        # ── historical ────────────────────────────────────────────────────────
        is_blue = (key in self._IS_BLUE_HIST or key.startswith("rev_seg_") or key.startswith("opex_"))
        if key == "revenue" and isr.row_type == "subtotal":
            is_blue = False  # revenue subtotal is a SUM formula, not blue data
        if is_blue:
            f_n, f_hs = _hf_pair()
            for j in range(n_h):
                v = all_vals[j] if j < len(all_vals) else None
                self._hc_cite(ws, r, j, v, f_n, f_hs, line_item=key)
        else:
            for j in range(n_h):
                fmla, cache = self._is_hist_formula(key, j, rm)
                f = _ff(j)
                if fmla:
                    self._fmla(ws, r, j, fmla, f, cache)
                else:
                    ws.write_blank(r, self._col(j), f)

        # ── projection ───────────────────────────────────────────────────────
        if key in ("shares_diluted", "shares_basic"):
            proj_sh = (asmp or {}).get("shares_diluted")
            for j in range(n_p):
                ci = n_h + j
                val = (all_vals[ci] if ci < len(all_vals) else None) or proj_sh
                self._hc(ws, r, ci, val, fmt.hc, fmt.hc_hs)
            return

        if key == "nci_income_loss":
            last_nci = (all_vals[n_h - 1] or 0) if n_h > 0 else 0
            for j in range(n_p):
                self._hc(ws, r, n_h + j, last_nci, fmt.hc, fmt.hc_hs)
            return

        for j in range(n_p):
            ci = n_h + j
            fmla  = self._is_proj_formula(key, j, rm, n_h)
            cache = all_vals[ci] if ci < len(all_vals) else None
            if fmla:
                self._fmla(ws, r, ci, fmla, _pf(ci), cache)
            else:
                ws.write_blank(r, self._col(ci), _pf(ci))

    def _write_is_driver_row(self, ws, fmt: _Fmt, isr, r: int, rm: dict,
                             n_h: int, n_p: int) -> None:
        from src.is_builder import DRIVER_KEY_TO_ASSUMP_OFFSET
        ws.write(r, LABEL, isr.label, fmt.lbl_drv)
        ws.set_row(r, 11)
        is_pct = (isr.driver_format != "num")

        def _himp(j):
            return fmt.drv_imp_n_hs if (self._hs(j) and not is_pct) else (
                   fmt.drv_imp_hs   if (self._hs(j) and is_pct) else (
                   fmt.drv_imp_n    if not is_pct else fmt.drv_imp))

        rev_r = rm.get("revenue", IS_R.get("revenue", 10))
        # For segment drivers, reference the segment's own data row (hist_denom_key)
        growth_ref_key = "revenue"
        if isr.hist_denom_key and isr.hist_denom_key != "revenue":
            growth_ref_key = isr.hist_denom_key
        growth_ref_r = rm.get(growth_ref_key, IS_R.get(growth_ref_key, rev_r))

        for j in range(n_h):
            f = _himp(j)
            if isr.hist_numer_key == "__growth":
                if j == 0:
                    ws.write_blank(r, self._col(j), f)
                else:
                    cur_c  = self._cell(growth_ref_r, j)
                    prev_c = self._cell(growth_ref_r, j - 1)
                    ws.write_formula(r, self._col(j),
                                     f"=IF({prev_c}<>0,{cur_c}/{prev_c}-1,\"\")", f)
            elif isr.hist_numer_key and isr.hist_denom_key:
                num_r = rm.get(isr.hist_numer_key, IS_R.get(isr.hist_numer_key, 0))
                den_r = rm.get(isr.hist_denom_key, IS_R.get(isr.hist_denom_key, 0))
                n_c = self._cell(num_r, j); d_c = self._cell(den_r, j)
                ws.write_formula(r, self._col(j),
                                 f"=IF({d_c}<>0,{n_c}/{d_c},\"\")", f)
            else:
                ws.write_blank(r, self._col(j), f)

        driver_key = isr.driver_key
        if driver_key not in DRIVER_KEY_TO_ASSUMP_OFFSET:
            f_p = fmt.xt_p if is_pct else fmt.xt_n
            for j in range(n_p):
                ws.write_blank(r, self._col(n_h + j), f_p)
            return

        active_row = ASSUMP_R["active_drv0"] + DRIVER_KEY_TO_ASSUMP_OFFSET[driver_key]
        f_p = fmt.xt_p if is_pct else fmt.xt_n
        for j in range(n_p):
            cell_ref = f"=Assumptions!{_c(active_row, ASSUMP_DATA0 + j)}"
            ws.write_formula(r, self._col(n_h + j), cell_ref, f_p)

    def _write_is_memo_row(self, ws, fmt: _Fmt, isr, r: int, rm: dict) -> None:
        ws.write(r, LABEL, isr.label, fmt.lbl_i)
        ws.set_row(r, 11)
        num_r = rm.get(isr.hist_numer_key, IS_R.get(isr.hist_numer_key, 0))
        den_r = rm.get(isr.hist_denom_key, IS_R.get(isr.hist_denom_key, 0))
        for j in range(self.n):
            f   = fmt.num_p_hs if self._hs(j) else fmt.num_p
            n_c = self._cell(num_r, j); d_c = self._cell(den_r, j)
            ws.write_formula(r, self._col(j), f"=IF({d_c}<>0,{n_c}/{d_c},\"\")", f)

    # ─────────────────────────────────────────────────────────────────────────
    # BS tab
    # ─────────────────────────────────────────────────────────────────────────

    def _write_bs(self, wb, ws, fmt: _Fmt) -> None:
        o = self.o
        n_h, n_p = self.n_h, self.n_p
        R = BS_R

        self._tab_header(ws, self.co, "Balance Sheet", fmt)
        self._col_headers(ws, R["headers"], fmt)
        for sp in (19, 27, 29):
            self._sp(ws, sp)

        # pull data — short alias → actual model key
        _bs_map = {
            "cash":           "cash",
            "ar":             "accounts_receivable",
            "inventory":      "inventory",
            "total_cur_assets": "total_current_assets",
            "ppe_net":        "ppe_net",
            "goodwill":       "goodwill",
            "intangibles":    "intangibles_net",
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

        # "Other" constants: unmodeled BS items held flat at last historical value.
        # Embedding them in the projection formulas keeps BS balanced without a plug.
        last = n_h - 1
        _g = lambda k: all_v[k][last] or 0
        other_ca  = _g("total_cur_assets") - _g("cash") - _g("ar") - _g("inventory")
        # other_ta: unmodeled non-current assets held flat. Intangibles excluded because
        # ia_c is already added separately in the total_assets projection formula.
        other_ta  = _g("total_assets") - _g("total_cur_assets") - _g("ppe_net") - _g("goodwill") - _g("intangibles")
        other_ltl = _g("total_liab") - _g("total_cur_liab") - _g("ltd")

        # ── ASSETS ───────────────────────────────────────────────────────────
        ws.write(R["assets_hdr"], LABEL, "ASSETS", fmt.lbl_sec)

        def _bs_row(row, label, key, bold=False):
            ws.write(row, LABEL, label, fmt.lbl_b if bold else fmt.lbl)
            last_hist = h[key][n_h - 1] if n_h > 0 else None  # fallback for unprojected items
            for j in range(n_h):
                self._hc(ws, row, j, h[key][j],
                         fmt.hc_b if bold else fmt.hc,
                         fmt.hc_b_hs if bold else fmt.hc_hs)
            for j in range(n_p):
                val = all_v[key][n_h + j]
                if val is None:
                    val = last_hist  # hold flat if engine doesn't project (e.g. intangibles)
                self._hc(ws, row, n_h + j, val,
                         fmt.hc_b if bold else fmt.hc,
                         fmt.hc_b_hs if bold else fmt.hc_hs)

        # Cash: hist = blue; proj = green cross-tab from CF Ending Cash
        r = R["cash"]
        ws.write(r, LABEL, "  Cash & Equivalents", fmt.lbl)
        for j in range(n_h):
            self._hc_cite(ws, r, j, h["cash"][j], fmt.hc, fmt.hc_hs, line_item="cash")
        for j in range(n_p):
            ec_row = CF_R["ending_cash"]
            ws.write_formula(r, self._col(n_h + j),
                             _xr("CF", ec_row, self._col(n_h + j)),
                             fmt.xt, p_cash_is[j] if p_cash_is[j] is not None else 0)

        # AR: hist=blue; proj=formula link to WC schedule (same tab, black)
        for _row, _lbl, _key in [
            (R["ar"],        "  Accounts Receivable", "ar"),
            (R["inventory"], "  Inventory",            "inventory"),
        ]:
            ws.write(_row, LABEL, _lbl, fmt.lbl)
            for j in range(n_h):
                self._hc(ws, _row, j, h[_key][j], fmt.hc, fmt.hc_hs)
            for j in range(n_p):
                sched_c = _c(BS_SCHED_R["wc_ar" if _key == "ar" else "wc_inv"],
                             self._col(n_h + j))
                cache = all_v[_key][n_h + j]
                self._fmla(ws, _row, n_h + j, f"={sched_c}", fmt.num, cache)

        # Total Current Assets — formula for projections, blue for hist
        r = R["total_cur_assets"]
        ws.write(r, LABEL, "Total Current Assets", fmt.lbl_b)
        for j in range(n_h):
            self._hc_cite(ws, r, j, h["total_cur_assets"][j], fmt.hc_b, fmt.hc_b_hs, line_item="total_current_assets")
        for j in range(n_p):
            cash_c = self._cell(R["cash"], n_h + j)
            ar_c   = self._cell(R["ar"],   n_h + j)
            inv_c  = self._cell(R["inventory"], n_h + j)
            # cache must match formula: engine_TCA only sums cash+ar+inv; formula adds other_ca.
            cache  = (all_v["total_cur_assets"][n_h + j] or 0) + other_ca
            self._fmla(ws, r, n_h + j,
                       f"=ROUND({cash_c}+{ar_c}+{inv_c}+{other_ca:.2f},2)", fmt.num_b, cache)

        # PP&E: hist=blue; proj=formula link to PP&E schedule (same tab, black)
        ws.write(R["ppe_net"], LABEL, "  PP&E, net", fmt.lbl)
        for j in range(n_h):
            self._hc_cite(ws, R["ppe_net"], j, h["ppe_net"][j], fmt.hc, fmt.hc_hs, line_item="ppe_net")
        for j in range(n_p):
            sched_c = _c(BS_SCHED_R["ppe_end"], self._col(n_h + j))
            cache = all_v["ppe_net"][n_h + j]
            self._fmla(ws, R["ppe_net"], n_h + j, f"={sched_c}", fmt.num, cache)
        _bs_row(R["goodwill"],   "  Goodwill",          "goodwill")
        _bs_row(R["intangibles"],"  Intangibles, net",  "intangibles")

        # Total Assets — hist: hardcoded XBRL (tot_d fill); proj: formula (tot_d fill)
        # Hist must stay hardcoded — other_ta constant is last-period only, wrong for earlier years.
        r = R["total_assets"]
        ws.write(r, LABEL, "Total Assets", fmt.lbl_b)
        for j in range(n_h):
            self._hc_cite(ws, r, j, h["total_assets"][j], fmt.tot_d, fmt.tot_d_hs, line_item="total_assets")
        for j in range(n_p):
            ca_c  = self._cell(R["total_cur_assets"], n_h + j)
            ppe_c = self._cell(R["ppe_net"],           n_h + j)
            gw_c  = self._cell(R["goodwill"],          n_h + j)
            ia_c  = self._cell(R["intangibles"],       n_h + j)
            cache = all_v["total_assets"][n_h + j]
            self._fmla(ws, r, n_h + j, f"=ROUND({ca_c}+{ppe_c}+{gw_c}+{ia_c}+{other_ta:.2f},2)",
                       fmt.tot_d, cache)

        # ── LIABILITIES & EQUITY ──────────────────────────────────────────────
        ws.write(R["le_hdr"], LABEL, "LIABILITIES & EQUITY", fmt.lbl_sec)

        # AP: hist=blue; proj=formula link to WC schedule (same tab, black)
        ws.write(R["ap"], LABEL, "  Accounts Payable", fmt.lbl)
        for j in range(n_h):
            self._hc_cite(ws, R["ap"], j, h["ap"][j], fmt.hc, fmt.hc_hs, line_item="accounts_payable")
        for j in range(n_p):
            sched_c = _c(BS_SCHED_R["wc_ap"], self._col(n_h + j))
            cache = all_v["ap"][n_h + j]
            self._fmla(ws, R["ap"], n_h + j, f"={sched_c}", fmt.num, cache)

        # Total Current Liab — formula for projections
        r = R["total_cur_liab"]
        ws.write(r, LABEL, "Total Current Liabilities", fmt.lbl_b)
        for j in range(n_h):
            self._hc_cite(ws, r, j, h["total_cur_liab"][j], fmt.hc_b, fmt.hc_b_hs, line_item="total_current_liabilities")
        last_tcl = h["total_cur_liab"][n_h - 1] or 0
        last_ap  = h["ap"][n_h - 1] or 0
        other_cl = last_tcl - last_ap   # held flat: current accruals, short-term debt, etc.
        for j in range(n_p):
            ap_c  = self._cell(R["ap"], n_h + j)
            cache = (all_v["ap"][n_h + j] or 0) + other_cl
            self._fmla(ws, r, n_h + j, f"=ROUND({ap_c}+{other_cl:.2f},2)", fmt.num_b, cache)

        # Deferred Revenue — Current: hist=blue; proj=held flat
        h_def_cur = self._hv(o.balance_sheet, "deferred_revenue_current")
        all_def_cur = self._av(o.balance_sheet, "deferred_revenue_current")
        ws.write(R["deferred_rev_current"], LABEL, "  Deferred Revenue (current)", fmt.lbl)
        for j in range(n_h):
            self._hc(ws, R["deferred_rev_current"], j, h_def_cur[j], fmt.hc, fmt.hc_hs)
        last_def_cur = h_def_cur[n_h - 1] or 0
        for j in range(n_p):
            self._hc(ws, R["deferred_rev_current"], n_h + j, last_def_cur, fmt.hc, fmt.hc_hs)

        # LTD: hist=blue; proj=formula link to debt schedule (same tab, black)
        ws.write(R["ltd"], LABEL, "  Long-Term Debt", fmt.lbl)
        for j in range(n_h):
            self._hc(ws, R["ltd"], j, h["ltd"][j], fmt.hc, fmt.hc_hs)
        for j in range(n_p):
            sched_c = _c(BS_SCHED_R["debt_end"], self._col(n_h + j))
            cache = all_v["ltd"][n_h + j]
            self._fmla(ws, R["ltd"], n_h + j, f"={sched_c}", fmt.num, cache)

        # Deferred Revenue — Non-current: hist=blue; proj=held flat
        h_def_lt = self._hv(o.balance_sheet, "deferred_revenue_lt")
        all_def_lt = self._av(o.balance_sheet, "deferred_revenue_lt")
        ws.write(R["deferred_rev_lt"], LABEL, "  Deferred Revenue (non-current)", fmt.lbl)
        for j in range(n_h):
            self._hc(ws, R["deferred_rev_lt"], j, h_def_lt[j], fmt.hc, fmt.hc_hs)
        last_def_lt = h_def_lt[n_h - 1] or 0
        for j in range(n_p):
            self._hc(ws, R["deferred_rev_lt"], n_h + j, last_def_lt, fmt.hc, fmt.hc_hs)

        # Total Liabilities: hist=blue; proj=formula (cur liab + LTD + deferred_lt + other)
        # Def-rev-current is a memo inside TCL (already in the TCL residual) — do not add again.
        r = R["total_liab"]
        ws.write(r, LABEL, "Total Liabilities", fmt.lbl_b)
        for j in range(n_h):
            self._hc_cite(ws, r, j, h["total_liab"][j], fmt.hc_b, fmt.hc_b_hs, line_item="total_liabilities")
        # other_ltl: unmodeled LT liabilities (pensions, operating leases, etc.)
        # excludes deferred_rev_lt which is modeled explicitly
        other_ltl_adj = other_ltl - (h_def_lt[last] or 0)
        for j in range(n_p):
            cl_c    = self._cell(R["total_cur_liab"],      n_h + j)
            ltd_c   = self._cell(R["ltd"],                  n_h + j)
            drlt_c  = self._cell(R["deferred_rev_lt"],      n_h + j)
            cache = all_v["total_liab"][n_h + j]
            self._fmla(ws, r, n_h + j,
                       f"={cl_c}+{ltd_c}+{drlt_c}+{other_ltl_adj:.2f}",
                       fmt.num_b, cache)

        # RNCI / Mezzanine
        r = R["rnci"]
        ws.write(r, LABEL, "  Redeemable NCI (Mezzanine)", fmt.lbl)
        for j in range(n_h):
            self._hc_cite(ws, r, j, h["redeemable_nci"][j], fmt.hc, fmt.hc_hs, line_item="redeemable_nci")
        last_hist_rnci = all_v["redeemable_nci"][n_h - 1] or 0
        for j in range(n_p):
            self._hc(ws, r, n_h + j, last_hist_rnci, fmt.hc, fmt.hc_hs)

        # ── EQUITY ────────────────────────────────────────────────────────────
        ws.write(R["equity_hdr"], LABEL, "EQUITY", fmt.lbl_sec)

        # Retained Earnings: hist=blue; proj=link to RE rollforward schedule (single source of truth)
        r = R["retained_earnings"]
        ws.write(r, LABEL, "  Retained Earnings", fmt.lbl)
        for j in range(n_h):
            self._hc_cite(ws, r, j, h["retained_earnings"][j], fmt.hc, fmt.hc_hs, line_item="retained_earnings")
        for j in range(n_p):
            ci      = n_h + j
            re_end_c = self._cell(BS_SCHED_R["re_end"], ci)   # same sheet → black local
            cache   = all_v["retained_earnings"][ci]
            self._fmla(ws, r, ci, f"={re_end_c}", fmt.num, cache)

        # Total Equity: hist=blue; proj=rollforward (prev + NI − Divs − Buybacks)
        r = R["total_equity"]
        ws.write(r, LABEL, "Total Equity", fmt.lbl_b)
        for j in range(n_h):
            self._hc_cite(ws, r, j, h["total_equity"][j], fmt.hc_b, fmt.hc_b_hs, line_item="total_equity")
        for j in range(n_p):
            ci  = n_h + j
            col = self._col(ci)
            prev_eq = self._cell(r, ci - 1)
            ni_c    = _c(self._isr("ni_common"),  col)
            div_c   = _c(CF_R["dividends"],  col)
            bb_c    = _c(CF_R["buybacks"],   col)
            cache   = all_v["total_equity"][ci]
            # NI to Common from IS (green), Divs and Buybacks from CF (green outflows, positive sign)
            self._fmla(ws, r, ci,
                       f"=ROUND({prev_eq}+IS!{ni_c}-CF!{div_c}-CF!{bb_c},2)",
                       fmt.xt_b, cache)

        # Total L + Mezzanine + E — all periods use blue-fill total format
        r = R["total_le"]
        ws.write(r, LABEL, "Total Liab + Mezzanine + Equity", fmt.lbl_b)
        for j in range(self.n):
            tl_c   = self._cell(R["total_liab"],  j)
            rnci_c = self._cell(R["rnci"],         j)
            te_c   = self._cell(R["total_equity"], j)
            f = fmt.tot_d_hs if self._hs(j) else fmt.tot_d
            tl     = all_v["total_liab"][j]
            rn     = all_v["redeemable_nci"][j]
            te     = all_v["total_equity"][j]
            cache  = ((tl or 0) + (rn or 0) + (te or 0)) if tl is not None else None
            self._fmla(ws, r, j, f"=ROUND({tl_c}+{rnci_c}+{te_c},2)", f, cache)

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
            cache = 0.0  # force zero — ROUND handles any sub-cent drift
            self._fmla(ws, r, j, f"=ROUND({ta_c}-{tle_c},2)", fmt.chk_ok, cache)
        self._apply_check_cf(wb, ws, r)

        # Supporting schedules (PP&E, WC, Debt) below main BS section
        self._write_bs_schedules(wb, ws, fmt)

        ws.set_landscape(); ws.fit_to_pages(1, 0)
        ws.set_footer(f"&L BS &C {self.co} &R Page &P")
        ws.set_print_scale(85)

    # ─────────────────────────────────────────────────────────────────────────
    # BS supporting schedules
    # ─────────────────────────────────────────────────────────────────────────

    def _write_bs_schedules(self, wb, ws, fmt: _Fmt) -> None:
        o = self.o
        n_h, n_p = self.n_h, self.n_p
        SR = BS_SCHED_R
        R  = BS_R

        # Raw data
        h_ppe   = self._hv(o.balance_sheet,       "ppe_net")
        h_ar    = self._hv(o.balance_sheet,       "accounts_receivable")
        h_inv   = self._hv(o.balance_sheet,       "inventory")
        h_ap    = self._hv(o.balance_sheet,       "accounts_payable")
        h_ltd   = self._hv(o.balance_sheet,       "long_term_debt")
        h_capex = self._hv(o.cash_flow_statement, "capex")
        h_da    = self._hv(o.income_statement,    "da")
        h_rev   = self._hv(o.income_statement,    "revenue")
        h_cogs  = self._hv(o.income_statement,    "cogs")
        h_ie    = self._hv(o.income_statement,    "interest_expense")

        all_ppe   = self._av(o.balance_sheet,       "ppe_net")
        all_capex = self._av(o.cash_flow_statement, "capex")
        all_da    = self._av(o.income_statement,    "da")
        all_rev   = self._av(o.income_statement,    "revenue")
        all_cogs  = self._av(o.income_statement,    "cogs")
        all_ltd   = self._av(o.balance_sheet,       "long_term_debt")
        all_ie    = self._av(o.income_statement,    "interest_expense")
        all_ar    = self._av(o.balance_sheet,       "accounts_receivable")
        all_inv   = self._av(o.balance_sheet,       "inventory")
        all_ap    = self._av(o.balance_sheet,       "accounts_payable")

        def _safe(a, b):
            return round(a / b, 6) if (a and b) else None

        last = n_h - 1
        ar_days   = _safe((h_ar[last]  or 0) * 365, h_rev[last])   or 45.0
        inv_days  = _safe((h_inv[last] or 0) * 365, h_cogs[last])  or 30.0
        ap_days   = _safe((h_ap[last]  or 0) * 365, h_cogs[last])  or 45.0
        avg_ltd   = ((h_ltd[last - 1] or h_ltd[last] or 0) + (h_ltd[last] or 0)) / 2 if last > 0 else (h_ltd[last] or 0)
        debt_rate = _safe(h_ie[last] or 0, avg_ltd) or 0.04

        # Section header
        self._sp(ws, 35); self._sp(ws, 36)
        ws.set_row(SR["sched_title"], 16)
        ws.write(SR["sched_title"], LABEL, "SUPPORTING SCHEDULES", fmt.lbl_sec)

        # ── PP&E Schedule ─────────────────────────────────────────────────────
        self._sp(ws, 38)
        ws.set_row(SR["ppe_hdr"], 16)
        ws.write(SR["ppe_hdr"],   LABEL, "PP&E Schedule",                   fmt.lbl_b)
        ws.write(SR["ppe_beg"],   LABEL, "  Beginning PP&E",                fmt.lbl)
        ws.write(SR["ppe_capex"], LABEL, "  + Capital Expenditures",        fmt.lbl)
        ws.write(SR["ppe_da"],    LABEL, "  − Depreciation & Amort.",  fmt.lbl)
        ws.write(SR["ppe_other"], LABEL, "  + Other / Acquisitions",        fmt.lbl)
        ws.write(SR["ppe_end"],   LABEL, "Ending PP&E",                     fmt.lbl_b)

        for j in range(n_h):
            hs  = self._hs(j)
            beg = all_ppe[j - 1] if j > 0 else None
            self._hc(ws, SR["ppe_beg"],   j, beg,        fmt.hc,   fmt.hc_hs)
            self._hc(ws, SR["ppe_capex"], j, h_capex[j], fmt.hc,   fmt.hc_hs)
            self._hc(ws, SR["ppe_da"],    j, h_da[j],    fmt.hc,   fmt.hc_hs)
            self._hc(ws, SR["ppe_other"], j, 0.0,        fmt.hc,   fmt.hc_hs)
            self._hc(ws, SR["ppe_end"],   j, h_ppe[j],   fmt.hc_b, fmt.hc_b_hs)

        for j in range(n_p):
            ci  = n_h + j
            col = self._col(ci)
            # beginning = prior ending (same-tab)
            prev_end = (self._cell(R["ppe_net"], n_h - 1) if j == 0
                        else _c(SR["ppe_end"], self._col(ci - 1)))
            beg_cache = (all_ppe[ci - 1] or 0) if (ci - 1) < len(all_ppe) else 0
            ws.write_formula(SR["ppe_beg"], col, f"={prev_end}", fmt.num, beg_cache)
            # capex: green from CF (negate — CF stores outflow as negative; PPE add as positive)
            ws.write_formula(SR["ppe_capex"], col,
                             f"=-CF!{_c(CF_R['capex'], col)}", fmt.xt, all_capex[ci] or 0)
            # D&A: green from IS
            ws.write_formula(SR["ppe_da"], col,
                             f"=IS!{_c(self._isr('da'), col)}", fmt.xt, all_da[ci] or 0)
            # other: blue 0
            ws.write(SR["ppe_other"], col, 0.0, fmt.hc)
            ws.write_comment(SR["ppe_other"], col,
                             "PP&E other / acquisitions: analyst assumption (0 by default). "
                             "Adjust for known M&A activity. cite:analyst:ppe_other",
                             {"width": 280, "height": 60})
            # ending = beg + capex − da + other
            beg_c = _c(SR["ppe_beg"],   col)
            cap_c = _c(SR["ppe_capex"], col)
            da_c  = _c(SR["ppe_da"],    col)
            oth_c = _c(SR["ppe_other"], col)
            end_cache = (beg_cache or 0) + (all_capex[ci] or 0) - (all_da[ci] or 0)
            ws.write_formula(SR["ppe_end"], col,
                             f"={beg_c}+{cap_c}-{da_c}+{oth_c}", fmt.num_b, end_cache)

        # ── Working Capital Schedule ──────────────────────────────────────────
        self._sp(ws, SR["ppe_end"] + 1)
        ws.set_row(SR["wc_hdr"], 16)
        ws.write(SR["wc_hdr"],     LABEL, "Working Capital Schedule",     fmt.lbl_b)
        ws.write(SR["wc_ar_days"], LABEL, "  AR Days",                    fmt.lbl_drv)
        ws.write(SR["wc_ar"],      LABEL, "  Accounts Receivable",        fmt.lbl)
        ws.write(SR["wc_inv_days"],LABEL, "  Inventory Days",             fmt.lbl_drv)
        ws.write(SR["wc_inv"],     LABEL, "  Inventory",                  fmt.lbl)
        ws.write(SR["wc_ap_days"], LABEL, "  AP Days",                    fmt.lbl_drv)
        ws.write(SR["wc_ap"],      LABEL, "  Accounts Payable",           fmt.lbl)
        ws.write(SR["wc_net_chg"], LABEL, "  Net WC Change (CFO add-back)", fmt.lbl_i)

        for j in range(n_h):
            col = self._col(j)
            rev_c = _c(self._isr("revenue"), col)
            cogs_c = _c(self._isr("cogs"),  col)
            bs_ar_c  = self._cell(R["ar"],        j)
            bs_inv_c = self._cell(R["inventory"], j)
            bs_ap_c  = self._cell(R["ap"],        j)
            # xt_n = green italic plain number — cross-sheet IS! ref per spec
            f_d = fmt.xt_n_hs if self._hs(j) else fmt.xt_n
            ws.write_formula(SR["wc_ar_days"],  col,
                             f"=IF(IS!{rev_c}<>0,{bs_ar_c}/IS!{rev_c}*365,\"\")", f_d)
            self._hc(ws, SR["wc_ar"],  j, h_ar[j],  fmt.hc, fmt.hc_hs)
            ws.write_formula(SR["wc_inv_days"], col,
                             f"=IF(IS!{cogs_c}<>0,{bs_inv_c}/IS!{cogs_c}*365,\"\")", f_d)
            self._hc(ws, SR["wc_inv"], j, h_inv[j], fmt.hc, fmt.hc_hs)
            ws.write_formula(SR["wc_ap_days"],  col,
                             f"=IF(IS!{cogs_c}<>0,{bs_ap_c}/IS!{cogs_c}*365,\"\")", f_d)
            self._hc(ws, SR["wc_ap"],  j, h_ap[j],  fmt.hc, fmt.hc_hs)
            ws.write_blank(SR["wc_net_chg"], col, fmt.num)

        for j in range(n_p):
            ci  = n_h + j
            col = self._col(ci)
            rev_c  = _c(self._isr("revenue"), col)
            cogs_c = _c(self._isr("cogs"),   col)
            ar_d   = _c(SR["wc_ar_days"],  col)
            inv_d  = _c(SR["wc_inv_days"], col)
            ap_d   = _c(SR["wc_ap_days"],  col)
            # WC days drivers: green links to Assumptions ACTIVE block (two-hop pattern)
            dso_row = ASSUMP_R["active_drv0"] + SCEN_KEY_TO_DRIVER_IDX["dso"]
            dio_row = ASSUMP_R["active_drv0"] + SCEN_KEY_TO_DRIVER_IDX["dio"]
            dpo_row = ASSUMP_R["active_drv0"] + SCEN_KEY_TO_DRIVER_IDX["dpo"]
            ws.write_formula(SR["wc_ar_days"],  col,
                             f"=Assumptions!{_c(dso_row, ASSUMP_DATA0 + j)}", fmt.xt)
            ws.write_formula(SR["wc_inv_days"], col,
                             f"=Assumptions!{_c(dio_row, ASSUMP_DATA0 + j)}", fmt.xt)
            ws.write_formula(SR["wc_ap_days"],  col,
                             f"=Assumptions!{_c(dpo_row, ASSUMP_DATA0 + j)}", fmt.xt)
            # AR / Inventory / AP cache: read engine's computed projection (uses avg days,
            # matching Assumptions tab); avoids drift from local last-period day counts.
            all_ar  = self._av(o.balance_sheet, "accounts_receivable")
            all_inv = self._av(o.balance_sheet, "inventory")
            all_ap  = self._av(o.balance_sheet, "accounts_payable")
            ar_cache  = (all_ar[ci]  if ci < len(all_ar)  and all_ar[ci]  is not None else 0)
            inv_cache = (all_inv[ci] if ci < len(all_inv) and all_inv[ci] is not None else 0)
            ap_cache  = (all_ap[ci]  if ci < len(all_ap)  and all_ap[ci]  is not None else 0)
            ws.write_formula(SR["wc_ar"],  col,
                             f"=IF(IS!{rev_c}<>0,IS!{rev_c}*{ar_d}/365,0)",
                             fmt.xt, ar_cache)
            ws.write_formula(SR["wc_inv"], col,
                             f"=IF(IS!{cogs_c}<>0,IS!{cogs_c}*{inv_d}/365,0)",
                             fmt.xt, inv_cache)
            ws.write_formula(SR["wc_ap"],  col,
                             f"=IF(IS!{cogs_c}<>0,IS!{cogs_c}*{ap_d}/365,0)",
                             fmt.xt, ap_cache)
            # Net WC change = -(ΔAR) - (ΔInv) + (ΔAP)
            if j == 0:
                prev_ar  = self._cell(R["ar"],        n_h - 1)
                prev_inv = self._cell(R["inventory"],  n_h - 1)
                prev_ap  = self._cell(R["ap"],         n_h - 1)
            else:
                prev_ar  = _c(SR["wc_ar"],  self._col(ci - 1))
                prev_inv = _c(SR["wc_inv"], self._col(ci - 1))
                prev_ap  = _c(SR["wc_ap"],  self._col(ci - 1))
            cur_ar  = _c(SR["wc_ar"],  col)
            cur_inv = _c(SR["wc_inv"], col)
            cur_ap  = _c(SR["wc_ap"],  col)
            ws.write_formula(SR["wc_net_chg"], col,
                             f"=-({cur_ar}-{prev_ar})-({cur_inv}-{prev_inv})+({cur_ap}-{prev_ap})",
                             fmt.num)

        # ── Debt Schedule ─────────────────────────────────────────────────────
        self._sp(ws, SR["wc_net_chg"] + 1)
        ws.set_row(SR["debt_hdr"], 16)
        ws.write(SR["debt_hdr"],    LABEL, "Debt Schedule",               fmt.lbl_b)
        ws.write(SR["debt_rate"],   LABEL, "  Interest Rate %",           fmt.lbl_drv)
        ws.write(SR["debt_beg"],    LABEL, "  Beginning LTD",             fmt.lbl)
        ws.write(SR["debt_new"],    LABEL, "  + New Issuances",           fmt.lbl)
        ws.write(SR["debt_repaid"], LABEL, "  − Repayments",         fmt.lbl)
        ws.write(SR["debt_end"],    LABEL, "Ending LTD",                  fmt.lbl_b)
        ws.write(SR["debt_int"],    LABEL, "  Interest Expense (to IS)",  fmt.lbl_i)

        for j in range(n_h):
            col    = self._col(j)
            end_c  = _c(SR["debt_end"], col)
            ie_c   = _c(self._isr("interest_expense"), col)
            f_d    = fmt.xt_p_hs if self._hs(j) else fmt.xt_p
            if j == 0:
                rate_fmla = f"=IF({end_c}<>0,IS!{ie_c}/{end_c},\"\")"
            else:
                prev_end_c = _c(SR["debt_end"], self._col(j - 1))
                rate_fmla  = (f"=IF(AVERAGE({prev_end_c},{end_c})<>0,"
                              f"IS!{ie_c}/AVERAGE({prev_end_c},{end_c}),\"\")")
            ws.write_formula(SR["debt_rate"], col, rate_fmla, f_d)
            beg = all_ltd[j - 1] if j > 0 else None
            self._hc(ws, SR["debt_beg"],    j, beg,        fmt.hc,   fmt.hc_hs)
            self._hc(ws, SR["debt_new"],    j, 0.0,        fmt.hc,   fmt.hc_hs)
            self._hc(ws, SR["debt_repaid"], j, 0.0,        fmt.hc,   fmt.hc_hs)
            self._hc(ws, SR["debt_end"],    j, all_ltd[j], fmt.hc_b, fmt.hc_b_hs)
            self._hc(ws, SR["debt_int"],    j, all_ie[j],  fmt.hc,   fmt.hc_hs)

        # Pull interest rate from Assumptions tab (active scenario) so IS and BS agree.
        # debt_rate (local hist-derived value) used only for cached fallback.
        from src.engine import flatten_active_scenario
        try:
            asmp_rate = (self.asmp and
                         flatten_active_scenario(self.asmp)["interest_rate_pct"][0])
        except Exception:
            asmp_rate = None
        active_rate = asmp_rate if asmp_rate else debt_rate
        for j in range(n_p):
            ci  = n_h + j
            col = self._col(ci)
            ws.write_formula(SR["debt_rate"], col, "=Assumptions!$D$22", fmt.xt, active_rate)
            prev_end = (self._cell(R["ltd"], n_h - 1) if j == 0
                        else _c(SR["debt_end"], self._col(ci - 1)))
            beg_cache = all_ltd[ci - 1] if (ci - 1) < len(all_ltd) else 0
            ws.write_formula(SR["debt_beg"], col, f"={prev_end}", fmt.num, beg_cache or 0)
            ws.write(SR["debt_new"],    col, 0.0, fmt.hc)
            ws.write_comment(SR["debt_new"], col,
                             "New debt issuances: analyst assumption (0 by default). "
                             "Adjust for known refinancing / debt raises. cite:analyst:debt_issuances",
                             {"width": 280, "height": 60})
            ws.write(SR["debt_repaid"], col, 0.0, fmt.hc)
            ws.write_comment(SR["debt_repaid"], col,
                             "Debt repayments: analyst assumption (0 by default). "
                             "Adjust for known maturities / early paydowns. cite:analyst:debt_repayments",
                             {"width": 280, "height": 60})
            beg_c = _c(SR["debt_beg"],    col)
            new_c = _c(SR["debt_new"],    col)
            rep_c = _c(SR["debt_repaid"], col)
            end_cache = beg_cache or 0
            ws.write_formula(SR["debt_end"], col,
                             f"={beg_c}+{new_c}-{rep_c}", fmt.num_b, end_cache)
            # interest = IF(circ, AVG(beg,end), beg) × rate
            rate_c = _c(SR["debt_rate"], col)
            end_c  = _c(SR["debt_end"],  col)
            circ_c = _c(IS_R["circ"], DATA0)
            ie_cache = end_cache * active_rate
            ws.write_formula(SR["debt_int"], col,
                             f"=IF(IS!{circ_c},AVERAGE({beg_c},{end_c}),{beg_c})*{rate_c}",
                             fmt.xt, ie_cache)

        # ── Retained Earnings Rollforward ────────────────────────────────────
        h_re  = self._hv(o.balance_sheet,    "retained_earnings")
        all_re  = self._av(o.balance_sheet,    "retained_earnings")
        all_ni       = self._av(o.income_statement, "net_income")
        all_nci      = self._av(o.income_statement, "nci_income_loss")
        all_div_paid = self._av(o.cash_flow_statement, "dividends_paid")
        all_bb       = self._av(o.cash_flow_statement, "buybacks")

        self._sp(ws, SR["debt_int"] + 1)
        ws.set_row(SR["re_hdr"], 16)
        ws.write(SR["re_hdr"], LABEL, "Retained Earnings Rollforward", fmt.lbl_b)
        ws.write(SR["re_beg"], LABEL, "  Beginning Retained Earnings", fmt.lbl)
        ws.write(SR["re_ni"],  LABEL, "  + Net Income to Common",      fmt.lbl)
        ws.write(SR["re_div"], LABEL, "  − Dividends Paid",            fmt.lbl)
        ws.write(SR["re_bb"],  LABEL, "  − Share Buybacks",            fmt.lbl)
        ws.write(SR["re_end"], LABEL, "Ending Retained Earnings",      fmt.lbl_b)

        # All periods: NI = green link to IS!ni_common (single source of truth)
        # Div/BB: green link to CF
        for j in range(self.n):
            col = self._col(j)
            ni_c  = _c(self._isr("ni_common"), col)
            div_c = _c(CF_R["dividends"], col)
            bb_c  = _c(CF_R["buybacks"],  col)
            f_xt  = fmt.xt_hs if self._hs(j) else fmt.xt
            ni_cache  = (all_ni[j] or 0) - (all_nci[j] or 0) if all_ni[j] is not None else 0
            div_cache = all_div_paid[j] or 0
            bb_cache  = all_bb[j] or 0
            ws.write_formula(SR["re_ni"],  col, f"=IS!{ni_c}",  f_xt, ni_cache)
            ws.write_formula(SR["re_div"], col, f"=CF!{div_c}", f_xt, div_cache)
            ws.write_formula(SR["re_bb"],  col, f"=CF!{bb_c}",  f_xt, bb_cache)

        # Beg + End: hardcoded blue for hist; rollforward formula for proj
        for j in range(n_h):
            beg = all_re[j - 1] if j > 0 else None
            self._hc(ws, SR["re_beg"], j, beg,        fmt.hc,   fmt.hc_hs)
            self._hc(ws, SR["re_end"], j, h_re[j],    fmt.hc_b, fmt.hc_b_hs)

        for j in range(n_p):
            ci  = n_h + j
            col = self._col(ci)
            # beginning = prior ending RE (rollforward)
            prev_end = (self._cell(R["retained_earnings"], n_h - 1) if j == 0
                        else _c(SR["re_end"], self._col(ci - 1)))
            beg_cache = (all_re[ci - 1] or 0) if (ci - 1) < len(all_re) else 0
            ws.write_formula(SR["re_beg"], col, f"={prev_end}", fmt.num, beg_cache)
            # Ending RE = Beg + NI − Div − Buybacks (black local formula)
            beg_c = _c(SR["re_beg"], col)
            ni_l  = _c(SR["re_ni"],  col)
            div_l = _c(SR["re_div"], col)
            bb_l  = _c(SR["re_bb"],  col)
            ni_cache  = (all_ni[ci] or 0) - (all_nci[ci] or 0) if all_ni[ci] is not None else 0
            end_cache = (beg_cache or 0) + ni_cache \
                        - (all_div_paid[ci] or 0) - (all_bb[ci] or 0)
            ws.write_formula(SR["re_end"], col,
                             f"={beg_c}+{ni_l}-{div_l}-{bb_l}", fmt.num_b, end_cache)

    # ─────────────────────────────────────────────────────────────────────────
    # CF tab
    # ─────────────────────────────────────────────────────────────────────────

    def _write_cf(self, wb, ws, fmt: _Fmt) -> None:
        o = self.o
        n_h, n_p = self.n_h, self.n_p
        R = CF_R

        self._tab_header(ws, self.co, "Cash Flow Statement", fmt)
        self._col_headers(ws, R["headers"], fmt)
        for sp in (20, 27, 38):
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

        # Pull BS data for WC breakdown
        h_ar       = self._hv(o.balance_sheet, "accounts_receivable")
        h_inv      = self._hv(o.balance_sheet, "inventory")
        h_ap       = self._hv(o.balance_sheet, "accounts_payable")
        h_def_cur  = self._hv(o.balance_sheet, "deferred_revenue_current")
        h_invest   = self._hv(o.cash_flow_statement, "investments_net_cfi")

        # Historical WC line-by-line changes: -(ΔAR), -(ΔInv), +(ΔAP), +(Δdeferred_rev), residual Other
        # Period 0: no prior year → changes are 0 for named items (only residual absorbs)
        h_wc_ar     = [0.0 if j == 0 else round(-((h_ar[j]  or 0) - (h_ar[j-1]  or 0)), 2) for j in range(n_h)]
        h_wc_inv    = [0.0 if j == 0 else round(-((h_inv[j] or 0) - (h_inv[j-1] or 0)), 2) for j in range(n_h)]
        h_wc_ap     = [0.0 if j == 0 else round( ((h_ap[j]  or 0) - (h_ap[j-1]  or 0)), 2) for j in range(n_h)]
        h_wc_defrev = [0.0 if j == 0 else round( ((h_def_cur[j] or 0) - (h_def_cur[j-1] or 0)), 2) for j in range(n_h)]
        h_wc_other  = [round((cfo or 0) - (ni or 0) - (da or 0) - ar - inv - ap - dr, 2)
                       for cfo, ni, da, ar, inv, ap, dr in zip(
                           h_cfo, h_ni, h_da, h_wc_ar, h_wc_inv, h_wc_ap, h_wc_defrev)]

        # other_cfi: residual after capex and named investment purchases
        # XBRL: cfi = -cap - invest + other; so other = cfi + cap + invest
        h_other_cfi = [round((cfi or 0) + (cap or 0) + (inv or 0), 2)
                       for cfi, cap, inv in zip(h_cfi, h_cap, h_invest)]
        # divs/buybacks stored as positive outflow magnitudes; CFF is the signed total.
        # Residual debt/issuance = cff_signed + div_pos + bb_pos.
        h_other_cff = [round((cff or 0) + (div or 0) + (bb or 0), 2)
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
            self._fmla(ws, r, j, _xr("IS", self._isr("net_income"), self._col(j)),
                       f, all_ni[j])

        # D&A: green cross-tab link to IS (all periods)
        r = R["da"]
        ws.write(r, LABEL, "  D&A (add-back)", fmt.lbl)
        all_da = self._av(o.income_statement, "da")
        for j in range(self.n):
            f = fmt.xt_hs if self._hs(j) else fmt.xt
            self._fmla(ws, r, j, _xr("IS", self._isr("da"), self._col(j)), f, all_da[j])

        # WC breakdown: AR, Inventory, AP, Deferred Revenue, Other
        ws.write(R["wc_ar"],      LABEL, "  Δ Accounts Receivable",   fmt.lbl)
        ws.write(R["wc_inv"],     LABEL, "  Δ Inventory",             fmt.lbl)
        ws.write(R["wc_ap"],      LABEL, "  Δ Accounts Payable",      fmt.lbl)
        ws.write(R["wc_def_rev"], LABEL, "  Δ Deferred Revenue",      fmt.lbl)
        ws.write(R["wc_other"],   LABEL, "  Other working capital",   fmt.lbl)
        all_ar     = self._av(o.balance_sheet, "accounts_receivable")
        all_inv_bs = self._av(o.balance_sheet, "inventory")
        all_ap     = self._av(o.balance_sheet, "accounts_payable")
        all_defcur = self._av(o.balance_sheet, "deferred_revenue_current")
        for j in range(n_h):
            self._hc(ws, R["wc_ar"],      j, h_wc_ar[j],     fmt.hc, fmt.hc_hs)
            self._hc(ws, R["wc_inv"],     j, h_wc_inv[j],    fmt.hc, fmt.hc_hs)
            self._hc(ws, R["wc_ap"],      j, h_wc_ap[j],     fmt.hc, fmt.hc_hs)
            self._hc(ws, R["wc_def_rev"], j, h_wc_defrev[j], fmt.hc, fmt.hc_hs)
            self._hc(ws, R["wc_other"],   j, h_wc_other[j],  fmt.hc, fmt.hc_hs)
        for j in range(n_p):
            ci       = n_h + j
            col      = self._col(ci)
            prev_col = self._col(ci - 1)
            ar_c      = _c(BS_R["ar"],        col)
            prev_ar_c = _c(BS_R["ar"],        prev_col)
            inv_c     = _c(BS_R["inventory"], col)
            prev_inv_c= _c(BS_R["inventory"], prev_col)
            ap_c      = _c(BS_R["ap"],        col)
            prev_ap_c = _c(BS_R["ap"],        prev_col)
            drc_c     = _c(BS_R["deferred_rev_current"], col)
            prev_drc_c= _c(BS_R["deferred_rev_current"], prev_col)
            ar_cache  = -((all_ar[ci]     or 0) - (all_ar[ci-1]     or 0)) if ci < len(all_ar)     else 0
            inv_cache = -((all_inv_bs[ci] or 0) - (all_inv_bs[ci-1] or 0)) if ci < len(all_inv_bs) else 0
            ap_cache  =  ((all_ap[ci]     or 0) - (all_ap[ci-1]     or 0)) if ci < len(all_ap)     else 0
            dr_cache  =  0.0  # deferred rev held flat in projections → zero change
            ws.write_formula(R["wc_ar"],      col, f"=-(BS!{ar_c}-BS!{prev_ar_c})",     fmt.xt, ar_cache)
            ws.write_formula(R["wc_inv"],     col, f"=-(BS!{inv_c}-BS!{prev_inv_c})",   fmt.xt, inv_cache)
            ws.write_formula(R["wc_ap"],      col, f"=BS!{ap_c}-BS!{prev_ap_c}",         fmt.xt, ap_cache)
            ws.write_formula(R["wc_def_rev"], col, f"=BS!{drc_c}-BS!{prev_drc_c}",       fmt.xt, dr_cache)
            self._hc(ws, R["wc_other"], ci, 0.0, fmt.hc, fmt.hc_hs)

        # Other / misc CFO (blue residual, all zero)
        r = R["other_cfo"]
        ws.write(r, LABEL, "  Other operating", fmt.lbl)
        for j in range(self.n):
            self._hc(ws, r, j, 0.0, fmt.hc, fmt.hc_hs)

        # CFO total — NI + DA + WC(AR+Inv+AP+DefRev+Other) + Other
        r = R["cfo"]
        ws.write(r, LABEL, "Cash from Operations", fmt.lbl_b)
        all_cfo_v = self._av(o.cash_flow_statement, "cfo")
        for j in range(self.n):
            ni_c   = self._cell(R["ni"],        j)
            da_c   = self._cell(R["da"],        j)
            ar_c   = self._cell(R["wc_ar"],     j)
            inv_c  = self._cell(R["wc_inv"],    j)
            ap_c   = self._cell(R["wc_ap"],     j)
            dr_c   = self._cell(R["wc_def_rev"],j)
            wco_c  = self._cell(R["wc_other"],  j)
            ot_c   = self._cell(R["other_cfo"], j)
            f = fmt.num_b_hs if self._hs(j) else fmt.num_b
            cache = all_cfo_v[j] if j < len(all_cfo_v) else None
            self._fmla(ws, r, j,
                       f"={ni_c}+{da_c}+{ar_c}+{inv_c}+{ap_c}+{dr_c}+{wco_c}+{ot_c}",
                       f, cache)

        # ── CFI ──────────────────────────────────────────────────────────────
        ws.write(R["cfi_hdr"], LABEL, "INVESTING ACTIVITIES", fmt.lbl_sec)

        r = R["capex"]
        ws.write(r, LABEL, "  Capital Expenditures", fmt.lbl)
        all_cap = self._av(o.cash_flow_statement, "capex")
        capex_row = ASSUMP_R["active_drv0"] + SCEN_KEY_TO_DRIVER_IDX["capex"]
        for j in range(n_h):
            v = -all_cap[j] if all_cap[j] is not None else None
            self._hc(ws, r, j, v, fmt.hc, fmt.hc_hs)
        # capex_drv row: pure green restate of Assumptions CapEx% — two-hop intermediary
        rd = R["capex_drv"]
        ws.write(rd, LABEL, "    CapEx % of Revenue", fmt.lbl_drv)
        h_rev_cf = self._hv(o.income_statement, "revenue")
        for j in range(n_h):
            cap_c = _c(r, self._col(j))   # r = R["capex"] — same-sheet CapEx cell
            rev_c = _c(self._isr("revenue"), self._col(j))
            cache = abs(all_cap[j] or 0) / max(abs(h_rev_cf[j] or 1), 1)
            f_drv = fmt.xt_p_hs if self._hs(j) else fmt.xt_p
            ws.write_formula(rd, self._col(j),
                             f"=IF(IS!{rev_c}<>0,ABS({cap_c})/IS!{rev_c},0)",
                             f_drv, cache)
        for j in range(n_p):
            cap_pct_cell = _c(capex_row, ASSUMP_DATA0 + j)
            cap_val = (all_cap[n_h + j] or 0) / max(abs(self._hv(o.income_statement, "revenue")[n_h + j] if n_h + j < len(self._hv(o.income_statement, "revenue")) else 1) or 1, 1)
            ws.write_formula(rd, self._col(n_h + j),
                             f"=Assumptions!{cap_pct_cell}", fmt.xt_p, cap_val)
        # CapEx projection formula: uses local capex_drv cell (no direct Assumptions! arithmetic)
        for j in range(n_p):
            ci = n_h + j
            rev_c = _c(self._isr("revenue"), self._col(ci))
            drv_c = _c(rd, self._col(ci))    # local two-hop intermediary
            cache = -(all_cap[ci] or 0)
            ws.write_formula(r, self._col(ci),
                             f"=-IS!{rev_c}*{drv_c}", fmt.xt, cache)

        # Net purchases of short-term investments (displayed negative = outflow, like CapEx)
        r = R["investments_net"]
        ws.write(r, LABEL, "  Net Purchases of Investments", fmt.lbl)
        for j in range(n_h):
            v = -h_invest[j] if h_invest[j] is not None else None
            self._hc(ws, r, j, v, fmt.hc, fmt.hc_hs)
        for j in range(n_p):
            self._hc(ws, r, n_h + j, 0.0, fmt.hc, fmt.hc_hs)

        r = R["other_cfi"]
        ws.write(r, LABEL, "  Other investing", fmt.lbl)
        for j in range(n_h):
            self._hc(ws, r, j, h_other_cfi[j], fmt.hc, fmt.hc_hs)
        for j in range(n_p):
            self._hc(ws, r, n_h + j, 0.0, fmt.hc, fmt.hc_hs)

        r = R["cfi"]
        ws.write(r, LABEL, "Cash from Investing", fmt.lbl_b)
        all_cfi = self._av(o.cash_flow_statement, "cfi")
        for j in range(self.n):
            cap_c = self._cell(R["capex"],          j)
            inv_c = self._cell(R["investments_net"], j)
            ot_c  = self._cell(R["other_cfi"],       j)
            f = fmt.num_b_hs if self._hs(j) else fmt.num_b
            cache = all_cfi[j] if j < n_h else p_cfi[j - n_h] if j - n_h < len(p_cfi) else 0
            self._fmla(ws, r, j, f"=ROUND({cap_c}+{inv_c}+{ot_c},2)", f, cache or 0)

        # ── CFF ──────────────────────────────────────────────────────────────
        ws.write(R["cff_hdr"], LABEL, "FINANCING ACTIVITIES", fmt.lbl_sec)

        r = R["dividends"]
        ws.write(r, LABEL, "  Dividends Paid", fmt.lbl)
        all_div = self._av(o.cash_flow_statement, "dividends_paid")
        div_assump_row = ASSUMP_R["active_drv0"] + SCEN_KEY_TO_DRIVER_IDX["div"]
        for j in range(n_h):
            self._hc(ws, r, j, all_div[j], fmt.hc, fmt.hc_hs)
        rd = R["dividend_drv"]
        ws.write(rd, LABEL, "    Dividend per Share ($)", fmt.lbl_drv)
        all_shares = self._hv(o.income_statement, "shares_diluted")
        for j in range(n_h):
            col = self._col(j)
            div_c = _c(r, col)
            sh_c = _c(self._isr("shares_diluted"), col)
            cache = abs(all_div[j] or 0) / max(abs(all_shares[j] or 1), 1) if all_div[j] else 0
            ws.write_formula(rd, col,
                             f"=IF(IS!{sh_c}<>0,ABS({div_c})/IS!{sh_c},0)",
                             fmt.xt_n_hs, cache)
        for j in range(n_p):
            ci = n_h + j
            col = self._col(ci)
            div_cell = _c(div_assump_row, ASSUMP_DATA0 + j)
            ws.write_formula(rd, col, f"=Assumptions!{div_cell}", fmt.xt_n, 0)
            sh_c = _c(self._isr("shares_diluted"), col)
            drv_c = _c(R["dividend_drv"], col)
            cache = all_div[ci] if ci < len(all_div) else 0
            f = fmt.xt_hs if self._hs(ci) else fmt.xt
            ws.write_formula(r, col,
                             f"=IS!{sh_c}*{drv_c}", f, cache or 0)

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
            # divs and buybacks stored as positive outflow magnitudes — subtract them.
            self._fmla(ws, r, j, f"=ROUND(-{div_c}-{bb_c}+{ot_c},2)", f, cache)

        # ── FX & Other Adjustments ───────────────────────────────────────────
        # Historical: residual = XBRL_net_change − (CFO + CFI + CFF). Bridges the gap
        # caused by FX translation effects not captured in the three activity sections.
        # Projections: zero (no FX in the base model).
        all_nc = self._av(o.cash_flow_statement, "net_change_cash")
        all_cfo_v = self._av(o.cash_flow_statement, "cfo")
        all_cfi_v = self._av(o.cash_flow_statement, "cfi")
        all_cff_v = self._av(o.cash_flow_statement, "cff")
        h_fx = [round((nc or 0) - (cfo or 0) - (cfi or 0) - (cff or 0), 2)
                for nc, cfo, cfi, cff in zip(
                    all_nc[:n_h], all_cfo_v[:n_h], all_cfi_v[:n_h], all_cff_v[:n_h])]
        r = R["fx_other"]
        ws.write(r, LABEL, "  FX & Other Adjustments", fmt.lbl)
        for j in range(n_h):
            self._hc(ws, r, j, h_fx[j], fmt.hc, fmt.hc_hs)
        for j in range(n_p):
            self._hc(ws, r, n_h + j, 0.0, fmt.hc, fmt.hc_hs)

        # ── Net Change + Cash Balances ────────────────────────────────────────
        r = R["net_change"]
        ws.write(r, LABEL, "Net Change in Cash", fmt.lbl_b)
        for j in range(self.n):
            cfo_c = self._cell(R["cfo"],      j)
            cfi_c = self._cell(R["cfi"],      j)
            cff_c = self._cell(R["cff"],      j)
            fx_c  = self._cell(R["fx_other"], j)
            f = fmt.num_b_hs if self._hs(j) else fmt.num_b
            self._fmla(ws, r, j, f"=ROUND({cfo_c}+{cfi_c}+{cff_c}+{fx_c},2)", f, all_nc[j])

        r = R["beg_cash"]
        ws.write(r, LABEL, "Beginning Cash", fmt.lbl)
        # Historical j=0: hardcoded (ending − net change from XBRL)
        ws.write(r, self._col(0), h_beg[0], fmt.hc)
        ws.write_comment(r, self._col(0),
                         "Beginning cash: derived from ending cash minus net cash change (XBRL). "
                         "cite:xbrl:us-gaap:CashAndCashEquivalentsAtCarryingValue",
                         {"width": 300, "height": 60})
        # Historical j>0: hardcoded XBRL cash of prior period (avoids formula rollforward drift)
        for j in range(1, n_h):
            self._hc(ws, r, j, h_cash[j - 1], fmt.hc, fmt.hc_hs)
        # Projected: formula from prior ending_cash (rollforward from anchored XBRL value)
        for j in range(n_p):
            prev_ec = self._cell(R["ending_cash"], n_h + j - 1)
            ws.write_formula(r, self._col(n_h + j), f"={prev_ec}", fmt.num)

        r = R["ending_cash"]
        ws.write(r, LABEL, "Ending Cash", fmt.lbl_b)
        all_cash = self._av(o.balance_sheet, "cash")
        # Historical: anchor to XBRL BS cash — guarantees chk_cash = 0 and proj BS balances.
        # Without this, cumulative rollforward drift offsets proj starting cash → BS imbalance.
        for j in range(n_h):
            self._hc(ws, r, j, h_cash[j], fmt.hc_bd, fmt.hc_bd_hs)
        # Projected: rollforward from anchored last-historical ending cash
        for j in range(n_p):
            bc_c = self._cell(R["beg_cash"],   n_h + j)
            nc_c = self._cell(R["net_change"], n_h + j)
            self._fmla(ws, r, n_h + j, f"={bc_c}+{nc_c}", fmt.num_bd,
                       p_cash[j] if j < len(p_cash) else None)

        # FCF = CFO − CapEx
        r = R["fcf"]
        ws.write(r, LABEL, "  Free Cash Flow  (CFO − CapEx)", fmt.lbl_i)
        for j in range(self.n):
            cfo_c = self._cell(R["cfo"],   j)
            cap_c = self._cell(R["capex"], j)
            f = fmt.num_hs if self._hs(j) else fmt.num
            all_cfo = self._av(o.cash_flow_statement, "cfo")
            cache   = (all_cfo[j] or 0) - abs(all_cap[j] or 0) if all_cfo[j] is not None else None
            self._fmla(ws, r, j, f"={cfo_c}-ABS({cap_c})", f, cache)

        # ── Validation Checks ─────────────────────────────────────────────────
        r = R["chk_ni"]
        ws.write(r, LABEL, "  Check: CF NI = IS NI  (should = 0)", fmt.lbl_chk)
        for j in range(self.n):
            ni_cf = self._cell(R["ni"], j)
            ni_is = _c(self._isr("net_income"), self._col(j))
            self._fmla(ws, r, j, f"=ROUND({ni_cf}-IS!{ni_is},2)", fmt.chk_xt, 0.0)
        self._apply_check_cf(wb, ws, r)

        r = R["chk_cash"]
        ws.write(r, LABEL, "  Check: CF Ending Cash = BS Cash  (should = 0)", fmt.lbl_chk)
        for j in range(self.n):
            ec_c  = self._cell(R["ending_cash"], j)
            bs_c  = _c(BS_R["cash"], self._col(j))
            self._fmla(ws, r, j, f"=ROUND({ec_c}-BS!{bs_c},2)", fmt.chk_xt, 0.0)
        self._apply_check_cf(wb, ws, r)

        ws.set_landscape(); ws.fit_to_pages(1, 0)
        ws.set_footer(f"&L CF &C {self.co} &R Page &P")
        ws.set_print_scale(85)

    # ─────────────────────────────────────────────────────────────────────────
    # DCF tab
    # ─────────────────────────────────────────────────────────────────────────

    def _write_dcf(self, wb, ws, fmt: _Fmt, dcf) -> None:
        R      = DCF_R
        n_proj = len(dcf.proj_periods)
        n_h    = self.n_h
        VC     = DATA0   # "value column" — single-value rows write to col C

        # ── columns ──────────────────────────────────────────────────────────
        ws.hide_gridlines(2)
        ws.set_column(MARGIN, MARGIN2, 3)
        ws.set_column(LABEL,  LABEL,  38)
        ws.set_column(VC,     VC,     16)
        for c in range(VC + 1, VC + max(n_proj, 5) + 1):
            ws.set_column(c, c, 14)

        # ── custom formats ────────────────────────────────────────────────────
        def _mk(**kw):
            return wb.add_format({"font_name": "Arial", "font_size": 10,
                                   "valign": "vcenter", **kw})
        factor_f = _mk(font_color=_Fmt.BLACK, align="right", num_format="0.0000")
        price_f  = _mk(font_color=_Fmt.BLACK, align="right",
                        num_format=_Fmt._PX, bold=True, top=2)
        price_xt = _mk(font_color=_Fmt.GREEN, align="right",
                        num_format=_Fmt._PX, bold=True, top=2)
        # Sensitivity: base cell (highlighted), base row, normal cell
        # DCF inline tables use SAME-sheet refs → black font per spec
        sens_base_f = _mk(font_color="#FFFFFF",    align="right",
                           num_format=_Fmt._PX, bold=True, bg_color=_Fmt.NAVY)
        sens_hi_f   = _mk(font_color=_Fmt.BLACK,   align="right",
                           num_format=_Fmt._PX, bg_color="#DEEAF1")  # black, light blue row
        sens_norm_f = _mk(font_color=_Fmt.BLACK,   align="right",
                           num_format=_Fmt._PX)   # black: same-sheet formula per spec

        # ── header ───────────────────────────────────────────────────────────
        ws.set_row(0, 4); ws.set_row(1, 4); ws.set_row(2, 26)
        ws.set_row(3, 4); ws.set_row(4, 18); ws.set_row(5, 14); ws.set_row(6, 8)
        last_c = VC + max(n_proj - 1, 4)
        self._span(ws, 2, LABEL, last_c, f"{self.co} — DCF Valuation", fmt.hbar)
        ws.write(4, LABEL, "Discounted Cash Flow Analysis", fmt.hsub)
        ws.write(5, LABEL, f"({self.ccy} $ in millions, unless noted)", fmt.hunit)

        # ── WACC build-up ─────────────────────────────────────────────────────
        ws.set_row(R["wacc_hdr"], 16)
        ws.write(R["wacc_hdr"], LABEL, "WACC BUILD-UP", fmt.lbl_sec)
        self._sp(ws, 9)

        # Beta, Rf, ERP, Kd, tax, weights: green cross-links from WACC tab (single source of truth)
        ws.write(R["beta"], LABEL, "  Beta (3–5Y)", fmt.lbl)
        ws.write_formula(R["beta"], VC,
                         f"=WACC!{_c(WACC_R['be_target'], DATA0)}", fmt.xt, dcf.beta)

        ws.write(R["rf"],  LABEL, "  Risk-Free Rate (10Y Treasury)", fmt.lbl)
        ws.write_formula(R["rf"], VC,
                         f"=WACC!{_c(WACC_R['rf'], DATA0)}", fmt.xt_p, dcf.risk_free_rate)

        ws.write(R["erp"], LABEL, "  Equity Risk Premium", fmt.lbl)
        ws.write_formula(R["erp"], VC,
                         f"=WACC!{_c(WACC_R['erp'], DATA0)}", fmt.xt_p, dcf.equity_risk_premium)

        beta_c = _c(R["beta"], VC); rf_c = _c(R["rf"], VC); erp_c = _c(R["erp"], VC)
        ws.write(R["ke"], LABEL, "  Cost of Equity  (CAPM = rf + β × ERP)", fmt.lbl)
        ws.write_formula(R["ke"], VC, f"={rf_c}+{beta_c}*{erp_c}", fmt.num_p,
                         dcf.cost_of_equity)
        ke_c = _c(R["ke"], VC)

        self._sp(ws, 14)

        ws.write(R["kd_pre"],    LABEL, "  Pre-Tax Cost of Debt", fmt.lbl)
        ws.write_formula(R["kd_pre"], VC,
                         f"=WACC!{_c(WACC_R['kd_pre'], DATA0)}", fmt.xt_p, dcf.cost_of_debt_pretax)

        ws.write(R["tax_shield"], LABEL, "  Effective Tax Rate", fmt.lbl)
        ws.write_formula(R["tax_shield"], VC,
                         f"=WACC!{_c(WACC_R['tax'], DATA0)}", fmt.xt_p, dcf.tax_rate)

        kd_pre_c = _c(R["kd_pre"], VC); tax_c = _c(R["tax_shield"], VC)
        ws.write(R["kd"], LABEL, "  After-Tax Cost of Debt  [kd × (1 − t)]", fmt.lbl)
        ws.write_formula(R["kd"], VC, f"={kd_pre_c}*(1-{tax_c})", fmt.num_p,
                         dcf.after_tax_cost_of_debt)
        kd_c = _c(R["kd"], VC)

        self._sp(ws, 18)

        ws.write(R["eq_wt"], LABEL, "  Equity Weight  (% of Total Capital)", fmt.lbl)
        ws.write_formula(R["eq_wt"], VC,
                         f"=WACC!{_c(WACC_R['we'], DATA0)}", fmt.xt_p, dcf.equity_weight)

        ws.write(R["d_wt"],  LABEL, "  Debt Weight  (% of Total Capital)", fmt.lbl)
        ws.write_formula(R["d_wt"], VC,
                         f"=WACC!{_c(WACC_R['wd'], DATA0)}", fmt.xt_p, dcf.debt_weight)

        eqw_c = _c(R["eq_wt"], VC); dw_c = _c(R["d_wt"], VC)
        ws.write(R["wacc"], LABEL, "  WACC  (single source of truth: WACC tab)", fmt.lbl_b)
        # Green link to WACC tab — single source of truth
        wacc_tab_cell = _c(WACC_R["wacc"], DATA0)
        ws.write_formula(R["wacc"], VC, f"=WACC!{wacc_tab_cell}", fmt.xt_p, dcf.wacc)
        wacc_c = _c(R["wacc"], VC)

        for sp in (22, 23):
            self._sp(ws, sp)

        # ── FCF projection ────────────────────────────────────────────────────
        ws.set_row(R["fcf_hdr"], 16)
        ws.write(R["fcf_hdr"], LABEL, "FREE CASH FLOW PROJECTION  (Unlevered FCFF)", fmt.lbl_sec)

        ws.set_row(R["fcf_headers"], 16)
        for i, period in enumerate(dcf.proj_periods):
            ws.write(R["fcf_headers"], VC + i, period, fmt.hcol)

        # EBIT — green cross-tab from IS
        ws.write(R["fcf_ebit"], LABEL, "  EBIT", fmt.lbl)
        for i in range(n_proj):
            ref = f"=IS!{_c(self._isr('ebit'), DATA0 + n_h + i)}"
            ws.write_formula(R["fcf_ebit"], VC + i, ref, fmt.xt)

        # NOPAT = EBIT × (1 − t). Cache uses engine's tax_rate × hist EBIT — matches formula.
        ws.write(R["fcf_nopat"], LABEL, "  NOPAT  [EBIT × (1 − t)]", fmt.lbl)
        is_ebit = self._av(self.o.income_statement, "ebit")
        for i in range(n_proj):
            ebit_ci = _c(R["fcf_ebit"], VC + i)
            ebit_v = is_ebit[n_h + i] if (n_h + i) < len(is_ebit) else 0
            nopat_cache = (ebit_v or 0) * (1 - dcf.tax_rate)
            ws.write_formula(R["fcf_nopat"], VC + i,
                             f"={ebit_ci}*(1-{tax_c})", fmt.num, nopat_cache)

        # Plus: D&A add-back — green cross-tab from IS
        ws.write(R["fcf_da"], LABEL, "  Plus: D&A", fmt.lbl_i)
        for i in range(n_proj):
            ref = f"=IS!{_c(self._isr('da'), DATA0 + n_h + i)}"
            ws.write_formula(R["fcf_da"], VC + i, ref, fmt.xt)

        # Less: CapEx — green cross-tab from CF (positive value, subtracted in formula)
        ws.write(R["fcf_capex"], LABEL, "  Less: Capital Expenditures", fmt.lbl_i)
        for i in range(n_proj):
            ref = f"=CF!{_c(CF_R['capex'], DATA0 + n_h + i)}"
            ws.write_formula(R["fcf_capex"], VC + i, ref, fmt.xt)

        # Less: ΔNWC — green formula from BS cross-links (NWC = AR + Inv − AP)
        ws.write(R["fcf_dwc"], LABEL, "  Less: Change in Net Working Capital", fmt.lbl_i)
        for i in range(n_proj):
            t     = n_h + i
            t_prv = t - 1  # = n_h-1 for i=0 (last historical)
            ar_t  = f"BS!{_c(BS_R['ar'],        DATA0 + t)}"
            inv_t = f"BS!{_c(BS_R['inventory'], DATA0 + t)}"
            ap_t  = f"BS!{_c(BS_R['ap'],        DATA0 + t)}"
            ar_p  = f"BS!{_c(BS_R['ar'],        DATA0 + t_prv)}"
            inv_p = f"BS!{_c(BS_R['inventory'], DATA0 + t_prv)}"
            ap_p  = f"BS!{_c(BS_R['ap'],        DATA0 + t_prv)}"
            ws.write_formula(
                R["fcf_dwc"], VC + i,
                f"=({ar_t}+{inv_t}-{ap_t})-({ar_p}+{inv_p}-{ap_p})",
                fmt.xt,
                dcf.dwc_proj[i] if i < len(dcf.dwc_proj) else 0,
            )

        # FCFF = NOPAT + DA − CapEx − ΔNWC
        ws.write(R["fcf_fcff"], LABEL, "  Unlevered Free Cash Flow  (FCFF)", fmt.lbl_b)
        for i in range(n_proj):
            nop_c = _c(R["fcf_nopat"], VC + i)
            da_ci = _c(R["fcf_da"],    VC + i)
            cap_c = _c(R["fcf_capex"], VC + i)
            dwc_c = _c(R["fcf_dwc"],   VC + i)
            # cap_c references the CF sheet where CapEx is already stored as negative (outflow)
            # so use +cap_c (not -cap_c) to subtract CapEx correctly: NOPAT + D&A + (negative CapEx) - ΔNWC
            ws.write_formula(R["fcf_fcff"], VC + i,
                             f"={nop_c}+{da_ci}+{cap_c}-{dwc_c}",
                             fmt.num_b,
                             dcf.fcff_proj[i] if i < len(dcf.fcff_proj) else 0)

        # Discount period t (mid-year convention shifts by -0.5)
        offset = -0.5 if dcf.mid_year_convention else 0.0
        period_label = "  Discount Period (t − 0.5, mid-year)" if dcf.mid_year_convention \
                       else "  Discount Period (t, year-end)"
        ws.write(R["fcf_t"], LABEL, period_label, fmt.lbl_i)
        for i in range(n_proj):
            ws.write(R["fcf_t"], VC + i, (i + 1) + offset, fmt.hc)
            ws.write_comment(R["fcf_t"], VC + i,
                             f"Discount period t={( i + 1) + offset:.1f}: "
                             "mid-year convention shifts by -0.5 per SPEC_methodology §3. "
                             "cite:analyst:mid_year_convention",
                             {"width": 280, "height": 72})

        # Discount factor = 1/(1+WACC)^period
        ws.write(R["fcf_factor"], LABEL, "  Discount Factor  [1 ÷ (1 + WACC)^t]", fmt.lbl_i)
        for i in range(n_proj):
            t_ci = _c(R["fcf_t"], VC + i)
            ws.write_formula(R["fcf_factor"], VC + i,
                             f"=1/(1+{wacc_c})^{t_ci}", factor_f,
                             dcf.discount_factors[i] if i < len(dcf.discount_factors) else 1.0)

        # PV of each FCF
        ws.write(R["fcf_pv"], LABEL, "  PV of FCF", fmt.lbl)
        for i in range(n_proj):
            fcff_ci = _c(R["fcf_fcff"],  VC + i)
            fac_ci  = _c(R["fcf_factor"], VC + i)
            pv_cache = dcf.pv_fcfs_per_period[i] if i < len(dcf.pv_fcfs_per_period) else 0
            ws.write_formula(R["fcf_pv"], VC + i, f"={fcff_ci}*{fac_ci}", fmt.num, pv_cache)

        for sp in (35, 36):
            self._sp(ws, sp)

        # Sum of PV(FCFs)
        pv_first = _c(R["fcf_pv"], VC)
        pv_last  = _c(R["fcf_pv"], VC + n_proj - 1)
        ws.write(R["pv_fcfs"], LABEL, "Sum of PV(FCFs)", fmt.lbl_b)
        ws.write_formula(R["pv_fcfs"], VC, f"=SUM({pv_first}:{pv_last})", fmt.num_bd,
                         dcf.pv_fcfs)
        pv_fcfs_c = _c(R["pv_fcfs"], VC)

        for sp in (38, 39):
            self._sp(ws, sp)

        # ── Terminal value ────────────────────────────────────────────────────
        ws.set_row(R["tv_hdr"], 16)
        ws.write(R["tv_hdr"], LABEL, "TERMINAL VALUE", fmt.lbl_sec)

        ws.write(R["tv_method"], LABEL, "  TV Method  (1 = EBITDA Multiple  |  2 = Gordon Growth)",
                 fmt.lbl)
        ws.write(R["tv_method"], VC, dcf.tv_method, fmt.hc)
        ws.write_comment(R["tv_method"], VC,
                         "TV method selector: 1 = Exit EBITDA Multiple, 2 = Gordon Growth. "
                         "Change to switch terminal value method used in EV bridge.",
                         {"width": 300, "height": 72})
        tv_meth_c = _c(R["tv_method"], VC)

        # Method 1 — exit EBITDA multiple
        ws.write(R["tv1_lbl"], LABEL, "  Method 1 — Exit EBITDA Multiple", fmt.lbl_sec)
        ws.write(R["tv1_mult"], LABEL, "    Exit EBITDA Multiple (×)", fmt.lbl_i)
        ws.write(R["tv1_mult"], VC, dcf.tv_ebitda_multiple, fmt.hc)
        ws.write_comment(R["tv1_mult"], VC,
                         "Exit EBITDA multiple: analyst assumption. "
                         "Source: peer-set median EV/EBITDA LTM per Public Comps tab. "
                         "cite:analyst:exit_multiple",
                         {"width": 280, "height": 72})
        tv_mult_c = _c(R["tv1_mult"], VC)

        last_proj_is_col = DATA0 + n_h + n_proj - 1
        ws.write(R["tv1_ebitda"], LABEL, "    Terminal Year EBITDA", fmt.lbl_i)
        ws.write_formula(R["tv1_ebitda"], VC,
                         f"=IS!{_c(self._isr('ebitda'), last_proj_is_col)}",
                         fmt.xt, dcf.terminal_ebitda)
        tv_ebitda_c = _c(R["tv1_ebitda"], VC)

        ws.write(R["tv1_tv"], LABEL, "    Terminal Value (EBITDA Multiple)", fmt.lbl)
        ws.write_formula(R["tv1_tv"], VC, f"={tv_ebitda_c}*{tv_mult_c}", fmt.num_bd,
                         dcf.tv_ebitda)
        tv1_c = _c(R["tv1_tv"], VC)

        # Method 2 — Gordon Growth
        ws.write(R["tv2_lbl"], LABEL, "  Method 2 — Gordon Growth Model", fmt.lbl_sec)
        ws.write(R["tv2_g"], LABEL, "    Long-Term Growth Rate", fmt.lbl_i)
        ws.write(R["tv2_g"], VC, dcf.tv_growth_rate, fmt.hc_p)
        ws.write_comment(R["tv2_g"], VC,
                         "Long-term terminal growth rate: ≤ nominal GDP growth (2-3.5%). "
                         "Source: analyst assumption. cite:analyst:terminal_growth",
                         {"width": 280, "height": 72})
        tv_g_c = _c(R["tv2_g"], VC)

        last_fcff_c = _c(R["fcf_fcff"], VC + n_proj - 1)
        ws.write(R["tv2_fcf"], LABEL, "    Terminal Year FCF", fmt.lbl_i)
        ws.write_formula(R["tv2_fcf"], VC, f"={last_fcff_c}", fmt.num,
                         dcf.fcff_proj[-1] if dcf.fcff_proj else 0)

        ws.write(R["tv2_tv"], LABEL, "    Terminal Value (Gordon Growth)", fmt.lbl)
        ws.write_formula(R["tv2_tv"], VC,
                         f"=IF({wacc_c}>{tv_g_c},"
                         f"{last_fcff_c}*(1+{tv_g_c})/({wacc_c}-{tv_g_c}),0)",
                         fmt.num_bd, dcf.tv_gordon)
        tv2_c = _c(R["tv2_tv"], VC)

        self._sp(ws, 50)

        ws.write(R["tv_selected"], LABEL, "  Selected Terminal Value", fmt.lbl_b)
        ws.write_formula(R["tv_selected"], VC,
                         f"=CHOOSE({tv_meth_c},{tv1_c},{tv2_c})",
                         fmt.num_bd, dcf.tv_selected)
        tv_sel_c = _c(R["tv_selected"], VC)

        ws.write(R["tv_pv"], LABEL, "  PV of Terminal Value", fmt.lbl_b)
        ws.write_formula(R["tv_pv"], VC,
                         f"={tv_sel_c}/(1+{wacc_c})^{n_proj}",
                         fmt.num_bd, dcf.pv_tv)
        pv_tv_c = _c(R["tv_pv"], VC)

        self._sp(ws, 53)

        # ── Enterprise Value bridge ───────────────────────────────────────────
        ws.set_row(R["ev_hdr"], 16)
        ws.write(R["ev_hdr"], LABEL, "ENTERPRISE VALUE BRIDGE", fmt.lbl_sec)

        ws.write(R["ev_pvfcfs"], LABEL, "  PV of Free Cash Flows", fmt.lbl)
        ws.write_formula(R["ev_pvfcfs"], VC, f"={pv_fcfs_c}", fmt.num_d, dcf.pv_fcfs)
        ev_pvf_c = _c(R["ev_pvfcfs"], VC)

        ws.write(R["ev_pvtv"], LABEL, "  PV of Terminal Value", fmt.lbl)
        ws.write_formula(R["ev_pvtv"], VC, f"={pv_tv_c}", fmt.num_d, dcf.pv_tv)
        ev_pvt_c = _c(R["ev_pvtv"], VC)

        ws.write(R["ev_total"], LABEL, "  Total Enterprise Value", fmt.lbl_b)
        ws.write_formula(R["ev_total"], VC, f"={ev_pvf_c}+{ev_pvt_c}", fmt.num_bd,
                         dcf.enterprise_value)
        ev_c = _c(R["ev_total"], VC)

        self._sp(ws, 58)

        # Debt and cash — green links from BS last projected period
        last_proj_bs_col = DATA0 + n_h + n_proj - 1
        ws.write(R["ev_debt"], LABEL, "  Less: Total Debt", fmt.lbl)
        ws.write_formula(R["ev_debt"], VC,
                         f"=BS!{_c(BS_R['ltd'], last_proj_bs_col)}",
                         fmt.xt_d, dcf.total_debt)
        debt_c = _c(R["ev_debt"], VC)

        ws.write(R["ev_cash"], LABEL, "  Plus: Cash & Equivalents", fmt.lbl)
        ws.write_formula(R["ev_cash"], VC,
                         f"=BS!{_c(BS_R['cash'], last_proj_bs_col)}",
                         fmt.xt_d, dcf.cash)
        cash_c = _c(R["ev_cash"], VC)

        ws.write(R["ev_net_debt"], LABEL, "  Net Debt  (Debt − Cash)", fmt.lbl)
        ws.write_formula(R["ev_net_debt"], VC, f"={debt_c}-{cash_c}", fmt.num_d,
                         dcf.net_debt)
        nd_c = _c(R["ev_net_debt"], VC)

        ws.write(R["ev_equity"], LABEL, "  Equity Value", fmt.lbl_b)
        ws.write_formula(R["ev_equity"], VC, f"={ev_c}-{nd_c}", fmt.num_bd,
                         dcf.equity_value)
        eq_val_c = _c(R["ev_equity"], VC)

        ws.write(R["ev_shares"], LABEL, "  Diluted Shares Outstanding (M)", fmt.lbl)
        ws.write_formula(R["ev_shares"], VC,
                         f"=IS!{_c(self._isr('shares_diluted'), last_proj_is_col)}",
                         fmt.xt, dcf.shares_diluted)
        sh_c = _c(R["ev_shares"], VC)

        ws.write(R["ev_price"], LABEL, "  Implied Share Price", fmt.lbl_b)
        # Equity ($M) / Shares ($M) = $ per share (no 1000x scaling).
        ws.write_formula(R["ev_price"], VC,
                         f"=IF({sh_c}<>0,{eq_val_c}/{sh_c},0)",
                         price_f, dcf.implied_price)

        for sp in (65, 66):
            self._sp(ws, sp)

        # ── Sensitivity tables ────────────────────────────────────────────────
        ws.set_row(R["sens_hdr"], 16)
        ws.write(R["sens_hdr"], LABEL, "SENSITIVITY ANALYSIS  (Implied Share Price)", fmt.lbl_sec)

        n_wacc = len(dcf.wacc_range)
        mid    = n_wacc // 2   # index of base-case WACC row

        # Fixed absolute cell refs shared by both tables
        _lc      = xl_col_to_name(LABEL)           # "C" — WACC row-header col
        _vc0     = xl_col_to_name(VC)              # "D"
        _fcff_er = R["fcf_fcff"] + 1               # Excel row of UFCF projection row
        _tv1e_er = R["tv1_ebitda"] + 1             # Excel row terminal EBITDA
        _debt_er = R["ev_debt"] + 1                # Excel row debt
        _cash_er = R["ev_cash"] + 1                # Excel row cash
        _shrs_er = R["ev_shares"] + 1              # Excel row shares
        # Discount period exponents (mid-year or year-end)
        _exps    = [(k + 1) + offset for k in range(n_proj)]   # [0.5,1.5,...] or [1,2,...]

        def _ufcf_sum(wacc_ref: str) -> str:
            return " + ".join(
                f"${xl_col_to_name(VC + k)}${_fcff_er}/(1+{wacc_ref})^{_exps[k]}"
                for k in range(n_proj)
            )

        # Table 1: WACC × Exit EBITDA Multiple
        ws.set_row(R["sens1_lbl"], 14)
        ws.write(R["sens1_lbl"], LABEL, "WACC  ↓  /  Exit Multiple  →", fmt.lbl_b)
        ws.set_row(R["sens1_col_hdr"], 14)
        # Write multiples as blue numerics (x-format) so formulas can reference them
        for j, mult in enumerate(dcf.ebitda_multiple_range):
            ws.write(R["sens1_col_hdr"], VC + j, mult, fmt.hc_m)
            ws.write_comment(R["sens1_col_hdr"], VC + j,
                             f"Exit multiple scenario: {mult:.1f}x EBITDA. "
                             "Edit to stress-test valuation. cite:analyst:exit_multiple",
                             {"width": 240, "height": 60})
        _mult_hdr_er = R["sens1_col_hdr"] + 1   # Excel row of multiple col headers

        for i, w in enumerate(dcf.wacc_range):
            r       = R["sens1_col_hdr"] + 1 + i
            r_excel = r + 1
            ws.write(r, LABEL, w, fmt.hc_p)     # numeric blue WACC input
            ws.write_comment(r, LABEL,
                             f"WACC scenario: {w:.1%}. Edit to stress-test valuation. "
                             "cite:analyst:wacc_sensitivity",
                             {"width": 240, "height": 60})
            wacc_ref = f"${_lc}${r_excel}"       # $C$71 — fixed col, fixed row
            for j, mult in enumerate(dcf.ebitda_multiple_range):
                col      = VC + j
                mult_ref = f"{xl_col_to_name(col)}${_mult_hdr_er}"  # D$70 mixed
                tv_pv    = (f"${_vc0}${_tv1e_er}*{mult_ref}"
                            f"/(1+{wacc_ref})^{n_proj}")
                bridge   = f"-${_vc0}${_debt_er}+${_vc0}${_cash_er}"
                shares   = f"${_vc0}${_shrs_er}"
                formula  = (f"=IF({shares}<>0,"
                            f"({_ufcf_sum(wacc_ref)}+{tv_pv}{bridge})/{shares},0)")
                cache    = dcf.sensitivity_ebitda[i][j]
                is_base  = (i == mid) and (j == len(dcf.ebitda_multiple_range) // 2)
                f = sens_base_f if is_base else (sens_hi_f if (i == mid) else sens_norm_f)
                ws.write_formula(r, col, formula, f, cache)

        self._sp(ws, R["sens1_col_hdr"] + 1 + n_wacc)   # spacer row after table 1

        # Table 2: WACC × Gordon Growth Rate
        ws.set_row(R["sens2_lbl"], 14)
        ws.write(R["sens2_lbl"], LABEL, "WACC  ↓  /  Terminal Growth Rate  →", fmt.lbl_b)
        ws.set_row(R["sens2_col_hdr"], 14)
        # Write growth rates as blue numerics (% format)
        for j, g in enumerate(dcf.gordon_growth_range):
            ws.write(R["sens2_col_hdr"], VC + j, g, fmt.hc_p)
            ws.write_comment(R["sens2_col_hdr"], VC + j,
                             f"Terminal growth scenario: {g:.1%}. Must be ≤ long-run GDP growth. "
                             "cite:analyst:terminal_growth",
                             {"width": 240, "height": 60})
        _g_hdr_er = R["sens2_col_hdr"] + 1   # Excel row of g col headers

        for i, w in enumerate(dcf.wacc_range):
            r       = R["sens2_col_hdr"] + 1 + i
            r_excel = r + 1
            ws.write(r, LABEL, w, fmt.hc_p)     # numeric blue WACC input
            ws.write_comment(r, LABEL,
                             f"WACC scenario: {w:.1%}. Edit to stress-test valuation. "
                             "cite:analyst:wacc_sensitivity",
                             {"width": 240, "height": 60})
            wacc_ref = f"${_lc}${r_excel}"
            for j, g in enumerate(dcf.gordon_growth_range):
                col     = VC + j
                g_ref   = f"{xl_col_to_name(col)}${_g_hdr_er}"   # E$78 mixed
                last_ufcf_col = xl_col_to_name(VC + n_proj - 1)
                tv_pv   = (f"IF({wacc_ref}>{g_ref},"
                           f"${last_ufcf_col}${_fcff_er}*(1+{g_ref})"
                           f"/(({wacc_ref}-{g_ref})*(1+{wacc_ref})^{n_proj}),0)")
                bridge  = f"-${_vc0}${_debt_er}+${_vc0}${_cash_er}"
                shares  = f"${_vc0}${_shrs_er}"
                formula = (f"=IF({shares}<>0,"
                           f"({_ufcf_sum(wacc_ref)}+{tv_pv}{bridge})/{shares},0)")
                cache   = dcf.sensitivity_gordon[i][j]
                is_base = (i == mid) and (j == len(dcf.gordon_growth_range) // 2)
                f = sens_base_f if is_base else (sens_hi_f if (i == mid) else sens_norm_f)
                ws.write_formula(r, col, formula, f, cache)

        # ── Cross-Checks ──────────────────────────────────────────────────────
        ws.set_row(R["xc_hdr"], 16)
        ws.write(R["xc_hdr"], LABEL, "CROSS-CHECKS", fmt.lbl_sec)
        ws.write(R["xc_tv_pct"],   LABEL, "  TV / EV %  (target: 60-80%)", fmt.lbl)
        ws.write(R["xc_tv_pct"],   VC, dcf.tv_pct_of_ev, fmt.hc_p)
        ws.write_comment(R["xc_tv_pct"], VC, "Computed: PV(TV)/EV. Target 60-80% for stable cos.", {"width": 240, "height": 60})
        ws.write(R["xc_wacc_g"],   LABEL, "  WACC − Terminal g  (target: > 2%)", fmt.lbl)
        ws.write(R["xc_wacc_g"],   VC, dcf.wacc_minus_g, fmt.hc_p)
        ws.write_comment(R["xc_wacc_g"], VC, "WACC minus terminal g. Must be >0 (Gordon convergence). Target >2%.", {"width": 260, "height": 60})
        ws.write(R["xc_imp_mult"], LABEL, "  Implied Exit Multiple  (Gordon TV ÷ Terminal EBITDA)", fmt.lbl)
        ws.write(R["xc_imp_mult"], VC, dcf.implied_exit_mult_from_gordon, fmt.hc)
        ws.write_comment(R["xc_imp_mult"], VC, "Implied exit multiple from Gordon TV / terminal EBITDA. Sanity check vs. Method 1.", {"width": 260, "height": 60})
        ws.write(R["xc_imp_g"],    LABEL, "  Implied Perpetuity g  (from Exit Multiple)", fmt.lbl)
        ws.write(R["xc_imp_g"],    VC, dcf.implied_g_from_exit_mult, fmt.hc_p)
        ws.write_comment(R["xc_imp_g"], VC, "Implied perpetuity g from exit multiple. Should be within ±1% of terminal g.", {"width": 260, "height": 60})
        ws.write(R["xc_current"],  LABEL, "  Current Share Price", fmt.lbl)
        ws.write(R["xc_current"],  VC, dcf.current_share_price, fmt.hc_d)
        ws.write_comment(R["xc_current"], VC, "Current market share price. Source: market data as of valuation date. cite:market:price", {"width": 260, "height": 60})
        ws.write(R["xc_upside"],   LABEL, "  Implied Upside / (Downside) vs Current", fmt.lbl_b)
        ws.write(R["xc_upside"],   VC, dcf.upside_downside_pct, fmt.hc_p)
        ws.write_comment(R["xc_upside"], VC, "Computed: (Implied price / Current price) - 1. Upside positive, downside negative.", {"width": 280, "height": 60})

        ws.set_landscape(); ws.fit_to_pages(1, 0)
        ws.set_footer(f"&L DCF &C {self.co} &R Page &P")
        ws.set_print_scale(80)

    # ─────────────────────────────────────────────────────────────────────────
    # Comps tab
    # ─────────────────────────────────────────────────────────────────────────

    def _write_comps(self, wb, ws, fmt: _Fmt, comps) -> None:
        """Trading comparables — peer multiples table + target-implied prices."""
        ws.hide_gridlines(2)
        ws.set_column(MARGIN, MARGIN2, 3)
        ws.set_column(LABEL,  LABEL,  10)             # ticker
        ws.set_column(LABEL + 1, LABEL + 1, 30)       # company name
        for c in range(LABEL + 2, LABEL + 12):
            ws.set_column(c, c, 13)

        def _mk(**kw):
            return wb.add_format({"font_name": "Arial", "font_size": 10,
                                   "valign": "vcenter", **kw})
        f_mult     = _mk(font_color=_Fmt.BLACK, align="right", num_format='0.0"x"')
        f_dol      = _mk(font_color=_Fmt.BLACK, align="right", num_format="$#,##0")
        f_pct      = _mk(font_color=_Fmt.BLACK, align="right", num_format="0.0%")
        f_eps      = _mk(font_color=_Fmt.BLACK, align="right", num_format=_Fmt._PX)
        f_target_l = _mk(font_color="#FFFFFF", bold=True, bg_color=_Fmt.NAVY, align="left")
        f_target_d = _mk(font_color="#FFFFFF", bold=True, bg_color=_Fmt.NAVY,
                          align="right", num_format="$#,##0")
        f_target_m = _mk(font_color="#FFFFFF", bold=True, bg_color=_Fmt.NAVY,
                          align="right", num_format='0.0"x"')
        f_target_p = _mk(font_color="#FFFFFF", bold=True, bg_color=_Fmt.NAVY,
                          align="right", num_format="0.0%")
        f_agg_l    = _mk(font_color=_Fmt.BLACK, bold=True, bg_color=_Fmt.LGRAY, align="left")
        f_agg_m    = _mk(font_color=_Fmt.BLACK, bold=True, bg_color=_Fmt.LGRAY,
                          align="right", num_format='0.0"x"')
        f_agg_p    = _mk(font_color=_Fmt.BLACK, bold=True, bg_color=_Fmt.LGRAY,
                          align="right", num_format="0.0%")
        f_imp      = _mk(font_color=_Fmt.BLACK, bold=True, align="right",
                          num_format=_Fmt._PX)

        # ── header bar ───────────────────────────────────────────────────────
        ws.set_row(0, 4); ws.set_row(1, 4); ws.set_row(2, 26)
        ws.set_row(3, 4); ws.set_row(4, 18); ws.set_row(5, 14); ws.set_row(6, 8)
        last_c = LABEL + 11
        self._span(ws, 2, LABEL, last_c, f"{self.co} — Trading Comparables", fmt.hbar)
        ws.write(4, LABEL, "Public Peer Trading Multiples", fmt.hsub)
        ws.write(5, LABEL, f"({self.ccy} $ in millions, except per-share)", fmt.hunit)

        # ── column headers ───────────────────────────────────────────────────
        headers = [
            "Ticker", "Company", "Mkt Cap", "EV", "Revenue (LTM)", "EBITDA (LTM)",
            "EBITDA %", "Rev Growth", "EV / Rev", "EV / EBITDA", "P / E", "EPS",
        ]
        ws.set_row(8, 16)
        for j, h in enumerate(headers):
            ws.write(8, LABEL + j, h, fmt.hcol)

        # ── target row (highlighted) ─────────────────────────────────────────
        r = 9
        t = comps.target
        ws.write(r, LABEL,     t.ticker,           f_target_l)
        ws.write(r, LABEL + 1, t.company_name,     f_target_l)
        ws.write(r, LABEL + 2, t.market_cap,       f_target_d)
        ws.write(r, LABEL + 3, t.enterprise_value, f_target_d)
        ws.write(r, LABEL + 4, t.revenue_ltm,      f_target_d)
        ws.write(r, LABEL + 5, t.ebitda_ltm,       f_target_d)
        ws.write(r, LABEL + 6, t.ebitda_margin,    f_target_p)
        ws.write(r, LABEL + 7, t.revenue_growth,   f_target_p)
        ws.write(r, LABEL + 8, t.ev_revenue,       f_target_m)
        ws.write(r, LABEL + 9, t.ev_ebitda,        f_target_m)
        ws.write(r, LABEL + 10, t.pe,              f_target_m)
        ws.write(r, LABEL + 11, t.eps_ltm,         f_eps)

        # spacer
        self._sp(ws, 10)

        # ── peer rows ────────────────────────────────────────────────────────
        ws.write(11, LABEL, "PEERS", fmt.lbl_sec)
        for i, p in enumerate(comps.peers):
            r = 12 + i
            ws.write(r, LABEL,      p.ticker,           fmt.lbl)
            ws.write(r, LABEL + 1,  p.company_name,     fmt.lbl)
            ws.write(r, LABEL + 2,  p.market_cap,       f_dol)
            ws.write(r, LABEL + 3,  p.enterprise_value, f_dol)
            ws.write(r, LABEL + 4,  p.revenue_ltm,      f_dol)
            ws.write(r, LABEL + 5,  p.ebitda_ltm,       f_dol)
            ws.write(r, LABEL + 6,  p.ebitda_margin,    f_pct)
            ws.write(r, LABEL + 7,  p.revenue_growth,   f_pct)
            ws.write(r, LABEL + 8,  p.ev_revenue,       f_mult)
            ws.write(r, LABEL + 9,  p.ev_ebitda,        f_mult)
            ws.write(r, LABEL + 10, p.pe,               f_mult)
            ws.write(r, LABEL + 11, p.eps_ltm,          f_eps)

        # ── aggregates ───────────────────────────────────────────────────────
        agg_start = 12 + len(comps.peers) + 1
        self._sp(ws, agg_start - 1)

        for offset, (name, agg) in enumerate(
            [("Median", comps.median), ("Mean", comps.mean)]
        ):
            r = agg_start + offset
            ws.write(r, LABEL,     name,                       f_agg_l)
            ws.write(r, LABEL + 1, "",                         f_agg_l)
            for c in range(LABEL + 2, LABEL + 6):
                ws.write_blank(r, c, f_agg_l)
            ws.write(r, LABEL + 6, agg["ebitda_margin"],       f_agg_p)
            ws.write(r, LABEL + 7, agg["revenue_growth"],      f_agg_p)
            ws.write(r, LABEL + 8, agg["ev_revenue"],          f_agg_m)
            ws.write(r, LABEL + 9, agg["ev_ebitda"],           f_agg_m)
            ws.write(r, LABEL + 10, agg["pe"],                 f_agg_m)
            ws.write_blank(r, LABEL + 11, f_agg_l)

        # ── target-implied price section ─────────────────────────────────────
        impl_start = agg_start + 4
        self._sp(ws, impl_start - 1)
        ws.set_row(impl_start, 16)
        ws.write(impl_start, LABEL,
                 f"IMPLIED VALUE FOR {comps.target.ticker} (peer-median multiples)",
                 fmt.lbl_sec)

        for offset, (method, price) in enumerate(comps.target_implied_price.items()):
            r = impl_start + 1 + offset
            ws.write(r, LABEL,     method, fmt.lbl)
            self._span(ws, r, LABEL + 1, LABEL + 2, "", fmt.lbl)
            ws.write(r, LABEL + 3, "Implied Share Price:", fmt.lbl_b)
            ws.write(r, LABEL + 4, price, f_imp)

        # ── diagnostics (if any) ─────────────────────────────────────────────
        if comps.diagnostics:
            diag_start = impl_start + 5 + len(comps.target_implied_price)
            ws.write(diag_start, LABEL, "Diagnostics", fmt.lbl_b)
            for i, d in enumerate(comps.diagnostics):
                ws.write(diag_start + 1 + i, LABEL + 1, d, fmt.lbl_i)

        ws.set_landscape(); ws.fit_to_pages(1, 0)
        ws.set_footer(f"&L Comps &C {self.co} &R Page &P")
        ws.set_print_scale(75)

    # ─────────────────────────────────────────────────────────────────────────
    # WACC tab
    # ─────────────────────────────────────────────────────────────────────────

    def _write_wacc(self, wb, ws, fmt: _Fmt, w: WACCOutput, ps: PeerSet | None) -> None:
        ws.hide_gridlines(2)
        ws.set_column(MARGIN, MARGIN2, 3)
        ws.set_column(LABEL, LABEL, 30)
        ws.set_column(DATA0, DATA0 + 5, 14)
        R = WACC_R

        ws.set_row(R["title"], 28)
        self._span(ws, R["title"], LABEL, DATA0 + 5, f"{self.co} — WACC Build-Up", fmt.hbar)
        ws.write(R["subtitle"], LABEL, "Peer-Set Beta Unlever / Relever + CAPM", fmt.hsub)
        src_lbl = ps.source if ps else "fallback"
        ws.write(R["units"], LABEL, f"(peer source: {src_lbl})", fmt.hunit)

        # ── Peer Set Table ────────────────────────────────────────────────────
        ws.set_row(R["peer_hdr"], 16)
        self._span(ws, R["peer_hdr"], LABEL, DATA0 + 5, "PEER SET", fmt.lbl_sec)
        # Column headers
        col_labels = ["Ticker", "Levered β", "D/E", "Tax", "Unlevered β", "Mkt Cap ($M)"]
        for i, lbl in enumerate(col_labels):
            ws.write(R["peer_cols"], LABEL + i if i == 0 else DATA0 + i - 1,
                     lbl, fmt.hcol)

        # Peer rows
        from src.wacc import _unlever_beta
        # Market cap: cross-link from Comps Peers tab (single source of truth per
        # SPEC_modeling_patterns §7) when that tab exists; else blue hardcode.
        comps_peers_exists = (self.pcomps is not None and bool(self.pcomps.peers))
        for j, p in enumerate(w.peers[:10]):
            r = R["peer_start"] + j
            ws.write(r, LABEL, p.ticker, fmt.lbl)
            ws.write(r, DATA0, p.levered_beta, fmt.hc)
            ws.write_comment(r, DATA0,
                             f"{p.ticker}: 5-year monthly levered beta vs. market. "
                             "Source: LLM / market data. cite:market:beta",
                             {"width": 260, "height": 60})
            ws.write(r, DATA0 + 1, p.de_ratio, fmt.hc_p)
            ws.write_comment(r, DATA0 + 1,
                             f"{p.ticker}: Debt/Equity ratio from most recent balance sheet. "
                             "cite:market:de_ratio", {"width": 260, "height": 60})
            ws.write(r, DATA0 + 2, p.tax_rate, fmt.hc_p)
            ws.write_comment(r, DATA0 + 2,
                             f"{p.ticker}: Effective tax rate (income tax / pretax income). "
                             "cite:xbrl:us-gaap:EffectiveIncomeTaxRateContinuingOperations",
                             {"width": 280, "height": 72})
            bu = _unlever_beta(p.levered_beta, p.de_ratio, p.tax_rate)
            # Unlevered beta as Excel formula: Bu = Bl / (1 + (1-t) * D/E)
            bl_c = _c(r, DATA0); de_p = _c(r, DATA0 + 1); t_p = _c(r, DATA0 + 2)
            ws.write_formula(r, DATA0 + 3,
                             f"={bl_c}/(1+(1-{t_p})*{de_p})", fmt.num, round(bu, 4))
            ticker_cell = _c(r, LABEL)
            if comps_peers_exists:
                # VLOOKUP: ticker col = C ($C), mkt cap = I ($I, 7th from C)
                ws.write_formula(
                    r, DATA0 + 4,
                    f"=IFERROR(VLOOKUP({ticker_cell},'Comps Peers'!$C:$I,7,0),0)",
                    fmt.xt_d, p.market_cap,
                )
            else:
                ws.write(r, DATA0 + 4, p.market_cap, fmt.hc_d)

        # Median row — formula over peer Bu column so it's a same-sheet formula (black)
        ws.write(R["peer_median"], LABEL, "Median Unlevered β", fmt.lbl_b)
        _bu_col = xl_col_to_name(DATA0 + 3)
        _ps_er  = R["peer_start"] + 1   # Excel row (1-based)
        _pe_er  = R["peer_start"] + 10
        ws.write_formula(R["peer_median"], DATA0 + 3,
                         f"=MEDIAN({_bu_col}{_ps_er}:{_bu_col}{_pe_er})",
                         fmt.num_b, w.median_unlevered_beta)

        # ── CAPM Build-Up ─────────────────────────────────────────────────────
        ws.set_row(R["capm_hdr"], 16)
        self._span(ws, R["capm_hdr"], LABEL, DATA0 + 5, "CAPM COST OF EQUITY", fmt.lbl_sec)
        rf_row    = ASSUMP_R["shared_drv0"] + 0
        erp_row   = ASSUMP_R["shared_drv0"] + 1
        de_row    = ASSUMP_R["shared_drv0"] + 2
        kd_pre_row = ASSUMP_R["shared_drv0"] + 3
        ws.write(R["rf"], LABEL, "  Risk-Free Rate (10Y Treasury)", fmt.lbl_drv)
        ws.write_formula(R["rf"], DATA0, f"=Assumptions!{_c(rf_row, ASSUMP_DATA0)}", fmt.xt_p)
        ws.write(R["erp"], LABEL, "  Equity Risk Premium", fmt.lbl_drv)
        ws.write_formula(R["erp"], DATA0, f"=Assumptions!{_c(erp_row, ASSUMP_DATA0)}", fmt.xt_p)
        ws.write(R["be_target"], LABEL, "  Target Levered β  (re-levered to target D/E)", fmt.lbl)
        median_c = _c(R["peer_median"], DATA0 + 3)
        # de_restate: pure green link to Assumptions D/E — required by two-hop rule
        ws.write(R["de_restate"], LABEL, "  Target D/E Ratio", fmt.lbl_drv)
        ws.write_formula(R["de_restate"], DATA0,
                         f"=Assumptions!{_c(de_row, ASSUMP_DATA0)}", fmt.xt_p,
                         w.target_de_ratio if hasattr(w, "target_de_ratio") else 0.30)
        de_c  = _c(R["de_restate"], DATA0)   # now a LOCAL cell ref (no Assumptions! in arithmetic)
        tax_c = _c(R["tax"], DATA0)
        ws.write_formula(R["be_target"], DATA0,
                         f"={median_c}*(1+(1-{tax_c})*{de_c})", fmt.num,
                         w.target_levered_beta)
        ws.write(R["ke"], LABEL, "  Cost of Equity  (Ke = Rf + β × ERP)", fmt.lbl_b)
        rf_c = _c(R["rf"], DATA0)
        erp_c = _c(R["erp"], DATA0)
        be_c = _c(R["be_target"], DATA0)
        ws.write_formula(R["ke"], DATA0, f"={rf_c}+{be_c}*{erp_c}", fmt.num_p_hs,
                         w.cost_of_equity)

        # ── Cost of Debt ──────────────────────────────────────────────────────
        ws.set_row(R["kd_hdr"], 16)
        self._span(ws, R["kd_hdr"], LABEL, DATA0 + 5, "COST OF DEBT", fmt.lbl_sec)
        ws.write(R["kd_pre"], LABEL, "  Pre-Tax Cost of Debt", fmt.lbl_drv)
        ws.write_formula(R["kd_pre"], DATA0, f"=Assumptions!{_c(kd_pre_row, ASSUMP_DATA0)}", fmt.xt_p)
        ws.write(R["tax"], LABEL, "  Effective Tax Rate", fmt.lbl_drv)
        ws.write(R["tax"], DATA0, w.tax_rate, fmt.hc_p)
        ws.write_comment(R["tax"], DATA0,
                         "Effective tax rate: LTM blended rate from company 10-K. "
                         "cite:xbrl:us-gaap:EffectiveIncomeTaxRateContinuingOperations",
                         {"width": 280, "height": 72})
        ws.write(R["kd_after"], LABEL, "  After-Tax Cost of Debt  [Kd × (1 − t)]", fmt.lbl_b)
        kd_c = _c(R["kd_pre"], DATA0)
        ws.write_formula(R["kd_after"], DATA0, f"={kd_c}*(1-{tax_c})", fmt.num_p_hs,
                         w.after_tax_cost_of_debt)

        # ── Capital Structure Weights ─────────────────────────────────────────
        ws.set_row(R["cap_hdr"], 16)
        self._span(ws, R["cap_hdr"], LABEL, DATA0 + 5, "CAPITAL STRUCTURE WEIGHTS", fmt.lbl_sec)
        ws.write(R["mkt_cap"], LABEL, "  Target Market Cap ($M)", fmt.lbl)
        ws.write(R["mkt_cap"], DATA0, w.target_market_cap, fmt.hc_d)
        ws.write_comment(R["mkt_cap"], DATA0,
                         "Target market cap: share price × diluted shares outstanding. "
                         "Source: current market data. cite:market:mktcap",
                         {"width": 280, "height": 72})
        ws.write(R["debt"], LABEL, "  Total Debt ($M)", fmt.lbl)
        ws.write(R["debt"], DATA0, w.target_debt, fmt.hc_d)
        ws.write_comment(R["debt"], DATA0,
                         "Total debt: long-term debt + current portion per latest BS. "
                         "cite:xbrl:us-gaap:LongTermDebt",
                         {"width": 280, "height": 72})
        ws.write(R["total_cap"], LABEL, "  Total Capital  (Equity + Debt)", fmt.lbl_b)
        mc_c = _c(R["mkt_cap"], DATA0)
        d_c  = _c(R["debt"], DATA0)
        ws.write_formula(R["total_cap"], DATA0, f"={mc_c}+{d_c}", fmt.num_bd,
                         w.target_total_capital)
        tc_c = _c(R["total_cap"], DATA0)
        ws.write(R["we"], LABEL, "  Equity Weight  (E / V)", fmt.lbl)
        ws.write_formula(R["we"], DATA0, f"=IF({tc_c}<>0,{mc_c}/{tc_c},0)", fmt.num_p,
                         w.equity_weight)
        ws.write(R["wd"], LABEL, "  Debt Weight  (D / V)", fmt.lbl)
        ws.write_formula(R["wd"], DATA0, f"=IF({tc_c}<>0,{d_c}/{tc_c},0)", fmt.num_p,
                         w.debt_weight)

        # ── Final WACC ────────────────────────────────────────────────────────
        ws.set_row(R["wacc"], 18)
        ws.write(R["wacc"], LABEL,
                 "WACC  (We × Ke + Wd × Kd_after_tax)", fmt.lbl_sec)
        we_c   = _c(R["we"], DATA0)
        ke_c   = _c(R["ke"], DATA0)
        wd_c   = _c(R["wd"], DATA0)
        kdat_c = _c(R["kd_after"], DATA0)
        ws.write_formula(R["wacc"], DATA0,
                         f"={we_c}*{ke_c}+{wd_c}*{kdat_c}", fmt.tot_p,
                         w.wacc)

        ws.set_landscape()
        ws.fit_to_pages(1, 0)
        ws.set_footer(f"&L WACC &C {self.co} &R Page &P")

    # ─────────────────────────────────────────────────────────────────────────
    # Sensitivities tab — 2D tables (WACC × g and WACC × Exit Mult)
    # ─────────────────────────────────────────────────────────────────────────

    def _write_sensitivities(self, wb, ws, fmt: _Fmt, dcf) -> None:
        ws.hide_gridlines(2)
        ws.set_column(MARGIN, MARGIN2, 3)
        ws.set_column(LABEL, LABEL, 18)
        ws.set_column(DATA0, DATA0 + 4, 13)
        R = SENS_R

        ws.set_row(R["title"], 28)
        self._span(ws, R["title"], LABEL, DATA0 + 4, f"{self.co} — Sensitivity Analysis", fmt.hbar)
        ws.write(R["subtitle"], LABEL, "Implied Share Price Sensitivities", fmt.hsub)
        ws.write(R["units"], LABEL, "(USD $ per share)", fmt.hunit)

        # Cross-sheet cell refs into DCF tab (absolute — $ on both col and row)
        n_proj   = len(dcf.proj_periods)
        off      = -0.5 if dcf.mid_year_convention else 0.0
        _exps    = [(k + 1) + off for k in range(n_proj)]
        _D       = xl_col_to_name(DATA0)              # "D"
        _lc      = xl_col_to_name(LABEL)              # "C"
        _fe      = DCF_R["fcf_fcff"] + 1              # Excel row: UFCF
        _t1e_er  = DCF_R["tv1_ebitda"] + 1            # Excel row: terminal EBITDA
        _debt_er = DCF_R["ev_debt"] + 1
        _cash_er = DCF_R["ev_cash"] + 1
        _shrs_er = DCF_R["ev_shares"] + 1

        def _ufcf_sum_dcf(wacc_ref: str) -> str:
            return " + ".join(
                f"DCF!${xl_col_to_name(DATA0 + k)}${_fe}/(1+{wacc_ref})^{_exps[k]}"
                for k in range(n_proj)
            )

        mid = len(dcf.wacc_range) // 2

        def _mk_price(**kw):
            return wb.add_format({"font_name": "Arial", "font_size": 10,
                                   "valign": "vcenter", "align": "right",
                                   "num_format": "$#,##0.00", **kw})
        sens_base_f = _mk_price(font_color="#FFFFFF",  bold=True, bg_color=_Fmt.NAVY)
        sens_hi_f   = _mk_price(font_color=_Fmt.GREEN, bg_color="#DEEAF1")
        sens_norm_f = _mk_price(font_color=_Fmt.GREEN)   # green: cross-sheet per spec

        # ── Table 1: WACC × Terminal Growth (Gordon) ──────────────────────────
        ws.set_row(R["tbl1_hdr"], 16)
        self._span(ws, R["tbl1_hdr"], LABEL, DATA0 + 4, "WACC × Terminal Growth (Gordon)", fmt.lbl_sec)
        ws.write(R["tbl1_axis"], LABEL, "WACC ↓  /  Terminal g →", fmt.lbl_i)

        # Write g values as blue % numerics (mixed-ref column headers)
        for j, g in enumerate(dcf.gordon_growth_range):
            ws.write(R["tbl1_cols"], DATA0 + j, g, fmt.hc_p)
            ws.write_comment(R["tbl1_cols"], DATA0 + j,
                             f"Terminal growth scenario: {g:.1%}. Must be ≤ long-run nominal GDP growth. "
                             "cite:analyst:terminal_growth",
                             {"width": 240, "height": 60})
        _g_hdr_er = R["tbl1_cols"] + 1   # Excel row

        last_ufcf_col = xl_col_to_name(DATA0 + n_proj - 1)
        for i, w_val in enumerate(dcf.wacc_range):
            r       = R["tbl1_start"] + i
            r_excel = r + 1
            ws.write(r, LABEL, w_val, fmt.hc_p)
            ws.write_comment(r, LABEL,
                             f"WACC scenario: {w_val:.1%}. Edit to stress-test valuation. "
                             "cite:analyst:wacc_sensitivity",
                             {"width": 240, "height": 60})
            wacc_ref = f"${_lc}${r_excel}"
            for j, g_val in enumerate(dcf.gordon_growth_range):
                col   = DATA0 + j
                g_ref = f"{xl_col_to_name(col)}${_g_hdr_er}"
                tv_pv = (f"IF({wacc_ref}>{g_ref},"
                         f"DCF!${last_ufcf_col}${_fe}*(1+{g_ref})"
                         f"/(({wacc_ref}-{g_ref})*(1+{wacc_ref})^{n_proj}),0)")
                bridge  = f"-DCF!${_D}${_debt_er}+DCF!${_D}${_cash_er}"
                shares  = f"DCF!${_D}${_shrs_er}"
                formula = (f"=IF({shares}<>0,"
                           f"({_ufcf_sum_dcf(wacc_ref)}+{tv_pv}{bridge})/{shares},0)")
                cache = dcf.sensitivity_gordon[i][j]
                is_base = (i == mid) and (j == len(dcf.gordon_growth_range) // 2)
                f = sens_base_f if is_base else (sens_hi_f if (i == mid) else sens_norm_f)
                ws.write_formula(r, col, formula, f, cache)

        # ── Table 2: WACC × Exit Multiple ─────────────────────────────────────
        ws.set_row(R["tbl2_hdr"], 16)
        self._span(ws, R["tbl2_hdr"], LABEL, DATA0 + 4, "WACC × Exit EBITDA Multiple", fmt.lbl_sec)
        ws.write(R["tbl2_axis"], LABEL, "WACC ↓  /  Exit Mult →", fmt.lbl_i)

        # Write multiples as blue x-format numerics
        for j, m in enumerate(dcf.ebitda_multiple_range):
            ws.write(R["tbl2_cols"], DATA0 + j, m, fmt.hc_m)
            ws.write_comment(R["tbl2_cols"], DATA0 + j,
                             f"Exit multiple scenario: {m:.1f}x EBITDA. "
                             "Edit to stress-test valuation. cite:analyst:exit_multiple",
                             {"width": 240, "height": 60})
        _mult_hdr_er = R["tbl2_cols"] + 1   # Excel row

        for i, w_val in enumerate(dcf.wacc_range):
            r       = R["tbl2_start"] + i
            r_excel = r + 1
            ws.write(r, LABEL, w_val, fmt.hc_p)
            ws.write_comment(r, LABEL,
                             f"WACC scenario: {w_val:.1%}. Edit to stress-test valuation. "
                             "cite:analyst:wacc_sensitivity",
                             {"width": 240, "height": 60})
            wacc_ref = f"${_lc}${r_excel}"
            for j, m_val in enumerate(dcf.ebitda_multiple_range):
                col      = DATA0 + j
                mult_ref = f"{xl_col_to_name(col)}${_mult_hdr_er}"
                tv_pv    = (f"DCF!${_D}${_t1e_er}*{mult_ref}"
                            f"/(1+{wacc_ref})^{n_proj}")
                bridge   = f"-DCF!${_D}${_debt_er}+DCF!${_D}${_cash_er}"
                shares   = f"DCF!${_D}${_shrs_er}"
                formula  = (f"=IF({shares}<>0,"
                            f"({_ufcf_sum_dcf(wacc_ref)}+{tv_pv}{bridge})/{shares},0)")
                cache = dcf.sensitivity_ebitda[i][j]
                is_base = (i == mid) and (j == len(dcf.ebitda_multiple_range) // 2)
                f = sens_base_f if is_base else (sens_hi_f if (i == mid) else sens_norm_f)
                ws.write_formula(r, col, formula, f, cache)

        ws.set_landscape()
        ws.fit_to_pages(1, 0)
        ws.set_footer(f"&L Sensitivities &C {self.co} &R Page &P")

    # ─────────────────────────────────────────────────────────────────────────
    # Public Comps tabs (Peers detail + Summary)
    # ─────────────────────────────────────────────────────────────────────────

    def _write_comps_peers(self, wb, ws, fmt: _Fmt, pc: PublicCompsOutput) -> None:
        ws.hide_gridlines(2)
        ws.set_column(MARGIN, MARGIN2, 3)
        ws.set_column(LABEL, LABEL, 14)
        ws.set_column(DATA0, DATA0 + 26, 12)

        ws.set_row(2, 28)
        self._span(ws, 2, LABEL, DATA0 + 26, f"{pc.target_company_name} — Public Comps  (Peer Detail)", fmt.hbar)
        ws.write(4, LABEL, f"As of {pc.as_of_date}  |  Source: {pc.source}", fmt.hsub)
        ws.write(5, LABEL, "(USD $ in millions, multiples per spec)", fmt.hunit)

        # Column headers
        cols = [
            "Ticker", "Tier", "Price", "52w High", "52w Low",
            "Shares (M)", "Mkt Cap ($M)", "Debt", "Cash", "EV ($M)",
            "LTM Rev", "LTM EBITDA", "LTM EBIT", "LTM NI", "LTM EPS",
            "EV/Rev", "EV/EBITDA", "EV/EBIT", "P/E", "% off High",
            "NTM Rev", "FY+1 Rev", "FY+2 Rev",
            "EV/Rev NTM", "EV/EBITDA NTM", "EV/EBITDA FY+1", "P/E NTM",
        ]
        for i, c in enumerate(cols):
            ws.write(7, LABEL + i, c, fmt.hcol)

        def _v(v, f):
            if v is None:
                ws.write(r, c_idx, "NM", f)
            else:
                ws.write(r, c_idx, v, f)

        for i, p in enumerate(pc.peers):
            r = 8 + i
            pct_off_hi = (1 - p.share_price / p.week52_high) if p.week52_high > 0 else 0
            row_vals = [
                p.ticker, p.tier, p.share_price, p.week52_high, p.week52_low,
                p.shares_diluted, p.market_cap, p.total_debt, p.cash, p.enterprise_value,
                p.ltm_revenue, p.ltm_ebitda, p.ltm_ebit, p.ltm_net_income, p.ltm_eps_diluted,
                p.ev_rev_ltm, p.ev_ebitda_ltm, p.ev_ebit_ltm, p.pe_ltm, pct_off_hi,
                p.ntm_revenue, p.fy1_revenue, p.fy2_revenue,
                p.ev_rev_ntm, p.ev_ebitda_ntm, p.ev_ebitda_fy1, p.pe_ntm,
            ]
            col_labels = [
                "Ticker", "Tier", "Share Price", "52w High", "52w Low",
                "Shares Diluted (M)", "Market Cap ($M)", "Total Debt ($M)", "Cash ($M)", "EV ($M)",
                "LTM Revenue ($M)", "LTM EBITDA ($M)", "LTM EBIT ($M)", "LTM Net Income ($M)", "LTM EPS",
                "EV/Revenue (LTM)", "EV/EBITDA (LTM)", "EV/EBIT (LTM)", "P/E (LTM)", "% off 52w High",
                "NTM Revenue ($M)", "FY+1 Revenue ($M)", "FY+2 Revenue ($M)",
                "EV/Revenue (NTM)", "EV/EBITDA (NTM)", "EV/EBITDA (FY+1)", "P/E (NTM)",
            ]
            formats = [fmt.lbl, fmt.hc] + [fmt.hc_d] * 3 + [fmt.hc] + [fmt.hc_d] * 4 + \
                      [fmt.hc_d] * 4 + [fmt.hc] + [fmt.hc_m] * 4 + [fmt.hc_p] + \
                      [fmt.hc_d] * 3 + [fmt.hc_m] * 3 + [fmt.hc_m]
            for c_idx_off, (val, f, lbl) in enumerate(zip(row_vals, formats, col_labels)):
                c_idx = LABEL + c_idx_off
                if val is None:
                    ws.write(r, c_idx, "NM", f)
                else:
                    ws.write(r, c_idx, val, f)
                    if isinstance(val, (int, float)):
                        try:
                            ws.write_comment(r, c_idx,
                                             f"{p.ticker} — {lbl}: source: LLM / market data. "
                                             "cite:market:comps_data",
                                             {"width": 240, "height": 60})
                        except Exception:
                            pass

        ws.set_landscape()
        ws.fit_to_pages(1, 0)
        ws.set_footer(f"&L Comps Peers &C {self.co} &R Page &P")
        ws.set_print_scale(70)

    def _write_comps_summary(self, wb, ws, fmt: _Fmt, pc: PublicCompsOutput) -> None:
        ws.hide_gridlines(2)
        ws.set_column(MARGIN, MARGIN2, 3)
        ws.set_column(LABEL, LABEL, 26)
        ws.set_column(DATA0, DATA0 + 6, 14)

        ws.set_row(2, 28)
        self._span(ws, 2, LABEL, DATA0 + 6, f"{pc.target_company_name} — Public Comps  (Summary Stats)", fmt.hbar)
        ws.write(4, LABEL, "Peer-Set Trading Multiples + Implied Target Valuation", fmt.hsub)
        ws.write(5, LABEL, f"As of {pc.as_of_date}  |  Source: {pc.source}", fmt.hunit)

        # Stats table header
        ws.set_row(7, 16)
        ws.write(7, LABEL, "STATISTICS BY MULTIPLE", fmt.lbl_sec)
        for i, h in enumerate(["Min", "P25", "Median", "Mean", "P75", "Max", "Count"]):
            ws.write(8, DATA0 + i, h, fmt.hcol)

        stat_names = ["Min", "P25", "Median", "Mean", "P75", "Max"]
        for i, key in enumerate([
            "ev_rev_ltm", "ev_ebitda_ltm", "ev_ebit_ltm", "pe_ltm",
            "ev_rev_ntm", "ev_ebitda_ntm", "pe_ntm",
        ]):
            r = 9 + i
            s = pc.stats.get(key)
            mult_name = s.multiple_name if s else key
            ws.write(r, LABEL, mult_name, fmt.lbl)
            if s and s.count > 0:
                vals = [s.min, s.p25, s.median, s.mean, s.p75, s.max]
                for j, v in enumerate(vals):
                    ws.write(r, DATA0 + j, v, fmt.hc_m)
                    try:
                        ws.write_comment(r, DATA0 + j,
                                         f"{mult_name} — {stat_names[j]}: computed across {s.count} peers. "
                                         "cite:model:comps_stats",
                                         {"width": 260, "height": 60})
                    except Exception:
                        pass
                ws.write(r, DATA0 + 6, s.count, fmt.hc)
                try:
                    ws.write_comment(r, DATA0 + 6,
                                     f"{mult_name} — peer count (NM outliers excluded). "
                                     "cite:model:comps_stats",
                                     {"width": 260, "height": 60})
                except Exception:
                    pass
            else:
                for j in range(7):
                    ws.write(r, DATA0 + j, "NM", fmt.hc)

        # Implied Valuation
        ws.set_row(15, 16)
        ws.write(15, LABEL, "IMPLIED TARGET VALUATION  (EV / EBITDA basis)", fmt.lbl_sec)
        ws.write(16, LABEL, "  Target LTM EBITDA ($M)", fmt.lbl)
        ws.write(16, DATA0, pc.target_ebitda, fmt.hc_d)
        ws.write_comment(16, DATA0,
                         "Target LTM EBITDA: source LLM / XBRL. cite:xbrl:us-gaap:OperatingIncomeLoss",
                         {"width": 280, "height": 60})
        ws.write(17, LABEL, "  Target Net Debt ($M)", fmt.lbl)
        ws.write(17, DATA0, pc.target_total_debt - pc.target_cash, fmt.hc_d)
        ws.write_comment(17, DATA0,
                         "Target net debt: total debt minus cash (XBRL). cite:xbrl:us-gaap:LongTermDebt",
                         {"width": 280, "height": 60})
        ws.write(18, LABEL, "  Target Diluted Shares (M)", fmt.lbl)
        ws.write(18, DATA0, pc.target_shares_diluted, fmt.hc)
        ws.write_comment(18, DATA0,
                         "Target diluted shares: XBRL / market data. cite:xbrl:us-gaap:CommonStockSharesOutstanding",
                         {"width": 280, "height": 60})

        ws.set_row(20, 16)
        ws.write(20, LABEL, "Implied Per-Share Price (low / median / high)", fmt.lbl_b)
        ws.write(20, DATA0,     pc.implied_price_low,    fmt.tot_d)
        ws.write(20, DATA0 + 1, pc.implied_price_median, fmt.tot_d)
        ws.write(20, DATA0 + 2, pc.implied_price_high,   fmt.tot_d)
        ws.write(21, DATA0,     "p25 multiple", fmt.lbl_i)
        ws.write(21, DATA0 + 1, "median",       fmt.lbl_i)
        ws.write(21, DATA0 + 2, "p75",          fmt.lbl_i)

        # Excluded peers
        if pc.excluded:
            ws.set_row(24, 16)
            ws.write(24, LABEL, "EXCLUDED CANDIDATES", fmt.lbl_sec)
            for i, (tk, reason) in enumerate(pc.excluded[:15]):
                ws.write(25 + i, LABEL, f"  {tk}", fmt.lbl)
                ws.write(25 + i, DATA0, reason, fmt.lbl_i)

        ws.set_landscape()
        ws.fit_to_pages(1, 0)
        ws.set_footer(f"&L Comps Summary &C {self.co} &R Page &P")

    # ─────────────────────────────────────────────────────────────────────────
    # Sources tab
    # ─────────────────────────────────────────────────────────────────────────

    def _write_sources(self, wb, ws, fmt: _Fmt) -> None:
        ws.hide_gridlines(2)
        ws.set_column(MARGIN, MARGIN2, 3)
        ws.set_column(LABEL, LABEL, 28)
        for col, w in [(DATA0, 12), (DATA0+1, 14), (DATA0+2, 14),
                       (DATA0+3, 30), (DATA0+4, 10)]:
            ws.set_column(col, col, w)

        self._span(ws, 2, LABEL, DATA0 + 5, self.co, fmt.hbar)
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
