//! The records the store keeps and the manager publishes.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Milliseconds since the Unix epoch.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// A download's identity (a random UUID).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DownloadId(pub String);

impl DownloadId {
    /// A fresh random id.
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }
    /// The id as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for DownloadId {
    /// A fresh random id (clippy's `new_without_default`; no `#[allow]` in this repo).
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for DownloadId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Where a download is in its life. Mirrors the store's CHECK constraint.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DownloadStatus {
    /// Waiting for a free slot.
    Queued,
    /// Asking the server about the file.
    Probing,
    /// Bytes are moving.
    Downloading,
    /// Stopped by the user or at shutdown; resumable.
    Paused,
    /// The file is in place under its final name.
    Completed,
    /// Stopped by an error; see `error_code`.
    Failed,
    /// Stopped by the user; partial data deleted.
    Cancelled,
    /// A finished torrent that is uploading (Plan 5).
    Seeding,
}

impl DownloadStatus {
    /// The stored text.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "QUEUED",
            Self::Probing => "PROBING",
            Self::Downloading => "DOWNLOADING",
            Self::Paused => "PAUSED",
            Self::Completed => "COMPLETED",
            Self::Failed => "FAILED",
            Self::Cancelled => "CANCELLED",
            Self::Seeding => "SEEDING",
        }
    }
    /// Parse the stored text.
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "QUEUED" => Self::Queued,
            "PROBING" => Self::Probing,
            "DOWNLOADING" => Self::Downloading,
            "PAUSED" => Self::Paused,
            "COMPLETED" => Self::Completed,
            "FAILED" => Self::Failed,
            "CANCELLED" => Self::Cancelled,
            "SEEDING" => Self::Seeding,
            _ => return None,
        })
    }
}

/// What kind of transfer a row is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DownloadKind {
    /// HTTP(S) through `mdm-engine`.
    Http,
    /// BitTorrent (Plan 5).
    Torrent,
}

/// An extra request header saved with a download.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Header {
    /// Header name.
    pub name: String,
    /// Header value.
    pub value: String,
}

/// A browser cookie saved with a download.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedCookie {
    /// Cookie name.
    pub name: String,
    /// Cookie value.
    pub value: String,
}

/// A request to add a download.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NewDownload {
    /// http or https URL.
    pub url: String,
    /// Target folder; `None` = the settings' download folder.
    pub dir: Option<PathBuf>,
    /// Override the server's file name.
    pub filename: Option<String>,
    /// The page the link was on.
    pub referrer: Option<String>,
    /// Extra headers.
    pub headers: Vec<Header>,
    /// Browser cookies for the URL.
    pub cookies: Vec<SavedCookie>,
    /// Add as PAUSED instead of QUEUED.
    pub start_paused: bool,
}

/// What a probe learned, as the store keeps it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProbeInfo {
    /// URL after redirects.
    pub final_url: String,
    /// File name actually used (may be `name (1).ext`).
    pub filename: String,
    /// Total size when known.
    pub size: Option<u64>,
    /// ETag.
    pub etag: Option<String>,
    /// Last-Modified.
    pub last_modified: Option<String>,
    /// MIME type.
    pub mime: Option<String>,
}

/// One download as stored and published.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadRow {
    /// Identity.
    pub id: DownloadId,
    /// HTTP or torrent.
    pub kind: DownloadKind,
    /// The URL as added.
    pub url: String,
    /// The URL after redirects, once probed.
    pub final_url: Option<String>,
    /// File name, once known.
    pub filename: Option<String>,
    /// Target folder.
    pub dir: PathBuf,
    /// Total size when known.
    pub size: Option<u64>,
    /// Bytes saved (durable) so far; the size when completed.
    pub downloaded: u64,
    /// Current status.
    pub status: DownloadStatus,
    /// ETag at the first probe.
    pub etag: Option<String>,
    /// Last-Modified at the first probe.
    pub last_modified: Option<String>,
    /// MIME type.
    pub mime: Option<String>,
    /// Referring page.
    pub referrer: Option<String>,
    /// Extra headers.
    pub headers: Vec<Header>,
    /// Cookies.
    pub cookies: Vec<SavedCookie>,
    /// Stable error code when failed.
    pub error_code: Option<String>,
    /// Human-readable error.
    pub error_message: Option<String>,
    /// Unix ms.
    pub created_at: i64,
    /// Unix ms.
    pub updated_at: i64,
    /// Unix ms, when completed.
    pub completed_at: Option<i64>,
}
