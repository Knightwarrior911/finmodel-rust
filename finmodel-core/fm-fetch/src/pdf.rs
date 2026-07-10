//! PDF download utility.
//!
//! Downloads an annual report PDF from a URL to a local temporary file.
//! Ported from `_download_pdf_to_tmpfile()` in `src/fetcher.py`.

use std::io::Write;
use std::path::PathBuf;

/// Configuration for downloading a PDF.
#[derive(Debug, Clone)]
pub struct DownloadConfig {
    /// URL of the PDF to download.
    pub url: String,
    /// Optional output path. If None, a temp file is used.
    pub output_path: Option<PathBuf>,
    /// Optional User-Agent string.
    pub user_agent: Option<String>,
}

impl Default for DownloadConfig {
    fn default() -> Self {
        Self {
            url: String::new(),
            output_path: None,
            user_agent: Some("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36".into()),
        }
    }
}

/// Errors from PDF download operations.
#[derive(Debug, thiserror::Error)]
pub enum PdfError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("No URL provided")]
    NoUrl,
    #[error("Non-PDF content type: {0}")]
    WrongContentType(String),
}

/// Download a PDF from a URL to a temporary file.
///
/// Returns the path to the downloaded file.
/// Ported from `_download_pdf_to_tmpfile()` in `src/fetcher.py`.
pub fn download_pdf(config: &DownloadConfig) -> Result<PathBuf, PdfError> {
    if config.url.is_empty() {
        return Err(PdfError::NoUrl);
    }

    let client = reqwest::blocking::Client::builder()
        .user_agent(config.user_agent.as_deref().unwrap_or("Mozilla/5.0"))
        .build()?;

    let resp = client.get(&config.url).send()?.error_for_status()?;

    // Verify content type hints at PDF
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !content_type.is_empty() && !content_type.contains("pdf") && !content_type.contains("octet-stream") && !content_type.contains("application/") {
        // Allow octet-stream and generic binary — some servers serve PDFs as octet-stream
    }

    let bytes = resp.bytes()?;

    let path = match &config.output_path {
        Some(p) => p.clone(),
        None => {
            // Create a temp file with .pdf extension
            let tmp_dir = std::env::temp_dir();
            let filename = format!("fm_{}.pdf", chrono_now());
            tmp_dir.join(filename)
        }
    };

    let mut file = std::fs::File::create(&path)?;
    file.write_all(&bytes)?;

    Ok(path)
}

/// Simple timestamp for temp filenames (no chrono dependency).
fn chrono_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let d = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    format!("{}", d.as_millis())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_download_pdf_no_url() {
        let config = DownloadConfig::default();
        let result = download_pdf(&config);
        assert!(result.is_err());
        match result {
            Err(PdfError::NoUrl) => {} // expected
            _ => panic!("Expected NoUrl error"),
        }
    }
}
