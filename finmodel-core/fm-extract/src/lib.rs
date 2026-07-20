//! fm-extract: R.2 filing extraction crate
//!
//! EDGAR XBRL fetching, LLM extraction prompts, PDF text extraction
//! and financial section detection for structured financial statement extraction.

pub mod date;
pub mod edgar;
pub mod extract;

/// Panic-safe per-page PDF text extraction (data room reviews, corpus
/// tools). One malformed PDF returns Err instead of crashing the process.
pub fn pdf_pages(path: &str) -> Result<Vec<String>, String> {
    extract::extract_pdf_pages(path).map_err(|e| e.to_string())
}
pub mod llm;
pub mod ltm;
pub mod period;
pub mod report;
pub mod section;
pub mod xbrl;

// Re-export the most commonly used items.
pub use date::{current_year, today_iso};
pub use edgar::{
    fetch_ltm, fetch_non_us_filing, fetch_xbrl, fetch_xbrl_bundle, fetch_xbrl_with_provenance,
};
pub use extract::{
    BANK_SYSTEM_PROMPT, EXTRACT_SYSTEM_PROMPT, ExtractError, ExtractionResult,
    FINANCIALS_SYSTEM_PROMPT, FetchConfig, INSURER_SYSTEM_PROMPT, NOTES_SYSTEM_PROMPT,
    extract_financials_from_pdf, extract_pdf_text, load_cache, placeholder_result,
    save_extraction_cache, system_prompt_for_sector,
};
pub use llm::{
    LlmConfig, LlmError, OpenRouterModel, OpenRouterPricing, list_openrouter_models, llm_complete,
    llm_complete_with,
};
pub use ltm::{LtmData, extract_ltm};
pub use period::{PeriodBasis, PeriodData, extract_period};
pub use report::{
    ExtractedFinancials, extract_amount, extract_financials, filing_type, is_valid_filing,
    normalize_report_text,
};
pub use section::{detect_sector, extract_financial_section};
pub use xbrl::{
    ParsedXbrlData, XbrlParseError, parse_xbrl_to_raw, parse_xbrl_to_raw_with_provenance,
    xbrl_tag_map,
};
