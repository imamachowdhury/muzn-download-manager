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

/// How strongly an intent wins over another that arrives later or earlier:
/// REMOVE beats CANCEL beats SHUTDOWN beats PAUSE beats NONE. A signal only
/// ever raises a slot's intent to a higher rank, never lowers it, so an
/// earlier REMOVE cannot be undone by a later PAUSE.
fn rank(intent: u8) -> u8 {
    match intent {
        INTENT_REMOVE => 4,
        INTENT_CANCEL => 3,
        INTENT_SHUTDOWN => 2,
        INTENT_PAUSE => 1,
        _ => 0,
    }
}

/// The control surface of one running download.
#[derive(Clone)]
pub(crate) struct Slot {
    intent: Arc<AtomicU8>,
    delete_file: Arc<AtomicBool>,
    /// Set by a `resume()` that arrived while this slot was still running: a
    /// later intent (PAUSE/SHUTDOWN/CANCEL/REMOVE) cancels the pending resume
    /// again, since it is asking for something else entirely.
    resume_after: Arc<AtomicBool>,
    /// The intent `settle` (or the Completed path) actually acted on, so the
    /// driver's end can tell a "late" signal — one that outranks this — from
    /// one it already handled.
    consumed: Arc<AtomicU8>,
    token: CancellationToken,
    control: Arc<Mutex<Option<DownloadControl>>>,
}

impl Slot {
    fn new() -> Self {
        Self {
            intent: Arc::new(AtomicU8::new(INTENT_NONE)),
            delete_file: Arc::new(AtomicBool::new(false)),
            resume_after: Arc::new(AtomicBool::new(false)),
            consumed: Arc::new(AtomicU8::new(INTENT_NONE)),
            token: CancellationToken::new(),
            control: Arc::new(Mutex::new(None)),
        }
    }

