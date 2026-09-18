mod support;

use std::sync::atomic::Ordering;
use std::time::Duration;

use mdm_engine::{DownloadSpec, Engine, EngineConfig, Outcome, RequestExtras, Status};
use support::*;
use url::Url;

fn engine(conns: u8) -> Engine {
    Engine::new(EngineConfig {
        max_connections: conns,
        retry_base_delay: Duration::from_millis(1),
        stall_timeout: Duration::from_millis(300),
        ..Default::default()
    })
    .unwrap()
}

fn spec(url: &str, dir: &std::path::Path) -> DownloadSpec {
    DownloadSpec {
        url: Url::parse(url).unwrap(),
        dir: dir.to_owned(),
        filename: None,
        extras: RequestExtras::default(),
        resume_from: None,
    }
}

#[tokio::test]
async fn segmented_happy_path() {
    let s = TestServer::start(16 * 1024 * 1024).await;
    let d = tempfile::tempdir().unwrap();
    let h = engine(8)
        .start(spec(&s.file_url(), d.path()))
        .await
        .unwrap();
    assert_eq!(h.probe().size, Some(16 * 1024 * 1024));
    let rx = h.subscribe();
    let out = h.wait().await;
    let Outcome::Completed(path) = out else {
        panic!("{out:?}")
    };
    assert_eq!(path.file_name().unwrap(), "file");
    assert_eq!(sha256_file(&path), sha256_bytes(&s.data));
    let last = rx.borrow().clone();
    assert_eq!(last.status, Status::Completed);
    assert_eq!(last.downloaded, 16 * 1024 * 1024);
    assert_eq!(last.total, Some(16 * 1024 * 1024));
    assert!(last.segments.len() >= 8, "8 planned (+ any stolen)");
    assert!(last.segments.iter().all(|x| x.is_done()));
    assert!(s.cfg.requests.load(Ordering::SeqCst) >= 8);
    assert!(!d.path().join("file.mdm.part").exists());
}

