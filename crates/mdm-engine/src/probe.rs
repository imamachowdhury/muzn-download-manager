//! Learning what a URL is before downloading it: size, range support,
//! validators, name. `HEAD` first; a `GET` with `Range: bytes=0-0` when the
//! server refuses HEAD or hides the length.

use reqwest::header::{
    HeaderName, ACCEPT_RANGES, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE,
    ETAG, LAST_MODIFIED, RANGE,
};
use reqwest::{Response, StatusCode};
use tokio::time::timeout;
use url::Url;

use crate::engine::Engine;
use crate::error::EngineError;
use crate::filename::filename_from;
use crate::request::RequestExtras;

/// What the server told us about a URL.
#[derive(Clone, Debug)]
pub struct Probe {
    /// The URL after redirects; every later request uses this.
    pub final_url: Url,
    /// Total size, when the server states it.
    pub size: Option<u64>,
    /// True when byte ranges are honoured.
    pub ranges: bool,
    /// `ETag`, used to detect a changed file on resume.
    pub etag: Option<String>,
    /// `Last-Modified`, same purpose.
    pub last_modified: Option<String>,
    /// `Content-Type` without parameters.
    pub mime: Option<String>,
    /// Sanitised file name.
    pub filename: String,
}

impl Engine {
    /// Probe a URL. Follows redirects, never downloads more than one byte, and
    /// waits at most `stall_timeout` for each response's headers.
    pub async fn probe(&self, url: &Url, extras: &RequestExtras) -> Result<Probe, EngineError> {
        if !matches!(url.scheme(), "http" | "https") {
            return Err(EngineError::InvalidUrl(format!(
                "unsupported scheme {}",
                url.scheme()
            )));
        }
        // Both requests wait at most `stall_timeout` for the response headers:
        // a silent HEAD falls through to the GET, a silent GET is a network error.
        let wait = self.cfg.stall_timeout;
        // A HEAD with a length settles things only when it also advertises
        // ranges: a length with no `Accept-Ranges` does not prove the server
        // will honour a `Range` request, so the GET fallback below still
        // runs. Its `Content-Length` is kept in case that GET answers `200`
        // without one of its own (a server that only states the length on HEAD).
        let mut head_size: Option<u64> = None;
        let head = extras.apply(self.client.head(url.clone())).send();
        if let Ok(Ok(r)) = timeout(wait, head).await {
            if r.status().is_success() {
                if let Some(size) = header_u64(&r, CONTENT_LENGTH) {
                    if accepts_ranges(&r) {
                        return Ok(build(r, Some(size), true));
                    }
                    head_size = Some(size);
                }
            }
        }
        let get = extras
            .apply(self.client.get(url.clone()))
            .header(RANGE, "bytes=0-0")
            .send();
        let r = timeout(wait, get).await.map_err(|_| {
            EngineError::Network("no response headers within stall_timeout".into())
        })??;
        let status = r.status();
        if !status.is_success() {
            return Err(EngineError::HttpStatus {
                status: status.as_u16(),
            });
        }
        let (size, ranges) = if status == StatusCode::PARTIAL_CONTENT {
            (content_range_total(&r), true)
        } else {
            (
                header_u64(&r, CONTENT_LENGTH).or(head_size),
                accepts_ranges(&r),
            )
        };
        Ok(build(r, size, ranges)) // the body is dropped unread
    }
}

fn build(r: Response, size: Option<u64>, ranges: bool) -> Probe {
    let final_url = r.url().clone();
    let cd = header_str(&r, CONTENT_DISPOSITION);
    Probe {
        filename: filename_from(cd.as_deref(), &final_url),
        etag: header_str(&r, ETAG),
        last_modified: header_str(&r, LAST_MODIFIED),
        mime: header_str(&r, CONTENT_TYPE)
            .map(|v| v.split(';').next().unwrap_or("").trim().to_owned()),
        final_url,
        size,
        ranges,
    }
}

fn header_str(r: &Response, name: HeaderName) -> Option<String> {
    r.headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
}

fn header_u64(r: &Response, name: HeaderName) -> Option<u64> {
    header_str(r, name).and_then(|v| v.trim().parse().ok())
}

fn accepts_ranges(r: &Response) -> bool {
    header_str(r, ACCEPT_RANGES)
        .map(|v| v.eq_ignore_ascii_case("bytes"))
        .unwrap_or(false)
}

/// `Content-Range: bytes 0-0/12345` → 12345; `*` → None.
fn content_range_total(r: &Response) -> Option<u64> {
    header_str(r, CONTENT_RANGE)?
        .rsplit('/')
        .next()?
        .trim()
        .parse()
        .ok()
}
