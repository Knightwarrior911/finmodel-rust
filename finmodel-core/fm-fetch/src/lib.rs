//! fm-fetch: R.2 filing fetch crate.
//!
//! EDGAR XBRL data pull, CIK lookup, and PDF download utilities.
//! Provides the network layer for the extraction pipeline.
pub mod discovery;
pub mod edgar;
pub mod jurisdiction;
pub mod market;
pub mod news;
pub mod pdf;
pub mod websearch;

pub use discovery::{find_annual_report_pdf_url, DiscoveryError};
pub use edgar::{
    cik_from_ticker, fetch_companyfacts, fetch_company_sic, recent_filings, search_filings,
    CompanyFacts, Filing, SicInfo, DEFAULT_FORM_TYPES,
};
pub use market::{fetch_quote, fetch_fx_rate, normalize_minor_unit, FetchError, Quote};
pub use pdf::{download_pdf, DownloadConfig};
pub use news::{fetch_headlines, parse_rss, Headline};
pub use websearch::{classify_status, fetch_page, parse_ddg_hits, FetchedPage, PageStatus, WebHit};
pub use jurisdiction::{
    detect_jurisdiction, has_annual_keyword, is_blocked_domain, regulator_candidates,
    regulator_site, Jurisdiction, ANNUAL_KEYWORDS, BLOCKED_DOMAINS, JURISDICTION_PATTERNS,
    REGULATOR_SITES,
};
