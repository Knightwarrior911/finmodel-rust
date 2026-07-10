//! Financial statement section detection in PDF text.
//!
//! Ported from `_extract_financial_section()` in `src/extractor.py`.
//! Finds the consolidated financial statements section by locating anchor
//! phrases (IS/BS/CFS headers) paired with actual numeric data rows.

/// Anchor phrases marking the start of consolidated income statement.
const IS_ANCHORS: &[&str] = &[
    "consolidated income statement",
    "consolidated statement of profit",
    "consolidated statement of operations",
    "income statement",
    "statement of income",
    "statement of operations",
    "statement of profit or loss",
    "profit and loss account",
];

/// Anchor phrases marking the start of consolidated balance sheet.
const BS_ANCHORS: &[&str] = &[
    "consolidated balance sheet",
    "consolidated statement of financial position",
    "balance sheet",
    "statement of financial position",
];

/// Anchor phrases marking the start of consolidated cash flow statement.
const CF_ANCHORS: &[&str] = &[
    "consolidated statement of cash flow",
    "consolidated cash flow statement",
    "statement of cash flows",
    "cash flow statement",
];

/// Fallback anchors — any of these triggers fallback 1.
const FALLBACK_ANCHORS: &[&str] = &[
    "consolidated income statement",
    "consolidated statement of profit",
    "consolidated statement of comprehensive income",
    "consolidated balance sheet",
    "consolidated statement of financial position",
    "consolidated statement of cash flow",
];

/// Return the text of the consolidated financial statements section.
///
/// Strategy (matches Python `_extract_financial_section`):
/// 1. Find pages where each face statement starts (IS, BS, CFS independently).
/// 2. Return each face + 3 following pages (covers face statements + key notes).
/// 3. Fallback 1: first bare anchor-phrase page + window.
/// 4. Fallback 2: first 150K chars of full report.
///
/// Ported from `src/extractor.py:_extract_financial_section`.
pub fn extract_financial_section(text_pages: &[String], notes_window: usize) -> String {
    let rev_re = regex::Regex::new(
        r"(?i)(?:revenues?|net sales|net revenue|net turnover|turnover|total revenue|sales revenue|net sales revenue)\b[^\n]*?\d[\d \u{00A0}\u{202F}]{2,}[^\n]*?\d[\d \u{00A0}\u{202F}]{2,}"
    ).ok();
    let tot_asset_re = regex::Regex::new(
        r"(?i)total (?:assets|equity)\b[^\n]*?\d[\d \u{00A0}\u{202F}]{2,}"
    ).ok();
    let cfo_re = regex::Regex::new(
        r"(?i)(?:operating activities|net cash)\b[^\n]*?\d[\d \u{00A0}\u{202F}]{2,}"
    ).ok();
    let year_re = regex::Regex::new(r"\b20[0-9]{2}\b").unwrap();

    // Check if a page has an anchor phrase + data row + >=2 year references
    let matches_face = |page: &str, phrases: &[&str], data_re: &Option<regex::Regex>| -> bool {
        let lower = page.to_lowercase();
        if !phrases.iter().any(|p| lower.contains(p)) {
            return false;
        }
        if let Some(re) = data_re {
            if !re.is_match(page) {
                return false;
            }
        }
        year_re.find_iter(page).count() >= 2
    };

    // Slice-based approach: find each face independently
    let mut slices: std::collections::BTreeMap<usize, String> = std::collections::BTreeMap::new();

    for (phrases, data_re) in &[
        (IS_ANCHORS, &rev_re),
        (BS_ANCHORS, &tot_asset_re),
        (CF_ANCHORS, &cfo_re),
    ] {
        for (i, page_text) in text_pages.iter().enumerate() {
            if matches_face(page_text, phrases, data_re) {
                // Take this page + next 3
                for j in i..std::cmp::min(i + 4, text_pages.len()) {
                    slices.entry(j).or_insert_with(|| text_pages[j].clone());
                }
                break;
            }
        }
    }

    if !slices.is_empty() {
        let ordered: Vec<&str> = slices.values().map(|s| s.as_str()).collect();
        let joined = ordered.join("\n");
        if joined.len() >= 3_000 {
            let capped: String = joined.chars().take(150_000).collect();
            return capped;
        }
    }

    // Fallback 1: first bare anchor-phrase page + window
    for (i, page_text) in text_pages.iter().enumerate() {
        let lower = page_text.to_lowercase();
        if FALLBACK_ANCHORS.iter().any(|a| lower.contains(a)) {
            let end = std::cmp::min(i + notes_window, text_pages.len());
            let result = text_pages[i..end].join("\n");
            if result.len() >= 5_000 {
                let capped: String = result.chars().take(150_000).collect();
                return capped;
            }
            break;
        }
    }

    // Fallback 2: head of full report
    let full = text_pages.join("\n");
    let capped: String = full.chars().take(150_000).collect();
    capped
}

/// Check if text matches bank-specific signatures.
pub fn is_bank_text(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("net interest income")
        || lower.contains("loans and advances to customers")
        || lower.contains("due to customers")
        || lower.contains("interest and similar income")
}

/// Check if text matches insurer-specific signatures.
pub fn is_insurer_text(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("gross written premium")
        || lower.contains("net earned premium")
        || lower.contains("insurance contract liabilities")
        || lower.contains("net claims incurred")
}

/// Detect sector from filing face text. Returns "industrial", "bank", or "insurer".
pub fn detect_sector(text_pages: &[String]) -> &'static str {
    let combined: String = text_pages.iter().take(10).cloned().collect::<Vec<String>>().join("\n");
    if is_insurer_text(&combined) {
        return "insurer";
    }
    if is_bank_text(&combined) {
        return "bank";
    }
    "industrial"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_sector_industrial() {
        let pages = vec![
            "Annual Report 2024\nRevenue 100 200 300\nOperating profit 30 40 50".to_string(),
        ];
        assert_eq!(detect_sector(&pages), "industrial");
    }

    #[test]
    fn test_detect_sector_bank() {
        let pages = vec![
            "Annual Report 2024\nNet interest income 100 110 120\nLoans and advances to customers 1000 1100 1200".to_string(),
        ];
        assert_eq!(detect_sector(&pages), "bank");
    }

    #[test]
    fn test_detect_sector_insurer() {
        let pages = vec![
            "Annual Report 2024\nGross written premium 500 550 600\nInsurance contract liabilities 2000 2100 2200".to_string(),
        ];
        assert_eq!(detect_sector(&pages), "insurer");
    }

    #[test]
    fn test_extract_financial_section_fallback() {
        let pages = vec!["page1 content".to_string(), "page2 content".to_string()];
        let result = extract_financial_section(&pages, 5);
        assert!(result.contains("page1"));
        assert!(result.contains("page2"));
    }
}
