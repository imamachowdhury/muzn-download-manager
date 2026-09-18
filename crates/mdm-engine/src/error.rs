//! One error type for the whole engine, with a stable code the UI can map.

/// Everything that can stop a download.
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    /// The URL could not be parsed or has an unsupported scheme.
    #[error("invalid URL: {0}")]
    InvalidUrl(String),
    /// Resume was asked for but the server no longer answers ranges.
    #[error("server does not support byte ranges")]
    RangeNotSupported,
    /// ETag / Last-Modified / size differ from the original probe.
    #[error("the file on the server changed since the download started")]
    SourceChanged,
    /// ENOSPC while pre-allocating or writing.
    #[error("not enough disk space")]
    DiskFull,
    /// A non-success HTTP status.
    #[error("HTTP {status}")]
    HttpStatus {
        /// The status code.
        status: u16,
    },
    /// Connect / read / reset / timeout.
    #[error("network error: {0}")]
    Network(String),
    /// Certificate or handshake failure — never retried.
    #[error("TLS error: {0}")]
    Tls(String),
    /// The caller cancelled.
    #[error("cancelled")]
    Cancelled,
    /// Any other I/O failure.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(windows)]
pub(crate) const ENOSPC_CODE: i32 = 112; // ERROR_DISK_FULL
#[cfg(not(windows))]
pub(crate) const ENOSPC_CODE: i32 = 28; // ENOSPC

impl EngineError {
    /// Stable identifier for UI mapping and logs.
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidUrl(_) => "INVALID_URL",
            Self::RangeNotSupported => "RANGE_NOT_SUPPORTED",
            Self::SourceChanged => "SOURCE_CHANGED",
            Self::DiskFull => "DISK_FULL",
            Self::HttpStatus { .. } => "HTTP_STATUS",
            Self::Network(_) => "NETWORK",
            Self::Tls(_) => "TLS",
            Self::Cancelled => "CANCELLED",
            Self::Io(_) => "IO",
        }
    }

    /// Worth retrying with backoff?
    pub fn is_transient(&self) -> bool {
        match self {
            Self::Network(_) => true,
            Self::HttpStatus { status } => *status == 429 || (500..=599).contains(status),
            _ => false,
        }
    }

    /// Map an I/O error, turning a full disk into [`EngineError::DiskFull`].
    pub fn from_io(e: std::io::Error) -> Self {
        if e.raw_os_error() == Some(ENOSPC_CODE) {
            Self::DiskFull
        } else {
            Self::Io(e)
        }
    }
}

impl From<reqwest::Error> for EngineError {
    fn from(e: reqwest::Error) -> Self {
        if e.is_builder() {
            return Self::InvalidUrl(e.to_string());
        }
        if let Some(status) = e.status() {
            return Self::HttpStatus {
                status: status.as_u16(),
            };
        }
        let text = e.to_string();
        // reqwest exposes no is_tls(); rustls errors carry these words.
        if text.contains("certificate") || text.contains("tls") || text.contains("TLS") {
            return Self::Tls(text);
        }
        Self::Network(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_stable_strings() {
        assert_eq!(EngineError::RangeNotSupported.code(), "RANGE_NOT_SUPPORTED");
        assert_eq!(EngineError::SourceChanged.code(), "SOURCE_CHANGED");
        assert_eq!(EngineError::DiskFull.code(), "DISK_FULL");
        assert_eq!(
            EngineError::HttpStatus { status: 404 }.code(),
            "HTTP_STATUS"
        );
        assert_eq!(EngineError::Network("x".into()).code(), "NETWORK");
        assert_eq!(EngineError::Cancelled.code(), "CANCELLED");
    }

    #[test]
    fn transient_vs_permanent() {
        assert!(EngineError::Network("reset".into()).is_transient());
        assert!(EngineError::HttpStatus { status: 503 }.is_transient());
        assert!(EngineError::HttpStatus { status: 429 }.is_transient());
        assert!(!EngineError::HttpStatus { status: 404 }.is_transient());
        assert!(!EngineError::HttpStatus { status: 403 }.is_transient());
        assert!(!EngineError::Tls("bad cert".into()).is_transient());
        assert!(!EngineError::DiskFull.is_transient());
        assert!(!EngineError::Cancelled.is_transient());
    }

    #[test]
    fn enospc_maps_to_disk_full() {
        let e = std::io::Error::from_raw_os_error(ENOSPC_CODE);
        assert_eq!(EngineError::from_io(e).code(), "DISK_FULL");
    }
}
