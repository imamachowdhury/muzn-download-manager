//! The orchestrator: probe, plan, run one worker per segment, publish
//! progress, steal work from the slowest segment, finish the file.

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::watch;
use tokio::task::{JoinHandle, JoinSet};
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::engine::Engine;
use crate::error::EngineError;
use crate::file::{PartFile, PART_SUFFIX};
use crate::plan::{plan_segments, SegmentState};
use crate::probe::Probe;
use crate::request::RequestExtras;
use crate::segment::{fetch_segment, SegmentJob, SegmentRuntime};

/// How often [`Progress`] is published while downloading.
pub const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);
/// A finished worker steals from a segment only if this much is left.
pub const STEAL_MIN_REMAINING: u64 = 2 * 1024 * 1024;
/// How often the part file is fsynced while downloading; see
/// [`Progress::durable_segments`].
pub const SYNC_INTERVAL: Duration = Duration::from_secs(1);
const SPEED_WINDOW: Duration = Duration::from_secs(2);

/// Where a download is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Workers are running.
    Downloading,
    /// Stopped by `pause()`; segments are safe to persist and resume.
    Paused,
    /// Renamed to its final name.
    Completed,
    /// A permanent error; see the [`Outcome`].
    Failed,
    /// Stopped by `cancel()`.
    Cancelled,
}

/// A snapshot for the UI.
#[derive(Clone, Debug)]
pub struct Progress {
    /// Total size when known.
    pub total: Option<u64>,
    /// Bytes written so far, all segments.
    pub downloaded: u64,
    /// Bytes per second over the last two seconds.
    pub speed_bps: u64,
    /// Seconds left at the current speed; `None` when unknown.
    pub eta_secs: Option<u64>,
    /// Every segment, including stolen ones.
    pub segments: Vec<SegmentState>,
    /// The segments as of the last completed fsync of the part file (taken
    /// just before it): only these bytes are known to survive a power cut.
    /// At start it is the starting state (resume segments, or all-zero fresh
    /// segments). On `Paused` / `Failed` the run syncs once more, so the final
    /// value equals the outcome's segments. Callers persist this, never
    /// `segments`, which may count bytes still in the OS cache.
    pub durable_segments: Vec<SegmentState>,
    /// Current status.
    pub status: Status,
}

/// How a download ended.
#[derive(Debug)]
pub enum Outcome {
    /// The final path.
    Completed(PathBuf),
    /// The durable segments (as of the last successful fsync) — safe to
    /// persist; pass them back as [`Resume::segments`].
    Paused(Vec<SegmentState>),
    /// A permanent error; the part file is kept for a later resume.
    Failed {
        /// Why.
        error: EngineError,
        /// The durable segments (as of the last successful fsync) — safe to
        /// persist.
        segments: Vec<SegmentState>,
    },
    /// Cancelled; the part file is left for the caller to delete.
    Cancelled,
}

/// What a resume needs to check the source is unchanged.
#[derive(Clone, Debug)]
pub struct Resume {
    /// From the previous `Outcome::Paused` / `Failed` or the last flushed progress.
    pub segments: Vec<SegmentState>,
    /// Size at the original probe.
    pub size: u64,
    /// ETag at the original probe.
    pub etag: Option<String>,
    /// Last-Modified at the original probe.
    pub last_modified: Option<String>,
}

/// One download request.
#[derive(Clone, Debug)]
pub struct DownloadSpec {
    /// Where from.
    pub url: Url,
    /// Directory for the file.
    pub dir: PathBuf,
    /// Override the probed name.
    pub filename: Option<String>,
    /// Headers and cookies.
    pub extras: RequestExtras,
    /// Continue a previous attempt.
    pub resume_from: Option<Resume>,
    /// Part-file paths the caller owns for downloads that are not running
    /// (paused, queued); treated exactly like live parts — a fresh start
    /// picks another name and never deletes them.
    pub reserved: Vec<PathBuf>,
}

const STOP_NONE: u8 = 0;
const STOP_PAUSE: u8 = 1;
const STOP_CANCEL: u8 = 2;

