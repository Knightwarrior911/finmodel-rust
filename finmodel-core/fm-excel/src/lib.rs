pub mod adhoc;
pub mod derive;
pub mod input;
pub mod is_structure;
pub mod model;
pub mod render;
pub mod sheets;
pub mod snapshot;

use std::fmt;

/// Combined error type for Excel generation and snapshot comparison.
#[derive(Debug)]
pub enum ExcelError {
    /// Wraps an underlying IO error (file open/save).
    Io(std::io::Error),
    /// Wraps a JSON parse error from snapshot loading.
    Json(serde_json::Error),
    /// Wraps a rust_xlsxwriter error.
    Xlsx(rust_xlsxwriter::XlsxError),
    /// Snapshot structural / parse problem (missing keys, bad shape).
    Snapshot(String),
}

impl fmt::Display for ExcelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExcelError::Io(e) => write!(f, "IO error: {e}"),
            ExcelError::Json(e) => write!(f, "JSON error: {e}"),
            ExcelError::Xlsx(e) => write!(f, "Xlsx error: {e}"),
            ExcelError::Snapshot(m) => write!(f, "Snapshot error: {m}"),
        }
    }
}

impl std::error::Error for ExcelError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ExcelError::Io(e) => Some(e),
            ExcelError::Json(e) => Some(e),
            ExcelError::Xlsx(e) => Some(e),
            ExcelError::Snapshot(_) => None,
        }
    }
}

impl From<std::io::Error> for ExcelError {
    fn from(e: std::io::Error) -> Self {
        ExcelError::Io(e)
    }
}

impl From<serde_json::Error> for ExcelError {
    fn from(e: serde_json::Error) -> Self {
        ExcelError::Json(e)
    }
}

impl From<rust_xlsxwriter::XlsxError> for ExcelError {
    fn from(e: rust_xlsxwriter::XlsxError) -> Self {
        ExcelError::Xlsx(e)
    }
}

/// Convenience alias for `Result<T, ExcelError>`.
pub type Result<T> = std::result::Result<T, ExcelError>;
