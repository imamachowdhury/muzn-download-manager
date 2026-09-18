use std::sync::atomic::Ordering;
use std::time::Duration;

use mdm_engine::{
    DownloadSpec, Engine, EngineConfig, Outcome, RequestExtras, Resume, SegmentState, Status,
};
use mdm_test_server::*;
use url::Url;

const SIZE: usize = 8 * 1024 * 1024;

fn engine() -> Engine {
    Engine::new(EngineConfig {
        max_connections: 4,
        retry_base_delay: Duration::from_millis(1),
        // None of these tests needs a stall: a short one let the hung bodies
        // reconnect and finish on a slow runner before the pause landed.
        stall_timeout: Duration::from_secs(30),
        ..Default::default()
    })
    .unwrap()
}

fn spec(s: &TestServer, dir: &std::path::Path, resume: Option<Resume>) -> DownloadSpec {
    DownloadSpec {
        url: Url::parse(&s.file_url()).unwrap(),
        dir: dir.to_owned(),
        filename: None,
        extras: RequestExtras::default(),
        resume_from: resume,
    }
}

/// Start with every first body hanging after 1 000 bytes, wait until all
/// four segments have those bytes, pause. Returns the paused segments.
async fn start_and_pause(s: &TestServer, dir: &std::path::Path) -> Vec<SegmentState> {
    s.cfg.hang_first.store(4, Ordering::SeqCst);
    let h = engine().start(spec(s, dir, None)).await.unwrap();
    let mut rx = h.subscribe();
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            rx.changed().await.unwrap();
            if rx.borrow().downloaded >= 4000 {
                break;
            }
        }
    })
    .await
    .expect("the four hung bodies should deliver 4 000 bytes within 20 s");
    h.pause();
    let Outcome::Paused(segs) = h.wait().await else {
        panic!("expected Paused")
    };
    s.cfg.hang_first.store(0, Ordering::SeqCst);
    segs
}

fn resume(s: &TestServer, segs: Vec<SegmentState>) -> Resume {
    Resume {
        segments: segs,
        size: s.data.len() as u64,
        etag: Some(s.cfg.etag.lock().unwrap().clone()),
        last_modified: Some("Thu, 18 Sep 2026 10:00:00 GMT".into()),
    }
}

#[tokio::test]
async fn pause_then_resume_completes_with_the_right_bytes() {
    let s = TestServer::start(SIZE).await;
    let d = tempfile::tempdir().unwrap();
    let segs = start_and_pause(&s, d.path()).await;
    assert_eq!(segs.len(), 4);
    let paused_bytes: u64 = segs.iter().map(|x| x.downloaded).sum();
    assert!(paused_bytes >= 4000 && paused_bytes < SIZE as u64);
    assert!(d.path().join("file.mdm.part").exists());

    let h = engine()
        .start(spec(&s, d.path(), Some(resume(&s, segs))))
        .await
        .unwrap();
    let first = h.subscribe().borrow().clone();
    assert!(
        first.downloaded >= paused_bytes,
        "progress starts where it stopped"
    );
    let Outcome::Completed(path) = h.wait().await else {
        panic!()
    };
    assert_eq!(sha256_file(&path), sha256_bytes(&s.data));
}

#[tokio::test]
async fn crash_recovery_tolerates_an_unflushed_window() {
    let s = TestServer::start(SIZE).await;
    let d = tempfile::tempdir().unwrap();
    let mut segs = start_and_pause(&s, d.path()).await;
    // Pretend the store was ~1 s behind: forget the last 500 bytes of every segment.
    for x in &mut segs {
        x.downloaded = x.downloaded.saturating_sub(500);
    }
    let Outcome::Completed(path) = engine()
        .start(spec(&s, d.path(), Some(resume(&s, segs))))
        .await
        .unwrap()
        .wait()
        .await
    else {
        panic!()
    };
    assert_eq!(sha256_file(&path), sha256_bytes(&s.data));
}

#[tokio::test]
async fn changed_etag_is_source_changed() {
    let s = TestServer::start(SIZE).await;
    let d = tempfile::tempdir().unwrap();
    let segs = start_and_pause(&s, d.path()).await;
    let r = resume(&s, segs);
    *s.cfg.etag.lock().unwrap() = "\"v2\"".into();
    let e = engine()
        .start(spec(&s, d.path(), Some(r)))
        .await
        .unwrap_err();
    assert_eq!(e.code(), "SOURCE_CHANGED");
}

