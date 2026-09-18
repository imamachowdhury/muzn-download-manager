//! Segmented HTTP download engine for Muzn Download Manager.
//!
//! No Tauri, no database: callers get a [`DownloadHandle`] and a stream of
//! [`Progress`]; persistence is theirs.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

/// Engine version, for User-Agent strings.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod engine;
pub mod error;
pub mod file;
pub mod filename;
pub mod plan;
pub mod probe;
pub mod request;
pub mod segment;
pub use engine::{Engine, EngineConfig, Proxy};
pub use error::EngineError;
pub use file::{PartFile, PART_SUFFIX};
pub use plan::{plan_segments, SegmentState, MAX_CONNECTIONS, MIN_SEGMENT_BYTES};
pub use probe::Probe;
pub use request::{Cookie, RequestExtras};

#[cfg(test)]
mod tests {
    #[test]
    fn version_is_set() {
        assert!(!super::VERSION.is_empty());
    }
}
