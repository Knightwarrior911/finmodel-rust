use serde::Serialize;

/// Command failures serialize to `{ "kind": "...", "message": "..." }` for the
/// frontend to render.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    Io(String),
    #[error("{0}")]
    Engine(String),
    #[error("{0}")]
    Config(String),
}

impl Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let kind = match self {
            AppError::Io(_) => "io",
            AppError::Engine(_) => "engine",
            AppError::Config(_) => "config",
        };
        let mut st = s.serialize_struct("AppError", 2)?;
        st.serialize_field("kind", kind)?;
        st.serialize_field("message", &self.to_string())?;
        st.end()
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        Self::Engine(e.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;
