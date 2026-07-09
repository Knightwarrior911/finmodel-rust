//! fm-extract: R.2 filing extraction crate
//!
//! Provides stubs for EDGAR XBRL data fetching and LLM extraction prompt templates
//! for structured financial statement extraction.

pub mod edgar;
pub mod extract;

// Re-export the most commonly used items.
pub use edgar::fetch_xbrl;
pub use extract::{ExtractError, ExtractionResult, FetchConfig, EXTRACT_SYSTEM_PROMPT};
