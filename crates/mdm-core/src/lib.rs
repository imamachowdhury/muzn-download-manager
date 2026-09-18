//! The download core of Muzn Download Manager: a SQLite store and a manager
//! that queues, drives and persists downloads through `mdm-engine`.
//! No Tauri and no UI here — the desktop app wraps this crate.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod error;
pub mod events;
pub mod manager;
pub mod model;
pub mod settings;
pub mod store;

pub use error::{CoreError, Result};
pub use events::{ManagerEvent, SegmentView};
pub use manager::{Manager, PERSIST_INTERVAL};
pub use model::*;
pub use settings::{ProxySetting, Settings};
pub use store::Store;