/// Dropping a [`DownloadHandle`] pauses its download: the stop code is set to
/// PAUSE *before* the token is cancelled, exactly like `pause()`, so the task
/// ends with its part file intact and resumable instead of running on as an
/// orphan. `wait()` disarms it once the task has ended.
#[derive(Debug)]
struct StopOnDrop {
    stop: Arc<AtomicU8>,
    cancel: CancellationToken,
    armed: bool,
}

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        if self.armed {
            // Only claim PAUSE if nothing else already set a stop code: a
            // `cancel()` just before the handle is dropped must still report
            // Cancelled, not be overwritten into a Paused.
            let _ = self.stop.compare_exchange(
                STOP_NONE,
                STOP_PAUSE,
                Ordering::SeqCst,
                Ordering::SeqCst,
            );
            self.cancel.cancel();
        }
    }
}

/// Pause or cancel a running download from anywhere, while another task owns
/// the [`DownloadHandle`] and awaits `wait()`.
#[derive(Clone, Debug)]
pub struct DownloadControl {
    stop: Arc<AtomicU8>,
    cancel: CancellationToken,
}

impl DownloadControl {
    /// Same as [`DownloadHandle::pause`].
    pub fn pause(&self) {
        self.stop.store(STOP_PAUSE, Ordering::SeqCst);
        self.cancel.cancel();
    }
    /// Same as [`DownloadHandle::cancel`].
    pub fn cancel(&self) {
        self.stop.store(STOP_CANCEL, Ordering::SeqCst);
        self.cancel.cancel();
    }
}

/// A running download.
///
/// Dropping the handle without `wait()`ing pauses the download (the task
/// stops and keeps the part file for a resume); it never leaves the task
/// running unowned.
#[derive(Debug)]
pub struct DownloadHandle {
    probe: Probe,
    part_path: PathBuf,
    filename: String,
    rx: watch::Receiver<Progress>,
    cancel: CancellationToken,
    stop: Arc<AtomicU8>,
    join: JoinHandle<Outcome>,
    // `DownloadHandle` itself must stay free of a `Drop` impl: `wait()` moves
    // `join` out of `self`. The drop behaviour lives in this field instead.
    guard: StopOnDrop,
}

impl DownloadHandle {
    /// What the probe learned.
    pub fn probe(&self) -> &Probe {
        &self.probe
    }
    /// The `.mdm.part` path.
    pub fn part_path(&self) -> &Path {
        &self.part_path
    }
    /// The name actually used for this download — the probed (or overridden)
    /// name, or `name (n).ext` when that name was already claimed by another
    /// live download (`Engine::start`).
    pub fn filename(&self) -> &str {
        &self.filename
    }
    /// Progress stream; the current value is available at once.
    pub fn subscribe(&self) -> watch::Receiver<Progress> {
        self.rx.clone()
    }
    /// A cloneable pause / cancel for use while another task owns this
    /// handle and awaits [`Self::wait`].
    pub fn control(&self) -> DownloadControl {
        DownloadControl {
            stop: self.stop.clone(),
            cancel: self.cancel.clone(),
        }
    }
    /// Stop the workers; `wait()` returns `Outcome::Paused`.
    pub fn pause(&self) {
        self.stop.store(STOP_PAUSE, Ordering::SeqCst);
        self.cancel.cancel();
    }
    /// Stop the workers; `wait()` returns `Outcome::Cancelled`.
    pub fn cancel(&self) {
        self.stop.store(STOP_CANCEL, Ordering::SeqCst);
        self.cancel.cancel();
    }
    /// Wait for the end. Dropping this future before it resolves pauses the
    /// download, like dropping the handle.
    pub async fn wait(mut self) -> Outcome {
        let outcome = (&mut self.join).await.unwrap_or_else(|_| Outcome::Failed {
            error: EngineError::Internal("download task panicked".into()),
            segments: Vec::new(),
        });
        self.guard.armed = false;
        outcome
    }
}

