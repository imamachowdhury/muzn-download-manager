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

/// Wait for a Progress event of `id` with at least `min` bytes.
async fn wait_bytes(
    rx: &mut tokio::sync::broadcast::Receiver<ManagerEvent>,
    id: &DownloadId,
    min: u64,
) {
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            if let Ok(ManagerEvent::Progress {
                id: pid,
                downloaded,
                ..
            }) = rx.recv().await
            {
                if &pid == id && downloaded >= min {
                    return;
                }
            }
        }
    })
    .await
    .expect("progress arrived");
}

/// An 8 MiB download parked at 4 000 bytes (four connections, four hung bodies).
async fn parked(s: &TestServer, m: &Manager) -> DownloadId {
    s.cfg.hang_first.store(4, Ordering::SeqCst);
    let mut rx = m.subscribe();
    let id = add(m, &s.file_url());
    wait_bytes(&mut rx, &id, 4000).await;
    id
}

#[tokio::test]
async fn pause_then_resume_completes_with_the_right_bytes() {
    let s = TestServer::start(8 * 1024 * 1024).await;
    let d = tempfile::tempdir().unwrap();
    let m = manager(d.path());
    let id = parked(&s, &m).await;
    m.pause(&id).unwrap();
    wait_for(&m, &id, DownloadStatus::Paused).await;
    let saved: u64 = m.segments(&id).unwrap().iter().map(|x| x.downloaded).sum();
    assert!(saved >= 4000, "the paused state was saved: {saved}");
    assert!(d.path().join("file.mdm.part").exists());
    s.cfg.hang_first.store(0, Ordering::SeqCst);
    m.resume(&id).unwrap();
    wait_for(&m, &id, DownloadStatus::Completed).await;
    assert_eq!(sha256_file(&d.path().join("file")), sha256_bytes(&s.data));
}

#[tokio::test]
async fn a_queued_download_pauses_without_starting() {
    let s = TestServer::start(1024).await;
    let d = tempfile::tempdir().unwrap();
    let m = manager(d.path());
    let id = m
        .add(NewDownload {
            url: s.file_url(),
            start_paused: true,
            ..Default::default()
        })
        .unwrap()
        .id;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(m.get(&id).unwrap().unwrap().status, DownloadStatus::Paused);
    assert_eq!(s.cfg.requests.load(Ordering::SeqCst), 0);
    m.resume(&id).unwrap();
    wait_for(&m, &id, DownloadStatus::Completed).await;
}

#[tokio::test]
async fn cancel_deletes_the_partial_data() {
    let s = TestServer::start(8 * 1024 * 1024).await;
    let d = tempfile::tempdir().unwrap();
    let m = manager(d.path());
    let id = parked(&s, &m).await;
    m.cancel(&id).unwrap();
    wait_for(&m, &id, DownloadStatus::Cancelled).await;
    assert!(!d.path().join("file.mdm.part").exists());
    assert!(m.segments(&id).unwrap().is_empty());
}

#[tokio::test]
async fn remove_while_running_deletes_the_row_and_the_part_file() {
    let s = TestServer::start(8 * 1024 * 1024).await;
    let d = tempfile::tempdir().unwrap();
    let m = manager(d.path());
    let id = parked(&s, &m).await;
    let mut rx = m.subscribe();
    m.remove(&id, false).unwrap();
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            if let Ok(ManagerEvent::Removed { id: rid }) = rx.recv().await {
                if rid == id {
                    return;
                }
            }
        }
    })
    .await
    .unwrap();
    assert!(m.get(&id).unwrap().is_none());
    assert!(!d.path().join("file.mdm.part").exists());
}

#[tokio::test]
async fn remove_a_completed_download_keeps_or_deletes_the_file_on_request() {
    let s = TestServer::start(1024).await;
    let d = tempfile::tempdir().unwrap();
    let m = manager(d.path());
    let keep = add(&m, &s.file_url());
    wait_for(&m, &keep, DownloadStatus::Completed).await;
    m.remove(&keep, false).unwrap();
    assert!(d.path().join("file").exists());
    let gone = add(&m, &s.file_url());
    let row = wait_for(&m, &gone, DownloadStatus::Completed).await;
    let name = row.filename.unwrap();
    m.remove(&gone, true).unwrap();
    assert!(!d.path().join(name).exists());
    assert!(m.get(&gone).unwrap().is_none());
}

