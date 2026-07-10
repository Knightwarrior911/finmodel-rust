//! fm-fetch: R.2 filing fetch crate.
//!
//! EDGAR XBRL data pull, CIK lookup, and PDF download utilities.
//! Provides the network layer for the extraction pipeline.
pub mod discovery;
pub mod edgar;
pub mod pdf;

pub use discovery::{find_annual_report_pdf_url, DiscoveryError};
pub use edgar::{cik_from_ticker, fetch_companyfacts, CompanyFacts};
pub use pdf::{download_pdf, DownloadConfig};