impl Engine {
    /// Probe, then start downloading. Errors here mean nothing was started.
    ///
    /// A fresh start discards any leftover `.mdm.part` of the same name; only a
    /// resume reuses it. A resume whose segments do not cover the file, or
    /// whose part file has the wrong length, is refused with
    /// [`EngineError::InvalidResume`].
    ///
    /// A second live download of the same name — or one whose part path is in
    /// [`DownloadSpec::reserved`] — gets `name (1).ext`, and a reserved part
    /// file is never deleted; a resume
    /// whose part file is already claimed by another live download is
    /// refused with [`EngineError::InvalidResume`] instead (its name is
    /// fixed, so there is nowhere else to put it).
    pub async fn start(&self, spec: DownloadSpec) -> Result<DownloadHandle, EngineError> {
        let probe = self.probe(&spec.url, &spec.extras).await?;
        let base_filename = spec
            .filename
            .clone()
            .unwrap_or_else(|| probe.filename.clone());

        // Claim a live part path exclusively before touching the filesystem:
        // a fresh start whose chosen name is already live is renamed to
        // `name (n).ext`; a resume of a live path is refused outright. The
        // claim is released (`PartClaim::drop`) when the run ends.
        let (filename, part_path, claim) = {
            let mut live = self.live_parts.lock().unwrap();
            let mut filename = base_filename.clone();
            let mut part_path = spec.dir.join(format!("{filename}{PART_SUFFIX}"));
            if spec.resume_from.is_some() {
                // The caller does not reserve its own part file; a reserved
                // path equal to it is not a conflict.
                if live.contains(&part_path) {
                    return Err(EngineError::InvalidResume(
                        "the part file is in use by another download".into(),
                    ));
                }
            } else {
                let mut n = 1u32;
                while live.contains(&part_path) || spec.reserved.contains(&part_path) {
                    filename = numbered_filename(&base_filename, n);
                    part_path = spec.dir.join(format!("{filename}{PART_SUFFIX}"));
                    n += 1;
                }
            }
            live.insert(part_path.clone());
            let claim = PartClaim {
                registry: self.live_parts.clone(),
                path: part_path.clone(),
            };
            (filename, part_path, claim)
        };

        let (segments, ranged): (Vec<Arc<SegmentRuntime>>, bool) = match &spec.resume_from {
            Some(r) => {
                if !probe.ranges {
                    return Err(EngineError::RangeNotSupported);
                }
                let changed = probe.size != Some(r.size)
                    || match (&probe.etag, &r.etag) {
                        (Some(a), Some(b)) => a != b,
                        (None, None) => probe.last_modified != r.last_modified,
                        _ => true,
                    };
                if changed {
                    return Err(EngineError::SourceChanged);
                }
                if !part_path.exists() {
                    return Err(EngineError::Io(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        "part file missing; start over",
                    )));
                }
                validate_resume(r, &part_path)?;
                (
                    r.segments
                        .iter()
                        .map(|s| Arc::new(SegmentRuntime::from_state(s)))
                        .collect(),
                    true,
                )
            }
            None => match (probe.size, probe.ranges) {
                (Some(size), true) => (
                    plan_segments(size, self.cfg.max_connections)
                        .iter()
                        .map(|s| Arc::new(SegmentRuntime::from_state(s)))
                        .collect(),
                    true,
                ),
                (Some(0), false) => (Vec::new(), false),
                (Some(size), false) => (
                    vec![Arc::new(SegmentRuntime::new(0, 0, Some(size - 1), 0))],
                    false,
                ),
                (None, _) => (vec![Arc::new(SegmentRuntime::new(0, 0, None, 0))], false),
            },
        };

        // `PartFile::open` never truncates, because a resume depends on the
        // bytes already there. A fresh start must therefore clear a leftover
        // part file itself: writing the new download over a larger old one
        // would leave the old tail behind the new bytes at `finish()`.
        if spec.resume_from.is_none() && part_path.exists() {
            std::fs::remove_file(&part_path).map_err(EngineError::from_io)?;
        }
        // The starting state is durable by definition: a resume's segments
        // were saved from a durable snapshot, fresh segments claim nothing.
        let durable: Vec<SegmentState> = {
            let mut v: Vec<SegmentState> = segments.iter().map(|s| s.snapshot()).collect();
            v.sort_by_key(|s| s.idx);
            v
        };
        let file = Arc::new(PartFile::open(&spec.dir, &filename, probe.size)?);
        // A redirect that left the caller's origin gets none of its cookies
        // or Authorization: workers fetch `final_url` directly, so unlike
        // `probe()` above (which goes through reqwest's own redirect-time
        // header stripping) nothing else would drop them for a cross-origin
        // target.
        let extras = if spec.url.origin() == probe.final_url.origin() {
            spec.extras.clone()
        } else {
            spec.extras.without_credentials()
        };
        let run = Run {
            engine: self.clone(),
            url: probe.final_url.clone(),
            extras,
            file,
            segments: Mutex::new(segments),
            durable: Mutex::new(durable),
            ranged,
            total: probe.size,
            cancel: CancellationToken::new(),
            stop: Arc::new(AtomicU8::new(STOP_NONE)),
            _claim: claim,
        };
        let (tx, rx) = watch::channel(run.progress(0, Status::Downloading));
        let cancel = run.cancel.clone();
        let stop = run.stop.clone();
        let join = tokio::spawn(run.run(tx));
        let guard = StopOnDrop {
            stop: stop.clone(),
            cancel: cancel.clone(),
            armed: true,
        };
        Ok(DownloadHandle {
            probe,
            part_path,
            filename,
            rx,
            cancel,
            stop,
            join,
            guard,
        })
    }
}

