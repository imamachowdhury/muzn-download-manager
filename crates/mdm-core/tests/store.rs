use std::path::Path;

use mdm_core::*;

fn new(url: &str) -> NewDownload {
    NewDownload {
        url: url.into(),
        dir: None,
        filename: None,
        referrer: Some("https://example.com/page".into()),
        headers: vec![Header {
            name: "X-A".into(),
            value: "1".into(),
        }],
        cookies: vec![SavedCookie {
            name: "s".into(),
            value: "v".into(),
        }],
        start_paused: false,
    }
}

#[test]
fn open_migrates_once_and_reopens() {
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("sub").join("mdm.db");
    let s = Store::open(&db).unwrap();
    assert_eq!(s.schema_version().unwrap(), 1);
    drop(s);
    let s = Store::open(&db).unwrap();
    assert_eq!(s.schema_version().unwrap(), 1);
}

#[test]
fn insert_get_list_round_trip() {
    let s = Store::open_in_memory().unwrap();
    let a = DownloadId::new();
    let b = DownloadId::new();
    let ra = s
        .insert(
            &a,
            Path::new("/dl"),
            &new("https://x/a.zip"),
            DownloadStatus::Queued,
            1_000,
        )
        .unwrap();
    s.insert(
        &b,
        Path::new("/dl"),
        &new("https://x/b.zip"),
        DownloadStatus::Paused,
        2_000,
    )
    .unwrap();
    assert_eq!(ra.status, DownloadStatus::Queued);
    assert_eq!(
        ra.headers,
        vec![Header {
            name: "X-A".into(),
            value: "1".into()
        }]
    );
    assert_eq!(ra.cookies.len(), 1);
    assert_eq!(ra.downloaded, 0);
    assert_eq!(s.get(&a).unwrap().unwrap(), ra);
    let ids: Vec<_> = s.list().unwrap().into_iter().map(|r| r.id).collect();
    assert_eq!(ids, vec![b, a], "newest first");
    assert!(s.get(&DownloadId::new()).unwrap().is_none());
}

#[test]
fn next_queued_is_fifo_and_skips_excluded() {
    let s = Store::open_in_memory().unwrap();
    let (a, b, c) = (DownloadId::new(), DownloadId::new(), DownloadId::new());
    s.insert(
        &a,
        Path::new("/d"),
        &new("https://x/a"),
        DownloadStatus::Queued,
        10,
    )
    .unwrap();
    s.insert(
        &b,
        Path::new("/d"),
        &new("https://x/b"),
        DownloadStatus::Paused,
        20,
    )
    .unwrap();
    s.insert(
        &c,
        Path::new("/d"),
        &new("https://x/c"),
        DownloadStatus::Queued,
        30,
    )
    .unwrap();
    assert_eq!(s.next_queued(&[]).unwrap(), Some(a.clone()));
    assert_eq!(
        s.next_queued(std::slice::from_ref(&a)).unwrap(),
        Some(c.clone())
    );
    assert_eq!(s.next_queued(&[a, c]).unwrap(), None);
}

#[test]
fn status_error_and_completion_fields() {
    let s = Store::open_in_memory().unwrap();
    let a = DownloadId::new();
    s.insert(
        &a,
        Path::new("/d"),
        &new("https://x/a"),
        DownloadStatus::Queued,
        10,
    )
    .unwrap();
    s.set_status(&a, DownloadStatus::Failed, Some(("NETWORK", "reset")), 20)
        .unwrap();
    let r = s.get(&a).unwrap().unwrap();
    assert_eq!(
        (r.error_code.as_deref(), r.error_message.as_deref()),
        (Some("NETWORK"), Some("reset"))
    );
    assert_eq!(r.updated_at, 20);
    s.set_probe(
        &a,
        &ProbeInfo {
            final_url: "https://cdn/a".into(),
            filename: "a.bin".into(),
            size: Some(99),
            etag: Some("\"e\"".into()),
            last_modified: None,
            mime: Some("application/zip".into()),
        },
        25,
    )
    .unwrap();
    s.set_status(&a, DownloadStatus::Completed, None, 30)
        .unwrap();
    let r = s.get(&a).unwrap().unwrap();
    assert_eq!(r.error_code, None);
    assert_eq!(r.completed_at, Some(30));
    assert_eq!((r.size, r.downloaded), (Some(99), 99));
    assert_eq!(r.filename.as_deref(), Some("a.bin"));
    assert_eq!(
        s.set_status(&DownloadId::new(), DownloadStatus::Queued, None, 1)
            .unwrap_err()
            .code(),
        "NOT_FOUND"
    );
}

