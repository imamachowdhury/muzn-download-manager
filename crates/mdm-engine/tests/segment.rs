mod support;

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use mdm_engine::segment::{fetch_segment, SegmentJob, SegmentRuntime};
use mdm_engine::{EngineError, PartFile, RequestExtras};
use support::*;
use tokio_util::sync::CancellationToken;
use url::Url;

fn job(
    s: &TestServer,
    dir: &std::path::Path,
    seg: Arc<SegmentRuntime>,
    ranged: bool,
) -> (SegmentJob, Arc<PartFile>) {
    let file = Arc::new(PartFile::open(dir, "out", Some(s.data.len() as u64)).unwrap());
    let j = SegmentJob {
        client: reqwest::Client::new(),
        url: Url::parse(&s.file_url()).unwrap(),
        extras: RequestExtras::default(),
        file: file.clone(),
        seg,
        ranged,
        cancel: CancellationToken::new(),
        retry_base_delay: Duration::from_millis(1),
        stall_timeout: Duration::from_millis(300),
    };
    (j, file)
}

#[tokio::test]
async fn writes_exactly_its_range() {
    let s = TestServer::start(100_000).await;
    let d = tempfile::tempdir().unwrap();
    let seg = Arc::new(SegmentRuntime::new(0, 1000, Some(4999), 0));
    let (j, file) = job(&s, d.path(), seg.clone(), true);
    fetch_segment(j).await.unwrap();
    assert!(seg.is_done());
    assert_eq!(seg.downloaded.load(Ordering::SeqCst), 4000);
    let bytes = std::fs::read(file.part_path()).unwrap();
    assert_eq!(&bytes[1000..5000], &s.data[1000..5000]);
    assert!(
        bytes[..1000].iter().all(|b| *b == 0),
        "nothing before the range"
    );
    assert!(
        bytes[5000..].iter().all(|b| *b == 0),
        "nothing after the range"
    );
}

#[tokio::test]
async fn resumes_from_downloaded_offset() {
    let s = TestServer::start(100_000).await;
    let d = tempfile::tempdir().unwrap();
    let seg = Arc::new(SegmentRuntime::new(0, 0, Some(9999), 6000));
    let (j, file) = job(&s, d.path(), seg.clone(), true);
    fetch_segment(j).await.unwrap();
    let bytes = std::fs::read(file.part_path()).unwrap();
    assert_eq!(&bytes[6000..10000], &s.data[6000..10000]);
    assert!(
        bytes[..6000].iter().all(|b| *b == 0),
        "did not refetch the first 6000"
    );
}

