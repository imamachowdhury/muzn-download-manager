//! One connection fetching one byte range into the shared part file.
//! Retries transient failures with exponential backoff, reconnects on stall,
//! stops at once on cancel, and notices when a stealer shrinks its `end`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use reqwest::header::{CONTENT_RANGE, RANGE};
use reqwest::StatusCode;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::error::EngineError;
use crate::file::PartFile;
use crate::plan::SegmentState;
use crate::request::RequestExtras;

/// Attempts in a row without progress before the segment fails.
pub const RETRY_MAX_ATTEMPTS: u32 = 10;
/// Longest backoff between attempts.
pub const RETRY_CAP: Duration = Duration::from_secs(60);
/// `end` of a single-stream segment until the stream tells us.
pub const UNKNOWN_END: u64 = u64::MAX;

/// The owner's retry rule (2026-09-18, "progress holei retry reset koro"):
/// only attempts in a row that wrote nothing count; a progressing attempt
/// resets the count and the backoff.
#[derive(Debug, Default)]
pub(crate) struct RetryBudget {
    no_progress: u32,
}

impl RetryBudget {
    /// A transient failure happened after writing `wrote` bytes. Returns the
    /// delay before the next attempt, or `None` when the budget is spent.
    pub(crate) fn on_failure(&mut self, wrote: u64, base: Duration) -> Option<Duration> {
        if wrote > 0 {
            self.no_progress = 0;
            return Some(base.min(RETRY_CAP));
        }
        self.no_progress += 1;
        if self.no_progress >= RETRY_MAX_ATTEMPTS {
            return None;
        }
        Some(
            base.saturating_mul(1 << (self.no_progress - 1).min(20))
                .min(RETRY_CAP),
        )
    }
}

/// Live counters for one segment, shared between the worker, the progress
/// ticker and the work stealer.
pub struct SegmentRuntime {
    /// Position in the plan.
    pub idx: u32,
    /// First byte.
    pub start: u64,
    /// Last byte, inclusive; [`UNKNOWN_END`] for a single stream. A stealer may lower it.
    /// A stealer must never lower it below `next_offset()`.
    pub end: AtomicU64,
    /// Bytes written so far, from `start`.
    pub downloaded: AtomicU64,
}

impl SegmentRuntime {
    /// New runtime; `end = None` means single stream.
    pub fn new(idx: u32, start: u64, end: Option<u64>, downloaded: u64) -> Self {
        Self {
            idx,
            start,
            end: AtomicU64::new(end.unwrap_or(UNKNOWN_END)),
            downloaded: AtomicU64::new(downloaded),
        }
    }
    /// From a stored state.
    pub fn from_state(s: &SegmentState) -> Self {
        Self::new(s.idx, s.start, Some(s.end), s.downloaded)
    }
    /// A copy safe to persist. `downloaded` is clamped to the (possibly shrunk) range.
    pub fn snapshot(&self) -> SegmentState {
        let end = self.end.load(Ordering::SeqCst);
        let downloaded = self.downloaded.load(Ordering::SeqCst);
        let end = if end == UNKNOWN_END {
            (self.start + downloaded).saturating_sub(1).max(self.start)
        } else {
            end
        };
        let downloaded = downloaded.min((end + 1).saturating_sub(self.start));
        SegmentState {
            idx: self.idx,
            start: self.start,
            end,
            downloaded,
        }
    }
    /// Absolute offset of the next byte to fetch.
    pub fn next_offset(&self) -> u64 {
        self.start + self.downloaded.load(Ordering::SeqCst)
    }
    /// Bytes still to fetch; `None` when the end is unknown.
    pub fn remaining(&self) -> Option<u64> {
        let end = self.end.load(Ordering::SeqCst);
        (end != UNKNOWN_END).then(|| (end + 1).saturating_sub(self.next_offset()))
    }
    /// True once the range is fully written.
    pub fn is_done(&self) -> bool {
        self.remaining() == Some(0)
    }
}

/// Everything one worker needs.
pub struct SegmentJob {
    /// Shared client.
    pub client: reqwest::Client,
    /// Final URL from the probe.
    pub url: Url,
    /// Headers and cookies.
    pub extras: RequestExtras,
    /// The part file.
    pub file: Arc<PartFile>,
    /// This worker's counters.
    pub seg: Arc<SegmentRuntime>,
    /// `true` = send `Range` and expect 206; `false` = plain GET from 0.
    pub ranged: bool,
    /// Stop signal.
    pub cancel: CancellationToken,
    /// From `EngineConfig`.
    pub retry_base_delay: Duration,
    /// From `EngineConfig`.
    pub stall_timeout: Duration,
}

/// Fetch the segment to completion, or fail with a permanent error, or
/// [`EngineError::Cancelled`]. A single stream (`ranged == false`) always
/// answers from byte 0, so every attempt restarts it from the beginning.
pub async fn fetch_segment(job: SegmentJob) -> Result<(), EngineError> {
    let mut budget = RetryBudget::default();
    loop {
        if job.cancel.is_cancelled() {
            return Err(EngineError::Cancelled);
        }
        let mut wrote = 0u64;
        match attempt_once(&job, &mut wrote).await {
            Ok(()) => return Ok(()),
            Err(e) if e.is_transient() => match budget.on_failure(wrote, job.retry_base_delay) {
                None => return Err(e),
                Some(delay) => {
                    tracing::debug!(idx = job.seg.idx, wrote, ?delay, error = %e, "segment retry");
                    tokio::select! {
                        _ = job.cancel.cancelled() => return Err(EngineError::Cancelled),
                        _ = tokio::time::sleep(delay) => {}
                    }
                }
            },
            Err(e) => return Err(e),
        }
    }
}

