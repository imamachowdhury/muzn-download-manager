//! Segmented HTTP download engine for Muzn Download Manager.
//!
//! No Tauri, no database: callers get a [`DownloadHandle`] and a stream of
//! [`Progress`]; persistence is theirs.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

/// Engine version, for User-Agent strings.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    #[test]
    fn version_is_set() {
        assert!(!super::VERSION.is_empty());
    }
}
