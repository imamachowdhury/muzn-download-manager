use std::sync::atomic::Ordering;
use std::time::Duration;

use mdm_engine::{DownloadSpec, Engine, EngineConfig, Outcome, RequestExtras, Status};
use mdm_test_server::*;
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
        reserved: Vec::new(),
    }
}

fn with_cookie(mut sp: DownloadSpec) -> DownloadSpec {
    sp.extras.cookies.push(mdm_engine::Cookie {
        name: "session".into(),
        value: "secret".into(),
    });
    sp
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
    // Task 6, item 1: a HEAD with a length but no Accept-Ranges no longer
    // settles the question, so the probe also does its GET Range: bytes=0-0
    // fallback before the real single-stream fetch — two GETs, not one.
    assert_eq!(s.cfg.requests.load(Ordering::SeqCst), 2);
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
async fn a_stream_of_unknown_length_completes() {
    let s = TestServer::start(100_000).await;
    s.cfg.chunked.store(true, Ordering::SeqCst);
    let d = tempfile::tempdir().unwrap();
    let h = engine(4)
        .start(spec(&s.file_url(), d.path()))
        .await
        .unwrap();
    assert_eq!(h.probe().size, None);
    let Outcome::Completed(path) = h.wait().await else {
        panic!()
    };
    assert_eq!(sha256_file(&path), sha256_bytes(&s.data));
}

#[tokio::test]
async fn an_empty_stream_of_unknown_length_completes() {
    // Final review 2026-09-18: it failed with "workers ended with bytes missing".
    let s = TestServer::start(0).await;
    s.cfg.chunked.store(true, Ordering::SeqCst);
    let d = tempfile::tempdir().unwrap();
    let h = engine(4)
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
    s.cfg.fail_first.store(1000, Ordering::SeqCst); // every GET answers 503 → 10 attempts without progress
    let h = engine(2)
        .start(spec(&s.file_url(), d.path()))
        .await
        .unwrap();
    let part = h.part_path().to_owned();
    let Outcome::Failed { error, segments } = h.wait().await else {
        panic!()
    };
    assert_eq!(error.code(), "HTTP_STATUS");
    assert_eq!(segments.len(), 2);
    assert!(part.exists(), "the part file stays for a later resume");
}

#[tokio::test]
async fn progress_reports_speed_and_a_durable_snapshot_while_downloading() {
    // Final review 2026-09-18: the saved state must only claim bytes that
    // reached the disk (a power cut loses the page cache).
    let s = TestServer::start(6 * 1024 * 1024).await;
    // 1.5 MiB per worker = 24 chunks of 64 KiB; 100 ms each keeps the download
    // running ~2.4 s, past the first SYNC_INTERVAL (1 s) and several ticks.
    // (2 ms finished in ~0.3 s: before any sync, sometimes before a speed.)
    s.cfg.chunk_delay_ms.store(100, Ordering::SeqCst);
    let d = tempfile::tempdir().unwrap();
    let h = engine(4)
        .start(spec(&s.file_url(), d.path()))
        .await
        .unwrap();
    let mut rx = h.subscribe();
    let first = rx.borrow_and_update().clone();
    assert_eq!(first.status, Status::Downloading);
    assert_eq!(
        first
            .durable_segments
            .iter()
            .map(|x| x.downloaded)
            .sum::<u64>(),
        0
    );
    let mut saw_speed = false;
    let mut saw_durable = false;
    while rx.changed().await.is_ok() {
        let p = rx.borrow_and_update().clone();
        if p.status != Status::Downloading {
            break;
        }
        saw_speed |= p.speed_bps > 0;
        let durable: u64 = p.durable_segments.iter().map(|x| x.downloaded).sum();
        assert!(
            durable <= p.downloaded,
            "durable {durable} > downloaded {}",
            p.downloaded
        );
        saw_durable |= durable > 0;
    }
    assert!(saw_speed, "a speed above zero was reported");
    assert!(
        saw_durable,
        "a durable snapshot was published during the download"
    );
    let Outcome::Completed(path) = h.wait().await else {
        panic!()
    };
    assert_eq!(sha256_file(&path), sha256_bytes(&s.data));
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

#[tokio::test]
async fn cookies_are_not_sent_to_another_origin_after_a_redirect() {
    // Final review 2026-09-18: workers fetch final_url directly, bypassing
    // reqwest's own cross-host header stripping.
    //
    // The redirect target is reached via `localhost`; on a machine where that
    // resolves to `::1` before `127.0.0.1` (observed here), connecting waits
    // out hyper's IPv6-then-IPv4 fallback (~300 ms) before it lands on the
    // server, which is bound to `127.0.0.1` only. `engine(2)`'s 300 ms
    // `stall_timeout` — plenty for every other test, which never crosses a
    // real network hop — is too tight a race against that exact fallback
    // window, so this test alone gets a longer one.
    let e = Engine::new(EngineConfig {
        max_connections: 2,
        retry_base_delay: Duration::from_millis(1),
        stall_timeout: Duration::from_secs(2),
        ..Default::default()
    })
    .unwrap();
    let s = TestServer::start(1024 * 1024).await;
    let d = tempfile::tempdir().unwrap();
    let h = e
        .start(with_cookie(spec(&s.redirect_to_ip_url(), d.path())))
        .await
        .unwrap();
    let Outcome::Completed(path) = h.wait().await else {
        panic!()
    };
    assert_eq!(sha256_file(&path), sha256_bytes(&s.data));
    assert_eq!(*s.cfg.last_cookie.lock().unwrap(), None);
}

#[tokio::test]
async fn cookies_are_kept_on_the_same_origin() {
    let s = TestServer::start(1024 * 1024).await;
    let d = tempfile::tempdir().unwrap();
    let h = engine(2)
        .start(with_cookie(spec(&s.redirect_url(), d.path())))
        .await
        .unwrap();
    let Outcome::Completed(_) = h.wait().await else {
        panic!()
    };
    assert_eq!(
        s.cfg.last_cookie.lock().unwrap().as_deref(),
        Some("session=secret")
    );
}

#[tokio::test]
async fn two_live_downloads_of_the_same_name_get_separate_part_files() {
    // Final review 2026-09-18: they shared one .mdm.part before.
    let s = TestServer::start(3 * 1024 * 1024).await;
    s.cfg.chunk_delay_ms.store(2, Ordering::SeqCst);
    let d = tempfile::tempdir().unwrap();
    let e = engine(2);
    let a = e.start(spec(&s.file_url(), d.path())).await.unwrap();
    let b = e.start(spec(&s.file_url(), d.path())).await.unwrap();
    assert_eq!(a.filename(), "file");
    assert_eq!(b.filename(), "file (1)");
    let (oa, ob) = tokio::join!(a.wait(), b.wait());
    let (Outcome::Completed(pa), Outcome::Completed(pb)) = (oa, ob) else {
        panic!()
    };
    assert_ne!(pa, pb);
    assert_eq!(sha256_file(&pa), sha256_bytes(&s.data));
    assert_eq!(sha256_file(&pb), sha256_bytes(&s.data));
}

#[tokio::test]
async fn a_reserved_part_file_is_neither_reused_nor_deleted() {
    // Task 5 review ruling: a paused / queued download's part file is not in
    // the live registry, so a fresh start of the same name deleted it.
    let s = TestServer::start(1024 * 1024).await;
    let d = tempfile::tempdir().unwrap();
    let reserved = d.path().join("file.mdm.part");
    std::fs::write(&reserved, b"a paused download's bytes").unwrap();
    let mut sp = spec(&s.file_url(), d.path());
    sp.reserved = vec![reserved.clone()];
    let h = engine(2).start(sp).await.unwrap();
    assert_eq!(h.filename(), "file (1)");
    let Outcome::Completed(path) = h.wait().await else {
        panic!()
    };
    assert_eq!(path.file_name().unwrap(), "file (1)");
    assert_eq!(sha256_file(&path), sha256_bytes(&s.data));
    assert_eq!(
        std::fs::read(&reserved).unwrap(),
        b"a paused download's bytes"
    );
}

#[tokio::test]
async fn a_reconfigured_engine_keeps_the_live_part_claims() {
    // Task 9 review: a settings change built a new engine with an empty
    // registry, so a same-name start on it shared a live download's part file.
    let s = TestServer::start(3 * 1024 * 1024).await;
    s.cfg.chunk_delay_ms.store(2, Ordering::SeqCst);
    let d = tempfile::tempdir().unwrap();
    let e = engine(2);
    let a = e.start(spec(&s.file_url(), d.path())).await.unwrap();
    let e2 = e
        .reconfigured(EngineConfig {
            max_connections: 2,
            ..Default::default()
        })
        .unwrap();
    let b = e2.start(spec(&s.file_url(), d.path())).await.unwrap();
    assert_eq!(a.filename(), "file");
    assert_eq!(b.filename(), "file (1)");
    let (oa, ob) = tokio::join!(a.wait(), b.wait());
    let (Outcome::Completed(pa), Outcome::Completed(pb)) = (oa, ob) else {
        panic!()
    };
    assert_ne!(pa, pb);
    assert_eq!(sha256_file(&pa), sha256_bytes(&s.data));
    assert_eq!(sha256_file(&pb), sha256_bytes(&s.data));
}
