//! fm-extract: R.2 filing extraction crate
//!
//! EDGAR XBRL fetching, LLM extraction prompts, PDF text extraction
//! and financial section detection for structured financial statement extraction.

pub mod date;
pub mod edgar;
pub mod extract;
pub mod llm;
pub mod ltm;
pub mod period;
pub mod report;
pub mod section;
pub mod xbrl;

// Re-export the most commonly used items.
pub use edgar::{fetch_xbrl, fetch_xbrl_with_provenance, fetch_xbrl_bundle, fetch_ltm, fetch_non_us_filing};
pub use date::{current_year, today_iso};
pub use ltm::{extract_ltm, LtmData};
pub use period::{extract_period, PeriodBasis, PeriodData};
pub use report::{
    extract_financials, extract_amount, filing_type, is_valid_filing, normalize_report_text,
    ExtractedFinancials,
};
pub use extract::{
    ExtractError, ExtractionResult, FetchConfig,
    EXTRACT_SYSTEM_PROMPT, FINANCIALS_SYSTEM_PROMPT,
    BANK_SYSTEM_PROMPT, INSURER_SYSTEM_PROMPT, NOTES_SYSTEM_PROMPT,
    system_prompt_for_sector,
    placeholder_result, load_cache, save_extraction_cache,
    extract_financials_from_pdf, extract_pdf_text,
};
pub use llm::{llm_complete, llm_complete_with, LlmConfig, list_openrouter_models, LlmError, OpenRouterModel, OpenRouterPricing};
pub use section::{extract_financial_section, detect_sector};
pub use xbrl::{xbrl_tag_map, parse_xbrl_to_raw, parse_xbrl_to_raw_with_provenance, ParsedXbrlData, XbrlParseError};
