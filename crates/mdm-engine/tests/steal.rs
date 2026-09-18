use std::sync::atomic::Ordering;
use std::time::Duration;

use mdm_engine::{
    DownloadSpec, Engine, EngineConfig, Outcome, PartFile, RequestExtras, Resume, SegmentState,
};
use mdm_test_server::*;
use url::Url;

const MIB: u64 = 1024 * 1024;

#[tokio::test]
async fn a_finished_worker_takes_half_of_the_largest_remaining_range() {
    let s = TestServer::start(12 * MIB as usize).await;
    s.cfg.chunk_delay_ms.store(3, Ordering::SeqCst); // ~190 chunks → ~0.6 s for the big segment
    let d = tempfile::tempdir().unwrap();
    // A skewed plan as if resumed: 1 MiB + 11 MiB. The small one finishes first.
    drop(PartFile::open(d.path(), "file", Some(12 * MIB)).unwrap());
    let segs = vec![
        SegmentState {
            idx: 0,
            start: 0,
            end: MIB - 1,
            downloaded: 0,
        },
        SegmentState {
            idx: 1,
            start: MIB,
            end: 12 * MIB - 1,
            downloaded: 0,
        },
    ];
    let engine = Engine::new(EngineConfig {
        max_connections: 2,
        retry_base_delay: Duration::from_millis(1),
        stall_timeout: Duration::from_secs(5),
        ..Default::default()
    })
    .unwrap();
    let h = engine
        .start(DownloadSpec {
            url: Url::parse(&s.file_url()).unwrap(),
            dir: d.path().to_owned(),
            filename: None,
            extras: RequestExtras::default(),
            resume_from: Some(Resume {
                segments: segs,
                size: 12 * MIB,
                etag: Some("\"v1\"".into()),
                last_modified: Some("Thu, 18 Sep 2026 10:00:00 GMT".into()),
            }),
        })
        .await
        .unwrap();
    let rx = h.subscribe();
    let Outcome::Completed(path) = h.wait().await else {
        panic!()
    };
    assert_eq!(sha256_file(&path), sha256_bytes(&s.data));
    let last = rx.borrow().clone();
    assert!(
        last.segments.len() >= 3,
        "a stolen segment appeared: {:?}",
        last.segments
    );
    assert!(last.segments.iter().all(|x| x.is_done()));
    // Stolen segments are contiguous with their victims: sorted by start, no gaps, no overlap.
    let mut by_start = last.segments.clone();
    by_start.sort_by_key(|x| x.start);
    for w in by_start.windows(2) {
        assert_eq!(w[0].end + 1, w[1].start, "{:?}", by_start);
    }
    assert_eq!(by_start.last().unwrap().end, 12 * MIB - 1);
}

#[tokio::test]
async fn nothing_is_stolen_when_less_than_two_mib_remain() {
    let s = TestServer::start(3 * MIB as usize).await;
    let d = tempfile::tempdir().unwrap();
    let engine = Engine::new(EngineConfig {
        max_connections: 2,
        ..Default::default()
    })
    .unwrap();
    let h = engine
        .start(DownloadSpec {
            url: Url::parse(&s.file_url()).unwrap(),
            dir: d.path().to_owned(),
            filename: None,
            extras: RequestExtras::default(),
            resume_from: None,
        })
        .await
        .unwrap();
    let rx = h.subscribe();
    let Outcome::Completed(_) = h.wait().await else {
        panic!()
    };
    assert_eq!(
        rx.borrow().segments.len(),
        2,
        "1.5 MiB halves never qualify"
    );
}