#[tokio::test]
async fn already_done_segment_makes_no_request() {
    let s = TestServer::start(10_000).await;
    let d = tempfile::tempdir().unwrap();
    let seg = Arc::new(SegmentRuntime::new(0, 0, Some(999), 1000));
    let (j, _) = job(&s, d.path(), seg, true);
    fetch_segment(j).await.unwrap();
    assert_eq!(s.cfg.requests.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn single_stream_learns_the_end() {
    let s = TestServer::start(12_345).await;
    let d = tempfile::tempdir().unwrap();
    let seg = Arc::new(SegmentRuntime::new(0, 0, None, 0));
    let (j, file) = job(&s, d.path(), seg.clone(), false);
    fetch_segment(j).await.unwrap();
    assert_eq!(seg.end.load(Ordering::SeqCst), 12_344);
    assert_eq!(sha256_file(file.part_path()), sha256_bytes(&s.data));
}

#[tokio::test]
async fn retries_after_connection_drop_and_503() {
    let s = TestServer::start(200_000).await;
    s.cfg.drop_after.store(50_000, Ordering::SeqCst);
    s.cfg.fail_first.store(2, Ordering::SeqCst);
    let d = tempfile::tempdir().unwrap();
    let seg = Arc::new(SegmentRuntime::new(0, 0, Some(199_999), 0));
    let (j, file) = job(&s, d.path(), seg, true);
    fetch_segment(j).await.unwrap();
    assert_eq!(sha256_file(file.part_path()), sha256_bytes(&s.data));
    assert!(
        s.cfg.requests.load(Ordering::SeqCst) >= 6,
        "2 x 503 + 4 partial bodies"
    );
}

#[tokio::test]
async fn gives_up_after_max_attempts() {
    let s = TestServer::start(200_000).await;
    s.cfg.drop_after.store(10, Ordering::SeqCst); // 20 000 attempts would be needed
    let d = tempfile::tempdir().unwrap();
    let seg = Arc::new(SegmentRuntime::new(0, 0, Some(199_999), 0));
    let (j, _) = job(&s, d.path(), seg, true);
    let e = fetch_segment(j).await.unwrap_err();
    assert_eq!(e.code(), "NETWORK");
    assert_eq!(s.cfg.requests.load(Ordering::SeqCst), 10);
}

#[tokio::test]
async fn permanent_error_fails_at_once() {
    let s = TestServer::start(10).await;
    let d = tempfile::tempdir().unwrap();
    let seg = Arc::new(SegmentRuntime::new(0, 0, Some(9), 0));
    let (mut j, _) = job(&s, d.path(), seg, true);
    j.url = Url::parse(&s.status_url(404)).unwrap();
    let e = fetch_segment(j).await.unwrap_err();
    assert!(matches!(e, EngineError::HttpStatus { status: 404 }));
}

#[tokio::test]
async fn range_ignored_by_server_is_range_not_supported() {
    let s = TestServer::start(10_000).await;
    s.cfg.ranges.store(false, Ordering::SeqCst);
    let d = tempfile::tempdir().unwrap();
    let seg = Arc::new(SegmentRuntime::new(0, 100, Some(199), 0));
    let (j, _) = job(&s, d.path(), seg, true);
    assert_eq!(
        fetch_segment(j).await.unwrap_err().code(),
        "RANGE_NOT_SUPPORTED"
    );
}

#[tokio::test]
async fn stall_reconnects() {
    let s = TestServer::start(100_000).await;
    s.cfg.hang_first.store(1, Ordering::SeqCst); // first body hangs after 1 000 bytes
    let d = tempfile::tempdir().unwrap();
    let seg = Arc::new(SegmentRuntime::new(0, 0, Some(99_999), 0));
    let (j, file) = job(&s, d.path(), seg, true);
    fetch_segment(j).await.unwrap();
    assert_eq!(sha256_file(file.part_path()), sha256_bytes(&s.data));
    assert_eq!(s.cfg.requests.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn header_stall_reconnects() {
    // Final review 2026-09-18: `stall_timeout` only guarded the body, so a
    // server that accepted and never answered hung the segment forever.
    let s = TestServer::start(100_000).await;
    s.cfg.hang_headers.store(1, Ordering::SeqCst); // first request: no headers, ever
    let d = tempfile::tempdir().unwrap();
    let seg = Arc::new(SegmentRuntime::new(0, 0, Some(99_999), 0));
    let (j, file) = job(&s, d.path(), seg, true);
    tokio::time::timeout(Duration::from_secs(10), fetch_segment(j))
        .await
        .expect("a silent server must time out, not hang the worker")
        .unwrap();
    assert_eq!(sha256_file(file.part_path()), sha256_bytes(&s.data));
    assert_eq!(s.cfg.requests.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn cancel_stops_and_keeps_progress() {
    let s = TestServer::start(2_000_000).await;
    s.cfg.hang_first.store(1, Ordering::SeqCst);
    let d = tempfile::tempdir().unwrap();
    let seg = Arc::new(SegmentRuntime::new(0, 0, Some(1_999_999), 0));
    let (j, _) = job(&s, d.path(), seg.clone(), true);
    let cancel = j.cancel.clone();
    let task = tokio::spawn(fetch_segment(j));
    // Poll instead of a fixed sleep: on a cold CI runner the first 1 000
    // bytes may not have landed yet after a flat 100 ms.
    for _ in 0..500 {
        if seg.downloaded.load(Ordering::SeqCst) == 1000 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    cancel.cancel();
    let e = task.await.unwrap().unwrap_err();
    assert_eq!(e.code(), "CANCELLED");
    assert_eq!(seg.downloaded.load(Ordering::SeqCst), 1000);
}

#[tokio::test]
async fn shrinking_end_stops_the_worker_early() {
    let s = TestServer::start(3_000_000).await;
    s.cfg.hang_first.store(1, Ordering::SeqCst);
    let d = tempfile::tempdir().unwrap();
    let seg = Arc::new(SegmentRuntime::new(0, 0, Some(2_999_999), 0));
    let (j, file) = job(&s, d.path(), seg.clone(), true);
    let task = tokio::spawn(fetch_segment(j));
    tokio::time::sleep(Duration::from_millis(100)).await;
    seg.end.store(1_999, Ordering::SeqCst); // a stealer took [2000, end]
    task.await.unwrap().unwrap(); // stall → reconnect for 1000-1999 → done
    assert!(seg.is_done());
    let bytes = std::fs::read(file.part_path()).unwrap();
    assert_eq!(&bytes[..2000], &s.data[..2000]);
}

#[tokio::test]
async fn single_stream_retry_restarts_from_the_beginning() {
    // Review finding 2026-09-18: a retried plain GET starts at byte 0, so the
    // worker must write from `start` again, never at start + downloaded.
    let s = TestServer::start(100_000).await;
    s.cfg.ranges.store(false, Ordering::SeqCst);
    s.cfg.hang_first.store(1, Ordering::SeqCst); // first body: 1 000 bytes then stall
    let d = tempfile::tempdir().unwrap();
    let seg = Arc::new(SegmentRuntime::new(0, 0, None, 0));
    let (j, file) = job(&s, d.path(), seg.clone(), false);
    fetch_segment(j).await.unwrap();
    assert_eq!(s.cfg.requests.load(Ordering::SeqCst), 2);
    assert_eq!(seg.end.load(Ordering::SeqCst), 99_999);
    assert_eq!(sha256_file(file.part_path()), sha256_bytes(&s.data));
}
