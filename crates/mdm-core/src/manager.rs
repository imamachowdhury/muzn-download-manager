//! The manager: a FIFO queue with at most `max_parallel` running downloads,
//! each driven through the engine, persisted to the store, announced as events.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mdm_engine::{
    Cookie, DownloadControl, DownloadSpec, Engine, EngineError, Outcome, Progress, RequestExtras,
    Resume, SegmentState, Status, PART_SUFFIX,
};
use tokio::sync::{broadcast, watch};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::error::{CoreError, Result};
use crate::events::{ManagerEvent, SegmentView};
use crate::model::*;
use crate::settings::Settings;
use crate::store::Store;

/// How often a running download's durable segments are saved.
pub const PERSIST_INTERVAL: Duration = Duration::from_secs(1);
const EVENT_CAPACITY: usize = 1024;

pub(crate) const INTENT_NONE: u8 = 0;
pub(crate) const INTENT_PAUSE: u8 = 1;
pub(crate) const INTENT_CANCEL: u8 = 2;
pub(crate) const INTENT_REMOVE: u8 = 3;
/// Paused because the app is closing: the row goes back to QUEUED so the
/// download continues at the next launch.
pub(crate) const INTENT_SHUTDOWN: u8 = 4;

/// The control surface of one running download.
#[derive(Clone)]
pub(crate) struct Slot {
    intent: Arc<AtomicU8>,
    delete_file: Arc<AtomicBool>,
    token: CancellationToken,
    control: Arc<Mutex<Option<DownloadControl>>>,
}

impl Slot {
    fn new() -> Self {
        Self {
            intent: Arc::new(AtomicU8::new(INTENT_NONE)),
            delete_file: Arc::new(AtomicBool::new(false)),
            token: CancellationToken::new(),
            control: Arc::new(Mutex::new(None)),
        }
    }
}

pub(crate) struct Inner {
    pub(crate) store: Store,
    engine: Mutex<Engine>,
    settings: Mutex<Settings>,
    pub(crate) running: Mutex<HashMap<DownloadId, Slot>>,
    pub(crate) tasks: Mutex<Vec<JoinHandle<()>>>,
    pub(crate) closing: AtomicBool,
    events: broadcast::Sender<ManagerEvent>,
}

/// The download manager. Cheap to clone; clones share one queue.
#[derive(Clone)]
pub struct Manager {
    pub(crate) inner: Arc<Inner>,
}

impl Manager {
    /// Open the database, put downloads interrupted by a crash or a quit back
    /// in the queue, and start it. Call inside a tokio runtime.
    pub fn open(db: &Path, default_download_dir: &Path) -> Result<Manager> {
        Self::with_store(Store::open(db)?, default_download_dir)
    }

    /// The same over an already opened store.
    pub fn with_store(store: Store, default_download_dir: &Path) -> Result<Manager> {
        let settings = store.load_settings(default_download_dir)?.validated()?;
        let engine = Engine::new(settings.engine_config()?)?;
        store.reset_interrupted(now_ms())?;
        let (events, _) = broadcast::channel(EVENT_CAPACITY);
        let inner = Arc::new(Inner {
            store,
            engine: Mutex::new(engine),
            settings: Mutex::new(settings),
            running: Mutex::default(),
            tasks: Mutex::default(),
            closing: AtomicBool::new(false),
            events,
        });
        Inner::schedule(&inner);
        Ok(Manager { inner })
    }

    /// Every event from now on. A slow receiver that falls 1 024 events behind
    /// gets `RecvError::Lagged` and should reload with `list()`.
    pub fn subscribe(&self) -> broadcast::Receiver<ManagerEvent> {
        self.inner.events.subscribe()
    }

    /// Every download, newest first.
    pub fn list(&self) -> Result<Vec<DownloadRow>> {
        self.inner.store.list()
    }

    /// One download.
    pub fn get(&self, id: &DownloadId) -> Result<Option<DownloadRow>> {
        self.inner.store.get(id)
    }

    /// The saved (durable) segments of a download.
    pub fn segments(&self, id: &DownloadId) -> Result<Vec<SegmentState>> {
        self.inner.store.load_segments(id)
    }

