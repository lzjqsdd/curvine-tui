//! Unified error type for RPC / IO failures surfaced to the UI.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("{0}")]
    Fs(String),

    #[error("{0}")]
    Other(String),
}

impl AppError {
    /// Convert any displayable curvine error into an app error.
    pub fn from_fs(e: impl std::fmt::Display) -> Self {
        AppError::Fs(e.to_string())
    }

    #[allow(dead_code)]
    pub fn other(msg: impl Into<String>) -> Self {
        AppError::Other(msg.into())
    }
}

impl From<curvine_error::FsError> for AppError {
    fn from(e: curvine_error::FsError) -> Self {
        AppError::Fs(e.to_string())
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::Other(e.to_string())
    }
}

impl From<anyhow::Error> for AppError {
    fn from(e: anyhow::Error) -> Self {
        AppError::Other(format!("{e:#}"))
    }
}