#[tokio::test]
async fn restart_starts_a_failed_download_from_zero() {
    let s = TestServer::start(1024 * 1024).await;
    let d = tempfile::tempdir().unwrap();
    let m = manager(d.path());
    // A probe-level failure is immediate (the probe does not retry): HEAD is
    // refused and the GET bytes=0-0 fallback answers 503. Segment-level 503s
    // would retry ten times with the default 1 s base delay — minutes.
    s.cfg.head_allowed.store(false, Ordering::SeqCst);
    s.cfg.fail_first.store(1000, Ordering::SeqCst);
    let id = add(&m, &s.file_url());
    let row = wait_for(&m, &id, DownloadStatus::Failed).await;
    assert_eq!(row.error_code.as_deref(), Some("HTTP_STATUS"));
    s.cfg.head_allowed.store(true, Ordering::SeqCst);
    s.cfg.fail_first.store(0, Ordering::SeqCst);
    m.restart(&id).unwrap();
    wait_for(&m, &id, DownloadStatus::Completed).await;
    assert_eq!(sha256_file(&d.path().join("file")), sha256_bytes(&s.data));
    assert_eq!(m.restart(&id).unwrap_err().code(), "INVALID_STATE");
}

#[tokio::test]
async fn shutdown_saves_running_downloads_and_queues_them_for_next_launch() {
    let s = TestServer::start(8 * 1024 * 1024).await;
    let d = tempfile::tempdir().unwrap();
    let m = manager(d.path());
    let id = parked(&s, &m).await;
    tokio::time::timeout(Duration::from_secs(20), m.shutdown())
        .await
        .unwrap();
    let row = m.get(&id).unwrap().unwrap();
    assert_eq!(row.status, DownloadStatus::Queued);
    assert!(row.downloaded >= 4000);
}