    /// Current settings.
    pub fn settings(&self) -> Settings {
        self.inner.settings.lock().unwrap().clone()
    }

    /// Validate, save and apply settings. Running downloads keep their engine;
    /// new ones use the new settings. More parallel slots start queued rows at once.
    /// The new engine shares the old one's live part claims. Call inside a
    /// tokio runtime (it may start downloads).
    pub fn set_settings(&self, s: Settings) -> Result<Settings> {
        let s = s.validated()?;
        let current = self.inner.engine.lock().unwrap().clone();
        let engine = current.reconfigured(s.engine_config()?)?;
        self.inner.store.save_settings(&s)?;
        *self.inner.engine.lock().unwrap() = engine;
        *self.inner.settings.lock().unwrap() = s.clone();
        Inner::schedule(&self.inner);
        Ok(s)
    }

    /// Add a download (QUEUED, or PAUSED with `start_paused`). Call inside a
    /// tokio runtime (it may start downloads).
    pub fn add(&self, mut new: NewDownload) -> Result<DownloadRow> {
        let url = Url::parse(new.url.trim())
            .map_err(|e| CoreError::InvalidUrl(format!("{}: {e}", new.url)))?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(CoreError::InvalidUrl(format!(
                "unsupported scheme {}",
                url.scheme()
            )));
        }
        new.url = url.to_string();
        let dir = new
            .dir
            .clone()
            .unwrap_or_else(|| self.inner.settings.lock().unwrap().download_dir.clone());
        let status = if new.start_paused {
            DownloadStatus::Paused
        } else {
            DownloadStatus::Queued
        };
        let row = self
            .inner
            .store
            .insert(&DownloadId::new(), &dir, &new, status, now_ms())?;
        self.inner.emit(ManagerEvent::Added {
            download: row.clone(),
        });
        Inner::schedule(&self.inner);
        Ok(row)
    }
}

impl Inner {
    pub(crate) fn emit(&self, e: ManagerEvent) {
        let _ = self.events.send(e);
    }

    pub(crate) fn require(&self, id: &DownloadId) -> Result<DownloadRow> {
        self.store
            .get(id)?
            .ok_or_else(|| CoreError::NotFound(id.to_string()))
    }

    pub(crate) fn set_status(
        &self,
        id: &DownloadId,
        status: DownloadStatus,
        error: Option<(&str, &str)>,
    ) -> Result<()> {
        self.store.set_status(id, status, error, now_ms())?;
        if let Some(download) = self.store.get(id)? {
            self.emit(ManagerEvent::Updated { download });
        }
        Ok(())
    }

    fn fail(&self, id: &DownloadId, e: &EngineError) -> Result<()> {
        self.set_status(id, DownloadStatus::Failed, Some((e.code(), &e.to_string())))
    }

    fn notice(&self, id: &DownloadId, message: String) {
        self.emit(ManagerEvent::Notice {
            id: id.clone(),
            message,
        });
    }

    fn part_path(row: &DownloadRow) -> Option<PathBuf> {
        row.filename
            .as_ref()
            .map(|f| row.dir.join(format!("{f}{PART_SUFFIX}")))
    }

    /// The part files of every OTHER row in `row`'s folder that may still
    /// resume (not completed, not cancelled) and has a name: a fresh start
    /// must neither take nor delete them (`DownloadSpec::reserved`).
    fn reserved_parts(&self, row: &DownloadRow) -> Result<Vec<PathBuf>> {
        Ok(self
            .store
            .list()?
            .iter()
            .filter(|r| r.id != row.id && r.dir == row.dir)
            .filter(|r| {
                !matches!(
                    r.status,
                    DownloadStatus::Completed | DownloadStatus::Cancelled
                )
            })
            .filter_map(Self::part_path)
            .collect())
    }

    /// Delete the part file (if any) and the saved segments.
    pub(crate) fn discard_partial(&self, row: &DownloadRow) -> Result<()> {
        if let Some(p) = Self::part_path(row) {
            remove_if_present(&p)?;
        }
        self.store.clear_segments(&row.id)
    }

