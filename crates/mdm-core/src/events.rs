//! What the manager announces; the desktop app forwards these to the UI.

use mdm_engine::SegmentState;
use serde::Serialize;

use crate::model::{DownloadId, DownloadRow};

/// One segment as the UI draws it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SegmentView {
    /// First byte.
    pub start: u64,
    /// Last byte, inclusive.
    pub end: u64,
    /// Bytes written from `start`.
    pub downloaded: u64,
}

impl From<&SegmentState> for SegmentView {
    fn from(s: &SegmentState) -> Self {
        Self {
            start: s.start,
            end: s.end,
            downloaded: s.downloaded,
        }
    }
}

/// Something changed.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ManagerEvent {
    /// A download was added.
    Added {
        /// The new row.
        download: DownloadRow,
    },
    /// A row changed (status, probe result, name).
    Updated {
        /// The row as it is now.
        download: DownloadRow,
    },
    /// Live progress of a running download (every engine update, ~4 per second).
    Progress {
        /// Which download.
        id: DownloadId,
        /// Total size when known.
        total: Option<u64>,
        /// Bytes written.
        downloaded: u64,
        /// Bytes per second.
        speed_bps: u64,
        /// Seconds left.
        eta_secs: Option<u64>,
        /// The segment map.
        segments: Vec<SegmentView>,
    },
    /// A row was removed.
    Removed {
        /// Which download.
        id: DownloadId,
    },
    /// Something the user should read (e.g. "starting over").
    Notice {
        /// Which download.
        id: DownloadId,
        /// Plain English.
        message: String,
    },
}