#[test]
fn reset_interrupted_requeues_active_rows_only() {
    let s = Store::open_in_memory().unwrap();
    let ids: Vec<_> = (0..4).map(|_| DownloadId::new()).collect();
    for (i, st) in [
        DownloadStatus::Probing,
        DownloadStatus::Downloading,
        DownloadStatus::Paused,
        DownloadStatus::Completed,
    ]
    .into_iter()
    .enumerate()
    {
        s.insert(&ids[i], Path::new("/d"), &new("https://x/f"), st, i as i64)
            .unwrap();
    }
    assert_eq!(s.reset_interrupted(100).unwrap(), 2);
    let st: Vec<_> = ids
        .iter()
        .map(|id| s.get(id).unwrap().unwrap().status)
        .collect();
    assert_eq!(
        st,
        vec![
            DownloadStatus::Queued,
            DownloadStatus::Queued,
            DownloadStatus::Paused,
            DownloadStatus::Completed
        ]
    );
}

#[test]
fn delete_removes_the_row() {
    let s = Store::open_in_memory().unwrap();
    let a = DownloadId::new();
    s.insert(
        &a,
        Path::new("/d"),
        &new("https://x/a"),
        DownloadStatus::Queued,
        1,
    )
    .unwrap();
    s.delete(&a).unwrap();
    assert!(s.get(&a).unwrap().is_none());
}

use mdm_engine::SegmentState;

fn seg(idx: u32, start: u64, end: u64, downloaded: u64) -> SegmentState {
    SegmentState {
        idx,
        start,
        end,
        downloaded,
    }
}

#[test]
fn segments_replace_and_sum_into_downloaded() {
    let s = Store::open_in_memory().unwrap();
    let a = DownloadId::new();
    s.insert(
        &a,
        Path::new("/d"),
        &new("https://x/a"),
        DownloadStatus::Queued,
        1,
    )
    .unwrap();
    s.save_segments(&a, &[seg(1, 50, 99, 10), seg(0, 0, 49, 50)])
        .unwrap();
    assert_eq!(
        s.load_segments(&a).unwrap(),
        vec![seg(0, 0, 49, 50), seg(1, 50, 99, 10)]
    );
    assert_eq!(s.get(&a).unwrap().unwrap().downloaded, 60);
    s.save_segments(&a, &[seg(0, 0, 99, 99)]).unwrap();
    assert_eq!(s.load_segments(&a).unwrap(), vec![seg(0, 0, 99, 99)]);
    s.clear_segments(&a).unwrap();
    assert!(s.load_segments(&a).unwrap().is_empty());
}

#[test]
fn deleting_a_download_deletes_its_segments() {
    let s = Store::open_in_memory().unwrap();
    let a = DownloadId::new();
    s.insert(
        &a,
        Path::new("/d"),
        &new("https://x/a"),
        DownloadStatus::Queued,
        1,
    )
    .unwrap();
    s.save_segments(&a, &[seg(0, 0, 9, 3)]).unwrap();
    s.delete(&a).unwrap();
    assert!(s.load_segments(&a).unwrap().is_empty());
}

#[test]
fn settings_default_then_round_trip() {
    let s = Store::open_in_memory().unwrap();
    let d = s.load_settings(Path::new("/home/u/Downloads")).unwrap();
    assert_eq!(d.download_dir, Path::new("/home/u/Downloads"));
    assert_eq!((d.max_connections, d.max_parallel), (8, 3));
    let mine = Settings {
        max_connections: 16,
        proxy: ProxySetting::Manual {
            url: "http://127.0.0.1:8080".into(),
        },
        ..d
    };
    s.save_settings(&mine).unwrap();
    assert_eq!(s.load_settings(Path::new("/elsewhere")).unwrap(), mine);
}

#[test]
fn settings_are_clamped_and_checked() {
    let base = Settings {
        download_dir: "/d".into(),
        ..Settings::default()
    };
    let v = Settings {
        max_connections: 200,
        max_parallel: 0,
        ..base.clone()
    }
    .validated()
    .unwrap();
    assert_eq!((v.max_connections, v.max_parallel), (32, 1));
    assert_eq!(
        Settings::default().validated().unwrap_err().code(),
        "SETTINGS"
    );
    let bad = Settings {
        proxy: ProxySetting::Manual {
            url: "not a url".into(),
        },
        ..base.clone()
    };
    assert_eq!(bad.validated().unwrap_err().code(), "SETTINGS");
    let cfg = base.engine_config().unwrap();
    assert_eq!(cfg.max_connections, 8);
}
