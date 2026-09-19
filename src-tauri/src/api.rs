//! What a command returns on failure, and the pure rules commands rely on.

use std::path::PathBuf;

use mdm_core::{CoreError, DownloadRow, DownloadStatus};
use serde::Serialize;

/// A failed command, as the UI receives it: `{ code, message }`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ApiError {
    /// Stable code (`CoreError::code()`, engine codes pass through).
    pub code: String,
    /// Plain text for the log / detail line.
    pub message: String,
}

/// Result of a command.
pub type ApiResult<T> = Result<T, ApiError>;

impl ApiError {
    /// Any code.
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_owned(),
            message: message.into(),
        }
    }
    /// The action does not fit the download's state.
    pub fn invalid_state(message: impl Into<String>) -> Self {
        Self::new("INVALID_STATE", message)
    }
    /// A failure outside the core (a plugin, the OS).
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new("INTERNAL", message)
    }
}

impl From<CoreError> for ApiError {
    fn from(e: CoreError) -> Self {
        Self::new(e.code(), e.to_string())
    }
}

/// The finished file of a completed download; anything else is INVALID_STATE.
/// The UI passes an id, never a path: this is the only way to a path.
pub fn completed_path(row: &DownloadRow) -> ApiResult<PathBuf> {
    match (row.status, &row.filename) {
        (DownloadStatus::Completed, Some(name)) => Ok(row.dir.join(name)),
        _ => Err(ApiError::invalid_state("the download has not finished")),
    }
}

/// The clipboard text, if it is exactly one http(s) link (the add dialog
/// pre-fills it); anything else is ignored.
pub fn clipboard_link(text: &str) -> Option<String> {
    let t = text.trim();
    if t.is_empty() || t.contains(char::is_whitespace) {
        return None;
    }
    let url = url::Url::parse(t).ok()?;
    matches!(url.scheme(), "http" | "https").then(|| t.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mdm_core::{DownloadId, DownloadKind};

    fn row(status: DownloadStatus, filename: Option<&str>) -> DownloadRow {
        DownloadRow {
            id: DownloadId("a".into()),
            kind: DownloadKind::Http,
            url: "https://x/y.zip".into(),
            final_url: None,
            filename: filename.map(str::to_owned),
            dir: PathBuf::from("/dl"),
            size: None,
            downloaded: 0,
            status,
            etag: None,
            last_modified: None,
            mime: None,
            referrer: None,
            headers: vec![],
            cookies: vec![],
            error_code: None,
            error_message: None,
            created_at: 0,
            updated_at: 0,
            completed_at: None,
        }
    }

    #[test]
    fn only_a_completed_download_has_a_file_to_open() {
        assert_eq!(
            completed_path(&row(DownloadStatus::Completed, Some("y.zip"))).unwrap(),
            PathBuf::from("/dl/y.zip")
        );
        for s in [
            DownloadStatus::Downloading,
            DownloadStatus::Paused,
            DownloadStatus::Failed,
        ] {
            assert_eq!(
                completed_path(&row(s, Some("y.zip"))).unwrap_err().code,
                "INVALID_STATE"
            );
        }
        assert!(completed_path(&row(DownloadStatus::Completed, None)).is_err());
    }

    #[test]
    fn the_clipboard_fills_the_add_dialog_only_with_one_web_link() {
        assert_eq!(
            clipboard_link("  https://x.com/a.zip \n").as_deref(),
            Some("https://x.com/a.zip")
        );
        assert_eq!(clipboard_link("http://x/a").as_deref(), Some("http://x/a"));
        assert_eq!(clipboard_link("ftp://x/a"), None);
        assert_eq!(clipboard_link("hello world"), None);
        assert_eq!(clipboard_link("https://x/a https://x/b"), None);
        assert_eq!(clipboard_link(""), None);
    }

    #[test]
    fn a_core_error_keeps_its_code() {
        let e: ApiError = CoreError::NotFound("abc".into()).into();
        assert_eq!(e.code, "NOT_FOUND");
        assert!(e.message.contains("abc"));
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(
            v,
            serde_json::json!({"code": "NOT_FOUND", "message": e.message})
        );
    }
}
