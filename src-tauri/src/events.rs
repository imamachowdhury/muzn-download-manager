//! Manager events → window events, and the completion notification.

use mdm_core::{DownloadStatus, Manager, ManagerEvent};
use tauri::{AppHandle, Emitter as _, Runtime};
use tauri_plugin_notification::NotificationExt as _;
use tokio::sync::broadcast::error::RecvError;

/// Live progress of a running download (`ManagerEvent::Progress` JSON).
pub const PROGRESS: &str = "download:progress";
/// Every other manager event: added / updated / removed / notice.
pub const STATUS: &str = "download:status";
/// Events were lost (the forwarder lagged): the UI reloads the list.
pub const RESYNC: &str = "download:resync";

/// Which window event carries a manager event.
pub fn event_name(e: &ManagerEvent) -> &'static str {
    match e {
        ManagerEvent::Progress { .. } => PROGRESS,
        _ => STATUS,
    }
}

/// The file name to announce when this event is a download completing.
pub fn completed_name(e: &ManagerEvent) -> Option<&str> {
    match e {
        ManagerEvent::Updated { download } if download.status == DownloadStatus::Completed => Some(
            download
                .filename
                .as_deref()
                .unwrap_or(download.url.as_str()),
        ),
        _ => None,
    }
}

/// Forward every manager event to the window, announce completions, and ask
/// the UI to reload when events were lost.
pub fn spawn_forwarder<R: Runtime>(app: AppHandle<R>, manager: Manager) {
    let mut rx = manager.subscribe();
    tauri::async_runtime::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(e) => {
                    if let Some(name) = completed_name(&e) {
                        if manager.settings().notify_on_complete {
                            notify_complete(&app, name);
                        }
                    }
                    if let Err(err) = app.emit(event_name(&e), &e) {
                        tracing::warn!(error = %err, "sending an event to the window failed");
                    }
                }
                Err(RecvError::Lagged(skipped)) => {
                    tracing::warn!(skipped, "the window fell behind; asking it to reload");
                    let _ = app.emit(RESYNC, ());
                }
                Err(RecvError::Closed) => break,
            }
        }
    });
}

fn notify_complete<R: Runtime>(app: &AppHandle<R>, name: &str) {
    if let Err(e) = app
        .notification()
        .builder()
        .title("Download complete")
        .body(name)
        .show()
    {
        tracing::warn!(error = %e, "showing the completion notification failed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mdm_core::{DownloadId, DownloadKind, DownloadRow};
    use std::path::PathBuf;

    fn row(status: DownloadStatus) -> DownloadRow {
        DownloadRow {
            id: DownloadId("a".into()),
            kind: DownloadKind::Http,
            url: "https://x/y.zip".into(),
            final_url: None,
            filename: Some("y.zip".into()),
            dir: PathBuf::from("/dl"),
            size: Some(3),
            downloaded: 3,
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
    fn progress_and_status_travel_on_their_own_events() {
        let p = ManagerEvent::Progress {
            id: DownloadId("a".into()),
            total: None,
            downloaded: 0,
            speed_bps: 0,
            eta_secs: None,
            segments: vec![],
        };
        assert_eq!(event_name(&p), PROGRESS);
        assert_eq!(
            event_name(&ManagerEvent::Removed {
                id: DownloadId("a".into())
            }),
            STATUS
        );
        assert_eq!(
            event_name(&ManagerEvent::Updated {
                download: row(DownloadStatus::Paused)
            }),
            STATUS
        );
    }

    #[test]
    fn only_a_completion_is_announced() {
        let done = ManagerEvent::Updated {
            download: row(DownloadStatus::Completed),
        };
        assert_eq!(completed_name(&done), Some("y.zip"));
        let added = ManagerEvent::Added {
            download: row(DownloadStatus::Completed),
        };
        assert_eq!(completed_name(&added), None, "added, not completing");
        let paused = ManagerEvent::Updated {
            download: row(DownloadStatus::Paused),
        };
        assert_eq!(completed_name(&paused), None);
    }
}
