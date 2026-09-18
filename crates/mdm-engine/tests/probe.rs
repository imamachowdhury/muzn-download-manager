use std::sync::atomic::Ordering;
use std::time::Duration;

use mdm_engine::{Engine, EngineConfig, EngineError, RequestExtras};
use mdm_test_server::*;
use url::Url;

fn engine() -> Engine {
    Engine::new(EngineConfig::default()).unwrap()
}

#[tokio::test]
async fn ranges_on_via_head() {
    let s = TestServer::start(50_000).await;
    let p = engine()
        .probe(
            &Url::parse(&s.file_url()).unwrap(),
            &RequestExtras::default(),
        )
        .await
        .unwrap();
    assert_eq!(p.size, Some(50_000));
    assert!(p.ranges);
    assert_eq!(p.etag.as_deref(), Some("\"v1\""));
    assert_eq!(
        p.last_modified.as_deref(),
        Some("Thu, 18 Sep 2026 10:00:00 GMT")
    );
    assert_eq!(p.mime.as_deref(), Some("application/octet-stream"));
    assert_eq!(p.filename, "file");
    assert_eq!(
        s.cfg.requests.load(Ordering::SeqCst),
        0,
        "HEAD was enough, no GET"
    );
}

#[tokio::test]
async fn ranges_off() {
    let s = TestServer::start(50_000).await;
    s.cfg.ranges.store(false, Ordering::SeqCst);
    let p = engine()
        .probe(
            &Url::parse(&s.file_url()).unwrap(),
            &RequestExtras::default(),
        )
        .await
        .unwrap();
    assert_eq!(p.size, Some(50_000));
    assert!(!p.ranges);
}

#[tokio::test]
async fn head_refused_falls_back_to_get_range() {
    let s = TestServer::start(50_000).await;
    s.cfg.head_allowed.store(false, Ordering::SeqCst);
    let p = engine()
        .probe(
            &Url::parse(&s.file_url()).unwrap(),
            &RequestExtras::default(),
        )
        .await
        .unwrap();
    assert_eq!(p.size, Some(50_000));
    assert!(p.ranges, "206 to bytes=0-0 proves range support");
    assert_eq!(s.cfg.requests.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn follows_redirects_and_keeps_final_url() {
    let s = TestServer::start(10).await;
    let p = engine()
        .probe(
            &Url::parse(&s.redirect_url()).unwrap(),
            &RequestExtras::default(),
        )
        .await
        .unwrap();
    assert!(p.final_url.as_str().ends_with("/file"));
    assert_eq!(p.filename, "file");
}

#[tokio::test]
async fn content_disposition_names_the_file() {
    let s = TestServer::start(10).await;
    *s.cfg.content_disposition.lock().unwrap() = Some("attachment; filename=\"a b.zip\"".into());
    let p = engine()
        .probe(
            &Url::parse(&s.file_url()).unwrap(),
            &RequestExtras::default(),
        )
        .await
        .unwrap();
    assert_eq!(p.filename, "a b.zip");
}

#[tokio::test]
async fn http_error_is_reported() {
    let s = TestServer::start(10).await;
    let e = engine()
        .probe(
            &Url::parse(&s.status_url(404)).unwrap(),
            &RequestExtras::default(),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(e, EngineError::HttpStatus { status: 404 }),
        "{e:?}"
    );
}

#[tokio::test]
async fn rejects_non_http_scheme() {
    let e = engine()
        .probe(&Url::parse("ftp://x/y").unwrap(), &RequestExtras::default())
        .await
        .unwrap_err();
    assert_eq!(e.code(), "INVALID_URL");
}

#[tokio::test]
async fn probe_does_not_hang_on_a_silent_server() {
    // Final review 2026-09-18: neither the HEAD nor the GET fallback had a
    // timeout for the response headers.
    let s = TestServer::start(10).await;
    s.cfg.hang_headers.store(100, Ordering::SeqCst);
    let e = Engine::new(EngineConfig {
        stall_timeout: Duration::from_millis(300),
        ..Default::default()
    })
    .unwrap();
    let r = tokio::time::timeout(
        Duration::from_secs(3),
        e.probe(
            &Url::parse(&s.file_url()).unwrap(),
            &RequestExtras::default(),
        ),
    )
    .await
    .expect("the probe must give up within 3 s");
    assert_eq!(r.unwrap_err().code(), "NETWORK");
}

#[tokio::test]
async fn sends_extras_without_breaking_the_request() {
    let s = TestServer::start(10).await;
    let x = RequestExtras {
        headers: vec![("X-Test".into(), "1".into())],
        cookies: vec![],
    };
    engine()
        .probe(&Url::parse(&s.file_url()).unwrap(), &x)
        .await
        .unwrap();
}