#[tokio::test]
async fn changed_size_is_source_changed() {
    let s = TestServer::start(SIZE).await;
    let d = tempfile::tempdir().unwrap();
    let segs = start_and_pause(&s, d.path()).await;
    let mut r = resume(&s, segs);
    r.size += 1;
    assert_eq!(
        engine()
            .start(spec(&s, d.path(), Some(r)))
            .await
            .unwrap_err()
            .code(),
        "SOURCE_CHANGED"
    );
}

#[tokio::test]
async fn server_that_lost_ranges_is_range_not_supported() {
    let s = TestServer::start(SIZE).await;
    let d = tempfile::tempdir().unwrap();
    let segs = start_and_pause(&s, d.path()).await;
    let r = resume(&s, segs);
    s.cfg.ranges.store(false, Ordering::SeqCst);
    assert_eq!(
        engine()
            .start(spec(&s, d.path(), Some(r)))
            .await
            .unwrap_err()
            .code(),
        "RANGE_NOT_SUPPORTED"
    );
}

#[tokio::test]
async fn missing_part_file_is_an_io_error() {
    let s = TestServer::start(SIZE).await;
    let d = tempfile::tempdir().unwrap();
    let segs = start_and_pause(&s, d.path()).await;
    std::fs::remove_file(d.path().join("file.mdm.part")).unwrap();
    assert_eq!(
        engine()
            .start(spec(&s, d.path(), Some(resume(&s, segs))))
            .await
            .unwrap_err()
            .code(),
        "IO"
    );
}

#[tokio::test]
async fn dropping_the_handle_stops_the_download_and_leaves_a_resumable_part() {
    // Spec §3 "process crash: drop the handle, start again". Final review
    // 2026-09-18: a dropped handle used to orphan its task, which kept the
    // part file open and raced a later `start` of the same name.
    let s = TestServer::start(SIZE).await;
    let d = tempfile::tempdir().unwrap();
    s.cfg.hang_first.store(4, Ordering::SeqCst);
    let h = engine().start(spec(&s, d.path(), None)).await.unwrap();
    let part = h.part_path().to_owned();
    let mut rx = h.subscribe();
    let segs = tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            rx.changed().await.unwrap();
            let p = rx.borrow();
            if p.downloaded >= 4000 {
                break p.segments.clone();
            }
        }
    })
    .await
    .expect("the four hung bodies should deliver 4 000 bytes within 20 s");

    drop(h);
    // The task ends (its progress sender goes away) and reports Paused: the
    // stop code was set to PAUSE before the token was cancelled.
    tokio::time::timeout(Duration::from_secs(5), async {
        while rx.changed().await.is_ok() {}
    })
    .await
    .expect("the download task should end within 5 s of dropping its handle");
    assert_eq!(rx.borrow().status, Status::Paused);

    tokio::time::sleep(Duration::from_millis(300)).await;
    let before = s.cfg.requests.load(Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(
        s.cfg.requests.load(Ordering::SeqCst),
        before,
        "no worker is left making requests"
    );
    assert!(part.exists(), "the part file is kept for a resume");

    s.cfg.hang_first.store(0, Ordering::SeqCst);
    let Outcome::Completed(path) = engine()
        .start(spec(&s, d.path(), Some(resume(&s, segs))))
        .await
        .unwrap()
        .wait()
        .await
    else {
        panic!("expected Completed")
    };
    assert_eq!(sha256_file(&path), sha256_bytes(&s.data));
}

#[tokio::test]
async fn corrupted_resume_state_is_refused_not_a_panic() {
    let s = TestServer::start(SIZE).await;
    let d = tempfile::tempdir().unwrap();
    let _ = start_and_pause(&s, d.path()).await;
    let bad = vec![SegmentState {
        idx: 0,
        start: 0,
        end: u64::MAX,
        downloaded: 0,
    }];
    let e = engine()
        .start(spec(&s, d.path(), Some(resume(&s, bad))))
        .await
        .unwrap_err();
    assert_eq!(e.code(), "INVALID_RESUME");
}

#[tokio::test]
async fn resume_rejects_segments_that_do_not_cover_the_file() {
    // Final review 2026-09-18: a resume used to trust its segments and could
    // "complete" a file with a zero-filled hole.
    let s = TestServer::start(SIZE).await;
    let d = tempfile::tempdir().unwrap();
    let mut segs = start_and_pause(&s, d.path()).await;
    segs.sort_by_key(|x| x.start);
    segs.pop();
    let e = engine()
        .start(spec(&s, d.path(), Some(resume(&s, segs))))
        .await
        .unwrap_err();
    assert_eq!(e.code(), "INVALID_RESUME");
    assert!(!e.is_transient());
}