#[tokio::test]
async fn single_stream_when_ranges_off() {
    let s = TestServer::start(3 * 1024 * 1024).await;
    s.cfg.ranges.store(false, Ordering::SeqCst);
    let d = tempfile::tempdir().unwrap();
    let h = engine(8)
        .start(spec(&s.file_url(), d.path()))
        .await
        .unwrap();
    let rx = h.subscribe();
    let Outcome::Completed(path) = h.wait().await else {
        panic!()
    };
    assert_eq!(sha256_file(&path), sha256_bytes(&s.data));
    assert_eq!(rx.borrow().segments.len(), 1);
    assert_eq!(s.cfg.requests.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn head_refused_is_still_segmented() {
    let s = TestServer::start(4 * 1024 * 1024).await;
    s.cfg.head_allowed.store(false, Ordering::SeqCst);
    let d = tempfile::tempdir().unwrap();
    let h = engine(4)
        .start(spec(&s.file_url(), d.path()))
        .await
        .unwrap();
    let rx = h.subscribe();
    let Outcome::Completed(path) = h.wait().await else {
        panic!()
    };
    assert_eq!(sha256_file(&path), sha256_bytes(&s.data));
    assert!(rx.borrow().segments.len() >= 4);
}

#[tokio::test]
async fn zero_byte_file_completes_immediately() {
    let s = TestServer::start(0).await;
    let d = tempfile::tempdir().unwrap();
    let h = engine(8)
        .start(spec(&s.file_url(), d.path()))
        .await
        .unwrap();
    let Outcome::Completed(path) = h.wait().await else {
        panic!()
    };
    assert_eq!(std::fs::metadata(&path).unwrap().len(), 0);
}

#[tokio::test]
async fn explicit_filename_and_clash() {
    let s = TestServer::start(1000).await;
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("x.bin"), b"old").unwrap();
    let mut sp = spec(&s.file_url(), d.path());
    sp.filename = Some("x.bin".into());
    let Outcome::Completed(path) = engine(2).start(sp).await.unwrap().wait().await else {
        panic!()
    };
    assert_eq!(path.file_name().unwrap(), "x (1).bin");
    assert_eq!(std::fs::read(d.path().join("x.bin")).unwrap(), b"old");
}

#[tokio::test]
async fn redirect_downloads_from_final_url() {
    let s = TestServer::start(2 * 1024 * 1024).await;
    let d = tempfile::tempdir().unwrap();
    let h = engine(2)
        .start(spec(&s.redirect_url(), d.path()))
        .await
        .unwrap();
    assert!(h.probe().final_url.as_str().ends_with("/file"));
    let Outcome::Completed(path) = h.wait().await else {
        panic!()
    };
    assert_eq!(sha256_file(&path), sha256_bytes(&s.data));
}

#[tokio::test]
async fn probe_failure_is_an_error_from_start() {
    let s = TestServer::start(10).await;
    let d = tempfile::tempdir().unwrap();
    let e = engine(2)
        .start(spec(&s.status_url(404), d.path()))
        .await
        .unwrap_err();
    assert_eq!(e.code(), "HTTP_STATUS");
}

#[tokio::test]
async fn survives_503_burst_and_connection_drops() {
    let s = TestServer::start(2 * 1024 * 1024).await;
    s.cfg.fail_first.store(3, Ordering::SeqCst);
    s.cfg.drop_after.store(300_000, Ordering::SeqCst);
    let d = tempfile::tempdir().unwrap();
    let Outcome::Completed(path) = engine(2)
        .start(spec(&s.file_url(), d.path()))
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
async fn permanent_mid_download_failure_reports_and_keeps_part() {
    let s = TestServer::start(2 * 1024 * 1024).await;
    let d = tempfile::tempdir().unwrap();
    s.cfg.drop_after.store(1, Ordering::SeqCst); // every body dies → 10 attempts → NETWORK
    let h = engine(2)
        .start(spec(&s.file_url(), d.path()))
        .await
        .unwrap();
    let part = h.part_path().to_owned();
    let Outcome::Failed { error, segments } = h.wait().await else {
        panic!()
    };
    assert_eq!(error.code(), "NETWORK");
    assert_eq!(segments.len(), 2);
    assert!(part.exists(), "the part file stays for a later resume");
}

#[tokio::test]
async fn progress_stream_reports_bytes_and_speed() {
    let s = TestServer::start(8 * 1024 * 1024).await;
    let d = tempfile::tempdir().unwrap();
    let h = engine(4)
        .start(spec(&s.file_url(), d.path()))
        .await
        .unwrap();
    let mut rx = h.subscribe();
    let first = rx.borrow_and_update().clone();
    assert_eq!(first.status, Status::Downloading);
    assert_eq!(first.total, Some(8 * 1024 * 1024));
    let _ = h.wait().await;
    let last = rx.borrow().clone();
    assert_eq!(last.status, Status::Completed);
    assert_eq!(last.downloaded, 8 * 1024 * 1024);
    assert_eq!(last.eta_secs, Some(0));
}

#[tokio::test]
async fn fresh_start_discards_a_stale_larger_part_file() {
    // Review finding 2026-09-18: PartFile::open never truncates, so a fresh
    // start over an old, bigger .part kept the old tail after the new bytes.
    let s = TestServer::start(1_000_000).await;
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("file.mdm.part"), vec![0xAAu8; 5_000_000]).unwrap();
    let Outcome::Completed(path) = engine(4)
        .start(spec(&s.file_url(), d.path()))
        .await
        .unwrap()
        .wait()
        .await
    else {
        panic!()
    };
    assert_eq!(std::fs::metadata(&path).unwrap().len(), 1_000_000);
    assert_eq!(sha256_file(&path), sha256_bytes(&s.data));
}
