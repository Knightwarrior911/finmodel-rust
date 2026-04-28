# SPEC_methodology.md - DCF Model Methodology

This file defines the **methodology** for a DCF valuation. Sector-specific adaptations are in **Section 9**.

---

## 1. Tab Structure

| Tab            | Content |
|----------------|---------|
| **Cover**      | Model overview, inputs summary, output summary |
| **Assumptions**| Operating drivers (growth, margins), WACC components, terminal assumptions, case toggle (optional) |
| **DCF**        | UFCF buildup, discount factors, terminal value, EV, equity bridge, per-share price |
| **WACC**       | Comparable beta unlever/relever, CAPM, capital structure weights |
| **Sensitivities** | Two 2D tables: WACC × terminal g, and WACC × exit multiple |

---

## 2. UFCF Buildup

**UFCF = EBIT × (1 - t) + D&A – CapEx – ΔNWC**

### 2.1 Inputs and Drivers

- **Revenue**  
  **Revenue Growth %**  [italic memo]

- **(-) Operating expenses** (or: COGS + OpEx detail)  
  **EBIT**  [bold]  
  **EBIT Margin %**  [italic memo]

- **(×) (1 – Tax Rate)**  
  **NOPAT**  [bold]

- **(+) Depreciation & Amortization**  [from PP&E logic or % of revenue]  
- **(-) Capital Expenditures**  [% of revenue or absolute]  
- **(-) Change in Net Working Capital**  [% of revenue or per-period delta]

**Unlevered Free Cash Flow**  [bold, $ format]  
**UFCF Margin %**  [italic memo]

### 2.2 Driver Placement
- Each driver lives **directly below** the line item it drives, italicized, indented.
- Drivers are pure green links to the Active block on Assumptions (two-hop pattern).
- Historical rows back-calculate the implied driver from the actual line items.

### 2.3 Mid-year vs. Year-end Convention
- **Default**: mid-year. Discount factor = `1 / (1 + WACC)^(t - 0.5)`.
- **Year-end**: `1 / (1 + WACC)^t`.
- Be consistent — don’t mix conventions within the same model.

---

## 3. Discount Factor Block

| Year | 1 | 2 | 3 | 4 | 5 | Terminal |
|------|---|---|---|---|---|----------|
| Period (mid-year) | 0.5 | 1.5 | 2.5 | 3.5 | 4.5 | 5.5 |
| Discount Factor | `=1/(1+WACC)^period` | | | | | |
| PV of UFCF | `=UFCF × DF` | | | | | |

The discount factor for the terminal value uses the same logic as the last explicit year.

---

## 4. Terminal Value

Build **BOTH** methods side by side. The user selects which to highlight as primary via a toggle on the DCF tab.

### 4.1 Perpetuity Growth Method (Gordon Growth)
- Terminal UFCF (Year N+1) = `UFCF_N × (1 + g)`
- Terminal Value (at end of year N) = `UFCF_N+1 / (WACC – g)`
- PV of Terminal Value = `TV × Discount Factor at year N`

**Rules:**
- `g` must be ≤ long-run nominal GDP growth (typically 2.0%-3.5%). Never above 4%.
- Flag if `WACC – g < 2%`.

### 4.2 Exit Multiple Method
- Terminal EBITDA (Year N) = `EBITDA_N`
- Exit Multiple = [hardcoded or peer-set median]
- Terminal Value = `EBITDA_N × Exit Multiple`
- PV of Terminal Value = `TV × Discount Factor at year N`

Use the peer-set median EV/EBITDA NTM multiple by default. Adjust for premium/cyclical businesses.

### 4.3 Implied vs. Cross-Check
- Implied perpetuity growth from exit multiple
- Implied exit multiple from perpetuity growth
- Show both. If they diverge >25%, one method is mis-calibrated.

### 4.4 Terminal Value as % of EV
- Typically 60-80% for stable businesses.
- Can exceed 90% for high-growth / pre-cash-flow companies (extend explicit period to 10 years).

