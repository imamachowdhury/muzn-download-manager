use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::Duration;

use mdm_core::*;
use mdm_test_server::*;

pub fn manager(dir: &Path) -> Manager {
    let m = Manager::with_store(Store::open_in_memory().unwrap(), dir).unwrap();
    m.set_settings(Settings {
        max_connections: 4,
        ..m.settings()
    })
    .unwrap();
    m
}

pub fn add(m: &Manager, url: &str) -> DownloadId {
    m.add(NewDownload {
        url: url.into(),
        ..Default::default()
    })
    .unwrap()
    .id
}

pub async fn wait_for(m: &Manager, id: &DownloadId, want: DownloadStatus) -> DownloadRow {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        let row = m.get(id).unwrap().unwrap();
        if row.status == want {
            return row;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for {want:?}; last row: {row:?}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn a_download_is_driven_to_completion() {
    let s = TestServer::start(4 * 1024 * 1024).await;
    let d = tempfile::tempdir().unwrap();
    let m = manager(d.path());
    let id = add(&m, &s.file_url());
    let row = wait_for(&m, &id, DownloadStatus::Completed).await;
    assert_eq!(row.filename.as_deref(), Some("file"));
    assert_eq!(
        (row.size, row.downloaded),
        (Some(4 * 1024 * 1024), 4 * 1024 * 1024)
    );
    assert!(row.completed_at.is_some());
    assert_eq!(sha256_file(&d.path().join("file")), sha256_bytes(&s.data));
    assert!(m.segments(&id).unwrap().is_empty());
}

#[tokio::test]
async fn events_announce_the_life_of_a_download() {
    let s = TestServer::start(2 * 1024 * 1024).await;
    s.cfg.chunk_delay_ms.store(2, Ordering::SeqCst);
    let d = tempfile::tempdir().unwrap();
    let m = manager(d.path());
    let mut rx = m.subscribe();
    let id = add(&m, &s.file_url());
    let mut kinds = Vec::new();
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            match rx.recv().await.unwrap() {
                ManagerEvent::Added { download } if download.id == id => {
                    kinds.push("added".to_string())
                }
                ManagerEvent::Updated { download } if download.id == id => {
                    kinds.push(format!("{:?}", download.status));
                    if download.status == DownloadStatus::Completed {
                        break;
                    }
                }
                ManagerEvent::Progress { id: pid, .. } if pid == id => {
                    kinds.push("progress".into())
                }
                _ => {}
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(kinds.first().map(String::as_str), Some("added"));
    for want in ["Probing", "Downloading", "progress", "Completed"] {
        assert!(
            kinds.iter().any(|k| k == want),
            "missing {want} in {kinds:?}"
        );
    }
}

#[tokio::test]
async fn the_queue_runs_at_most_max_parallel_in_fifo_order() {
    let s = TestServer::start(1024 * 1024).await;
    s.cfg.chunk_delay_ms.store(5, Ordering::SeqCst);
    let d = tempfile::tempdir().unwrap();
    let m = manager(d.path());
    m.set_settings(Settings {
        max_parallel: 1,
        ..m.settings()
    })
    .unwrap();
    let ids: Vec<_> = (0..3).map(|_| add(&m, &s.file_url())).collect();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    loop {
        let rows = m.list().unwrap();
        let active = rows
            .iter()
            .filter(|r| {
                matches!(
                    r.status,
                    DownloadStatus::Probing | DownloadStatus::Downloading
                )
            })
            .count();
        assert!(active <= 1, "{active} running with max_parallel 1");
        if rows.iter().all(|r| r.status == DownloadStatus::Completed) {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "queue did not finish"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let done: Vec<i64> = ids
        .iter()
        .map(|id| m.get(id).unwrap().unwrap().completed_at.unwrap())
        .collect();
    assert!(
        done.windows(2).all(|w| w[0] <= w[1]),
        "completed in add order: {done:?}"
    );
    let names: Vec<String> = ids
        .iter()
        .map(|id| m.get(id).unwrap().unwrap().filename.unwrap())
        .collect();
    assert_eq!(names, vec!["file", "file (1)", "file (2)"]);
}

#[tokio::test]
async fn a_failed_download_records_the_engine_code() {
    let s = TestServer::start(10).await;
    let d = tempfile::tempdir().unwrap();
    let m = manager(d.path());
    let id = add(&m, &s.status_url(404));
    let row = wait_for(&m, &id, DownloadStatus::Failed).await;
    assert_eq!(row.error_code.as_deref(), Some("HTTP_STATUS"));
}

#[tokio::test]
async fn only_http_urls_are_accepted() {
    let d = tempfile::tempdir().unwrap();
    let m = manager(d.path());
    let e = m
        .add(NewDownload {
            url: "ftp://example.com/x".into(),
            ..Default::default()
        })
        .unwrap_err();
    assert_eq!(e.code(), "INVALID_URL");
    let e = m
        .add(NewDownload {
            url: "not a url".into(),
            ..Default::default()
        })
        .unwrap_err();
    assert_eq!(e.code(), "INVALID_URL");
}

#[tokio::test]
async fn a_stream_of_unknown_size_records_its_size_on_completion() {
    let s = TestServer::start(50_000).await;
    s.cfg.chunked.store(true, Ordering::SeqCst);
    let d = tempfile::tempdir().unwrap();
    let m = manager(d.path());
    let id = add(&m, &s.file_url());
    let row = wait_for(&m, &id, DownloadStatus::Completed).await;
    assert_eq!((row.size, row.downloaded), (Some(50_000), 50_000));
}

#[tokio::test]
async fn a_new_download_never_deletes_a_paused_ones_part_file() {
    // Task 5 review ruling: a paused row's part file is not in the engine's
    // live registry, so a fresh start of the same name used to delete it.
    let s = TestServer::start(1024 * 1024).await;
    let d = tempfile::tempdir().unwrap();
    let store = Store::open_in_memory().unwrap();
    let paused = DownloadId::new();
    store
        .insert(
            &paused,
            d.path(),
            &NewDownload {
                url: s.file_url(),
                ..Default::default()
            },
            DownloadStatus::Paused,
            now_ms(),
        )
        .unwrap();
    store
        .set_probe(
            &paused,
            &ProbeInfo {
                final_url: s.file_url(),
                filename: "file".into(),
                size: Some(1024 * 1024),
                etag: None,
                last_modified: None,
                mime: None,
            },
            now_ms(),
        )
        .unwrap();
    let part = d.path().join("file.mdm.part");
    std::fs::write(&part, b"the paused download's bytes").unwrap();

    let m = Manager::with_store(store, d.path()).unwrap();
    let id = add(&m, &s.file_url());
    let row = wait_for(&m, &id, DownloadStatus::Completed).await;
    assert_eq!(row.filename.as_deref(), Some("file (1)"));
    assert_eq!(
        sha256_file(&d.path().join("file (1)")),
        sha256_bytes(&s.data)
    );
    assert_eq!(
        std::fs::read(&part).unwrap(),
        b"the paused download's bytes"
    );
    assert_eq!(
        m.get(&paused).unwrap().unwrap().status,
        DownloadStatus::Paused
    );
}
