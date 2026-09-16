use std::path::PathBuf;

use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    Message(String),
    #[error("path is not available: {0}")]
    MissingPath(String),
    #[error("path is outside the selected repository: {0}")]
    OutsideRepository(String),
    #[error("unsafe path or symlink: {0}")]
    UnsafePath(String),
    #[error("report is too large; maximum supported size is {0} bytes")]
    OversizedReport(u64),
    #[error("report JSON is invalid: {0}")]
    InvalidJson(String),
    #[error("unsupported operation: {0}")]
    Unsupported(String),
    #[error("database error: {0}")]
    Database(String),
    #[error("I/O error for {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("process error: {0}")]
    Process(String),
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Database(value.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(value: serde_json::Error) -> Self {
        Self::InvalidJson(value.to_string())
    }
}

impl From<std::io::Error> for AppError {
    fn from(value: std::io::Error) -> Self {
        Self::Io {
            path: PathBuf::new(),
            source: value,
        }
    }
}

pub type AppResult<T> = Result<T, AppError>;