    /// Raise the intent only if it outranks whatever is already there —
    /// intents rank, they never overwrite a stronger one that arrived first.
    /// When it does raise: clear a pending resume (this is asking for
    /// something else), wake a probe waiting on the token, and stop an
    /// engine control if one is attached, according to the intent that just
    /// won: PAUSE/SHUTDOWN pause it, everything else (CANCEL, REMOVE)
    /// cancels it. When it does not raise, nothing here is touched.
    fn signal(&self, intent: u8) {
        let raised = self
            .intent
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |cur| {
                (rank(intent) > rank(cur)).then_some(intent)
            })
            .is_ok();
        if !raised {
            return;
        }
        self.resume_after.store(false, Ordering::SeqCst);
        self.token.cancel();
        if let Some(control) = self.control.lock().unwrap().as_ref() {
            if intent == INTENT_PAUSE || intent == INTENT_SHUTDOWN {
                control.pause();
            } else {
                control.cancel();
            }
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

    /// Pause: a running download stops and saves its state; a queued one simply waits.
    ///
    /// Holds the `running` lock for the whole call — either to signal the
    /// slot, or, if the row is not running, across the store write — so
    /// `schedule` (which needs the same lock to start a row) can never start
    /// this one mid-action.
    pub fn pause(&self, id: &DownloadId) -> Result<()> {
        let running = self.inner.running.lock().unwrap();
        if let Some(slot) = running.get(id) {
            slot.signal(INTENT_PAUSE);
            return Ok(());
        }
        let row = self.inner.require(id)?;
        if row.status == DownloadStatus::Queued {
            self.inner.set_status(id, DownloadStatus::Paused, None)?;
        }
        Ok(())
    }

    /// Resume a paused, failed or cancelled download (a cancelled one starts from zero).
    /// If it is still running, remember the request instead: a later
    /// PAUSE/SHUTDOWN/CANCEL/REMOVE clears it, otherwise the driver queues
    /// the row again once it stops.
    pub fn resume(&self, id: &DownloadId) -> Result<()> {
        let running = self.inner.running.lock().unwrap();
        if let Some(slot) = running.get(id) {
            slot.resume_after.store(true, Ordering::SeqCst);
            return Ok(());
        }
        let row = self.inner.require(id)?;
        match row.status {
            DownloadStatus::Paused | DownloadStatus::Failed | DownloadStatus::Cancelled => {
                self.inner.set_status(id, DownloadStatus::Queued, None)?;
            }
            DownloadStatus::Queued => return Ok(()),
            s => {
                return Err(CoreError::InvalidState(format!(
                    "cannot resume a {} download",
                    s.as_str()
                )))
            }
        }
        drop(running);
        Inner::schedule(&self.inner);
        Ok(())
    }

    /// Cancel: stop and delete the partial data.
    pub fn cancel(&self, id: &DownloadId) -> Result<()> {
        let running = self.inner.running.lock().unwrap();
        if let Some(slot) = running.get(id) {
            slot.signal(INTENT_CANCEL);
            return Ok(());
        }
        let row = self.inner.require(id)?;
        match row.status {
            DownloadStatus::Completed => {
                Err(CoreError::InvalidState("the download is complete".into()))
            }
            DownloadStatus::Cancelled => Ok(()),
            _ => {
                self.inner.discard_partial(&row)?;
                self.inner.set_status(id, DownloadStatus::Cancelled, None)
            }
        }
    }

    /// Remove from the list; `delete_file` also deletes a finished file.
    pub fn remove(&self, id: &DownloadId, delete_file: bool) -> Result<()> {
        let running = self.inner.running.lock().unwrap();
        if let Some(slot) = running.get(id) {
            slot.delete_file.store(delete_file, Ordering::SeqCst);
            slot.signal(INTENT_REMOVE);
            return Ok(());
        }
        let row = self.inner.require(id)?;
        self.inner.remove_now(&row, delete_file)
    }

    /// Start over from byte 0 (after SOURCE_CHANGED, for example).
    pub fn restart(&self, id: &DownloadId) -> Result<()> {
        let running = self.inner.running.lock().unwrap();
        if running.contains_key(id) {
            return Err(CoreError::InvalidState(
                "pause the download before restarting it".into(),
            ));
        }
        let row = self.inner.require(id)?;
        if row.status == DownloadStatus::Completed {
            return Err(CoreError::InvalidState("the download is complete".into()));
        }
        self.inner.discard_partial(&row)?;
        self.inner.set_status(id, DownloadStatus::Queued, None)?;
        drop(running);
        Inner::schedule(&self.inner);
        Ok(())
    }

    /// Pause everything queued or running. A row removed by someone else
    /// mid-loop (NOT_FOUND) is skipped; any other error stops the sweep.
    pub fn pause_all(&self) -> Result<()> {
        for row in self.list()? {
            if matches!(
                row.status,
                DownloadStatus::Queued | DownloadStatus::Probing | DownloadStatus::Downloading
            ) {
                match self.pause(&row.id) {
                    Ok(()) => {}
                    Err(CoreError::NotFound(_)) => {}
                    Err(e) => return Err(e),
                }
            }
        }
        Ok(())
    }

    /// Resume everything paused. Same NOT_FOUND tolerance as `pause_all`.
    pub fn resume_all(&self) -> Result<()> {
        for row in self.list()? {
            if row.status == DownloadStatus::Paused {
                match self.resume(&row.id) {
                    Ok(()) => {}
                    Err(CoreError::NotFound(_)) => {}
                    Err(e) => return Err(e),
                }
            }
        }
        Ok(())
    }

    /// Before the app exits: stop scheduling, pause everything running, wait
    /// until each has saved its state. Those rows are QUEUED again, so they
    /// continue at the next launch.
    ///
    /// `closing` is set and every slot signalled while still holding
    /// `running`, so a `schedule` racing this call either sees `closing`
    /// first and does nothing, or has already started a row that this same
    /// lock will then signal — never a row started after we stopped looking.
    pub async fn shutdown(&self) {
        let tasks = {
            let running = self.inner.running.lock().unwrap();
            self.inner.closing.store(true, Ordering::SeqCst);
            for slot in running.values() {
                slot.signal(INTENT_SHUTDOWN);
            }
            std::mem::take(&mut *self.inner.tasks.lock().unwrap())
        };
        for t in tasks {
            let _ = t.await;
        }
    }

    /// Tests only: abort every driver the way a crash would. The engine's
    /// forwarder may still save one last durable snapshot.
    #[doc(hidden)]
    pub fn simulate_crash(&self) {
        self.inner.closing.store(true, Ordering::SeqCst);
        for t in self.inner.tasks.lock().unwrap().drain(..) {
            t.abort();
        }
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

    /// Finish a download that stopped because it was asked to. Records the
    /// intent it acted on into `slot.consumed`, so the driver's end can tell
    /// a signal that arrived after this from one already handled here.
    fn settle(&self, id: &DownloadId, slot: &Slot) -> Result<()> {
        let row = self.require(id)?;
        let intent = slot.intent.load(Ordering::SeqCst);
        let result = match intent {
            INTENT_CANCEL => {
                self.discard_partial(&row)?;
                self.set_status(id, DownloadStatus::Cancelled, None)
            }
            INTENT_REMOVE => self.remove_now(&row, slot.delete_file.load(Ordering::SeqCst)),
            INTENT_SHUTDOWN => self.set_status(id, DownloadStatus::Queued, None),
            _ => self.set_status(id, DownloadStatus::Paused, None),
        };
        slot.consumed.store(intent, Ordering::SeqCst);
        result
    }

    /// After the driver stops, however it stopped: under the `running` lock,
    /// apply any signal that outranks what was already consumed (a "late"
    /// signal — one that arrived after `settle` or the Completed path had
    /// already decided the outcome), apply a pending `resume`, then drop the
    /// slot. Holding the lock for all of this is what makes it safe: nothing
    /// else can act on this row (or start a new one) while it runs.
    fn finish(&self, id: &DownloadId, slot: &Slot) {
        let mut running = self.running.lock().unwrap();
        let final_intent = slot.intent.load(Ordering::SeqCst);
        let consumed = slot.consumed.load(Ordering::SeqCst);
        if rank(final_intent) > rank(consumed) {
            if let Ok(Some(row)) = self.store.get(id) {
                let _ = match final_intent {
                    INTENT_REMOVE => self.remove_now(&row, slot.delete_file.load(Ordering::SeqCst)),
                    INTENT_CANCEL if row.status != DownloadStatus::Completed => self
                        .discard_partial(&row)
                        .and_then(|()| self.set_status(id, DownloadStatus::Cancelled, None)),
                    INTENT_SHUTDOWN if row.status == DownloadStatus::Paused => {
                        self.set_status(id, DownloadStatus::Queued, None)
                    }
                    // CANCEL on an already-Completed row, PAUSE, or SHUTDOWN
                    // on anything but Paused: nothing more to do.
                    _ => Ok(()),
                };
            }
        }
        if slot.resume_after.load(Ordering::SeqCst) {
            if let Ok(Some(row)) = self.store.get(id) {
                if matches!(
                    row.status,
                    DownloadStatus::Paused | DownloadStatus::Failed | DownloadStatus::Cancelled
                ) {
                    let _ = self.set_status(id, DownloadStatus::Queued, None);
                }
            }
        }
        running.remove(id);
    }

    /// Start queued downloads while there are free slots.
    pub(crate) fn schedule(this: &Arc<Inner>) {
        let max = this.settings.lock().unwrap().max_parallel as usize;
        loop {
            let mut running = this.running.lock().unwrap();
            if this.closing.load(Ordering::SeqCst) || running.len() >= max {
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
    inner.finish(&id, &slot);
    Inner::schedule(&inner);
}

async fn drive_inner(inner: &Arc<Inner>, id: &DownloadId, slot: &Slot) -> Result<()> {
    let mut started_over = false;
    let mut first_pass = true;
    loop {
        let Some(row) = inner.store.get(id)? else {
            return Ok(());
        };
        // Belt and braces: `schedule` only ever starts a QUEUED row while
        // holding the same lock a concurrent `pause` needs, so this should
        // never actually see anything else here — but if it ever does, do
        // not touch a row a pause (or worse) has already claimed.
        if first_pass {
            first_pass = false;
            if row.status != DownloadStatus::Queued {
                return Ok(());
            }
        }
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
                    slot.consumed.store(INTENT_REMOVE, Ordering::SeqCst);
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
