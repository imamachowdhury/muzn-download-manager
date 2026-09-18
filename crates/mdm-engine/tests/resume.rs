mod support;

use std::sync::atomic::Ordering;
use std::time::Duration;

use mdm_engine::{
    DownloadSpec, Engine, EngineConfig, Outcome, RequestExtras, Resume, SegmentState,
};
use support::*;
use url::Url;

const SIZE: usize = 8 * 1024 * 1024;

fn engine() -> Engine {
    Engine::new(EngineConfig {
        max_connections: 4,
        retry_base_delay: Duration::from_millis(1),
        stall_timeout: Duration::from_millis(400),
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