/// `stem (n).ext` for a live-name clash, using the same split rule as
/// `file::free_name`'s own numbering (`file::split_ext`).
fn numbered_filename(filename: &str, n: u32) -> String {
    let (stem, ext) = crate::file::split_ext(filename);
    format!("{stem} ({n}){ext}")
}

/// Releases a live part path when the run ends.
struct PartClaim {
    registry: Arc<Mutex<HashSet<PathBuf>>>,
    path: PathBuf,
}

impl Drop for PartClaim {
    fn drop(&mut self) {
        self.registry.lock().unwrap().remove(&self.path);
    }
}

/// Check that saved resume state describes a `r.size`-byte file: segments that
/// run contiguously from byte 0 to `size - 1` (none for an empty file), no
/// segment claiming more bytes than its range, and a part file of exactly
/// `size` bytes. Anything else would "complete" a file with holes.
fn validate_resume(r: &Resume, part_path: &Path) -> Result<(), EngineError> {
    let bad = |why: String| Err(EngineError::InvalidResume(why));
    let mut segs: Vec<&SegmentState> = r.segments.iter().collect();
    segs.sort_by_key(|s| s.start);
    if r.size == 0 {
        if !segs.is_empty() {
            return bad("segments given for an empty file".into());
        }
    } else {
        if segs.is_empty() {
            return bad("no segments".into());
        }
        let mut expect = 0u64;
        for s in &segs {
            if s.start != expect {
                return bad(format!(
                    "segment {} starts at {}, expected {expect}",
                    s.idx, s.start
                ));
            }
            if s.end < s.start {
                return bad(format!("segment {} ends before it starts", s.idx));
            }
            // `end - start + 1` and `end + 1` can each overflow on corrupted
            // input (e.g. `end: u64::MAX`); `checked_*` turns that into a
            // refusal instead of a panic.
            let len = match s.end.checked_sub(s.start).and_then(|x| x.checked_add(1)) {
                Some(len) => len,
                None => return bad(format!("segment {} has an invalid range", s.idx)),
            };
            if s.downloaded > len {
                return bad(format!(
                    "segment {} claims {} bytes of a {len}-byte range",
                    s.idx, s.downloaded,
                ));
            }
            expect = match s.end.checked_add(1) {
                Some(e) => e,
                None => return bad(format!("segment {} end overflows", s.idx)),
            };
        }
        if expect != r.size {
            return bad(format!(
                "segments end at byte {}, the file has {} bytes",
                expect.saturating_sub(1),
                r.size
            ));
        }
    }
    let len = std::fs::metadata(part_path)
        .map_err(EngineError::from_io)?
        .len();
    if len != r.size {
        return bad(format!("part file is {len} bytes, the file has {}", r.size));
    }
    Ok(())
}

struct Run {
    engine: Engine,
    url: Url,
    extras: RequestExtras,
    file: Arc<PartFile>,
    segments: Mutex<Vec<Arc<SegmentRuntime>>>,
    /// The snapshot taken just before the last completed fsync.
    durable: Mutex<Vec<SegmentState>>,
    ranged: bool,
    total: Option<u64>,
    cancel: CancellationToken,
    stop: Arc<AtomicU8>,
    // Never given its own accessor: it exists only to release the live-part
    // claim (`PartClaim::drop`) when `Run` is dropped at the end of `run()`.
    // `Run` itself must stay free of a `Drop` impl (see `run()`'s partial
    // move of `self.file`), and a field whose type implements `Drop` does
    // not force that on the containing struct.
    _claim: PartClaim,
}

