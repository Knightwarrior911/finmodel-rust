//! Dynamic Income-Statement structure — port of `src/is_builder.py`. Produces
//! the `ISRow` list (standard + sector archetypes + XBRL detail), the key→row
//! map, and the empty-IS fallback map that BS/CF fall back to when no IS body is
//! built.

use std::collections::HashMap;

/// IS body starts at 0-based row 10 (Excel row 11).
pub const IS_BODY_START: u32 = 10;

/// driver_key → offset into the Assumptions ACTIVE block (`active_drv0` + offset).
pub fn driver_assump_offset(driver_key: &str) -> Option<u32> {
    Some(match driver_key {
        "revenue_growth_pct" => 0,
        "gross_margin_pct" => 1,
        "sga_pct_rev" => 2,
        "rd_pct_rev" => 3,
        "da_pct_rev" => 4,
        "capex_pct_rev" => 5,
        "tax_rate_pct" => 6,
        "interest_rate_pct" => 7,
        "dso_days" => 8,
        "dio_days" => 9,
        "dpo_days" => 10,
        "dividend_per_share" => 11,
        "terminal_growth_rate" => 12,
        "exit_ebitda_multiple" => 13,
        // Per-segment revenue-growth drivers map to the revenue-growth slot.
        k if k.ends_with("_growth_pct") => 0,
        _ => return None,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowType {
    SectionHeader,
    LineItem,
    Subtotal,
    Driver,
    Memo,
    Spacer,
}

/// One row of the dynamic IS structure.
#[derive(Clone, Debug)]
pub struct ISRow {
    pub key: String,
    pub label: String,
    pub row_type: RowType,
    pub bold: bool,
    pub driver_key: String,
    pub driver_format: String, // "pct" | "num"
    pub hist_numer_key: String, // "__growth" = YoY growth
    pub hist_denom_key: String,
}

fn li(key: &str, label: &str, bold: bool) -> ISRow {
    ISRow { key: key.into(), label: label.into(), row_type: RowType::LineItem, bold, driver_key: String::new(), driver_format: "num".into(), hist_numer_key: String::new(), hist_denom_key: String::new() }
}
fn st(key: &str, label: &str) -> ISRow {
    ISRow { key: key.into(), label: label.into(), row_type: RowType::Subtotal, bold: true, driver_key: String::new(), driver_format: "num".into(), hist_numer_key: String::new(), hist_denom_key: String::new() }
}
fn sec(label: &str) -> ISRow {
    ISRow { key: String::new(), label: label.into(), row_type: RowType::SectionHeader, bold: true, driver_key: String::new(), driver_format: "num".into(), hist_numer_key: String::new(), hist_denom_key: String::new() }
}
fn drv(label: &str, driver_key: &str, hist_numer: &str, hist_denom: &str) -> ISRow {
    ISRow {
        key: format!("__drv_{driver_key}"),
        label: format!("  {label}"),
        row_type: RowType::Driver,
        bold: false,
        driver_key: driver_key.into(),
        driver_format: "pct".into(),
        hist_numer_key: hist_numer.into(),
        hist_denom_key: hist_denom.into(),
    }
}
fn mo(key: &str, label: &str, numer: &str, denom: &str) -> ISRow {
    ISRow { key: key.into(), label: label.into(), row_type: RowType::Memo, bold: false, driver_key: String::new(), driver_format: "pct".into(), hist_numer_key: numer.into(), hist_denom_key: denom.into() }
}
fn sp() -> ISRow {
    ISRow { key: String::new(), label: String::new(), row_type: RowType::Spacer, bold: false, driver_key: String::new(), driver_format: "num".into(), hist_numer_key: String::new(), hist_denom_key: String::new() }
}

/// A revenue segment (label + data key).
pub struct Segment {
    pub label: String,
    pub key: String,
}
/// An operating-expense line item from XBRL disclosure.
pub struct OpexItem {
    pub label: String,
    pub key: String,
    pub category: String, // "cogs" | "opex_rd" | "opex"
}
/// A cost-of-revenue sub-item (detailed COGS breakdown).
pub struct CogsDetail {
    pub label: String,
    pub key: String,
}

/// Standard-sector IS with no XBRL detail — the archetype fallback.
pub fn build_standard_is(has_cogs: bool, has_rd: bool, has_sga: bool) -> Vec<ISRow> {
    build_standard_is_detailed(has_cogs, has_rd, has_sga, &[], &[], &[])
}

/// Standard-sector IS, optionally driven by XBRL segment/opex detail. Mirrors
/// `_build_standard_is`. Extra opex items (idx>0) must carry `opex_`-prefixed
/// keys so they project held-flat and are subtracted into EBIT.
pub fn build_standard_is_detailed(
    has_cogs: bool,
    has_rd: bool,
    has_sga: bool,
    segments: &[Segment],
    opex_items: &[OpexItem],
    cogs_detail: &[CogsDetail],
) -> Vec<ISRow> {
    let mut rows: Vec<ISRow> = Vec::new();

    // ── Revenue ──────────────────────────────────────────────────────────────
    if !segments.is_empty() {
        for seg in segments {
            rows.push(li(&seg.key, &format!("  {}", seg.label), false));
            rows.push(drv(&format!("{} Growth %", seg.label), &format!("{}_growth_pct", seg.key), "__growth", &seg.key));
        }
        rows.push(sp());
        rows.push(st("revenue", "Total Revenue"));
        rows.push(sp());
    } else {
        rows.push(li("revenue", "Revenue", true));
        rows.push(drv("Revenue Growth %", "revenue_growth_pct", "__growth", "revenue"));
        rows.push(sp());
    }

    // ── COGS / OpEx ──────────────────────────────────────────────────────────
    if !opex_items.is_empty() {
        let cogs_items: Vec<&OpexItem> = opex_items.iter().filter(|o| o.category == "cogs").collect();
        let rd_items: Vec<&OpexItem> = opex_items.iter().filter(|o| o.category == "opex_rd").collect();
        let other_oe: Vec<&OpexItem> = opex_items.iter().filter(|o| o.category == "opex").collect();

        if !cogs_items.is_empty() {
            rows.push(sec("COST OF REVENUES"));
            if !cogs_detail.is_empty() {
                for cd in cogs_detail {
                    rows.push(li(&cd.key, &format!("  {}", cd.label), false));
                }
                rows.push(st("cogs", "  Total Cost of Revenues"));
            } else {
                for ci in &cogs_items {
                    rows.push(li("cogs", &format!("  {}", ci.label), false));
                }
            }
            rows.push(st("gross_profit", "Gross Profit"));
            rows.push(drv("Gross Margin %", "gross_margin_pct", "gross_profit", "revenue"));
            rows.push(sp());
        }
        if !rd_items.is_empty() || !other_oe.is_empty() {
            rows.push(sec("OPERATING EXPENSES"));
            for (idx, ri) in rd_items.iter().enumerate() {
                let key = if idx == 0 { "rd".to_string() } else { ri.key.clone() };
                rows.push(li(&key, &format!("  {}", ri.label), false));
                if idx == 0 {
                    rows.push(drv("R&D % of Revenue", "rd_pct_rev", "rd", "revenue"));
                }
            }
            for (idx, oi) in other_oe.iter().enumerate() {
                let key = if idx == 0 { "sga".to_string() } else { oi.key.clone() };
                rows.push(li(&key, &format!("  {}", oi.label), false));
                if idx == 0 {
                    rows.push(drv(&format!("{} % of Revenue", oi.label), "sga_pct_rev", "sga", "revenue"));
                }
            }
        }
    } else {
        if has_cogs {
            rows.push(sec("COST OF REVENUES"));
            rows.push(li("cogs", "  Cost of Revenue", false));
            rows.push(st("gross_profit", "Gross Profit"));
            rows.push(drv("Gross Margin %", "gross_margin_pct", "gross_profit", "revenue"));
            rows.push(sp());
        }
        rows.push(sec("OPERATING EXPENSES"));
        if has_rd {
            rows.push(li("rd", "  Research & Development", false));
            rows.push(drv("R&D % of Revenue", "rd_pct_rev", "rd", "revenue"));
        }
        if has_sga {
            rows.push(li("sga", "  Selling, General & Administrative", false));
            rows.push(drv("SG&A % of Revenue", "sga_pct_rev", "sga", "revenue"));
        }
    }

    // ── Common tail ──────────────────────────────────────────────────────────
    rows.push(sp());
    rows.push(st("ebit", "Operating Income (EBIT)"));
    rows.push(mo("ebit_margin", "  EBIT Margin %", "ebit", "revenue"));
    rows.push(sp());
    rows.push(st("ebita", "EBITA"));
    rows.push(mo("ebita_margin", "  EBITA Margin %", "ebita", "revenue"));
    rows.push(sp());
    rows.push(li("da", "  (+) Depreciation & Amortization", false));
    rows.push(drv("D&A % of Revenue", "da_pct_rev", "da", "revenue"));
    rows.push(st("ebitda", "EBITDA"));
    rows.push(mo("ebitda_margin", "  EBITDA Margin %", "ebitda", "revenue"));
    rows.push(sp());
    rows.push(sec("OTHER INCOME / EXPENSE"));
    rows.push(li("interest_expense", "  Interest Expense", false));
    rows.push(drv("Interest Rate %", "interest_rate_pct", "", ""));
    rows.push(li("interest_income", "  Interest Income", false));
    rows.push(st("ebt", "EBT"));
    rows.push(sp());
    rows.push(li("income_tax", "  Income Tax", false));
    rows.push(drv("Effective Tax Rate %", "tax_rate_pct", "income_tax", "ebt"));
    rows.push(st("net_income", "Net Income"));
    rows.push(mo("net_margin", "  Net Margin %", "net_income", "revenue"));
    rows.push(li("nci_income_loss", "  Less: Net Income to NCI", false));
    rows.push(st("ni_common", "Net Income to Common"));
    rows.push(mo("ni_common_margin", "  Net Margin % (to Common)", "ni_common", "revenue"));
    rows.push(sp());
    rows.push(sec("PER SHARE DATA"));
    rows.push(li("eps_diluted", "  EPS — Diluted", false));
    rows.push(li("eps_basic", "  EPS — Basic", false));
    rows.push(li("shares_diluted", "  Shares — Diluted (wtd avg)", false));
    rows.push(li("shares_basic", "  Shares — Basic (wtd avg)", false));

    rows
}

/// Utility IS (O&M / taxes-other / other-opex slot repurposing).
pub fn build_utility_is() -> Vec<ISRow> {
    vec![
        li("revenue", "Operating Revenues", true),
        drv("Revenue Growth %", "revenue_growth_pct", "__growth", "revenue"),
        sp(),
        sec("OPERATING EXPENSES"),
        li("utility_om", "  Operation & Maintenance", false),
        drv("O&M % of Revenue", "gross_margin_pct", "utility_om", "revenue"),
        li("da", "  Depreciation & Amortization", false),
        drv("D&A % of Revenue", "da_pct_rev", "da", "revenue"),
        li("utility_taxes_other", "  Taxes other than income taxes", false),
        drv("Taxes other % of Revenue", "sga_pct_rev", "utility_taxes_other", "revenue"),
        li("utility_other", "  Other operating expenses", false),
        drv("Other OpEx % of Revenue", "rd_pct_rev", "utility_other", "revenue"),
        st("utility_total_opex", "Total Operating Expenses"),
        sp(),
        st("ebit", "Operating Income (EBIT)"),
        mo("ebit_margin", "  EBIT Margin %", "ebit", "revenue"),
        sp(),
        st("ebitda", "EBITDA  (EBIT + D&A)"),
        mo("ebitda_margin", "  EBITDA Margin %", "ebitda", "revenue"),
        sp(),
        sec("OTHER INCOME / EXPENSE"),
        li("interest_expense", "  Interest Expense", false),
        drv("Interest Rate %", "interest_rate_pct", "", ""),
        li("interest_income", "  Interest Income", false),
        st("ebt", "EBT"),
        sp(),
        li("income_tax", "  Income Tax", false),
        drv("Effective Tax Rate %", "tax_rate_pct", "income_tax", "ebt"),
        st("net_income", "Net Income"),
        mo("net_margin", "  Net Margin %", "net_income", "revenue"),
        li("nci_income_loss", "  Less: Net Income to NCI", false),
        st("ni_common", "Net Income to Common"),
        mo("ni_common_margin", "  Net Margin % (to Common)", "ni_common", "revenue"),
        sp(),
        sec("PER SHARE DATA"),
        li("eps_diluted", "  EPS — Diluted", false),
        li("eps_basic", "  EPS — Basic", false),
        li("shares_diluted", "  Shares — Diluted (wtd avg)", false),
        li("shares_basic", "  Shares — Basic (wtd avg)", false),
    ]
}

/// Bank IS (interest income / NII / efficiency / provision slots).
pub fn build_bank_is() -> Vec<ISRow> {
    vec![
        sec("INTEREST INCOME"),
        li("revenue", "Interest & Fee Income", true),
        drv("Interest Income Growth %", "revenue_growth_pct", "__growth", "revenue"),
        sp(),
        sec("INTEREST EXPENSE"),
        li("cogs", "  Interest Expense", false),
        st("gross_profit", "Net Interest Income"),
        drv("Net Interest Margin (NIM) %", "gross_margin_pct", "gross_profit", "revenue"),
        sp(),
        sec("NON-INTEREST INCOME / EXPENSE"),
        li("sga", "  Non-Interest Expense", false),
        drv("Efficiency Ratio % of Revenue", "sga_pct_rev", "sga", "revenue"),
        li("rd", "  Provision for Credit Losses", false),
        drv("Credit Cost % of Revenue", "rd_pct_rev", "rd", "revenue"),
        sp(),
        st("ebit", "Pre-Tax, Pre-Provision Income"),
        mo("ebit_margin", "  PTPP Margin %", "ebit", "revenue"),
        sp(),
        li("da", "  (+) D&A", false),
        drv("D&A % of Revenue", "da_pct_rev", "da", "revenue"),
        st("ebitda", "Pre-Tax, Pre-Provision Income + D&A"),
        mo("ebitda_margin", "  EBITDA Margin %", "ebitda", "revenue"),
        sp(),
        sec("BELOW THE LINE"),
        li("interest_income", "  Other Interest Income", false),
        li("interest_expense", "  Long-Term Debt Interest", false),
        drv("Long-Term Debt Interest Rate %", "interest_rate_pct", "", ""),
        st("ebt", "Pre-Tax Income"),
        sp(),
        li("income_tax", "  Income Tax", false),
        drv("Effective Tax Rate %", "tax_rate_pct", "income_tax", "ebt"),
        st("net_income", "Net Income"),
        mo("net_margin", "  Net Margin %", "net_income", "revenue"),
        li("nci_income_loss", "  Less: Net Income to NCI", false),
        st("ni_common", "Net Income to Common"),
        mo("ni_common_margin", "  Net Margin % (to Common)", "ni_common", "revenue"),
        sp(),
        sec("PER SHARE DATA"),
        li("eps_diluted", "  EPS — Diluted", false),
        li("eps_basic", "  EPS — Basic", false),
        li("shares_diluted", "  Shares — Diluted (wtd avg)", false),
        li("shares_basic", "  Shares — Basic (wtd avg)", false),
    ]
}

/// Insurance IS (premiums / benefits / combined-ratio slots).
pub fn build_insurance_is() -> Vec<ISRow> {
    vec![
        sec("REVENUES"),
        li("revenue", "Premiums Earned", true),
        drv("Premium Growth %", "revenue_growth_pct", "__growth", "revenue"),
        sp(),
        sec("BENEFITS & EXPENSES"),
        li("cogs", "  Benefits / Losses & LAE Incurred", false),
        li("rd", "  Acquisition & Underwriting Expenses", false),
        drv("Acquisition Cost % of Premiums", "rd_pct_rev", "rd", "revenue"),
        li("sga", "  General & Administrative Expenses", false),
        drv("G&A % of Premiums", "sga_pct_rev", "sga", "revenue"),
        st("gross_profit", "Total Benefits & Expenses"),
        drv("Combined Ratio %", "gross_margin_pct", "gross_profit", "revenue"),
        sp(),
        st("ebit", "Underwriting Income"),
        mo("ebit_margin", "  Underwriting Margin %", "ebit", "revenue"),
        sp(),
        li("da", "  (+) D&A", false),
        drv("D&A % of Premiums", "da_pct_rev", "da", "revenue"),
        st("ebitda", "EBITDA (adj.)"),
        mo("ebitda_margin", "  EBITDA Margin %", "ebitda", "revenue"),
        sp(),
        sec("NON-UNDERWRITING INCOME / EXPENSE"),
        li("interest_income", "  Net Investment Income", false),
        li("interest_expense", "  Interest Expense", false),
        drv("Interest Rate %", "interest_rate_pct", "", ""),
        st("ebt", "Pre-Tax Income"),
        sp(),
        li("income_tax", "  Income Tax", false),
        drv("Effective Tax Rate %", "tax_rate_pct", "income_tax", "ebt"),
        st("net_income", "Net Income"),
        mo("net_margin", "  Net Margin %", "net_income", "revenue"),
        li("nci_income_loss", "  Less: Net Income to NCI", false),
        st("ni_common", "Net Income to Common"),
        mo("ni_common_margin", "  Net Margin % (to Common)", "ni_common", "revenue"),
        sp(),
        sec("PER SHARE DATA"),
        li("eps_diluted", "  EPS — Diluted", false),
        li("eps_basic", "  EPS — Basic", false),
        li("shares_diluted", "  Shares — Diluted (wtd avg)", false),
        li("shares_basic", "  Shares — Basic (wtd avg)", false),
    ]
}

/// REIT IS (NOI / FFO / AFFO).
pub fn build_reit_is() -> Vec<ISRow> {
    vec![
        sec("REVENUES"),
        li("revenue", "Rental & Property Revenue", true),
        drv("Revenue Growth %", "revenue_growth_pct", "__growth", "revenue"),
        sp(),
        sec("PROPERTY OPERATING EXPENSES"),
        li("cogs", "  Property Operating Expenses", false),
        st("gross_profit", "Net Operating Income (NOI)"),
        drv("NOI Margin %", "gross_margin_pct", "gross_profit", "revenue"),
        sp(),
        sec("CORPORATE EXPENSES"),
        li("sga", "  General & Administrative", false),
        drv("G&A % of Revenue", "sga_pct_rev", "sga", "revenue"),
        li("rd", "  Other Operating Expenses", false),
        drv("Other OpEx % of Revenue", "rd_pct_rev", "rd", "revenue"),
        sp(),
        li("da", "  Depreciation & Amortization", false),
        drv("D&A % of Revenue", "da_pct_rev", "da", "revenue"),
        st("ebit", "Operating Income (EBIT)"),
        mo("ebit_margin", "  EBIT Margin %", "ebit", "revenue"),
        sp(),
        st("ebitda", "EBITDA"),
        mo("ebitda_margin", "  EBITDA Margin %", "ebitda", "revenue"),
        sp(),
        sec("FINANCING COSTS"),
        li("interest_expense", "  Interest Expense", false),
        drv("Interest Rate %", "interest_rate_pct", "", ""),
        li("interest_income", "  Interest Income", false),
        st("ebt", "EBT"),
        sp(),
        li("income_tax", "  Income Tax", false),
        drv("Effective Tax Rate %", "tax_rate_pct", "income_tax", "ebt"),
        st("net_income", "Net Income"),
        mo("net_margin", "  Net Margin %", "net_income", "revenue"),
        li("nci_income_loss", "  Less: Net Income to NCI", false),
        st("ni_common", "Net Income to Common"),
        mo("ni_common_margin", "  Net Margin % (to Common)", "ni_common", "revenue"),
        sp(),
        sec("FFO / AFFO  (supplemental REIT metrics)"),
        li("ffo", "  FFO  (Net Income + D&A)", true),
        li("affo", "  AFFO  (FFO − Recurring CapEx, approx.)", false),
        sp(),
        sec("PER SHARE DATA"),
        li("eps_diluted", "  EPS — Diluted", false),
        li("eps_basic", "  EPS — Basic", false),
        li("shares_diluted", "  Shares — Diluted (wtd avg)", false),
        li("shares_basic", "  Shares — Basic (wtd avg)", false),
    ]
}

/// Dispatch to the sector-appropriate IS structure. Mirrors `build_is_structure`.
pub fn build_is_structure(sector: &str, has_cogs: bool, has_rd: bool, has_sga: bool) -> Vec<ISRow> {
    match sector {
        "utility" => build_utility_is(),
        "bank" => build_bank_is(),
        "insurance" => build_insurance_is(),
        "reit" => build_reit_is(),
        _ => build_standard_is(has_cogs, has_rd, has_sga),
    }
}

/// Map each non-empty key → 0-based row (first occurrence). Mirrors
/// `compute_is_row_map`. Driver keys are stored as `__drv_<driver_key>`.
pub fn compute_is_row_map(rows: &[ISRow]) -> HashMap<String, u32> {
    let mut map = HashMap::new();
    for (i, r) in rows.iter().enumerate() {
        if !r.key.is_empty() {
            map.entry(r.key.clone()).or_insert(IS_BODY_START + i as u32);
        }
    }
    map
}

/// Empty-IS fallback rows (0-based) — the fixed positions BS/CF reference when
/// no IS body is built (matches writer.py `IS_R` for these keys, and the
/// committed empty-IS snapshots).
pub fn fallback_is_row(key: &str) -> u32 {
    match key {
        "circ" => 7,
        "revenue" => 10,
        "cogs" => 12,
        "da" => 24,
        "interest_expense" => 28,
        "net_income" => 34,
        "ni_common" => 37,
        "shares_diluted" => 42,
        _ => 10,
    }
}

/// Override line-item labels with actual XBRL concept labels (preserving leading
/// indentation). Mirrors `_apply_filing_labels`. Certain keys keep the hardcoded
/// label (the taxonomy label is worse).
pub fn apply_filing_labels(rows: &mut [ISRow], filing_labels: &HashMap<String, String>) {
    if filing_labels.is_empty() {
        return;
    }
    const SKIP: [&str; 5] = ["da", "ebitda", "ebit", "gross_profit", "net_income"];
    for r in rows.iter_mut() {
        if r.row_type == RowType::LineItem && !r.key.is_empty() && !SKIP.contains(&r.key.as_str()) {
            if let Some(xl) = filing_labels.get(&r.key) {
                let ws_len = r.label.len() - r.label.trim_start().len();
                r.label = format!("{}{}", &r.label[..ws_len], xl);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filing_labels_override_preserving_indent() {
        let mut rows = build_standard_is(true, true, true);
        let mut fl = HashMap::new();
        fl.insert("rd".to_string(), "Research and development expenses".to_string());
        fl.insert("da".to_string(), "SHOULD BE SKIPPED".to_string());
        apply_filing_labels(&mut rows, &fl);
        let rd = rows.iter().find(|r| r.key == "rd").unwrap();
        assert_eq!(rd.label, "  Research and development expenses");
        let da = rows.iter().find(|r| r.key == "da").unwrap();
        assert_eq!(da.label, "  (+) Depreciation & Amortization");
    }

    #[test]
    fn standard_row_map_positions() {
        let rm = compute_is_row_map(&build_standard_is(true, true, true));
        assert_eq!(rm.get("revenue"), Some(&10));
        assert_eq!(rm.get("da"), Some(&30));
        assert_eq!(rm.get("net_income"), Some(&43));
        assert_eq!(rm.get("shares_diluted"), Some(&52));
    }
}
