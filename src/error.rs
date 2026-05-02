use std::io;

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("URL parse error: {0}")]
    Url(#[from] url::ParseError),

    #[error("DNS resolution failed: {0}")]
    Dns(String),

    #[error("All mirrors exhausted for: {0}")]
    AllMirrorsExhausted(String),

    #[error("Circuit breaker open for: {0}")]
    CircuitBreakerOpen(String),

    #[error("File size mismatch: expected {expected}, got {actual}")]
    SizeMismatch { expected: u64, actual: u64 },

    #[error("Checksum mismatch for: {0}")]
    ChecksumMismatch(String),

    #[error("Server does not support range requests")]
    RangeNotSupported,

    #[error("Download aborted by user")]
    Aborted,

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, DownloadError>;
