//! In-process HTTP server for engine tests. Every switch is an atomic so a
//! test flips behaviour mid-download without restarting anything.
#![allow(dead_code)]

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::{Path as AxPath, State};
use axum::http::{header, HeaderMap, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use futures_util::stream;
use sha2::{Digest, Sha256};

pub struct ServerCfg {
    pub ranges: AtomicBool,
    pub head_allowed: AtomicBool,
    pub fail_first: AtomicU32,
    pub drop_after: AtomicU64,
    pub etag: Mutex<String>,
    pub requests: AtomicU32,
    pub content_disposition: Mutex<Option<String>>,
}

impl Default for ServerCfg {
    fn default() -> Self {
        Self {
            ranges: AtomicBool::new(true),
            head_allowed: AtomicBool::new(true),
            fail_first: AtomicU32::new(0),
            drop_after: AtomicU64::new(0),
            etag: Mutex::new("\"v1\"".to_owned()),
            requests: AtomicU32::new(0),
            content_disposition: Mutex::new(None),
        }
    }
}

#[derive(Clone)]
struct AppState {
    data: Arc<Vec<u8>>,
    cfg: Arc<ServerCfg>,
}

pub struct TestServer {
    pub base: String,
    pub data: Arc<Vec<u8>>,
    pub cfg: Arc<ServerCfg>,
}

impl TestServer {
    pub async fn start(size: usize) -> TestServer {
        let data = Arc::new(payload(size));
        let cfg = Arc::new(ServerCfg::default());
        let state = AppState {
            data: data.clone(),
            cfg: cfg.clone(),
        };
        let app = Router::new()
            .route("/file", get(file))
            .route("/redirect", get(redirect))
            .route("/status/:code", get(status))
            .with_state(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        TestServer {
            base: format!("http://{addr}"),
            data,
            cfg,
        }
    }
    pub fn file_url(&self) -> String {
        format!("{}/file", self.base)
    }
    pub fn redirect_url(&self) -> String {
        format!("{}/redirect", self.base)
    }
    pub fn status_url(&self, code: u16) -> String {
        format!("{}/status/{code}", self.base)
    }
}

async fn file(State(s): State<AppState>, method: Method, headers: HeaderMap) -> Response {
    let cfg = &s.cfg;
    if method == Method::HEAD && !cfg.head_allowed.load(Ordering::SeqCst) {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    if method == Method::GET {
        cfg.requests.fetch_add(1, Ordering::SeqCst);
        // fetch_update: decrement while > 0, and only then answer 503.
        let failed = cfg
            .fail_first
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok();
        if failed {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    }
    let total = s.data.len() as u64;
    let ranges = cfg.ranges.load(Ordering::SeqCst);
    let range = if ranges {
        headers
            .get(header::RANGE)
            .and_then(|v| parse_range(v.to_str().ok()?, total))
    } else {
        None
    };

    let mut rb = Response::builder()
        .header(header::ETAG, cfg.etag.lock().unwrap().clone())
        .header(header::LAST_MODIFIED, "Thu, 18 Sep 2026 10:00:00 GMT")
        .header(header::CONTENT_TYPE, "application/octet-stream");
    if ranges {
        rb = rb.header(header::ACCEPT_RANGES, "bytes");
    }
    if let Some(cd) = cfg.content_disposition.lock().unwrap().clone() {
        rb = rb.header(header::CONTENT_DISPOSITION, cd);
    }
    let (status, start, end) = match range {
        Some((a, b)) => (StatusCode::PARTIAL_CONTENT, a, b),
        None => (StatusCode::OK, 0, total.saturating_sub(1)),
    };
    let len = if total == 0 { 0 } else { end - start + 1 };
    rb = rb.status(status).header(header::CONTENT_LENGTH, len);
    if status == StatusCode::PARTIAL_CONTENT {
        rb = rb.header(
            header::CONTENT_RANGE,
            format!("bytes {start}-{end}/{total}"),
        );
    }
    if method == Method::HEAD {
        return rb.body(Body::empty()).unwrap();
    }
    let slice = s.data[start as usize..(start + len) as usize].to_vec();
    let drop_after = cfg.drop_after.load(Ordering::SeqCst);
    let body = if drop_after > 0 && drop_after < len {
        let good = slice[..drop_after as usize].to_vec();
        Body::from_stream(stream::iter(vec![
            Ok::<_, std::io::Error>(bytes::Bytes::from(good)),
            Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionReset,
                "test drop",
            )),
        ]))
    } else {
        Body::from(slice)
    };
    rb.body(body).unwrap()
}

fn parse_range(v: &str, total: u64) -> Option<(u64, u64)> {
    let spec = v.strip_prefix("bytes=")?;
    let (a, b) = spec.split_once('-')?;
    let start: u64 = a.parse().ok()?;
    let end: u64 = if b.is_empty() {
        total - 1
    } else {
        b.parse().ok()?
    };
    (start <= end && end < total).then_some((start, end))
}

async fn redirect() -> Response {
    (StatusCode::FOUND, [(header::LOCATION, "/file")]).into_response()
}

async fn status(AxPath(code): AxPath<u16>) -> Response {
    StatusCode::from_u16(code)
        .unwrap_or(StatusCode::IM_A_TEAPOT)
        .into_response()
}

/// Deterministic pseudo-random bytes (LCG), same on every run and platform.
pub fn payload(size: usize) -> Vec<u8> {
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    (0..size)
        .map(|_| {
            x = x
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (x >> 56) as u8
        })
        .collect()
}

pub fn sha256_bytes(b: &[u8]) -> String {
    format!("{:x}", Sha256::digest(b))
}

pub fn sha256_file(path: &Path) -> String {
    sha256_bytes(&std::fs::read(path).unwrap())
}
