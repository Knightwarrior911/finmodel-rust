//! fm-fetch: R.2 filing fetch crate.
//!
//! EDGAR XBRL data pull, CIK lookup, and PDF download utilities.
//! Provides the network layer for the extraction pipeline.
pub mod cache;

pub mod discovery;
pub mod edgar;
pub mod jurisdiction;
pub mod market;
pub mod news;
pub mod pdf;
pub mod retry;
pub mod segments;
pub mod websearch;

pub use discovery::{DiscoveryError, find_annual_report_pdf_url};
pub use edgar::{
    CompanyFacts, DEFAULT_FORM_TYPES, Filing, SicInfo, cik_from_ticker, fetch_company_sic,
    fetch_companyfacts, fetch_filing_doc, recent_filings, search_filings, split_filing_items,
};
pub use jurisdiction::{
    ANNUAL_KEYWORDS, BLOCKED_DOMAINS, JURISDICTION_PATTERNS, Jurisdiction, REGULATOR_SITES,
    detect_jurisdiction, has_annual_keyword, is_blocked_domain, regulator_candidates,
    regulator_site,
};
pub use market::{FetchError, Quote, fetch_fx_rate, fetch_quote, normalize_minor_unit};
pub use news::{Headline, fetch_headlines, parse_rss};
pub use pdf::{DownloadConfig, download_pdf};
pub use websearch::{FetchedPage, PageStatus, WebHit, classify_status, fetch_page, parse_ddg_hits};
