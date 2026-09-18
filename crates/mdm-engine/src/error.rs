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
    /// The saved state does not describe this file (segments that do not
    /// cover it, or a part file of the wrong length); start over.
    #[error("invalid resume state: {0}")]
    InvalidResume(String),
    /// Any other I/O failure.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// An invariant inside the engine broke (a worker panicked, a file handle
    /// leaked). Not retryable; report it.
    #[error("internal error: {0}")]
    Internal(String),
}

#[cfg(windows)]
pub(crate) const ENOSPC_CODE: i32 = 112; // ERROR_DISK_FULL
#[cfg(not(windows))]
pub(crate) const ENOSPC_CODE: i32 = 28; // ENOSPC

/// Walk an error's `source()` chain, deepest cause last.
fn source_messages(e: &(dyn std::error::Error + 'static)) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = e.source();
    while let Some(s) = cur {
        out.push(s.to_string());
        cur = s.source();
    }
    out
}

/// True when any cause in the chain is a TLS failure. The top-level text is
/// deliberately NOT inspected: it carries the URL, and a host named
/// "tls.example.com" must not turn a network blip into a permanent error.
fn is_tls_chain(sources: &[String]) -> bool {
    sources.iter().any(|m| {
        let l = m.to_ascii_lowercase();
        l.contains("certificate")
            || l.contains("tls")
            || l.contains("handshake")
            || l.contains("alert")
    })
}

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
            Self::InvalidResume(_) => "INVALID_RESUME",
            Self::Io(_) => "IO",
            Self::Internal(_) => "INTERNAL",
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
        let sources = source_messages(&e);
        let text = match sources.last() {
            Some(cause) => format!("{e}: {cause}"),
            None => e.to_string(),
        };
        if is_tls_chain(&sources) {
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
        assert_eq!(EngineError::InvalidUrl("x".into()).code(), "INVALID_URL");
        assert_eq!(EngineError::Io(std::io::Error::other("x")).code(), "IO");
        assert_eq!(EngineError::Tls("x".into()).code(), "TLS");
        assert_eq!(
            EngineError::InvalidResume("x".into()).code(),
            "INVALID_RESUME"
        );
        assert!(!EngineError::InvalidResume("x".into()).is_transient());
        assert_eq!(EngineError::Internal("x".into()).code(), "INTERNAL");
        assert!(!EngineError::Internal("x".into()).is_transient());
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

    #[test]
    fn tls_is_detected_from_the_source_chain_not_the_top_level_text() {
        // Review finding 2026-09-18: reqwest's Display never carries the TLS cause.
        assert!(is_tls_chain(&[
            "client error (Connect)".into(),
            "invalid peer certificate: Expired".into()
        ]));
        assert!(is_tls_chain(&[
            "received fatal alert: HandshakeFailure".into()
        ]));
        assert!(!is_tls_chain(&["connection reset by peer".into()]));
        assert!(
            !is_tls_chain(&[]),
            "a URL containing 'tls' is never inspected"
        );
    }

    #[test]
    fn source_messages_walks_the_chain_deepest_last() {
        use std::error::Error;
        use std::fmt;

        #[derive(Debug)]
        struct Inner;
        impl fmt::Display for Inner {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "deep")
            }
        }
        impl Error for Inner {}

        #[derive(Debug)]
        struct Outer(Inner);
        impl fmt::Display for Outer {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "outer")
            }
        }
        impl Error for Outer {
            fn source(&self) -> Option<&(dyn Error + 'static)> {
                Some(&self.0)
            }
        }

        let msgs = source_messages(&Outer(Inner));
        assert_eq!(msgs, vec!["deep".to_string()]);
    }
}