    /// Delete the row now (not running): part file always, the finished file on request.
    pub(crate) fn remove_now(&self, row: &DownloadRow, delete_file: bool) -> Result<()> {
        self.discard_partial(row)?;
        if delete_file && row.status == DownloadStatus::Completed {
            if let Some(f) = &row.filename {
                remove_if_present(&row.dir.join(f))?;
            }
        }
        self.store.delete(&row.id)?;
        self.emit(ManagerEvent::Removed { id: row.id.clone() });
        Ok(())
    }

    /// Finish a download that stopped because it was asked to.
    fn settle(&self, id: &DownloadId, slot: &Slot) -> Result<()> {
        let row = self.require(id)?;
        match slot.intent.load(Ordering::SeqCst) {
            INTENT_CANCEL => {
                self.discard_partial(&row)?;
                self.set_status(id, DownloadStatus::Cancelled, None)
            }
            INTENT_REMOVE => self.remove_now(&row, slot.delete_file.load(Ordering::SeqCst)),
            INTENT_SHUTDOWN => self.set_status(id, DownloadStatus::Queued, None),
            _ => self.set_status(id, DownloadStatus::Paused, None),
        }
    }

    /// Start queued downloads while there are free slots.
    pub(crate) fn schedule(this: &Arc<Inner>) {
        if this.closing.load(Ordering::SeqCst) {
            return;
        }
        let max = this.settings.lock().unwrap().max_parallel as usize;
        loop {
            let mut running = this.running.lock().unwrap();
            if running.len() >= max {
                return;
            }
            let exclude: Vec<DownloadId> = running.keys().cloned().collect();
            let next = match this.store.next_queued(&exclude) {
                Ok(n) => n,
                Err(e) => {
                    tracing::error!(error = %e, "reading the queue failed");
                    return;
                }
            };
            let Some(id) = next else { return };
            let slot = Slot::new();
            running.insert(id.clone(), slot.clone());
            drop(running);
            let task = tokio::spawn(drive(this.clone(), id, slot));
            let mut tasks = this.tasks.lock().unwrap();
            tasks.retain(|t| !t.is_finished());
            tasks.push(task);
        }
    }
}

