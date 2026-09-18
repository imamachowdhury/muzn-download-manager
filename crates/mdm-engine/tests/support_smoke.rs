mod support;

use support::*;

#[tokio::test]
async fn serves_full_file_and_ranges() {
    let s = TestServer::start(10_000).await;
    let c = reqwest::Client::new();

    let full = c.get(s.file_url()).send().await.unwrap();
    assert_eq!(full.status(), 200);
    assert_eq!(full.headers()["accept-ranges"], "bytes");
    assert_eq!(full.headers()["etag"], "\"v1\"");
    assert_eq!(full.bytes().await.unwrap().as_ref(), &s.data[..]);

    let part = c
        .get(s.file_url())
        .header("Range", "bytes=100-199")
        .send()
        .await
        .unwrap();
    assert_eq!(part.status(), 206);
    assert_eq!(part.headers()["content-range"], "bytes 100-199/10000");
    assert_eq!(part.bytes().await.unwrap().as_ref(), &s.data[100..200]);

    let head = c.head(s.file_url()).send().await.unwrap();
    assert_eq!(head.status(), 200);
    assert_eq!(head.headers()["content-length"], "10000");
}

#[tokio::test]
async fn switches_change_behaviour() {
    let s = TestServer::start(1_000).await;
    let c = reqwest::Client::new();

    s.cfg
        .ranges
        .store(false, std::sync::atomic::Ordering::SeqCst);
    let r = c
        .get(s.file_url())
        .header("Range", "bytes=0-9")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200, "ranges off → 200 with the whole body");
    assert!(r.headers().get("accept-ranges").is_none());

    s.cfg
        .head_allowed
        .store(false, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(c.head(s.file_url()).send().await.unwrap().status(), 405);

    s.cfg
        .fail_first
        .store(2, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(c.get(s.file_url()).send().await.unwrap().status(), 503);
    assert_eq!(c.get(s.file_url()).send().await.unwrap().status(), 503);
    assert_eq!(c.get(s.file_url()).send().await.unwrap().status(), 200);

    s.cfg
        .drop_after
        .store(300, std::sync::atomic::Ordering::SeqCst);
    let r = c.get(s.file_url()).send().await.unwrap();
    assert!(r.bytes().await.is_err(), "body must fail after 300 bytes");

    assert_eq!(c.get(s.status_url(404)).send().await.unwrap().status(), 404);
    let r = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap()
        .get(s.redirect_url())
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 302);
}

#[test]
fn payload_is_deterministic() {
    assert_eq!(payload(64), payload(64));
    assert_eq!(sha256_bytes(&payload(64)), sha256_bytes(&payload(64)));
}