#[tokio::test]
async fn unknown_ids_are_not_found() {
    let d = tempfile::tempdir().unwrap();
    let m = manager(d.path());
    let x = DownloadId::new();
    for e in [
        m.pause(&x),
        m.resume(&x),
        m.cancel(&x),
        m.remove(&x, false),
        m.restart(&x),
    ] {
        assert_eq!(e.unwrap_err().code(), "NOT_FOUND");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn remove_then_shutdown_still_removes() {
    // Review finding 2026-09-19: a later signal overwrote an earlier one.
    let s = TestServer::start(8 * 1024 * 1024).await;
    let d = tempfile::tempdir().unwrap();
    let m = manager(d.path());
    let id = parked(&s, &m).await;
    m.remove(&id, false).unwrap();
    tokio::time::timeout(Duration::from_secs(20), m.shutdown())
        .await
        .unwrap();
    assert!(m.get(&id).unwrap().is_none());
    assert!(!d.path().join("file.mdm.part").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancel_then_pause_all_stays_cancelled() {
    let s = TestServer::start(8 * 1024 * 1024).await;
    let d = tempfile::tempdir().unwrap();
    let m = manager(d.path());
    let id = parked(&s, &m).await;
    m.cancel(&id).unwrap();
    m.pause_all().unwrap();
    wait_for(&m, &id, DownloadStatus::Cancelled).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        m.get(&id).unwrap().unwrap().status,
        DownloadStatus::Cancelled
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pausing_a_queued_download_keeps_it_from_starting() {
    let s = TestServer::start(2 * 1024 * 1024).await;
    s.cfg.chunk_delay_ms.store(5, Ordering::SeqCst);
    let d = tempfile::tempdir().unwrap();
    let m = manager(d.path());
    m.set_settings(Settings {
        max_parallel: 1,
        ..m.settings()
    })
    .unwrap();
    let first = add(&m, &s.file_url());
    let second = add(&m, &s.file_url());
    m.pause(&second).unwrap();
    wait_for(&m, &first, DownloadStatus::Completed).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        m.get(&second).unwrap().unwrap().status,
        DownloadStatus::Paused
    );
    m.resume_all().unwrap();
    wait_for(&m, &second, DownloadStatus::Completed).await;
}

#[tokio::test]
async fn a_failed_download_resumes_from_its_saved_segments() {
    let s = TestServer::start(8 * 1024 * 1024).await;
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("db.sqlite");
    let m = Manager::open(&db, d.path()).unwrap();
    m.set_settings(Settings {
        max_connections: 4,
        ..m.settings()
    })
    .unwrap();
    let id = parked(&s, &m).await;
    m.pause(&id).unwrap();
    wait_for(&m, &id, DownloadStatus::Paused).await;
    // Turn the paused row into a FAILED one with saved segments, as a mid-download failure leaves it.
    Store::open(&db)
        .unwrap()
        .set_status(
            &id,
            DownloadStatus::Failed,
            Some(("NETWORK", "test")),
            now_ms(),
        )
        .unwrap();
    s.cfg.hang_first.store(0, Ordering::SeqCst);
    let before = s.cfg.requests.load(Ordering::SeqCst);
    m.resume(&id).unwrap();
    wait_for(&m, &id, DownloadStatus::Completed).await;
    assert_eq!(sha256_file(&d.path().join("file")), sha256_bytes(&s.data));
    assert!(s.cfg.requests.load(Ordering::SeqCst) > before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pause_resume_pause_ends_paused() {
    // Re-review finding 2026-09-19: a pending resume survived a later pause.
    let s = TestServer::start(8 * 1024 * 1024).await;
    let d = tempfile::tempdir().unwrap();
    let m = manager(d.path());
    let id = parked(&s, &m).await;
    m.pause(&id).unwrap();
    m.resume(&id).unwrap();
    m.pause(&id).unwrap();
    wait_for(&m, &id, DownloadStatus::Paused).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(m.get(&id).unwrap().unwrap().status, DownloadStatus::Paused);
}

#[tokio::test]
async fn a_crash_resumes_from_the_saved_segments_at_the_next_launch() {
    let s = TestServer::start(12 * 1024 * 1024).await;
    s.cfg.chunk_delay_ms.store(40, Ordering::SeqCst); // ~2 s for the whole file
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("state").join("mdm.db");
    let a = Manager::open(&db, d.path()).unwrap();
    a.set_settings(Settings {
        max_connections: 4,
        ..a.settings()
    })
    .unwrap();
    let id = add(&a, &s.file_url());
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    while a.get(&id).unwrap().unwrap().downloaded == 0 {
        assert!(
            tokio::time::Instant::now() < deadline,
            "no durable progress was saved"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    a.simulate_crash();
    // The aborted driver dropped its handle, which pauses the engine; give the
    // engine's own task a moment to stop writing before a new one opens the file.
    tokio::time::sleep(Duration::from_millis(500)).await;
    drop(a);
    {
        let store = Store::open(&db).unwrap();
        let row = store.get(&id).unwrap().unwrap();
        assert_eq!(
            row.status,
            DownloadStatus::Downloading,
            "a crash saves no status"
        );
        assert!(
            store
                .load_segments(&id)
                .unwrap()
                .iter()
                .map(|x| x.downloaded)
                .sum::<u64>()
                > 0
        );
    }
    s.cfg.chunk_delay_ms.store(0, Ordering::SeqCst);
    let before = s.cfg.requests.load(Ordering::SeqCst);
    let b = Manager::open(&db, d.path()).unwrap();
    wait_for(&b, &id, DownloadStatus::Completed).await;
    assert_eq!(sha256_file(&d.path().join("file")), sha256_bytes(&s.data));
    assert!(s.cfg.requests.load(Ordering::SeqCst) > before);
}

#[tokio::test]
async fn a_server_that_lost_ranges_is_restarted_once_as_one_stream() {
    let s = TestServer::start(8 * 1024 * 1024).await;
    let d = tempfile::tempdir().unwrap();
    let m = manager(d.path());
    let id = parked(&s, &m).await;
    m.pause(&id).unwrap();
    wait_for(&m, &id, DownloadStatus::Paused).await;
    s.cfg.hang_first.store(0, Ordering::SeqCst);
    s.cfg.ranges.store(false, Ordering::SeqCst);
    let mut rx = m.subscribe();
    m.resume(&id).unwrap();
    wait_for(&m, &id, DownloadStatus::Completed).await;
    assert_eq!(sha256_file(&d.path().join("file")), sha256_bytes(&s.data));
    let mut noticed = false;
    while let Ok(ev) = rx.try_recv() {
        noticed |= matches!(ev, ManagerEvent::Notice { id: ref n, .. } if n == &id);
    }
    assert!(noticed, "the user was told the download started over");
}

#[tokio::test]
async fn a_changed_file_waits_for_the_user_to_restart() {
    let s = TestServer::start(8 * 1024 * 1024).await;
    let d = tempfile::tempdir().unwrap();
    let m = manager(d.path());
    let id = parked(&s, &m).await;
    m.pause(&id).unwrap();
    wait_for(&m, &id, DownloadStatus::Paused).await;
    s.cfg.hang_first.store(0, Ordering::SeqCst);
    *s.cfg.etag.lock().unwrap() = "\"v2\"".into();
    m.resume(&id).unwrap();
    let row = wait_for(&m, &id, DownloadStatus::Failed).await;
    assert_eq!(row.error_code.as_deref(), Some("SOURCE_CHANGED"));
    m.restart(&id).unwrap();
    wait_for(&m, &id, DownloadStatus::Completed).await;
    assert_eq!(sha256_file(&d.path().join("file")), sha256_bytes(&s.data));
}

#[tokio::test]
async fn settings_survive_a_restart_of_the_app() {
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("mdm.db");
    let a = Manager::open(&db, d.path()).unwrap();
    a.set_settings(Settings {
        max_parallel: 5,
        max_connections: 12,
        ..a.settings()
    })
    .unwrap();
    drop(a);
    let b = Manager::open(&db, Path::new("/somewhere/else")).unwrap();
    let s = b.settings();
    assert_eq!(
        (s.max_parallel, s.max_connections, s.download_dir.as_path()),
        (5, 12, d.path())
    );
}