fn remove_if_present(p: &Path) -> Result<()> {
    match std::fs::remove_file(p) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

fn extras_of(row: &DownloadRow) -> RequestExtras {
    let mut headers: Vec<(String, String)> = row
        .headers
        .iter()
        .map(|h| (h.name.clone(), h.value.clone()))
        .collect();
    if let Some(r) = &row.referrer {
        headers.push(("Referer".into(), r.clone()));
    }
    RequestExtras {
        headers,
        cookies: row
            .cookies
            .iter()
            .map(|c| Cookie {
                name: c.name.clone(),
                value: c.value.clone(),
            })
            .collect(),
    }
}

/// The saved state no longer fits the server: start over from byte 0 (once).
fn needs_fresh_start(e: &EngineError) -> bool {
    matches!(
        e,
        EngineError::RangeNotSupported | EngineError::InvalidResume(_)
    ) || matches!(e, EngineError::Io(io) if io.kind() == std::io::ErrorKind::NotFound)
}

async fn drive(inner: Arc<Inner>, id: DownloadId, slot: Slot) {
    if let Err(e) = drive_inner(&inner, &id, &slot).await {
        tracing::error!(%id, error = %e, "driving a download failed");
        let _ = inner.set_status(
            &id,
            DownloadStatus::Failed,
            Some((e.code(), &e.to_string())),
        );
    }
    inner.running.lock().unwrap().remove(&id);
    Inner::schedule(&inner);
}

async fn drive_inner(inner: &Arc<Inner>, id: &DownloadId, slot: &Slot) -> Result<()> {
    let mut started_over = false;
    loop {
        let Some(row) = inner.store.get(id)? else {
            return Ok(());
        };
        let segments = inner.store.load_segments(id)?;
        let resume = match (row.size, segments.is_empty()) {
            (Some(size), false) => Some(Resume {
                segments,
                size,
                etag: row.etag.clone(),
                last_modified: row.last_modified.clone(),
            }),
            _ => None,
        };
        let resuming = resume.is_some();
        inner.set_status(id, DownloadStatus::Probing, None)?;
        let spec = DownloadSpec {
            url: Url::parse(&row.url).map_err(|e| CoreError::InvalidUrl(e.to_string()))?,
            dir: row.dir.clone(),
            filename: row.filename.clone(),
            extras: extras_of(&row),
            resume_from: resume,
            reserved: inner.reserved_parts(&row)?,
        };
        let engine = inner.engine.lock().unwrap().clone();
        let started = tokio::select! {
            r = engine.start(spec) => r,
            _ = slot.token.cancelled() => return inner.settle(id, slot),
        };
        let handle = match started {
            Ok(h) => h,
            Err(_) if slot.token.is_cancelled() => return inner.settle(id, slot),
            Err(e) if resuming && !started_over && needs_fresh_start(&e) => {
                started_over = true;
                inner.discard_partial(&row)?;
                inner.notice(id, format!("Starting over from the beginning: {e}"));
                continue;
            }
            Err(e) => return inner.fail(id, &e),
        };

        let p = handle.probe();
        let info = ProbeInfo {
            final_url: p.final_url.to_string(),
            filename: handle.filename().to_owned(),
            size: p.size,
            etag: p.etag.clone(),
            last_modified: p.last_modified.clone(),
            mime: p.mime.clone(),
        };
        inner.store.set_probe(id, &info, now_ms())?;
        inner.set_status(id, DownloadStatus::Downloading, None)?;
        *slot.control.lock().unwrap() = Some(handle.control());
        // A pause / cancel that arrived while the probe was running.
        match slot.intent.load(Ordering::SeqCst) {
            INTENT_NONE => {}
            INTENT_PAUSE | INTENT_SHUTDOWN => handle.pause(),
            _ => handle.cancel(),
        }
        let forwarder = tokio::spawn(forward_progress(
            inner.clone(),
            id.clone(),
            handle.subscribe(),
        ));
        let outcome = handle.wait().await;
        let _ = forwarder.await;
        *slot.control.lock().unwrap() = None;

        match outcome {
            Outcome::Completed(path) => {
                inner.store.clear_segments(id)?;
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    inner.store.set_filename(id, name, now_ms())?;
                }
                if info.size.is_none() {
                    let len = std::fs::metadata(&path)?.len();
                    inner.store.set_size(id, len, now_ms())?;
                }
                inner.set_status(id, DownloadStatus::Completed, None)?;
                if slot.intent.load(Ordering::SeqCst) == INTENT_REMOVE {
                    let row = inner.require(id)?;
                    inner.remove_now(&row, slot.delete_file.load(Ordering::SeqCst))?;
                }
                return Ok(());
            }
            Outcome::Paused(segs) => inner.store.save_segments(id, &segs)?,
            Outcome::Cancelled => {}
            Outcome::Failed { error, segments } => {
                inner.store.save_segments(id, &segments)?;
                if slot.intent.load(Ordering::SeqCst) == INTENT_NONE {
                    if matches!(error, EngineError::RangeNotSupported) && !started_over {
                        started_over = true;
                        let row = inner.require(id)?;
                        inner.discard_partial(&row)?;
                        inner.notice(
                            id,
                            "The server stopped accepting ranged requests; starting over as one stream"
                                .into(),
                        );
                        continue;
                    }
                    return inner.fail(id, &error);
                }
            }
        }
        return inner.settle(id, slot);
    }
}

async fn forward_progress(inner: Arc<Inner>, id: DownloadId, mut rx: watch::Receiver<Progress>) {
    let mut last_save = Instant::now();
    while rx.changed().await.is_ok() {
        let p = rx.borrow_and_update().clone();
        inner.emit(ManagerEvent::Progress {
            id: id.clone(),
            total: p.total,
            downloaded: p.downloaded,
            speed_bps: p.speed_bps,
            eta_secs: p.eta_secs,
            segments: p.segments.iter().map(SegmentView::from).collect(),
        });
        if p.status == Status::Downloading && last_save.elapsed() >= PERSIST_INTERVAL {
            if let Err(e) = inner.store.save_segments(&id, &p.durable_segments) {
                tracing::warn!(%id, error = %e, "saving progress failed");
            }
            last_save = Instant::now();
        }
    }
}