#[tokio::test]
async fn resume_rejects_a_part_file_of_the_wrong_length() {
    let s = TestServer::start(SIZE).await;
    let d = tempfile::tempdir().unwrap();
    let segs = start_and_pause(&s, d.path()).await;
    let part = d.path().join("file.mdm.part");
    std::fs::OpenOptions::new()
        .write(true)
        .open(&part)
        .unwrap()
        .set_len(SIZE as u64 - 1)
        .unwrap();
    assert_eq!(
        engine()
            .start(spec(&s, d.path(), Some(resume(&s, segs))))
            .await
            .unwrap_err()
            .code(),
        "INVALID_RESUME"
    );
}

#[tokio::test]
async fn cancel_leaves_the_part_file_for_the_caller() {
    let s = TestServer::start(SIZE).await;
    let d = tempfile::tempdir().unwrap();
    s.cfg.hang_first.store(4, Ordering::SeqCst);
    let h = engine().start(spec(&s, d.path(), None)).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    h.cancel();
    let part = h.part_path().to_owned();
    assert!(matches!(h.wait().await, Outcome::Cancelled));
    assert!(part.exists());
    std::fs::remove_file(part).unwrap();
}

#[tokio::test]
async fn a_control_pauses_while_another_task_waits() {
    let s = TestServer::start(SIZE).await;
    let d = tempfile::tempdir().unwrap();
    s.cfg.hang_first.store(4, Ordering::SeqCst);
    let h = engine().start(spec(&s, d.path(), None)).await.unwrap();
    let control = h.control();
    let waiter = tokio::spawn(h.wait());
    tokio::time::sleep(Duration::from_millis(100)).await;
    control.pause();
    let out = tokio::time::timeout(Duration::from_secs(10), waiter)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(out, Outcome::Paused(_)), "{out:?}");
}

#[tokio::test]
async fn cancel_then_drop_still_reports_cancelled() {
    // Final review 2026-09-18: dropping the handle after cancel() must not turn
    // the cancel into a pause.
    let s = TestServer::start(SIZE).await;
    let d = tempfile::tempdir().unwrap();
    s.cfg.hang_first.store(4, Ordering::SeqCst);
    let h = engine().start(spec(&s, d.path(), None)).await.unwrap();
    let mut rx = h.subscribe();
    h.cancel();
    drop(h);
    tokio::time::timeout(Duration::from_secs(10), async {
        while rx.changed().await.is_ok() {}
    })
    .await
    .unwrap();
    assert_eq!(rx.borrow().status, Status::Cancelled);
}

#[tokio::test]
async fn a_pause_that_arrives_after_the_last_byte_still_completes() {
    // Every segment of the resume is already complete; the pause lands before
    // the run task is even polled (current-thread runtime), yet the file is
    // whole, so the honest outcome is Completed, not a Paused complete file.
    let s = TestServer::start(SIZE).await;
    let d = tempfile::tempdir().unwrap();
    let segs = start_and_pause(&s, d.path()).await;
    std::fs::write(d.path().join("file.mdm.part"), s.data.as_slice()).unwrap();
    let full: Vec<SegmentState> = segs
        .iter()
        .map(|x| SegmentState {
            downloaded: x.end - x.start + 1,
            ..x.clone()
        })
        .collect();
    let h = engine()
        .start(spec(&s, d.path(), Some(resume(&s, full))))
        .await
        .unwrap();
    h.pause();
    let out = h.wait().await;
    let Outcome::Completed(path) = out else {
        panic!("{out:?}")
    };
    assert_eq!(sha256_file(&path), sha256_bytes(&s.data));
}

#[tokio::test]
async fn a_paused_outcome_is_durable() {
    let s = TestServer::start(SIZE).await;
    let d = tempfile::tempdir().unwrap();
    s.cfg.hang_first.store(4, Ordering::SeqCst);
    let h = engine().start(spec(&s, d.path(), None)).await.unwrap();
    let mut rx = h.subscribe();
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            rx.changed().await.unwrap();
            if rx.borrow().downloaded >= 4000 {
                break;
            }
        }
    })
    .await
    .expect("the four hung bodies should deliver 4 000 bytes within 20 s");
    h.pause();
    let Outcome::Paused(segs) = h.wait().await else {
        panic!()
    };
    assert!(
        segs.iter().map(|s| s.downloaded).sum::<u64>() > 0,
        "the download paused with bytes written"
    );
    assert_eq!(rx.borrow().durable_segments, segs);
}