async fn attempt_once(job: &SegmentJob, wrote: &mut u64) -> Result<(), EngineError> {
    let seg = &job.seg;
    let next = if job.ranged {
        seg.next_offset()
    } else {
        // A plain GET always answers from byte 0: a retry re-fetches the
        // whole stream, so any partial progress from a dropped attempt is
        // discarded rather than resumed at the wrong file offset.
        seg.downloaded.store(0, Ordering::SeqCst);
        seg.start
    };
    let end = seg.end.load(Ordering::SeqCst);
    if job.ranged && end != UNKNOWN_END && next > end {
        return Ok(());
    }
    let mut rb = job.extras.apply(job.client.get(job.url.clone()));
    if job.ranged {
        rb = rb.header(RANGE, format!("bytes={next}-{end}"));
    }
    // `stall_timeout` also bounds the wait for the response headers: a server
    // that accepts the connection and never answers is a stall like any other.
    let resp = tokio::select! {
        _ = job.cancel.cancelled() => return Err(EngineError::Cancelled),
        r = tokio::time::timeout(job.stall_timeout, rb.send()) => match r {
            Err(_) => {
                return Err(EngineError::Network(
                    "no response headers within stall_timeout".into(),
                ))
            }
            Ok(r) => r?,
        },
    };
    let status = resp.status();
    if job.ranged {
        if status != StatusCode::PARTIAL_CONTENT {
            return Err(if status.is_success() {
                EngineError::RangeNotSupported
            } else {
                EngineError::HttpStatus {
                    status: status.as_u16(),
                }
            });
        }
        let starts_at = resp
            .headers()
            .get(CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| {
                v.strip_prefix("bytes ")?
                    .split('-')
                    .next()?
                    .parse::<u64>()
                    .ok()
            });
        if starts_at != Some(next) {
            return Err(EngineError::Network(format!(
                "Content-Range starts at {starts_at:?}, wanted {next}"
            )));
        }
    } else if !status.is_success() {
        return Err(EngineError::HttpStatus {
            status: status.as_u16(),
        });
    }

    let mut stream = resp.bytes_stream();
    let mut pos = next;
    loop {
        let chunk = tokio::select! {
            _ = job.cancel.cancelled() => return Err(EngineError::Cancelled),
            r = tokio::time::timeout(job.stall_timeout, stream.next()) => match r {
                Err(_) => return Err(EngineError::Network("no data for stall_timeout".into())),
                Ok(None) => break,
                Ok(Some(Err(e))) => return Err(e.into()),
                Ok(Some(Ok(b))) => b,
            },
        };
        let end = seg.end.load(Ordering::SeqCst);
        let mut buf: &[u8] = &chunk;
        if end != UNKNOWN_END {
            let room = (end + 1).saturating_sub(pos);
            if (buf.len() as u64) > room {
                buf = &buf[..room as usize];
            }
        }
        if buf.is_empty() {
            break; // the range was shrunk under us; what we have is enough
        }
        job.file.write_at(pos, buf).map_err(EngineError::from_io)?;
        pos += buf.len() as u64;
        *wrote += buf.len() as u64;
        seg.downloaded.store(pos - seg.start, Ordering::SeqCst);
        if end != UNKNOWN_END && pos > end {
            break;
        }
    }
    let end = seg.end.load(Ordering::SeqCst);
    if end == UNKNOWN_END {
        seg.end
            .store(pos.saturating_sub(1).max(seg.start), Ordering::SeqCst);
        return Ok(());
    }
    if pos <= end {
        return Err(EngineError::Network(format!(
            "connection closed at {pos}, wanted {end}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ten_failures_without_progress_spend_the_budget() {
        let mut b = RetryBudget::default();
        let base = Duration::from_secs(1);
        for i in 1..10 {
            assert!(b.on_failure(0, base).is_some(), "failure {i} still retries");
        }
        assert_eq!(b.on_failure(0, base), None, "the 10th failure gives up");
    }

    #[test]
    fn progress_resets_the_count_and_the_backoff() {
        // Review finding 2026-09-19: the progressing attempt must not count.
        let mut b = RetryBudget::default();
        let base = Duration::from_secs(1);
        for _ in 0..9 {
            b.on_failure(0, base).unwrap();
        }
        assert_eq!(
            b.on_failure(1, base),
            Some(base),
            "progress: base delay again"
        );
        for i in 1..10 {
            assert!(
                b.on_failure(0, base).is_some(),
                "failure {i} after progress still retries"
            );
        }
        assert_eq!(
            b.on_failure(0, base),
            None,
            "the 10th in a row after progress gives up"
        );
    }

    #[test]
    fn backoff_doubles_and_caps() {
        let mut b = RetryBudget::default();
        let base = Duration::from_secs(1);
        let delays: Vec<u64> = (0..9)
            .map(|_| b.on_failure(0, base).unwrap().as_secs())
            .collect();
        assert_eq!(delays, vec![1, 2, 4, 8, 16, 32, 60, 60, 60]);
    }
}