impl Run {
    fn job(&self, seg: Arc<SegmentRuntime>) -> SegmentJob {
        SegmentJob {
            client: self.engine.client.clone(),
            url: self.url.clone(),
            extras: self.extras.clone(),
            file: self.file.clone(),
            seg,
            ranged: self.ranged,
            cancel: self.cancel.clone(),
            retry_base_delay: self.engine.cfg.retry_base_delay,
            stall_timeout: self.engine.cfg.stall_timeout,
        }
    }

    fn snapshot(&self) -> Vec<SegmentState> {
        let mut v: Vec<SegmentState> = self
            .segments
            .lock()
            .unwrap()
            .iter()
            .map(|s| s.snapshot())
            .collect();
        v.sort_by_key(|s| s.idx);
        v
    }

    fn progress(&self, speed_bps: u64, status: Status) -> Progress {
        let segments = self.snapshot();
        let downloaded = segments.iter().map(|s| s.downloaded).sum();
        let eta_secs = match (self.total, speed_bps) {
            (Some(t), _) if t <= downloaded => Some(0),
            (Some(t), s) if s > 0 => Some((t - downloaded).div_ceil(s)),
            _ => None,
        };
        Progress {
            total: self.total,
            downloaded,
            speed_bps,
            eta_secs,
            segments,
            durable_segments: self.durable.lock().unwrap().clone(),
            status,
        }
    }

    /// fsync the part file off the async threads, then record `snap` as durable.
    async fn sync_to(&self, snap: Vec<SegmentState>) {
        let file = self.file.clone();
        match tokio::task::spawn_blocking(move || file.sync()).await {
            Ok(Ok(())) => *self.durable.lock().unwrap() = snap,
            Ok(Err(e)) => tracing::warn!(error = %e, "sync of the part file failed"),
            Err(e) => tracing::warn!(error = %e, "sync task failed"),
        }
    }

    /// Split the largest remaining range and return a job for its second half.
    fn steal(&self) -> Option<SegmentJob> {
        if !self.ranged {
            return None;
        }
        let mut segs = self.segments.lock().unwrap();
        let victim = segs
            .iter()
            .filter(|s| s.remaining().unwrap_or(0) > STEAL_MIN_REMAINING)
            .max_by_key(|s| s.remaining().unwrap_or(0))?
            .clone();
        let next = victim.next_offset();
        let end = victim.end.load(Ordering::SeqCst);
        let remaining = (end + 1).saturating_sub(next);
        if remaining <= STEAL_MIN_REMAINING {
            return None;
        }
        let mid = next + remaining / 2;
        victim.end.store(mid - 1, Ordering::SeqCst);
        let idx = segs.iter().map(|s| s.idx).max().map_or(0, |m| m + 1);
        let fresh = Arc::new(SegmentRuntime::new(idx, mid, Some(end), 0));
        segs.push(fresh.clone());
        tracing::debug!(victim = victim.idx, new = idx, mid, end, "work stolen");
        drop(segs);
        Some(self.job(fresh))
    }