---

## 5. WACC Construction

### 5.1 CAPM Cost of Equity
`Ke = Rf + Beta × ERP`

| Component | Source / Convention |
|-----------|---------------------|
| Rf (risk-free rate) | 10-year US Treasury yield (or local gov’t bond) |
| ERP | 5.0%-6.0% for US (Damodaran or Duff & Phelps) |
| Beta | Re-levered peer-set unlevered beta |
| Country risk premium | Add for emerging-market issuers |
| Size premium | Add for small-cap stocks |

### 5.2 Beta Calculation
Build peer-set table on WACC tab:

- Unlever: `Bu = Be / (1 + (1 - t) × D/E)`
- Re-lever: `Be_target = Bu_median × (1 + (1 - t) × (D/E)_target)`

Use median unlevered beta.

### 5.3 Cost of Debt
- Pre-tax: yield to maturity or credit-adjusted spread + Rf.
- After-tax: `Kd × (1 - t)`.

### 5.4 Capital Structure Weights
Use **target** capital structure (long-run optimal mix), not current.

`We = E / (D + E + Pref)`  
`Wd = D / (D + E + Pref)`

Equity at market value, debt at book value (or market for distressed).

### 5.5 WACC Formula
`WACC = We × Ke + Wd × Kd × (1 - t) + Wp × Kp`

---

## 6. Equity Bridge

- **Enterprise Value** (sum of PV of UFCFs + PV of TV)
- (-) Total Debt
- (-) Preferred Stock
- (-) Noncontrolling Interest
- (+) Cash & Cash Equivalents
- (+) Short-term Investments
- (+) Long-term Investments (if material)
- **Equity Value**  [bold]
- **Diluted Shares Outstanding** (treasury method)
- **Implied Per-Share Price** = Equity Value / Diluted Shares
- **Upside / (Downside) %** = (Implied / Current – 1)

**Notes:**
- Use balance sheet date values for debt, cash, NCI.
- Always use diluted shares (treasury method).

---

## 7. Sensitivity Tables

Build two 2D tables on the Sensitivities tab.

### 7.1 WACC × Terminal Growth
### 7.2 WACC × Exit Multiple

**Implementation rules:**
- Use mixed cell references (`$A5`, `B$4`).
- Do **not** use Excel Data Table feature.
- Highlight base case cell.
- Center headers; bold them.

---

## 8. Cross-Checks

Display on DCF tab:
- TV / EV % (should be 60-80%)
- WACC – Terminal g
- Implied Exit Multiple & Implied Perpetuity g
- Implied Per-Share / Current Price (typically within ±30%)

If any cross-check fails, revisit assumptions.

---

## 9. Sector Adaptations

### 9.1 Banks and Financial Institutions
- Use Dividend Discount Model or Residual Income instead of UFCF DCF.
- Drivers: ROE, payout ratio, cost of equity.

### 9.2 Insurance
- Focus on FCFE or embedded/appraisal value.

### 9.3 REITs
- Use FFO/AFFO or NAV approach.

### 9.4 High-growth / Pre-profit Tech
- Extend explicit forecast to 10 years (two-stage model).

### 9.5 Cyclical / Commodity-exposed
- Normalize terminal year to mid-cycle margins.

### 9.6 Asset-heavy, Capital-intensive
- Strong focus on reinvestment rate (CapEx ≈ D&A in terminal).

---

## 10. Common Errors

- WACC uses current (not target) capital structure.
- Terminal g ≈ WACC (explosive TV).
- Inconsistent mid-year / year-end discounting.
- Double-counting operating leases.
- Tax rate flat at statutory instead of effective/normalized.
- Beta from company itself instead of peer median.
- Omitting RSUs/options in diluted shares.
- Incorrect cash netting (restricted cash).

---

**End of SPEC_methodology.md (DCF)**