    async fn run(self, tx: watch::Sender<Progress>) -> Outcome {
        let mut set: JoinSet<Result<(), EngineError>> = JoinSet::new();
        for seg in self.segments.lock().unwrap().iter().cloned() {
            set.spawn(fetch_segment(self.job(seg)));
        }
        let mut ticker = tokio::time::interval(PROGRESS_INTERVAL);
        let mut meter = SpeedMeter::default();
        let mut failure: Option<EngineError> = None;
        let mut last_sync = Instant::now();
        // Set when any worker returns `Ok(())`; only meaningful for the
        // single-stream-of-unknown-length case below, where it is the only
        // sign the download is actually complete.
        let mut worker_ok = false;
        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    let p = self.progress(0, Status::Downloading);
                    let speed = meter.push(p.downloaded);
                    tx.send_replace(Progress { speed_bps: speed, ..p });
                    if last_sync.elapsed() >= SYNC_INTERVAL {
                        // The snapshot is taken BEFORE the fsync: every byte
                        // it counts was written before it, so the sync covers
                        // it. The `Arc<PartFile>` clone lives only inside
                        // `sync_to`, so `Arc::try_unwrap` below still works.
                        let snap = self.snapshot();
                        self.sync_to(snap).await;
                        last_sync = Instant::now();
                    }
                }
                res = set.join_next() => match res {
                    None => break,
                    Some(Ok(Ok(()))) => {
                        worker_ok = true;
                        if failure.is_none() && !self.cancel.is_cancelled() {
                            if let Some(job) = self.steal() {
                                set.spawn(fetch_segment(job));
                            }
                        }
                    }
                    Some(Ok(Err(EngineError::Cancelled))) => {}
                    Some(Ok(Err(e))) => {
                        if failure.is_none() {
                            failure = Some(e);
                            self.cancel.cancel();
                        }
                    }
                    Some(Err(_)) => {
                        if failure.is_none() {
                            failure = Some(EngineError::Internal("worker panicked".into()));
                            self.cancel.cancel();
                        }
                    }
                }
            }
        }

        // Every worker has ended, so the segments no longer move. A run that
        // is not heading for completion syncs once more, so a Paused / Failed
        // outcome is durable; this happens while `self` is still whole.
        let stop = self.stop.load(Ordering::SeqCst);
        // A single stream of unknown length has no `end` to reach: its
        // worker returning Ok means the server ended the body normally.
        let all_done = if !self.ranged && self.total.is_none() {
            failure.is_none() && worker_ok
        } else {
            self.snapshot().iter().all(|s| s.is_done())
        };
        if failure.is_some() || stop != STOP_NONE || !all_done {
            self.sync_to(self.snapshot()).await;
        }

        // Everything the final `Progress` needs is read from `self` HERE, while
        // `self` is still whole: taking the part file out below is a partial
        // move, after which no `&self` method may be called. `Run` must stay
        // free of a `Drop` impl for that move to be allowed.
        let segments = self.snapshot();
        let downloaded: u64 = segments.iter().map(|s| s.downloaded).sum();
        let total = self.total;
        let durable = self.durable.lock().unwrap().clone();
        let file = self.file;

        // Every outcome but Completed carries the DURABLE snapshot: the caller
        // persists it, and after a failed final sync or `finish()` the live
        // segments would claim bytes that never reached the disk. After a
        // successful final sync the two are equal.
        let outcome = if let Some(error) = failure {
            Outcome::Failed {
                error,
                segments: durable.clone(),
            }
        } else if stop == STOP_CANCEL {
            Outcome::Cancelled
        } else if all_done {
            match Arc::try_unwrap(file) {
                Ok(file) => match file.finish() {
                    Ok(path) => Outcome::Completed(path),
                    Err(error) => Outcome::Failed {
                        error,
                        segments: durable.clone(),
                    },
                },
                Err(_) => Outcome::Failed {
                    error: EngineError::Internal("part file still in use".into()),
                    segments: durable.clone(),
                },
            }
        } else if stop == STOP_PAUSE {
            Outcome::Paused(durable.clone())
        } else {
            Outcome::Failed {
                error: EngineError::Internal("workers ended with bytes missing".into()),
                segments: durable.clone(),
            }
        };
        let status = match &outcome {
            Outcome::Completed(_) => Status::Completed,
            Outcome::Paused(_) => Status::Paused,
            Outcome::Failed { .. } => Status::Failed,
            Outcome::Cancelled => Status::Cancelled,
        };
        // `finish()` fsyncs before the rename: a completed file is durable whole.
        let durable_segments = match &outcome {
            Outcome::Completed(_) => segments.clone(),
            _ => durable,
        };
        tx.send_replace(Progress {
            total,
            downloaded,
            speed_bps: 0,
            // Nothing is being fetched any more: the only honest estimate is
            // "no time left", and only when every byte is in.
            eta_secs: match total {
                Some(t) if downloaded >= t => Some(0),
                _ => None,
            },
            segments,
            durable_segments,
            status,
        });
        outcome
    }
}

#[derive(Default)]
struct SpeedMeter {
    samples: VecDeque<(Instant, u64)>,
}

impl SpeedMeter {
    /// Record `downloaded` now; return bytes/s over the window.
    fn push(&mut self, downloaded: u64) -> u64 {
        let now = Instant::now();
        self.samples.push_back((now, downloaded));
        while let Some(&(t, _)) = self.samples.front() {
            if now.duration_since(t) > SPEED_WINDOW && self.samples.len() > 2 {
                self.samples.pop_front();
            } else {
                break;
            }
        }
        let (t0, b0) = *self.samples.front().unwrap();
        let dt = now.duration_since(t0).as_secs_f64();
        if dt < 0.05 {
            return 0;
        }
        ((downloaded.saturating_sub(b0)) as f64 / dt) as u64
    }
}
