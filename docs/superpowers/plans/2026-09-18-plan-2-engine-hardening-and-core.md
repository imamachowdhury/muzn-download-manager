# Plan 2 — Engine hardening and the download core (`mdm-core`)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the engine gaps Plan 1's reviews deferred (including the owner's retry-reset decision), then build `crates/mdm-core` — the SQLite store and the download manager (queue, pause / resume / cancel / remove, crash recovery, progress events) that the desktop app in Plan 3 will wrap with Tauri commands.

**Architecture:** The engine keeps its shape; a new `DownloadControl` lets a caller pause or cancel while another task awaits `wait()`, progress carries a *durable* segment snapshot taken at the last `sync_data`, and all disk I/O leaves the async threads. The in-process test server moves out of `mdm-engine/tests/support` into its own dev crate `crates/mdm-test-server` so `mdm-core` can test against it too. `mdm-core` owns a `Store` (rusqlite, bundled SQLite, schema versioned by `PRAGMA user_version`) and a `Manager` that schedules at most `max_parallel` downloads FIFO, drives each through `Engine::start`, persists the durable snapshot every second, recovers interrupted downloads at startup, and broadcasts `ManagerEvent`s. No Tauri, no UI in this plan.

**Tech Stack:** Rust stable, tokio, reqwest (existing), rusqlite 0.32 (`bundled`), uuid 1 (`v4`), serde + serde_json, thiserror, tracing; dev: axum 0.7, tempfile, sha2 (via `mdm-test-server`).

**Spec:** `docs/superpowers/specs/2026-09-18-muzn-download-manager-design.md` (§2 storage, §3 retry/durability, §8 phase 4 — the store/queue half). Backlog this plan draws from: `docs/superpowers/plans/plan-2-engine-backlog.md`.

## Global Constraints

- Product name in user-facing strings: **Muzn Download Manager**; identifiers `mdm`. MIT. English only.
- `mdm-engine` and `mdm-core` never depend on Tauri or any UI crate. `mdm-test-server` is `publish = false` and only ever a dev-dependency.
- reqwest stays `default-features = false` with `rustls-tls`, `http2`, `stream`; never `gzip` / `brotli` / `deflate`.
- **Retry (owner, 2026-09-18: "haan, progress holei retry reset koro, Plan 2-e dao"):** a segment's retry budget is `RETRY_MAX_ATTEMPTS` (10) *consecutive attempts without progress*; an attempt that wrote at least one byte resets the counter to 0 and the next backoff to `retry_base_delay`.
- Stable error codes: `INVALID_URL`, `RANGE_NOT_SUPPORTED`, `SOURCE_CHANGED`, `INVALID_RESUME`, `DISK_FULL`, `HTTP_STATUS`, `NETWORK`, `TLS`, `CANCELLED`, `IO`, and (new in this plan) `INTERNAL`. `INTERNAL` is never transient.
- Store schema (spec §2) with two column renames because `start` / `end` are SQL keywords: `segments(download_id, idx, start_byte, end_byte, downloaded)`. Status CHECK = `QUEUED, PROBING, DOWNLOADING, PAUSED, COMPLETED, FAILED, CANCELLED, SEEDING`; kind CHECK = `http, torrent`. Adding a status means a migration that changes the CHECK.
- Manager defaults: `max_parallel_downloads` 3, `max_connections` 8, FIFO by creation time. Progress is persisted from the **durable** snapshot at most once per second and always at a pause / failure.
- Gates before every commit: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`. Never weaken an assertion; a changed behaviour gets a regression test naming the decision and date.
- Commit messages tell the story and end with `Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>`. Source edits through the editor tools, never `sed -i` / `node -e`.
- This repository's commits use the GitHub noreply address already configured in the repo; never change `user.email`.

## Decisions made while planning

- **Plan 2 is engine + core only; the Tauri shell and React UI are Plan 3.** The spec's phase 4 ("store + commands + events; UI …") is split so every task here stays testable with `cargo test` and no window. The spec named `src-tauri/src/store`; the store and manager live in `crates/mdm-core` instead so they compile and test without Tauri — the spec's own rule "engines know nothing about Tauri" extended to the core. Plan 3's `src-tauri` becomes a thin command/event layer over `mdm_core::Manager`.
- **Crash recovery = re-queue.** At startup, rows left `PROBING` / `DOWNLOADING` become `QUEUED` and resume from their stored segments (IDM behaviour: an interrupted download continues).
- **Automatic fresh restart, once, for "the saved state no longer fits":** a resume refused with `RANGE_NOT_SUPPORTED`, `INVALID_RESUME`, or `IO` (part file missing), and a download that fails mid-way with `RANGE_NOT_SUPPORTED` (the server stopped honouring ranges → the spec's single-stream fallback), are restarted from byte 0 once, with a `Notice` event. `SOURCE_CHANGED` is NOT restarted automatically — the spec says the UI asks the user; the row becomes `FAILED` and `Manager::restart` starts over.
- **Live part names are exclusive inside one `Engine`.** A second live download that would use the same `.mdm.part` gets `name (1).ext` (an in-memory registry; the app is single-instance, so cross-process locking is not needed).
- **`DownloadControl`** (cloneable pause / cancel) instead of making `wait()` borrow: the manager awaits `wait()` in the driving task while `pause()` arrives from another.

## File structure

```
Cargo.toml                         + workspace deps rusqlite, uuid, serde, serde_json
crates/mdm-test-server/            NEW dev-only crate (moved from mdm-engine/tests/support)
  Cargo.toml
  src/lib.rs                       TestServer, ServerCfg, payload, sha256_*  (Tasks 5–6 add switches)
  tests/smoke.rs                   moved from mdm-engine/tests/support_smoke.rs
crates/mdm-engine/
  Cargo.toml                       dev-dep mdm-test-server; drop axum/sha2 dev-deps
  src/segment.rs                   retry reset (T2); spawn_blocking writes (T4); 206 w/o Content-Range (T6)
  src/download.rs                  DownloadControl, compare_exchange, all_done-first, INTERNAL (T3); durable snapshot + sync (T4); part registry (T5); checked resume math (T6)
  src/engine.rs                    live part registry field (T5)
  src/error.rs                     INTERNAL (T3)
  src/request.rs                   strip_credentials, forbidden-header filter (T5)
  src/probe.rs                     HEAD without Accept-Ranges → GET 0-0 (T6)
  tests/*.rs                       `use mdm_test_server::*` instead of `mod support`
crates/mdm-core/                   NEW
  Cargo.toml
  src/lib.rs                       re-exports
  src/error.rs                     CoreError
  src/model.rs                     DownloadId, DownloadStatus, DownloadRow, NewDownload
  src/store.rs                     Store: schema v1, downloads/segments/settings
  src/settings.rs                  Settings (serde, defaults) + EngineConfig mapping
  src/events.rs                    ManagerEvent
  src/manager.rs                   Manager: add/schedule/drive/pause/resume/cancel/remove/restart/recover
  tests/store.rs, tests/manager.rs
docs/CORE.md                       how the core works (T12)
```

---

### Task 1: Move the test server into its own dev crate (`crates/mdm-test-server`)

**Files:**
- Create: `crates/mdm-test-server/Cargo.toml`, `crates/mdm-test-server/src/lib.rs`, `crates/mdm-test-server/tests/smoke.rs`
- Delete: `crates/mdm-engine/tests/support/mod.rs`, `crates/mdm-engine/tests/support_smoke.rs`
- Modify: `crates/mdm-engine/Cargo.toml` (dev-deps), every `crates/mdm-engine/tests/*.rs` (`mod support; use support::*;` → `use mdm_test_server::*;`)

**Interfaces:**
- Produces: crate `mdm_test_server` with exactly the public API `tests/support/mod.rs` has today — `TestServer { base, data, cfg }`, `TestServer::start(size) / file_url() / redirect_url() / status_url(code)`, `ServerCfg` (all current switches, same defaults), `payload(size)`, `sha256_bytes`, `sha256_file`. Later tasks add switches to it.

- [ ] **Step 1: Create the crate**

`crates/mdm-test-server/Cargo.toml`:
```toml
[package]
name = "mdm-test-server"
description = "In-process HTTP server for Muzn Download Manager tests (never published)"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
authors.workspace = true
publish = false

[dependencies]
axum.workspace = true
tokio = { workspace = true, features = ["full"] }
futures-util.workspace = true
bytes.workspace = true
sha2.workspace = true
```

`crates/mdm-test-server/src/lib.rs`: the current content of `crates/mdm-engine/tests/support/mod.rs`, moved with `git mv` so history follows, with the module doc turned into a crate doc (`//!` stays) and `#![allow(dead_code)]` removed (every item is `pub` now, so nothing is dead). Keep every function body byte-for-byte.

```bash
git mv crates/mdm-engine/tests/support/mod.rs crates/mdm-test-server/src/lib.rs
git mv crates/mdm-engine/tests/support_smoke.rs crates/mdm-test-server/tests/smoke.rs
```
In `tests/smoke.rs` replace `mod support;` + `use support::*;` with `use mdm_test_server::*;`, and add `reqwest` as a dev-dependency of the new crate (the smoke tests use a plain client):
```toml
[dev-dependencies]
reqwest.workspace = true
```

- [ ] **Step 2: Point the engine tests at the crate**

`crates/mdm-engine/Cargo.toml` `[dev-dependencies]`: add `mdm-test-server = { path = "../mdm-test-server" }`; remove `axum` and `sha2` if `grep -rn "axum\|sha2" crates/mdm-engine/tests crates/mdm-engine/src` finds no other use (it should find none). In each of `tests/download.rs`, `tests/probe.rs`, `tests/resume.rs`, `tests/segment.rs`, `tests/steal.rs`: replace the two lines `mod support;` and `use support::*;` with `use mdm_test_server::*;`.

- [ ] **Step 3: Gates**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: the same 78 tests pass (3 of them now under `mdm-test-server`). No test body changed.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "refactor(test): the test server becomes its own dev-only crate

mdm-core (next) tests against the same server, so it cannot stay a
private module of the engine's tests. No behaviour changed.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 2: The retry budget resets on progress (owner decision)

**Files:**
- Modify: `crates/mdm-engine/src/segment.rs` (`fetch_segment`, `attempt_once`)
- Modify: `crates/mdm-engine/tests/segment.rs`, `crates/mdm-engine/tests/download.rs`

**Interfaces:**
- Consumes: `fetch_segment`, `attempt_once`, `RETRY_MAX_ATTEMPTS`, `RETRY_CAP` (segment.rs).
- Produces: unchanged signatures of `fetch_segment` / `SegmentJob`; new semantics — 10 consecutive attempts without a written byte fail the segment.

- [ ] **Step 1: Write the failing test**

In `crates/mdm-engine/tests/segment.rs` add:
```rust
#[tokio::test]
async fn a_link_that_drops_every_50_kb_still_completes() {
    // Owner decision 2026-09-18 ("progress holei retry reset koro"): an attempt
    // that wrote bytes resets the retry budget, so forty drops do not fail a
    // segment that keeps moving forward.
    let s = TestServer::start(2_000_000).await;
    s.cfg.drop_after.store(50_000, Ordering::SeqCst);
    let d = tempfile::tempdir().unwrap();
    let seg = Arc::new(SegmentRuntime::new(0, 0, Some(1_999_999), 0));
    let (j, file) = job(&s, d.path(), seg, true);
    fetch_segment(j).await.unwrap();
    assert_eq!(sha256_file(file.part_path()), sha256_bytes(&s.data));
    assert!(s.cfg.requests.load(Ordering::SeqCst) >= 40);
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p mdm-engine --test segment a_link_that_drops`
Expected: FAIL with `NETWORK` after 10 requests.

- [ ] **Step 3: Implement**

In `segment.rs`, `attempt_once` gains an out-parameter counting the bytes it wrote:
```rust
async fn attempt_once(job: &SegmentJob, wrote: &mut u64) -> Result<(), EngineError> {
```
and right after the successful `write_at` (next to `pos += buf.len() as u64;`) add `*wrote += buf.len() as u64;`.

Replace `fetch_segment`'s loop body:
```rust
pub async fn fetch_segment(job: SegmentJob) -> Result<(), EngineError> {
    let mut attempt: u32 = 0;
    loop {
        if job.cancel.is_cancelled() {
            return Err(EngineError::Cancelled);
        }
        let mut wrote = 0u64;
        match attempt_once(&job, &mut wrote).await {
            Ok(()) => return Ok(()),
            Err(e) if e.is_transient() => {
                // Owner decision 2026-09-18: an attempt that moved the download
                // forward resets the budget — only attempts in a row that wrote
                // nothing count towards RETRY_MAX_ATTEMPTS.
                if wrote > 0 {
                    attempt = 0;
                }
                attempt += 1;
                if attempt >= RETRY_MAX_ATTEMPTS {
                    return Err(e);
                }
                let delay = job
                    .retry_base_delay
                    .saturating_mul(1 << (attempt - 1).min(20))
                    .min(RETRY_CAP);
                tracing::debug!(idx = job.seg.idx, attempt, wrote, ?delay, error = %e, "segment retry");
                tokio::select! {
                    _ = job.cancel.cancelled() => return Err(EngineError::Cancelled),
                    _ = tokio::time::sleep(delay) => {}
                }
            }
            Err(e) => return Err(e),
        }
    }
}
```
Update the doc comment of `RETRY_MAX_ATTEMPTS`: "Attempts in a row without progress before the segment fails."

- [ ] **Step 4: Rework the two tests whose failure relied on the old count**

They used bodies that write a few bytes per attempt, which now counts as progress and never fails. Switch them to answers that write nothing (503), keeping their meaning:

`tests/segment.rs` `gives_up_after_max_attempts`:
```rust
#[tokio::test]
async fn gives_up_after_max_attempts() {
    // Since 2026-09-18 the budget counts attempts WITHOUT progress: 503s write
    // nothing, so ten of them in a row fail the segment.
    let s = TestServer::start(200_000).await;
    s.cfg.fail_first.store(1000, Ordering::SeqCst);
    let d = tempfile::tempdir().unwrap();
    let seg = Arc::new(SegmentRuntime::new(0, 0, Some(199_999), 0));
    let (j, _) = job(&s, d.path(), seg, true);
    let e = fetch_segment(j).await.unwrap_err();
    assert_eq!(e.code(), "HTTP_STATUS");
    assert_eq!(s.cfg.requests.load(Ordering::SeqCst), 10);
}
```
`tests/download.rs` `permanent_mid_download_failure_reports_and_keeps_part`: replace `s.cfg.drop_after.store(1, …)` with `s.cfg.fail_first.store(1000, Ordering::SeqCst); // every GET answers 503 → 10 attempts without progress` and the expected code `"NETWORK"` with `"HTTP_STATUS"`. The HEAD probe is not affected (`fail_first` only answers GETs).

Also pin the deterministic count in `retries_after_connection_drop_and_503`: `assert_eq!(s.cfg.requests.load(Ordering::SeqCst), 6, "2 x 503 + 4 partial bodies");` (it was `>= 6`).

- [ ] **Step 5: Run and gate**

Run: `cargo test -p mdm-engine --test segment --test download` then the full gates.
Expected: all pass, including the new test.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat(engine): the retry budget resets whenever an attempt made progress

Owner decision 2026-09-18: a link that drops every few megabytes kept
failing large downloads after ten drops although every attempt moved
forward. Only ten attempts in a row that write nothing fail a segment now.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 3: `DownloadControl`, honest end states, `INTERNAL`

**Files:**
- Modify: `crates/mdm-engine/src/error.rs`, `crates/mdm-engine/src/download.rs`, `crates/mdm-engine/src/lib.rs`
- Test: `crates/mdm-engine/tests/resume.rs` (end-state tests), `crates/mdm-engine/src/error.rs` (codes)

**Interfaces:**
- Produces:
  ```rust
  // error.rs
  #[error("internal error: {0}")] EngineError::Internal(String)   // code "INTERNAL", never transient
  // download.rs
  #[derive(Clone, Debug)]
  pub struct DownloadControl { /* stop: Arc<AtomicU8>, cancel: CancellationToken */ }
  impl DownloadControl { pub fn pause(&self); pub fn cancel(&self); }
  impl DownloadHandle { pub fn control(&self) -> DownloadControl; }
  ```
  End-state order in `Run::run`: a worker failure → `Failed`; else `STOP_CANCEL` → `Cancelled`; else every segment done → finish → `Completed`; else `STOP_PAUSE` → `Paused`; else `Failed(Internal)`. `StopOnDrop` sets PAUSE only if no stop code is set yet.

- [ ] **Step 1: Write the failing tests**

`src/error.rs` `codes_are_stable_strings`: add
```rust
        assert_eq!(EngineError::Internal("x".into()).code(), "INTERNAL");
        assert!(!EngineError::Internal("x".into()).is_transient());
```
`tests/resume.rs` (uses the file's existing `engine()`, `spec()`, `start_and_pause()`, `resume()` helpers and `SIZE`):
```rust
#[tokio::test]
async fn a_control_pauses_while_another_task_waits() {
    let s = TestServer::start(SIZE).await;
    let d = tempfile::tempdir().unwrap();
    s.cfg.hang_first.store(4, Ordering::SeqCst);
    let h = engine().start(spec(&s, d.path(), None)).await.unwrap();
    let control = h.control();
    let waiter = tokio::spawn(h.wait());
    tokio::time::sleep(Duration::from_millis(100)).await;
    control.pause();
    let out = tokio::time::timeout(Duration::from_secs(10), waiter).await.unwrap().unwrap();
    assert!(matches!(out, Outcome::Paused(_)), "{out:?}");
}

#[tokio::test]
async fn cancel_then_drop_still_reports_cancelled() {
    // Final review 2026-09-18: dropping the handle after cancel() must not turn
    // the cancel into a pause.
    let s = TestServer::start(SIZE).await;
    let d = tempfile::tempdir().unwrap();
    s.cfg.hang_first.store(4, Ordering::SeqCst);
    let h = engine().start(spec(&s, d.path(), None)).await.unwrap();
    let mut rx = h.subscribe();
    h.cancel();
    drop(h);
    tokio::time::timeout(Duration::from_secs(10), async {
        while rx.changed().await.is_ok() {}
    })
    .await
    .unwrap();
    assert_eq!(rx.borrow().status, Status::Cancelled);
}

#[tokio::test]
async fn a_pause_that_arrives_after_the_last_byte_still_completes() {
    // Every segment of the resume is already complete; the pause lands before
    // the run task is even polled (current-thread runtime), yet the file is
    // whole, so the honest outcome is Completed, not a Paused complete file.
    let s = TestServer::start(SIZE).await;
    let d = tempfile::tempdir().unwrap();
    let segs = start_and_pause(&s, d.path()).await;
    std::fs::write(d.path().join("file.mdm.part"), s.data.as_slice()).unwrap();
    let full: Vec<SegmentState> = segs
        .iter()
        .map(|x| SegmentState { downloaded: x.end - x.start + 1, ..x.clone() })
        .collect();
    let h = engine().start(spec(&s, d.path(), Some(resume(&s, full)))).await.unwrap();
    h.pause();
    let out = h.wait().await;
    let Outcome::Completed(path) = out else { panic!("{out:?}") };
    assert_eq!(sha256_file(&path), sha256_bytes(&s.data));
}
```
Add the imports those tests need at the top of `tests/resume.rs` (`Status`, `SegmentState` from `mdm_engine`).

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p mdm-engine --test resume` and `cargo test -p mdm-engine --lib error`
Expected: compile errors (`control`, `Internal`), then — after stubbing — the last two tests fail with `Paused`.

- [ ] **Step 3: Implement**

`error.rs`: add the variant (doc: "An invariant inside the engine broke (a worker panicked, a file handle leaked). Not retryable; report it."), `code()` arm `Self::Internal(_) => "INTERNAL"`; `is_transient` already returns false for it.

`download.rs`:
```rust
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
```
`DownloadHandle::control(&self) -> DownloadControl` returns clones of `stop` and `cancel`. `pause()` / `cancel()` on the handle delegate to the same logic (keep them).

`StopOnDrop::drop`: replace the unconditional store with
```rust
            let _ = self.stop.compare_exchange(STOP_NONE, STOP_PAUSE, Ordering::SeqCst, Ordering::SeqCst);
            self.cancel.cancel();
```
`wait()`'s panic mapping and the three invariant paths in `run` (`"part file still in use"`, `"workers ended with bytes missing"`, the `Some(Err(_))` worker panic) use `EngineError::Internal(..)` instead of `Network(..)`.

Reorder the outcome in `run`:
```rust
        let outcome = if let Some(error) = failure {
            Outcome::Failed { error, segments: segments.clone() }
        } else if stop == STOP_CANCEL {
            Outcome::Cancelled
        } else if all_done {
            match Arc::try_unwrap(file) {
                Ok(file) => match file.finish() {
                    Ok(path) => Outcome::Completed(path),
                    Err(error) => Outcome::Failed { error, segments: segments.clone() },
                },
                Err(_) => Outcome::Failed {
                    error: EngineError::Internal("part file still in use".into()),
                    segments: segments.clone(),
                },
            }
        } else if stop == STOP_PAUSE {
            Outcome::Paused(segments.clone())
        } else {
            Outcome::Failed {
                error: EngineError::Internal("workers ended with bytes missing".into()),
                segments: segments.clone(),
            }
        };
```
`lib.rs`: re-export `DownloadControl`. `docs/ENGINE.md`: add `INTERNAL` to the codes list and one line under "Progress, pause, resume": "`handle.control()` gives a cloneable pause / cancel for use while another task awaits `wait()`. A pause that arrives after the last byte still completes the file."

- [ ] **Step 4: Run and gate**

Run: `cargo test -p mdm-engine` then the full gates. Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(engine): DownloadControl, cancel survives a dropped handle, a complete file completes

The core needs to pause a download while its driving task awaits
wait(). Dropping a handle after cancel() no longer reports a pause, a
pause that lands after the last byte no longer leaves a complete file
unrenamed, and internal failures carry INTERNAL instead of a retryable
NETWORK.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 4: Durable progress and disk I/O off the async threads

**Files:**
- Modify: `crates/mdm-engine/src/download.rs` (`Progress`, `Run`), `crates/mdm-engine/src/segment.rs` (write path)
- Test: `crates/mdm-engine/tests/download.rs`, `crates/mdm-engine/tests/resume.rs`

**Interfaces:**
- Produces:
  ```rust
  pub struct Progress { /* existing fields */ pub durable_segments: Vec<SegmentState> }
  pub const SYNC_INTERVAL: Duration = Duration::from_secs(1);
  ```
  `durable_segments` = the segment snapshot taken just before the last completed `PartFile::sync()`; at start it is the starting state (resume segments, or all-zero fresh segments). On `Paused` / `Failed` the run syncs once more and the final `Progress.durable_segments` equals the outcome's segments. Callers persist `durable_segments`, never `segments`.
- Every `PartFile::write_at` / `sync` runs inside `tokio::task::spawn_blocking`.

- [ ] **Step 1: Write the failing tests**

`tests/download.rs` — replace `progress_stream_reports_bytes_and_speed` (its name promised a speed it never asserted) with:
```rust
#[tokio::test]
async fn progress_reports_speed_and_a_durable_snapshot_while_downloading() {
    // Final review 2026-09-18: the saved state must only claim bytes that
    // reached the disk (a power cut loses the page cache).
    let s = TestServer::start(6 * 1024 * 1024).await;
    s.cfg.chunk_delay_ms.store(2, Ordering::SeqCst);
    let d = tempfile::tempdir().unwrap();
    let h = engine(4).start(spec(&s.file_url(), d.path())).await.unwrap();
    let mut rx = h.subscribe();
    let first = rx.borrow_and_update().clone();
    assert_eq!(first.status, Status::Downloading);
    assert_eq!(first.durable_segments.iter().map(|x| x.downloaded).sum::<u64>(), 0);
    let mut saw_speed = false;
    let mut saw_durable = false;
    while rx.changed().await.is_ok() {
        let p = rx.borrow_and_update().clone();
        if p.status != Status::Downloading {
            break;
        }
        saw_speed |= p.speed_bps > 0;
        let durable: u64 = p.durable_segments.iter().map(|x| x.downloaded).sum();
        assert!(durable <= p.downloaded, "durable {durable} > downloaded {}", p.downloaded);
        saw_durable |= durable > 0;
    }
    assert!(saw_speed, "a speed above zero was reported");
    assert!(saw_durable, "a durable snapshot was published during the download");
    let Outcome::Completed(path) = h.wait().await else { panic!() };
    assert_eq!(sha256_file(&path), sha256_bytes(&s.data));
}
```
`tests/resume.rs`:
```rust
#[tokio::test]
async fn a_paused_outcome_is_durable() {
    let s = TestServer::start(SIZE).await;
    let d = tempfile::tempdir().unwrap();
    s.cfg.hang_first.store(4, Ordering::SeqCst);
    let h = engine().start(spec(&s, d.path(), None)).await.unwrap();
    let rx = h.subscribe();
    tokio::time::sleep(Duration::from_millis(300)).await;
    h.pause();
    let Outcome::Paused(segs) = h.wait().await else { panic!() };
    assert_eq!(rx.borrow().durable_segments, segs);
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p mdm-engine --test download progress_reports --test resume a_paused_outcome`
Expected: compile error — `durable_segments` does not exist.

- [ ] **Step 3: Implement**

`segment.rs` write path — the chunk is a `bytes::Bytes`; hand an owned slice to a blocking thread:
```rust
        let len = buf.len();
        let data = chunk.slice(..len); // `buf` is `&chunk[..len]`
        let file = job.file.clone();
        tokio::task::spawn_blocking(move || file.write_at(pos, &data))
            .await
            .map_err(|e| EngineError::Internal(format!("write task: {e}")))?
            .map_err(EngineError::from_io)?;
```
(`buf` is always a prefix of `chunk` in the current code; keep the truncation logic that computes it and use its length.)

`download.rs`:
- `Progress` gains `pub durable_segments: Vec<SegmentState>` (doc as in Interfaces); `pub const SYNC_INTERVAL: Duration = Duration::from_secs(1);`.
- `Run` gains `durable: Mutex<Vec<SegmentState>>`, initialised in `Engine::start` to the starting snapshot (take it right after building `segments`).
- `Run::progress` fills `durable_segments: self.durable.lock().unwrap().clone()`.
- Add
  ```rust
  /// fsync the part file off the async threads, then record `snap` as durable.
  async fn sync_to(&self, snap: Vec<SegmentState>) {
      let file = self.file.clone();
      match tokio::task::spawn_blocking(move || file.sync()).await {
          Ok(Ok(())) => *self.durable.lock().unwrap() = snap,
          Ok(Err(e)) => tracing::warn!(error = %e, "sync of the part file failed"),
          Err(e) => tracing::warn!(error = %e, "sync task failed"),
      }
  }
  ```
- In the worker loop, keep a `let mut last_sync = Instant::now();`; in the ticker arm, after publishing progress: `if last_sync.elapsed() >= SYNC_INTERVAL { let snap = self.snapshot(); self.sync_to(snap).await; last_sync = Instant::now(); }` (the `Arc<PartFile>` clone lives only inside `sync_to`, so `Arc::try_unwrap` at the end still succeeds).
- After the loop and BEFORE the "read everything into locals" block (Ruling A of Plan 1 — `self` must still be whole): if the run is not heading for completion (`failure.is_some() || stop != STOP_NONE || !all_done` — compute `all_done` and `stop` first, then sync), call `self.sync_to(self.snapshot()).await`. Then read `let durable = self.durable.lock().unwrap().clone();` into a local together with the others, and put `durable_segments: durable` into the final `Progress`. For `Completed` set `durable_segments` to the final `segments` (`finish()` fsyncs).
- `lib.rs`: re-export `SYNC_INTERVAL`. `docs/ENGINE.md`, "Progress, pause, resume": "Persist `durable_segments` — the segments as of the last fsync (every second, and once more at a pause or failure). `segments` may count bytes still in the OS cache." And under "fetch": "Writes and fsyncs run on blocking threads."

- [ ] **Step 4: Run and gate**

Run: `cargo test -p mdm-engine` (twice — timing) then the full gates. Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(engine): progress carries a durable snapshot; disk I/O leaves the async threads

The store will save what Progress reports, and after a power cut only
fsynced bytes survive. The part file is synced every second and at a
pause or failure; durable_segments names what reached the disk. Writes
and syncs run on blocking threads so three downloads on a slow disk no
longer stall the runtime.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 5: Credentials stay with their origin; live part names are exclusive

**Files:**
- Modify: `crates/mdm-engine/src/request.rs`, `crates/mdm-engine/src/engine.rs`, `crates/mdm-engine/src/download.rs`
- Modify: `crates/mdm-test-server/src/lib.rs` (route `/redirect-to-ip`, `last_cookie` switch)
- Test: `crates/mdm-engine/src/request.rs` (unit), `crates/mdm-engine/tests/download.rs`

**Interfaces:**
- Produces:
  ```rust
  impl RequestExtras {
      /// Copy without `Cookie`, `Authorization` and `Proxy-Authorization` (and no cookies).
      pub fn without_credentials(&self) -> RequestExtras;
  }
  // apply() never sends caller headers named Range, Host or Content-Length (the engine owns them).
  impl DownloadHandle { pub fn filename(&self) -> &str; }   // the name actually used, maybe "name (1).ext"
  ```
  Workers get `spec.extras` when `probe.final_url` has the same origin (scheme, host, port) as `spec.url`, otherwise `spec.extras.without_credentials()`. `Engine` keeps a registry of live part paths (`Arc<Mutex<HashSet<PathBuf>>>`, shared by clones); a fresh start whose part path is live picks `stem (n).ext`; a resume whose part path is live fails with `EngineError::InvalidResume("the part file is in use by another download")`. The claim is released when the run ends (a guard field in `Run`, never a `Drop` impl on `Run` itself).

- [ ] **Step 1: Write the failing tests**

`src/request.rs` tests:
```rust
    #[test]
    fn without_credentials_strips_cookies_and_auth_headers() {
        let x = RequestExtras {
            headers: vec![
                ("Referer".into(), "https://a/".into()),
                ("authorization".into(), "Bearer t".into()),
                ("Proxy-Authorization".into(), "Basic p".into()),
                ("Cookie".into(), "raw=1".into()),
            ],
            cookies: vec![Cookie { name: "s".into(), value: "1".into() }],
        };
        let y = x.without_credentials();
        assert_eq!(y.headers, vec![("Referer".to_string(), "https://a/".to_string())]);
        assert!(y.cookies.is_empty());
    }

    #[test]
    fn apply_never_sends_engine_owned_headers() {
        let x = RequestExtras {
            headers: vec![("Range".into(), "bytes=0-1".into()), ("host".into(), "evil".into()), ("Content-Length".into(), "5".into())],
            cookies: vec![],
        };
        let req = x.apply(reqwest::Client::new().get("https://example.invalid/")).build().unwrap();
        assert!(req.headers().get("range").is_none());
        assert!(req.headers().get("content-length").is_none());
        assert_ne!(req.headers().get("host").map(|v| v.to_str().unwrap()), Some("evil"));
    }
```
Test server (`crates/mdm-test-server/src/lib.rs`): add `pub last_cookie: Mutex<Option<String>>` to `ServerCfg` (default `None`; doc "the `Cookie` header of the last GET of /file") and record it in `file()` for GETs; add a route `/redirect-to-ip` that answers `302` with `Location: http://127.0.0.1:<port>/file` (absolute — store the bound port in `AppState`), and `pub fn redirect_to_ip_url(&self) -> String` returning `http://localhost:<port>/redirect-to-ip`.

`tests/download.rs`:
```rust
fn with_cookie(mut sp: DownloadSpec) -> DownloadSpec {
    sp.extras.cookies.push(mdm_engine::Cookie { name: "session".into(), value: "secret".into() });
    sp
}

#[tokio::test]
async fn cookies_are_not_sent_to_another_origin_after_a_redirect() {
    // Final review 2026-09-18: workers fetch final_url directly, bypassing
    // reqwest's own cross-host header stripping.
    let s = TestServer::start(1024 * 1024).await;
    let d = tempfile::tempdir().unwrap();
    let h = engine(2).start(with_cookie(spec(&s.redirect_to_ip_url(), d.path()))).await.unwrap();
    let Outcome::Completed(path) = h.wait().await else { panic!() };
    assert_eq!(sha256_file(&path), sha256_bytes(&s.data));
    assert_eq!(*s.cfg.last_cookie.lock().unwrap(), None);
}

#[tokio::test]
async fn cookies_are_kept_on_the_same_origin() {
    let s = TestServer::start(1024 * 1024).await;
    let d = tempfile::tempdir().unwrap();
    let h = engine(2).start(with_cookie(spec(&s.redirect_url(), d.path()))).await.unwrap();
    let Outcome::Completed(_) = h.wait().await else { panic!() };
    assert_eq!(s.cfg.last_cookie.lock().unwrap().as_deref(), Some("session=secret"));
}

#[tokio::test]
async fn two_live_downloads_of_the_same_name_get_separate_part_files() {
    // Final review 2026-09-18: they shared one .mdm.part before.
    let s = TestServer::start(3 * 1024 * 1024).await;
    s.cfg.chunk_delay_ms.store(2, Ordering::SeqCst);
    let d = tempfile::tempdir().unwrap();
    let e = engine(2);
    let a = e.start(spec(&s.file_url(), d.path())).await.unwrap();
    let b = e.start(spec(&s.file_url(), d.path())).await.unwrap();
    assert_eq!(a.filename(), "file");
    assert_eq!(b.filename(), "file (1)");
    let (oa, ob) = tokio::join!(a.wait(), b.wait());
    let (Outcome::Completed(pa), Outcome::Completed(pb)) = (oa, ob) else { panic!() };
    assert_ne!(pa, pb);
    assert_eq!(sha256_file(&pa), sha256_bytes(&s.data));
    assert_eq!(sha256_file(&pb), sha256_bytes(&s.data));
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p mdm-engine` — compile errors, then the cookie test sees `Some("session=secret")` and the name test sees two `file`s.

- [ ] **Step 3: Implement**

`request.rs`:
```rust
const CREDENTIAL_HEADERS: [&str; 3] = ["cookie", "authorization", "proxy-authorization"];
const ENGINE_HEADERS: [&str; 3] = ["range", "host", "content-length"];

impl RequestExtras {
    /// A copy that carries no credentials: used when a redirect left the
    /// origin the caller's cookies and auth headers were meant for.
    pub fn without_credentials(&self) -> RequestExtras {
        RequestExtras {
            headers: self
                .headers
                .iter()
                .filter(|(k, _)| !CREDENTIAL_HEADERS.iter().any(|c| k.eq_ignore_ascii_case(c)))
                .cloned()
                .collect(),
            cookies: Vec::new(),
        }
    }
}
```
and in `apply` skip `k` when `ENGINE_HEADERS.iter().any(|h| k.eq_ignore_ascii_case(h))`.

`engine.rs`: `Engine` gains `pub(crate) live_parts: Arc<Mutex<HashSet<PathBuf>>>` (initialised empty in `Engine::new`; clones share it).

`download.rs`:
```rust
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
```
In `Engine::start`, after the filename is chosen and before any file work: lock `live_parts`; for a resume, a live `part_path` → `InvalidResume`; for a fresh start, while the path is live, derive `stem (n).ext` with the same split rule as `file::free_name` (last dot, not a leading dot) for n = 1, 2, …, and use that as `filename` / `part_path`; insert and build `PartClaim`. Store the claim in a new `Run` field `_claim: PartClaim` and the final `filename` in a new `DownloadHandle` field returned by `filename()`. Worker extras: `if spec.url.origin() == probe.final_url.origin() { spec.extras.clone() } else { spec.extras.without_credentials() }`.

`docs/ENGINE.md`: replace the "caller keeps (dir, filename) unique" sentence with "A second live download of the same name gets `name (1).ext`; a resume of a part file that is in use is refused (`INVALID_RESUME`)." Add under Flow: "Cookies and `Authorization` are dropped when a redirect leaves the original origin; `Range`, `Host` and `Content-Length` from the caller are never sent." Update the same sentence in the doc comment of `Engine::start`.

- [ ] **Step 4: Run and gate**, **Step 5: Commit**

```bash
git add -A
git commit -m "fix(engine): credentials stay with their origin; live downloads never share a part file

Workers fetched the redirect target directly with the caller's cookies
and Authorization; they are dropped now when the origin changes. Two
live downloads of one name got one .mdm.part; the second is named
'name (1)'.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 6: Probe and segment edge cases

**Files:**
- Modify: `crates/mdm-engine/src/probe.rs`, `crates/mdm-engine/src/segment.rs`, `crates/mdm-engine/src/download.rs`
- Modify: `crates/mdm-test-server/src/lib.rs` (switches `advertise_ranges`, `chunked`, `omit_content_range`)
- Test: `crates/mdm-engine/tests/probe.rs`, `tests/segment.rs`, `tests/download.rs`, `tests/resume.rs`

**Interfaces:**
- Produces (behaviour only):
  1. A HEAD that answers a length but no `Accept-Ranges` no longer settles the question: the probe continues with `GET Range: bytes=0-0` (a `206` proves ranges).
  2. A single stream of unknown length (no `Content-Length`) that ends normally is complete, including a zero-byte one.
  3. A `206` without a usable `Content-Range` is `RANGE_NOT_SUPPORTED` at once (no ten retries).
  4. `validate_resume` never overflows: `end + 1` / `end - start + 1` on corrupted input → `INVALID_RESUME`.
- Test-server switches (default preserves today's behaviour): `advertise_ranges: AtomicBool` (true; false = omit `Accept-Ranges` but still honour `Range`), `chunked: AtomicBool` (false; true = no `Content-Length` on HEAD or GET, body streamed), `omit_content_range: AtomicBool` (false; true = a 206 without `Content-Range`).

- [ ] **Step 1: Write the failing tests**

`tests/probe.rs`:
```rust
#[tokio::test]
async fn ranges_that_are_not_advertised_are_still_found() {
    let s = TestServer::start(50_000).await;
    s.cfg.advertise_ranges.store(false, Ordering::SeqCst);
    let p = engine().probe(&Url::parse(&s.file_url()).unwrap(), &RequestExtras::default()).await.unwrap();
    assert!(p.ranges, "a 206 to bytes=0-0 proves range support");
    assert_eq!(p.size, Some(50_000));
    assert_eq!(s.cfg.requests.load(Ordering::SeqCst), 1);
}
```
`tests/segment.rs`:
```rust
#[tokio::test]
async fn a_206_without_content_range_is_range_not_supported_at_once() {
    let s = TestServer::start(10_000).await;
    s.cfg.omit_content_range.store(true, Ordering::SeqCst);
    let d = tempfile::tempdir().unwrap();
    let seg = Arc::new(SegmentRuntime::new(0, 100, Some(199), 0));
    let (j, _) = job(&s, d.path(), seg, true);
    assert_eq!(fetch_segment(j).await.unwrap_err().code(), "RANGE_NOT_SUPPORTED");
    assert_eq!(s.cfg.requests.load(Ordering::SeqCst), 1);
}
```
`tests/download.rs`:
```rust
#[tokio::test]
async fn a_stream_of_unknown_length_completes() {
    let s = TestServer::start(100_000).await;
    s.cfg.chunked.store(true, Ordering::SeqCst);
    let d = tempfile::tempdir().unwrap();
    let h = engine(4).start(spec(&s.file_url(), d.path())).await.unwrap();
    assert_eq!(h.probe().size, None);
    let Outcome::Completed(path) = h.wait().await else { panic!() };
    assert_eq!(sha256_file(&path), sha256_bytes(&s.data));
}

#[tokio::test]
async fn an_empty_stream_of_unknown_length_completes() {
    // Final review 2026-09-18: it failed with "workers ended with bytes missing".
    let s = TestServer::start(0).await;
    s.cfg.chunked.store(true, Ordering::SeqCst);
    let d = tempfile::tempdir().unwrap();
    let h = engine(4).start(spec(&s.file_url(), d.path())).await.unwrap();
    let Outcome::Completed(path) = h.wait().await else { panic!() };
    assert_eq!(std::fs::metadata(&path).unwrap().len(), 0);
}
```
`tests/resume.rs`:
```rust
#[tokio::test]
async fn corrupted_resume_state_is_refused_not_a_panic() {
    let s = TestServer::start(SIZE).await;
    let d = tempfile::tempdir().unwrap();
    let _ = start_and_pause(&s, d.path()).await;
    let bad = vec![SegmentState { idx: 0, start: 0, end: u64::MAX, downloaded: 0 }];
    let e = engine().start(spec(&s, d.path(), Some(resume(&s, bad)))).await.unwrap_err();
    assert_eq!(e.code(), "INVALID_RESUME");
}
```

- [ ] **Step 2: Run to verify they fail** (compile errors on the new switches first).

- [ ] **Step 3: Implement**

1. `probe.rs`: in the HEAD branch, return early only when `accepts_ranges(&r)` is true; a HEAD with a length but no `Accept-Ranges` falls through to the GET fallback (keep the HEAD's `Content-Length` for the case the GET answers `200` without a length).
2. Single stream of unknown length: in `Run::run`, compute `all_done` as
   ```rust
        // A single stream of unknown length has no `end` to reach: its worker
        // returning Ok means the server ended the body normally.
        let all_done = if !self.ranged && self.total.is_none() {
            failure.is_none() && worker_ok
        } else {
            segments.iter().all(|s| s.is_done())
        };
   ```
   with `worker_ok` a `bool` set to true in the `Some(Ok(Ok(())))` arm. In `segment.rs`, for `end == UNKNOWN_END` with zero bytes written, leave `end` at `UNKNOWN_END` (do not store `start`), so the snapshot of an empty stream is `downloaded: 0`.
3. `segment.rs`: when the status is `206` and `Content-Range` is missing or unparsable → `EngineError::RangeNotSupported`; a parsable one that starts elsewhere stays `Network` (transient).
4. `download.rs` `validate_resume`: `let len = s.end.checked_sub(s.start).and_then(|x| x.checked_add(1))` and `expect = s.end.checked_add(1)` — `None` → `bad(..)`.
5. Test server: implement the three switches in `file()` (HEAD and GET), each documented; defaults leave every existing test unchanged.

- [ ] **Step 4: Run and gate** (full suite twice), **Step 5: Commit**

```bash
git add -A
git commit -m "fix(engine): hidden range support, streams of unknown length, a bare 206, corrupted resume state

Servers that honour ranges without advertising them now get segments;
a body without Content-Length completes even when empty; a 206 without
Content-Range fails honestly at once; overflowing resume state is
refused instead of panicking.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 7: `mdm-core` crate, model and the store's downloads table

**Files:**
- Modify: root `Cargo.toml` (`[workspace.dependencies]`)
- Create: `crates/mdm-core/Cargo.toml`, `src/lib.rs`, `src/error.rs`, `src/model.rs`, `src/store.rs`, `tests/store.rs`

**Interfaces:**
- Consumes: `mdm_engine::SegmentState` (Task 8 uses it; declared here as a dependency).
- Produces:
  ```rust
  // error.rs
  #[derive(Debug, thiserror::Error)]
  pub enum CoreError { Db(rusqlite::Error), NotFound(String), InvalidUrl(String), InvalidState(String),
                       Settings(String), Engine(mdm_engine::EngineError), Io(std::io::Error), Json(serde_json::Error) }
  impl CoreError { pub fn code(&self) -> &'static str }   // DB, NOT_FOUND, INVALID_URL, INVALID_STATE, SETTINGS, <engine code>, IO, JSON
  pub type Result<T> = std::result::Result<T, CoreError>;

  // model.rs  (all serde Serialize + Deserialize; field names camelCase on the wire)
  pub struct DownloadId(pub String);            // uuid v4; new(), as_str(), Display
  pub enum DownloadStatus { Queued, Probing, Downloading, Paused, Completed, Failed, Cancelled, Seeding }
       // as_str() -> "QUEUED" …; parse(&str) -> Option<Self>; serde SCREAMING_SNAKE_CASE
  pub enum DownloadKind { Http, Torrent }       // "http" / "torrent"
  pub struct Header { pub name: String, pub value: String }
  pub struct SavedCookie { pub name: String, pub value: String }
  pub struct NewDownload { pub url: String, pub dir: Option<PathBuf>, pub filename: Option<String>,
                           pub referrer: Option<String>, pub headers: Vec<Header>, pub cookies: Vec<SavedCookie>,
                           pub start_paused: bool }
  pub struct ProbeInfo { pub final_url: String, pub filename: String, pub size: Option<u64>,
                         pub etag: Option<String>, pub last_modified: Option<String>, pub mime: Option<String> }
  pub struct DownloadRow { pub id: DownloadId, pub kind: DownloadKind, pub url: String, pub final_url: Option<String>,
                           pub filename: Option<String>, pub dir: PathBuf, pub size: Option<u64>, pub downloaded: u64,
                           pub status: DownloadStatus, pub etag: Option<String>, pub last_modified: Option<String>,
                           pub mime: Option<String>, pub referrer: Option<String>, pub headers: Vec<Header>,
                           pub cookies: Vec<SavedCookie>, pub error_code: Option<String>, pub error_message: Option<String>,
                           pub created_at: i64, pub updated_at: i64, pub completed_at: Option<i64> }   // times = unix ms
  pub fn now_ms() -> i64;

  // store.rs
  pub struct Store { /* Mutex<rusqlite::Connection> */ }
  impl Store {
      pub fn open(path: &Path) -> Result<Store>;         // creates parent dirs; WAL; foreign_keys ON; migrates
      pub fn open_in_memory() -> Result<Store>;
      pub fn schema_version(&self) -> Result<i64>;
      pub fn insert(&self, id: &DownloadId, dir: &Path, new: &NewDownload, status: DownloadStatus, now: i64) -> Result<DownloadRow>;
      pub fn get(&self, id: &DownloadId) -> Result<Option<DownloadRow>>;
      pub fn list(&self) -> Result<Vec<DownloadRow>>;    // newest first
      pub fn next_queued(&self, exclude: &[DownloadId]) -> Result<Option<DownloadId>>;  // oldest QUEUED not excluded
      pub fn set_status(&self, id: &DownloadId, status: DownloadStatus, error: Option<(&str, &str)>, now: i64) -> Result<()>;
      pub fn set_probe(&self, id: &DownloadId, p: &ProbeInfo, now: i64) -> Result<()>;
      pub fn set_filename(&self, id: &DownloadId, filename: &str, now: i64) -> Result<()>;
      pub fn reset_interrupted(&self, now: i64) -> Result<usize>;   // PROBING/DOWNLOADING → QUEUED
      pub fn delete(&self, id: &DownloadId) -> Result<()>;
  }
  ```
  `set_status` clears `error_code/error_message` when `error` is `None`, sets `completed_at = now` for `Completed` (and `NULL` otherwise), and returns `NotFound` for an unknown id. `downloaded` in a row = `size` (or 0) when `Completed`, otherwise the sum of its segments (Task 8 adds the segments table; until then the sum is 0 — the column expression is written once here and works when the table exists, since the schema is created whole).

- [ ] **Step 1: Workspace deps and crate**

Root `Cargo.toml` `[workspace.dependencies]` add:
```toml
rusqlite = { version = "0.32", features = ["bundled"] }
uuid = { version = "1", features = ["v4"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```
`crates/mdm-core/Cargo.toml`:
```toml
[package]
name = "mdm-core"
description = "Download store and manager for Muzn Download Manager"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
authors.workspace = true

[dependencies]
mdm-engine = { path = "../mdm-engine" }
tokio = { workspace = true, features = ["rt", "sync", "time", "macros"] }
tokio-util.workspace = true
url.workspace = true
thiserror.workspace = true
tracing.workspace = true
rusqlite.workspace = true
uuid.workspace = true
serde.workspace = true
serde_json.workspace = true

[dev-dependencies]
mdm-test-server = { path = "../mdm-test-server" }
tempfile.workspace = true
tokio = { workspace = true, features = ["full"] }
```
`src/lib.rs`:
```rust
//! The download core of Muzn Download Manager: a SQLite store and a manager
//! that queues, drives and persists downloads through `mdm-engine`.
//! No Tauri and no UI here — the desktop app wraps this crate.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod error;
pub mod model;
pub mod store;

pub use error::{CoreError, Result};
pub use model::*;
pub use store::Store;
```

- [ ] **Step 2: Write the failing tests** — `crates/mdm-core/tests/store.rs`:
```rust
use std::path::Path;

use mdm_core::*;

fn new(url: &str) -> NewDownload {
    NewDownload {
        url: url.into(),
        dir: None,
        filename: None,
        referrer: Some("https://example.com/page".into()),
        headers: vec![Header { name: "X-A".into(), value: "1".into() }],
        cookies: vec![SavedCookie { name: "s".into(), value: "v".into() }],
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
    let ra = s.insert(&a, Path::new("/dl"), &new("https://x/a.zip"), DownloadStatus::Queued, 1_000).unwrap();
    s.insert(&b, Path::new("/dl"), &new("https://x/b.zip"), DownloadStatus::Paused, 2_000).unwrap();
    assert_eq!(ra.status, DownloadStatus::Queued);
    assert_eq!(ra.headers, vec![Header { name: "X-A".into(), value: "1".into() }]);
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
    s.insert(&a, Path::new("/d"), &new("https://x/a"), DownloadStatus::Queued, 10).unwrap();
    s.insert(&b, Path::new("/d"), &new("https://x/b"), DownloadStatus::Paused, 20).unwrap();
    s.insert(&c, Path::new("/d"), &new("https://x/c"), DownloadStatus::Queued, 30).unwrap();
    assert_eq!(s.next_queued(&[]).unwrap(), Some(a.clone()));
    assert_eq!(s.next_queued(&[a.clone()]).unwrap(), Some(c.clone()));
    assert_eq!(s.next_queued(&[a, c]).unwrap(), None);
}

#[test]
fn status_error_and_completion_fields() {
    let s = Store::open_in_memory().unwrap();
    let a = DownloadId::new();
    s.insert(&a, Path::new("/d"), &new("https://x/a"), DownloadStatus::Queued, 10).unwrap();
    s.set_status(&a, DownloadStatus::Failed, Some(("NETWORK", "reset")), 20).unwrap();
    let r = s.get(&a).unwrap().unwrap();
    assert_eq!((r.error_code.as_deref(), r.error_message.as_deref()), (Some("NETWORK"), Some("reset")));
    assert_eq!(r.updated_at, 20);
    s.set_probe(&a, &ProbeInfo { final_url: "https://cdn/a".into(), filename: "a.bin".into(), size: Some(99),
        etag: Some("\"e\"".into()), last_modified: None, mime: Some("application/zip".into()) }, 25).unwrap();
    s.set_status(&a, DownloadStatus::Completed, None, 30).unwrap();
    let r = s.get(&a).unwrap().unwrap();
    assert_eq!(r.error_code, None);
    assert_eq!(r.completed_at, Some(30));
    assert_eq!((r.size, r.downloaded), (Some(99), 99));
    assert_eq!(r.filename.as_deref(), Some("a.bin"));
    assert_eq!(s.set_status(&DownloadId::new(), DownloadStatus::Queued, None, 1).unwrap_err().code(), "NOT_FOUND");
}

#[test]
fn reset_interrupted_requeues_active_rows_only() {
    let s = Store::open_in_memory().unwrap();
    let ids: Vec<_> = (0..4).map(|_| DownloadId::new()).collect();
    for (i, st) in [DownloadStatus::Probing, DownloadStatus::Downloading, DownloadStatus::Paused, DownloadStatus::Completed].into_iter().enumerate() {
        s.insert(&ids[i], Path::new("/d"), &new("https://x/f"), st, i as i64).unwrap();
    }
    assert_eq!(s.reset_interrupted(100).unwrap(), 2);
    let st: Vec<_> = ids.iter().map(|id| s.get(id).unwrap().unwrap().status).collect();
    assert_eq!(st, vec![DownloadStatus::Queued, DownloadStatus::Queued, DownloadStatus::Paused, DownloadStatus::Completed]);
}

#[test]
fn delete_removes_the_row() {
    let s = Store::open_in_memory().unwrap();
    let a = DownloadId::new();
    s.insert(&a, Path::new("/d"), &new("https://x/a"), DownloadStatus::Queued, 1).unwrap();
    s.delete(&a).unwrap();
    assert!(s.get(&a).unwrap().is_none());
}
```

- [ ] **Step 3: Run to verify it fails** — `cargo test -p mdm-core` → compile errors.

- [ ] **Step 4: Implement**

`src/error.rs`:
```rust
//! One error type for the core, with a stable code for the UI.

/// Everything the core can refuse or fail with.
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    /// SQLite failed.
    #[error("database: {0}")]
    Db(#[from] rusqlite::Error),
    /// No download with this id.
    #[error("not found: {0}")]
    NotFound(String),
    /// The URL cannot be downloaded (not http/https, not parseable).
    #[error("invalid URL: {0}")]
    InvalidUrl(String),
    /// The action does not fit the download's state (e.g. cancel a completed one).
    #[error("invalid state: {0}")]
    InvalidState(String),
    /// A settings value is out of range or unusable.
    #[error("settings: {0}")]
    Settings(String),
    /// The engine refused or failed.
    #[error(transparent)]
    Engine(#[from] mdm_engine::EngineError),
    /// File-system error outside the engine (deleting a part file…).
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
    /// A stored JSON column could not be read.
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

/// Result alias for the core.
pub type Result<T> = std::result::Result<T, CoreError>;

impl CoreError {
    /// Stable identifier for UI mapping; engine errors keep the engine's code.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Db(_) => "DB",
            Self::NotFound(_) => "NOT_FOUND",
            Self::InvalidUrl(_) => "INVALID_URL",
            Self::InvalidState(_) => "INVALID_STATE",
            Self::Settings(_) => "SETTINGS",
            Self::Engine(e) => e.code(),
            Self::Io(_) => "IO",
            Self::Json(_) => "JSON",
        }
    }
}
```
`src/model.rs`:
```rust
//! The records the store keeps and the manager publishes.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Milliseconds since the Unix epoch.
pub fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// A download's identity (a random UUID).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DownloadId(pub String);

impl DownloadId {
    /// A fresh random id.
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }
    /// The id as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for DownloadId {
    /// A fresh random id (clippy's `new_without_default`; no `#[allow]` in this repo).
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for DownloadId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Where a download is in its life. Mirrors the store's CHECK constraint.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DownloadStatus {
    /// Waiting for a free slot.
    Queued,
    /// Asking the server about the file.
    Probing,
    /// Bytes are moving.
    Downloading,
    /// Stopped by the user or at shutdown; resumable.
    Paused,
    /// The file is in place under its final name.
    Completed,
    /// Stopped by an error; see `error_code`.
    Failed,
    /// Stopped by the user; partial data deleted.
    Cancelled,
    /// A finished torrent that is uploading (Plan 5).
    Seeding,
}

impl DownloadStatus {
    /// The stored text.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "QUEUED",
            Self::Probing => "PROBING",
            Self::Downloading => "DOWNLOADING",
            Self::Paused => "PAUSED",
            Self::Completed => "COMPLETED",
            Self::Failed => "FAILED",
            Self::Cancelled => "CANCELLED",
            Self::Seeding => "SEEDING",
        }
    }
    /// Parse the stored text.
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "QUEUED" => Self::Queued,
            "PROBING" => Self::Probing,
            "DOWNLOADING" => Self::Downloading,
            "PAUSED" => Self::Paused,
            "COMPLETED" => Self::Completed,
            "FAILED" => Self::Failed,
            "CANCELLED" => Self::Cancelled,
            "SEEDING" => Self::Seeding,
            _ => return None,
        })
    }
}

/// What kind of transfer a row is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DownloadKind {
    /// HTTP(S) through `mdm-engine`.
    Http,
    /// BitTorrent (Plan 5).
    Torrent,
}

/// An extra request header saved with a download.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Header {
    /// Header name.
    pub name: String,
    /// Header value.
    pub value: String,
}

/// A browser cookie saved with a download.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedCookie {
    /// Cookie name.
    pub name: String,
    /// Cookie value.
    pub value: String,
}

/// A request to add a download.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NewDownload {
    /// http or https URL.
    pub url: String,
    /// Target folder; `None` = the settings' download folder.
    pub dir: Option<PathBuf>,
    /// Override the server's file name.
    pub filename: Option<String>,
    /// The page the link was on.
    pub referrer: Option<String>,
    /// Extra headers.
    pub headers: Vec<Header>,
    /// Browser cookies for the URL.
    pub cookies: Vec<SavedCookie>,
    /// Add as PAUSED instead of QUEUED.
    pub start_paused: bool,
}

/// What a probe learned, as the store keeps it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProbeInfo {
    /// URL after redirects.
    pub final_url: String,
    /// File name actually used (may be `name (1).ext`).
    pub filename: String,
    /// Total size when known.
    pub size: Option<u64>,
    /// ETag.
    pub etag: Option<String>,
    /// Last-Modified.
    pub last_modified: Option<String>,
    /// MIME type.
    pub mime: Option<String>,
}

/// One download as stored and published.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadRow {
    /// Identity.
    pub id: DownloadId,
    /// HTTP or torrent.
    pub kind: DownloadKind,
    /// The URL as added.
    pub url: String,
    /// The URL after redirects, once probed.
    pub final_url: Option<String>,
    /// File name, once known.
    pub filename: Option<String>,
    /// Target folder.
    pub dir: PathBuf,
    /// Total size when known.
    pub size: Option<u64>,
    /// Bytes saved (durable) so far; the size when completed.
    pub downloaded: u64,
    /// Current status.
    pub status: DownloadStatus,
    /// ETag at the first probe.
    pub etag: Option<String>,
    /// Last-Modified at the first probe.
    pub last_modified: Option<String>,
    /// MIME type.
    pub mime: Option<String>,
    /// Referring page.
    pub referrer: Option<String>,
    /// Extra headers.
    pub headers: Vec<Header>,
    /// Cookies.
    pub cookies: Vec<SavedCookie>,
    /// Stable error code when failed.
    pub error_code: Option<String>,
    /// Human-readable error.
    pub error_message: Option<String>,
    /// Unix ms.
    pub created_at: i64,
    /// Unix ms.
    pub updated_at: i64,
    /// Unix ms, when completed.
    pub completed_at: Option<i64>,
}
```
`src/store.rs`:
```rust
//! SQLite persistence: one file, schema versioned by `PRAGMA user_version`.
//! Calls are short and synchronous behind one mutex; the manager calls them
//! from async code, which is fine at this size (a few rows written a second).

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::error::{CoreError, Result};
use crate::model::*;

/// Newest schema this build knows.
pub const SCHEMA_VERSION: i64 = 1;

const SCHEMA_V1: &str = r#"
CREATE TABLE downloads (
    id            TEXT PRIMARY KEY,
    kind          TEXT NOT NULL CHECK (kind IN ('http', 'torrent')),
    url           TEXT NOT NULL,
    final_url     TEXT,
    filename      TEXT,
    dir           TEXT NOT NULL,
    size          INTEGER,
    status        TEXT NOT NULL CHECK (status IN ('QUEUED','PROBING','DOWNLOADING','PAUSED','COMPLETED','FAILED','CANCELLED','SEEDING')),
    etag          TEXT,
    last_modified TEXT,
    mime          TEXT,
    referrer      TEXT,
    headers_json  TEXT NOT NULL DEFAULT '[]',
    cookies_json  TEXT NOT NULL DEFAULT '[]',
    error_code    TEXT,
    error_message TEXT,
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL,
    completed_at  INTEGER
);
CREATE INDEX downloads_queue ON downloads (status, created_at);
CREATE TABLE segments (
    download_id TEXT NOT NULL REFERENCES downloads (id) ON DELETE CASCADE,
    idx         INTEGER NOT NULL,
    start_byte  INTEGER NOT NULL,
    end_byte    INTEGER NOT NULL,
    downloaded  INTEGER NOT NULL,
    PRIMARY KEY (download_id, idx)
);
CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
"#;

const ROW_SELECT: &str = "SELECT d.id, d.kind, d.url, d.final_url, d.filename, d.dir, d.size, d.status,
    d.etag, d.last_modified, d.mime, d.referrer, d.headers_json, d.cookies_json, d.error_code,
    d.error_message, d.created_at, d.updated_at, d.completed_at,
    (SELECT COALESCE(SUM(s.downloaded), 0) FROM segments s WHERE s.download_id = d.id)
    FROM downloads d";

/// The database.
pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    /// Open (creating folders and the file if needed) and migrate.
    pub fn open(path: &Path) -> Result<Store> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        Self::init(conn)
    }

    /// A private in-memory database (tests).
    pub fn open_in_memory() -> Result<Store> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Store> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version > SCHEMA_VERSION {
            return Err(CoreError::InvalidState(format!(
                "database schema {version} is newer than this build ({SCHEMA_VERSION})"
            )));
        }
        if version < 1 {
            conn.execute_batch(SCHEMA_V1)?;
            conn.pragma_update(None, "user_version", 1)?;
        }
        Ok(Store { conn: Mutex::new(conn) })
    }

    /// `PRAGMA user_version`.
    pub fn schema_version(&self) -> Result<i64> {
        Ok(self.conn.lock().unwrap().query_row("PRAGMA user_version", [], |r| r.get(0))?)
    }

    /// Add a row.
    pub fn insert(&self, id: &DownloadId, dir: &Path, new: &NewDownload, status: DownloadStatus, now: i64) -> Result<DownloadRow> {
        self.conn.lock().unwrap().execute(
            "INSERT INTO downloads (id, kind, url, filename, dir, status, referrer, headers_json, cookies_json, created_at, updated_at)
             VALUES (?1, 'http', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
            params![
                id.as_str(),
                new.url,
                new.filename,
                dir.to_string_lossy(),
                status.as_str(),
                new.referrer,
                serde_json::to_string(&new.headers)?,
                serde_json::to_string(&new.cookies)?,
                now
            ],
        )?;
        self.get(id)?.ok_or_else(|| CoreError::NotFound(id.to_string()))
    }

    /// One row.
    pub fn get(&self, id: &DownloadId) -> Result<Option<DownloadRow>> {
        let conn = self.conn.lock().unwrap();
        let row = conn
            .query_row(&format!("{ROW_SELECT} WHERE d.id = ?1"), [id.as_str()], raw_row)
            .optional()?;
        row.map(decode).transpose()
    }

    /// Every row, newest first.
    pub fn list(&self) -> Result<Vec<DownloadRow>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!("{ROW_SELECT} ORDER BY d.created_at DESC, d.rowid DESC"))?;
        let raws = stmt.query_map([], raw_row)?.collect::<std::result::Result<Vec<_>, _>>()?;
        raws.into_iter().map(decode).collect()
    }

    /// The oldest QUEUED row not in `exclude`.
    pub fn next_queued(&self, exclude: &[DownloadId]) -> Result<Option<DownloadId>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id FROM downloads WHERE status = 'QUEUED' ORDER BY created_at, rowid")?;
        let ids = stmt.query_map([], |r| r.get::<_, String>(0))?;
        for id in ids {
            let id = DownloadId(id?);
            if !exclude.contains(&id) {
                return Ok(Some(id));
            }
        }
        Ok(None)
    }

    /// Change the status; `error = None` clears any stored error.
    pub fn set_status(&self, id: &DownloadId, status: DownloadStatus, error: Option<(&str, &str)>, now: i64) -> Result<()> {
        let (code, message) = match error {
            Some((c, m)) => (Some(c), Some(m)),
            None => (None, None),
        };
        let completed_at = (status == DownloadStatus::Completed).then_some(now);
        let n = self.conn.lock().unwrap().execute(
            "UPDATE downloads SET status = ?2, error_code = ?3, error_message = ?4, updated_at = ?5, completed_at = ?6 WHERE id = ?1",
            params![id.as_str(), status.as_str(), code, message, now, completed_at],
        )?;
        found(n, id)
    }

    /// Record what the probe learned.
    pub fn set_probe(&self, id: &DownloadId, p: &ProbeInfo, now: i64) -> Result<()> {
        let n = self.conn.lock().unwrap().execute(
            "UPDATE downloads SET final_url = ?2, filename = ?3, size = ?4, etag = ?5, last_modified = ?6, mime = ?7, updated_at = ?8 WHERE id = ?1",
            params![id.as_str(), p.final_url, p.filename, p.size.map(|s| s as i64), p.etag, p.last_modified, p.mime, now],
        )?;
        found(n, id)
    }

    /// Change the file name (the engine may pick `name (1).ext`).
    pub fn set_filename(&self, id: &DownloadId, filename: &str, now: i64) -> Result<()> {
        let n = self.conn.lock().unwrap().execute(
            "UPDATE downloads SET filename = ?2, updated_at = ?3 WHERE id = ?1",
            params![id.as_str(), filename, now],
        )?;
        found(n, id)
    }

    /// At startup: rows that were probing or downloading when the app stopped go back to the queue.
    pub fn reset_interrupted(&self, now: i64) -> Result<usize> {
        Ok(self.conn.lock().unwrap().execute(
            "UPDATE downloads SET status = 'QUEUED', updated_at = ?1 WHERE status IN ('PROBING', 'DOWNLOADING')",
            [now],
        )?)
    }

    /// Delete a row and its segments.
    pub fn delete(&self, id: &DownloadId) -> Result<()> {
        self.conn.lock().unwrap().execute("DELETE FROM downloads WHERE id = ?1", [id.as_str()])?;
        Ok(())
    }

    pub(crate) fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap()
    }
}

fn found(n: usize, id: &DownloadId) -> Result<()> {
    if n == 0 {
        Err(CoreError::NotFound(id.to_string()))
    } else {
        Ok(())
    }
}

type Raw = (
    String, String, String, Option<String>, Option<String>, String, Option<i64>, String,
    Option<String>, Option<String>, Option<String>, Option<String>, String, String, Option<String>,
    Option<String>, i64, i64, Option<i64>, i64,
);

fn raw_row(r: &Row<'_>) -> rusqlite::Result<Raw> {
    Ok((
        r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?,
        r.get(8)?, r.get(9)?, r.get(10)?, r.get(11)?, r.get(12)?, r.get(13)?, r.get(14)?,
        r.get(15)?, r.get(16)?, r.get(17)?, r.get(18)?, r.get(19)?,
    ))
}

fn decode(raw: Raw) -> Result<DownloadRow> {
    let (id, kind, url, final_url, filename, dir, size, status, etag, last_modified, mime, referrer,
        headers, cookies, error_code, error_message, created_at, updated_at, completed_at, seg_sum) = raw;
    let status = DownloadStatus::parse(&status)
        .ok_or_else(|| CoreError::InvalidState(format!("unknown status {status}")))?;
    let size = size.map(|s| s as u64);
    Ok(DownloadRow {
        id: DownloadId(id),
        kind: if kind == "torrent" { DownloadKind::Torrent } else { DownloadKind::Http },
        url,
        final_url,
        filename,
        dir: PathBuf::from(dir),
        size,
        downloaded: if status == DownloadStatus::Completed { size.unwrap_or(0) } else { seg_sum as u64 },
        status,
        etag,
        last_modified,
        mime,
        referrer,
        headers: serde_json::from_str(&headers)?,
        cookies: serde_json::from_str(&cookies)?,
        error_code,
        error_message,
        created_at,
        updated_at,
        completed_at,
    })
}
```
(`cargo fmt` will reflow the tuple lines; that is fine. If clippy flags `type_complexity` on `Raw`, keep the alias — the alias IS the fix clippy asks for.)

- [ ] **Step 5: Run and gate** — `cargo test -p mdm-core` (6 passing) and the full gates.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat(core): mdm-core crate with the download model and the SQLite store

One file, schema versioned by user_version, the spec's tables with the
segment columns renamed off SQL keywords. The queue order, the startup
re-queue of interrupted rows and the status/error bookkeeping live here.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 8: Segments and settings in the store

**Files:**
- Create: `crates/mdm-core/src/settings.rs`
- Modify: `crates/mdm-core/src/store.rs`, `crates/mdm-core/src/lib.rs`
- Test: `crates/mdm-core/tests/store.rs`

**Interfaces:**
- Consumes: `mdm_engine::{SegmentState, EngineConfig, Proxy}`.
- Produces:
  ```rust
  impl Store {
      pub fn save_segments(&self, id: &DownloadId, segs: &[SegmentState]) -> Result<()>;   // replaces all, one transaction
      pub fn load_segments(&self, id: &DownloadId) -> Result<Vec<SegmentState>>;          // ordered by idx
      pub fn clear_segments(&self, id: &DownloadId) -> Result<()>;
      pub fn load_settings(&self, default_dir: &Path) -> Result<Settings>;                // missing → defaults with default_dir
      pub fn save_settings(&self, s: &Settings) -> Result<()>;
  }
  // settings.rs
  #[serde(rename_all = "camelCase", default)]
  pub struct Settings { pub download_dir: PathBuf, pub max_connections: u8, pub max_parallel: u8,
                        pub user_agent: Option<String>, pub proxy: ProxySetting }
  #[serde(tag = "mode", rename_all = "lowercase")]
  pub enum ProxySetting { System, None, Manual { url: String } }
  impl Default for Settings   // dir empty, 8, 3, None, System
  impl Settings {
      pub fn validated(self) -> Result<Settings>;         // clamps connections 1..=32, parallel 1..=10; empty dir → SETTINGS error; bad proxy URL → SETTINGS error
      pub fn engine_config(&self) -> Result<EngineConfig>;
  }
  ```
  Settings are one JSON value under key `settings` in the `settings` table; unknown or missing fields take defaults (`#[serde(default)]`), so older saves keep loading.

- [ ] **Step 1: Failing tests** — append to `tests/store.rs`:
```rust
use mdm_engine::SegmentState;

fn seg(idx: u32, start: u64, end: u64, downloaded: u64) -> SegmentState {
    SegmentState { idx, start, end, downloaded }
}

#[test]
fn segments_replace_and_sum_into_downloaded() {
    let s = Store::open_in_memory().unwrap();
    let a = DownloadId::new();
    s.insert(&a, Path::new("/d"), &new("https://x/a"), DownloadStatus::Queued, 1).unwrap();
    s.save_segments(&a, &[seg(1, 50, 99, 10), seg(0, 0, 49, 50)]).unwrap();
    assert_eq!(s.load_segments(&a).unwrap(), vec![seg(0, 0, 49, 50), seg(1, 50, 99, 10)]);
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
    s.insert(&a, Path::new("/d"), &new("https://x/a"), DownloadStatus::Queued, 1).unwrap();
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
    let mine = Settings { max_connections: 16, proxy: ProxySetting::Manual { url: "http://127.0.0.1:8080".into() }, ..d };
    s.save_settings(&mine).unwrap();
    assert_eq!(s.load_settings(Path::new("/elsewhere")).unwrap(), mine);
}

#[test]
fn settings_are_clamped_and_checked() {
    let base = Settings { download_dir: "/d".into(), ..Settings::default() };
    let v = Settings { max_connections: 200, max_parallel: 0, ..base.clone() }.validated().unwrap();
    assert_eq!((v.max_connections, v.max_parallel), (32, 1));
    assert_eq!(Settings::default().validated().unwrap_err().code(), "SETTINGS");
    let bad = Settings { proxy: ProxySetting::Manual { url: "not a url".into() }, ..base.clone() };
    assert_eq!(bad.validated().unwrap_err().code(), "SETTINGS");
    let cfg = base.engine_config().unwrap();
    assert_eq!(cfg.max_connections, 8);
}
```

- [ ] **Step 2: Run to verify it fails.**

- [ ] **Step 3: Implement**

`src/settings.rs`:
```rust
//! User settings: stored as one JSON value, defaults for anything missing.

use std::path::PathBuf;

use mdm_engine::{EngineConfig, Proxy, MAX_CONNECTIONS};
use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};

/// How downloads reach the network.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum ProxySetting {
    /// The OS / environment proxy.
    System,
    /// Direct.
    None,
    /// This proxy URL.
    Manual {
        /// e.g. `http://127.0.0.1:8080`.
        url: String,
    },
}

/// Everything the user can change.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Where new downloads go.
    pub download_dir: PathBuf,
    /// Connections per download (1–32).
    pub max_connections: u8,
    /// Downloads running at once (1–10).
    pub max_parallel: u8,
    /// Custom User-Agent; `None` = the engine's.
    pub user_agent: Option<String>,
    /// Proxy policy.
    pub proxy: ProxySetting,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            download_dir: PathBuf::new(),
            max_connections: 8,
            max_parallel: 3,
            user_agent: None,
            proxy: ProxySetting::System,
        }
    }
}

impl Settings {
    /// Clamp the numbers into range and refuse unusable values.
    pub fn validated(mut self) -> Result<Settings> {
        if self.download_dir.as_os_str().is_empty() {
            return Err(CoreError::Settings("the download folder is not set".into()));
        }
        self.max_connections = self.max_connections.clamp(1, MAX_CONNECTIONS);
        self.max_parallel = self.max_parallel.clamp(1, 10);
        if let ProxySetting::Manual { url } = &self.proxy {
            url::Url::parse(url).map_err(|e| CoreError::Settings(format!("proxy URL: {e}")))?;
        }
        Ok(self)
    }

    /// The engine configuration these settings describe.
    pub fn engine_config(&self) -> Result<EngineConfig> {
        let mut cfg = EngineConfig { max_connections: self.max_connections, ..EngineConfig::default() };
        if let Some(ua) = &self.user_agent {
            cfg.user_agent = ua.clone();
        }
        cfg.proxy = match &self.proxy {
            ProxySetting::System => Proxy::System,
            ProxySetting::None => Proxy::None,
            ProxySetting::Manual { url } => Proxy::Manual(
                url::Url::parse(url).map_err(|e| CoreError::Settings(format!("proxy URL: {e}")))?,
            ),
        };
        Ok(cfg)
    }
}
```
`store.rs` additions:
```rust
use mdm_engine::SegmentState;
use crate::settings::Settings;

impl Store {
    /// Replace a download's segments.
    pub fn save_segments(&self, id: &DownloadId, segs: &[SegmentState]) -> Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM segments WHERE download_id = ?1", [id.as_str()])?;
        {
            let mut ins = tx.prepare(
                "INSERT INTO segments (download_id, idx, start_byte, end_byte, downloaded) VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            for s in segs {
                ins.execute(params![id.as_str(), s.idx, s.start as i64, s.end as i64, s.downloaded as i64])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// A download's segments, by idx.
    pub fn load_segments(&self, id: &DownloadId) -> Result<Vec<SegmentState>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT idx, start_byte, end_byte, downloaded FROM segments WHERE download_id = ?1 ORDER BY idx",
        )?;
        let rows = stmt.query_map([id.as_str()], |r| {
            Ok(SegmentState {
                idx: r.get(0)?,
                start: r.get::<_, i64>(1)? as u64,
                end: r.get::<_, i64>(2)? as u64,
                downloaded: r.get::<_, i64>(3)? as u64,
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    /// Forget a download's segments (a fresh start follows).
    pub fn clear_segments(&self, id: &DownloadId) -> Result<()> {
        self.conn().execute("DELETE FROM segments WHERE download_id = ?1", [id.as_str()])?;
        Ok(())
    }

    /// Stored settings, or defaults with `default_dir` as the download folder.
    pub fn load_settings(&self, default_dir: &Path) -> Result<Settings> {
        let raw: Option<String> = self
            .conn()
            .query_row("SELECT value FROM settings WHERE key = 'settings'", [], |r| r.get(0))
            .optional()?;
        let mut s = match raw {
            Some(json) => serde_json::from_str(&json)?,
            None => Settings::default(),
        };
        if s.download_dir.as_os_str().is_empty() {
            s.download_dir = default_dir.to_owned();
        }
        Ok(s)
    }

    /// Save settings.
    pub fn save_settings(&self, s: &Settings) -> Result<()> {
        self.conn().execute(
            "INSERT INTO settings (key, value) VALUES ('settings', ?1) ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            [serde_json::to_string(s)?],
        )?;
        Ok(())
    }
}
```
(`SegmentState.idx` is `u32`; rusqlite converts `u32` both ways.) `lib.rs`: `pub mod settings; pub use settings::{ProxySetting, Settings};`. Add `url.workspace = true` is already a dependency.

- [ ] **Step 4: Run and gate**, **Step 5: Commit**

```bash
git add -A
git commit -m "feat(core): segments and settings in the store

Segments are replaced whole in one transaction and summed into each
row's saved-bytes figure; settings are one JSON value with defaults for
anything missing, clamped and checked before the engine sees them.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 9: The manager — add, queue, drive to completion, events

**Files:**
- Create: `crates/mdm-core/src/events.rs`, `crates/mdm-core/src/manager.rs`, `crates/mdm-core/tests/manager.rs`
- Modify: `crates/mdm-core/src/lib.rs`, `crates/mdm-core/src/store.rs` (`set_size`)

**Interfaces:**
- Consumes: `Store` (Tasks 7–8), `Settings`, engine `Engine::start`, `DownloadHandle::{probe, filename, control, subscribe, wait}` (Tasks 3, 5), `Progress.durable_segments` (Task 4), `PART_SUFFIX`.
- Produces:
  ```rust
  // events.rs  (serde Serialize; tag "type", camelCase variants and fields)
  pub struct SegmentView { pub start: u64, pub end: u64, pub downloaded: u64 }     // From<&SegmentState>
  pub enum ManagerEvent {
      Added { download: DownloadRow },
      Updated { download: DownloadRow },          // any status / probe / name change: the whole row
      Progress { id: DownloadId, total: Option<u64>, downloaded: u64, speed_bps: u64, eta_secs: Option<u64>, segments: Vec<SegmentView> },
      Removed { id: DownloadId },
      Notice { id: DownloadId, message: String },
  }
  // manager.rs
  pub const PERSIST_INTERVAL: Duration = Duration::from_secs(1);
  #[derive(Clone)] pub struct Manager { /* Arc<Inner> */ }
  impl Manager {
      pub fn open(db: &Path, default_download_dir: &Path) -> Result<Manager>;          // inside a tokio runtime
      pub fn with_store(store: Store, default_download_dir: &Path) -> Result<Manager>;
      pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<ManagerEvent>;
      pub fn list(&self) -> Result<Vec<DownloadRow>>;
      pub fn get(&self, id: &DownloadId) -> Result<Option<DownloadRow>>;
      pub fn segments(&self, id: &DownloadId) -> Result<Vec<mdm_engine::SegmentState>>;
      pub fn settings(&self) -> Settings;
      pub fn set_settings(&self, s: Settings) -> Result<Settings>;                     // validates, rebuilds the engine, saves, reschedules
      pub fn add(&self, new: NewDownload) -> Result<DownloadRow>;                      // http/https only → INVALID_URL
  }
  // store.rs
  pub fn set_size(&self, id: &DownloadId, size: u64, now: i64) -> Result<()>;
  ```
  Driving one download: `PROBING` → `Engine::start` (resume when the row has a size and stored segments) → `set_probe` (the engine's actual file name) → `DOWNLOADING` → progress events for every engine update, durable segments saved every `PERSIST_INTERVAL` → outcome. `Completed`: segments cleared, file name from the final path, size from the file when it was unknown, `COMPLETED`. A failure: segments saved, `FAILED` with the engine's code and message. A finished slot frees a place in the queue at once.

- [ ] **Step 1: Failing tests** — `crates/mdm-core/tests/manager.rs`:
```rust
use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::Duration;

use mdm_core::*;
use mdm_test_server::*;

pub fn manager(dir: &Path) -> Manager {
    let m = Manager::with_store(Store::open_in_memory().unwrap(), dir).unwrap();
    m.set_settings(Settings { max_connections: 4, ..m.settings() }).unwrap();
    m
}

pub fn add(m: &Manager, url: &str) -> DownloadId {
    m.add(NewDownload { url: url.into(), ..Default::default() }).unwrap().id
}

pub async fn wait_for(m: &Manager, id: &DownloadId, want: DownloadStatus) -> DownloadRow {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        let row = m.get(id).unwrap().unwrap();
        if row.status == want {
            return row;
        }
        assert!(tokio::time::Instant::now() < deadline, "timed out waiting for {want:?}; last row: {row:?}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn a_download_is_driven_to_completion() {
    let s = TestServer::start(4 * 1024 * 1024).await;
    let d = tempfile::tempdir().unwrap();
    let m = manager(d.path());
    let id = add(&m, &s.file_url());
    let row = wait_for(&m, &id, DownloadStatus::Completed).await;
    assert_eq!(row.filename.as_deref(), Some("file"));
    assert_eq!((row.size, row.downloaded), (Some(4 * 1024 * 1024), 4 * 1024 * 1024));
    assert!(row.completed_at.is_some());
    assert_eq!(sha256_file(&d.path().join("file")), sha256_bytes(&s.data));
    assert!(m.segments(&id).unwrap().is_empty());
}

#[tokio::test]
async fn events_announce_the_life_of_a_download() {
    let s = TestServer::start(2 * 1024 * 1024).await;
    s.cfg.chunk_delay_ms.store(2, Ordering::SeqCst);
    let d = tempfile::tempdir().unwrap();
    let m = manager(d.path());
    let mut rx = m.subscribe();
    let id = add(&m, &s.file_url());
    let mut kinds = Vec::new();
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            match rx.recv().await.unwrap() {
                ManagerEvent::Added { download } if download.id == id => kinds.push("added".to_string()),
                ManagerEvent::Updated { download } if download.id == id => {
                    kinds.push(format!("{:?}", download.status));
                    if download.status == DownloadStatus::Completed {
                        break;
                    }
                }
                ManagerEvent::Progress { id: pid, .. } if pid == id => kinds.push("progress".into()),
                _ => {}
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(kinds.first().map(String::as_str), Some("added"));
    for want in ["Probing", "Downloading", "progress", "Completed"] {
        assert!(kinds.iter().any(|k| k == want), "missing {want} in {kinds:?}");
    }
}

#[tokio::test]
async fn the_queue_runs_at_most_max_parallel_in_fifo_order() {
    let s = TestServer::start(1024 * 1024).await;
    s.cfg.chunk_delay_ms.store(5, Ordering::SeqCst);
    let d = tempfile::tempdir().unwrap();
    let m = manager(d.path());
    m.set_settings(Settings { max_parallel: 1, ..m.settings() }).unwrap();
    let ids: Vec<_> = (0..3).map(|_| add(&m, &s.file_url())).collect();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    loop {
        let rows = m.list().unwrap();
        let active = rows.iter().filter(|r| matches!(r.status, DownloadStatus::Probing | DownloadStatus::Downloading)).count();
        assert!(active <= 1, "{active} running with max_parallel 1");
        if rows.iter().all(|r| r.status == DownloadStatus::Completed) {
            break;
        }
        assert!(tokio::time::Instant::now() < deadline, "queue did not finish");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let done: Vec<i64> = ids.iter().map(|id| m.get(id).unwrap().unwrap().completed_at.unwrap()).collect();
    assert!(done.windows(2).all(|w| w[0] <= w[1]), "completed in add order: {done:?}");
    let names: Vec<String> = ids.iter().map(|id| m.get(id).unwrap().unwrap().filename.unwrap()).collect();
    assert_eq!(names, vec!["file", "file (1)", "file (2)"]);
}

#[tokio::test]
async fn a_failed_download_records_the_engine_code() {
    let s = TestServer::start(10).await;
    let d = tempfile::tempdir().unwrap();
    let m = manager(d.path());
    let id = add(&m, &s.status_url(404));
    let row = wait_for(&m, &id, DownloadStatus::Failed).await;
    assert_eq!(row.error_code.as_deref(), Some("HTTP_STATUS"));
}

#[tokio::test]
async fn only_http_urls_are_accepted() {
    let d = tempfile::tempdir().unwrap();
    let m = manager(d.path());
    let e = m.add(NewDownload { url: "ftp://example.com/x".into(), ..Default::default() }).unwrap_err();
    assert_eq!(e.code(), "INVALID_URL");
    let e = m.add(NewDownload { url: "not a url".into(), ..Default::default() }).unwrap_err();
    assert_eq!(e.code(), "INVALID_URL");
}

#[tokio::test]
async fn a_stream_of_unknown_size_records_its_size_on_completion() {
    let s = TestServer::start(50_000).await;
    s.cfg.chunked.store(true, Ordering::SeqCst);
    let d = tempfile::tempdir().unwrap();
    let m = manager(d.path());
    let id = add(&m, &s.file_url());
    let row = wait_for(&m, &id, DownloadStatus::Completed).await;
    assert_eq!((row.size, row.downloaded), (Some(50_000), 50_000));
}
```

- [ ] **Step 2: Run to verify it fails** — compile errors.

- [ ] **Step 3: Implement**

`src/events.rs`:
```rust
//! What the manager announces; the desktop app forwards these to the UI.

use mdm_engine::SegmentState;
use serde::Serialize;

use crate::model::{DownloadId, DownloadRow};

/// One segment as the UI draws it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SegmentView {
    /// First byte.
    pub start: u64,
    /// Last byte, inclusive.
    pub end: u64,
    /// Bytes written from `start`.
    pub downloaded: u64,
}

impl From<&SegmentState> for SegmentView {
    fn from(s: &SegmentState) -> Self {
        Self { start: s.start, end: s.end, downloaded: s.downloaded }
    }
}

/// Something changed.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ManagerEvent {
    /// A download was added.
    Added {
        /// The new row.
        download: DownloadRow,
    },
    /// A row changed (status, probe result, name).
    Updated {
        /// The row as it is now.
        download: DownloadRow,
    },
    /// Live progress of a running download (every engine update, ~4 per second).
    Progress {
        /// Which download.
        id: DownloadId,
        /// Total size when known.
        total: Option<u64>,
        /// Bytes written.
        downloaded: u64,
        /// Bytes per second.
        speed_bps: u64,
        /// Seconds left.
        eta_secs: Option<u64>,
        /// The segment map.
        segments: Vec<SegmentView>,
    },
    /// A row was removed.
    Removed {
        /// Which download.
        id: DownloadId,
    },
    /// Something the user should read (e.g. "starting over").
    Notice {
        /// Which download.
        id: DownloadId,
        /// Plain English.
        message: String,
    },
}
```
`store.rs`: `pub fn set_size(&self, id, size: u64, now: i64) -> Result<()>` — `UPDATE downloads SET size = ?2, updated_at = ?3 WHERE id = ?1`, `NotFound` on 0 rows.

`src/manager.rs` (this task: everything except pause / resume / cancel / remove / restart / shutdown / crash, which Task 10 adds — but write `Slot`, the intents and `settle` now so Task 10 only adds entry points):
```rust
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

    /// Ask the driver to stop for `intent`; reaches the engine directly when it runs.
    pub(crate) fn signal(&self, intent: u8) {
        self.intent.store(intent, Ordering::SeqCst);
        self.token.cancel();
        if let Some(c) = self.control.lock().unwrap().as_ref() {
            if matches!(intent, INTENT_PAUSE | INTENT_SHUTDOWN) {
                c.pause();
            } else {
                c.cancel();
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
    pub fn set_settings(&self, s: Settings) -> Result<Settings> {
        let s = s.validated()?;
        let engine = Engine::new(s.engine_config()?)?;
        self.inner.store.save_settings(&s)?;
        *self.inner.engine.lock().unwrap() = engine;
        *self.inner.settings.lock().unwrap() = s.clone();
        Inner::schedule(&self.inner);
        Ok(s)
    }

    /// Add a download (QUEUED, or PAUSED with `start_paused`).
    pub fn add(&self, mut new: NewDownload) -> Result<DownloadRow> {
        let url = Url::parse(new.url.trim()).map_err(|e| CoreError::InvalidUrl(format!("{}: {e}", new.url)))?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(CoreError::InvalidUrl(format!("unsupported scheme {}", url.scheme())));
        }
        new.url = url.to_string();
        let dir = new.dir.clone().unwrap_or_else(|| self.inner.settings.lock().unwrap().download_dir.clone());
        let status = if new.start_paused { DownloadStatus::Paused } else { DownloadStatus::Queued };
        let row = self.inner.store.insert(&DownloadId::new(), &dir, &new, status, now_ms())?;
        self.inner.emit(ManagerEvent::Added { download: row.clone() });
        Inner::schedule(&self.inner);
        Ok(row)
    }
}

impl Inner {
    pub(crate) fn emit(&self, e: ManagerEvent) {
        let _ = self.events.send(e);
    }

    pub(crate) fn slot(&self, id: &DownloadId) -> Option<Slot> {
        self.running.lock().unwrap().get(id).cloned()
    }

    pub(crate) fn require(&self, id: &DownloadId) -> Result<DownloadRow> {
        self.store.get(id)?.ok_or_else(|| CoreError::NotFound(id.to_string()))
    }

    pub(crate) fn set_status(&self, id: &DownloadId, status: DownloadStatus, error: Option<(&str, &str)>) -> Result<()> {
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
        self.emit(ManagerEvent::Notice { id: id.clone(), message });
    }

    fn part_path(row: &DownloadRow) -> Option<PathBuf> {
        row.filename.as_ref().map(|f| row.dir.join(format!("{f}{PART_SUFFIX}")))
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
    let mut headers: Vec<(String, String)> = row.headers.iter().map(|h| (h.name.clone(), h.value.clone())).collect();
    if let Some(r) = &row.referrer {
        headers.push(("Referer".into(), r.clone()));
    }
    RequestExtras {
        headers,
        cookies: row.cookies.iter().map(|c| Cookie { name: c.name.clone(), value: c.value.clone() }).collect(),
    }
}

/// The saved state no longer fits the server: start over from byte 0 (once).
fn needs_fresh_start(e: &EngineError) -> bool {
    matches!(e, EngineError::RangeNotSupported | EngineError::InvalidResume(_))
        || matches!(e, EngineError::Io(io) if io.kind() == std::io::ErrorKind::NotFound)
}

async fn drive(inner: Arc<Inner>, id: DownloadId, slot: Slot) {
    if let Err(e) = drive_inner(&inner, &id, &slot).await {
        tracing::error!(%id, error = %e, "driving a download failed");
        let _ = inner.set_status(&id, DownloadStatus::Failed, Some((e.code(), &e.to_string())));
    }
    inner.running.lock().unwrap().remove(&id);
    Inner::schedule(&inner);
}

async fn drive_inner(inner: &Arc<Inner>, id: &DownloadId, slot: &Slot) -> Result<()> {
    let mut started_over = false;
    loop {
        let Some(row) = inner.store.get(id)? else { return Ok(()) };
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
        let forwarder = tokio::spawn(forward_progress(inner.clone(), id.clone(), handle.subscribe()));
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
                            "The server stopped accepting ranged requests; starting over as one stream".into(),
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
```
`lib.rs`: `pub mod events; pub mod manager; pub use events::{ManagerEvent, SegmentView}; pub use manager::{Manager, PERSIST_INTERVAL};`.

- [ ] **Step 4: Run and gate** — `cargo test -p mdm-core --test manager` (twice) and the full gates.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(core): the manager queues and drives downloads through the engine

A FIFO queue with max_parallel slots; each download is probed,
recorded, downloaded with its durable progress saved every second, and
finished or failed with the engine's code. Every change is broadcast as
an event the desktop app will forward to the UI.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 10: Pause, resume, cancel, remove, restart, shutdown

**Files:**
- Modify: `crates/mdm-core/src/manager.rs`
- Test: `crates/mdm-core/tests/manager.rs`

**Interfaces:**
- Consumes: `Slot::signal`, `Inner::{settle, discard_partial, remove_now, set_status, require, slot}`, intents (Task 9).
- Produces:
  ```rust
  impl Manager {
      pub fn pause(&self, id: &DownloadId) -> Result<()>;        // running → engine pause → PAUSED (segments saved); QUEUED → PAUSED; others: no-op
      pub fn resume(&self, id: &DownloadId) -> Result<()>;       // PAUSED / FAILED / CANCELLED → QUEUED (error cleared); running / QUEUED: no-op; COMPLETED → INVALID_STATE
      pub fn cancel(&self, id: &DownloadId) -> Result<()>;       // part file + segments deleted → CANCELLED; COMPLETED → INVALID_STATE
      pub fn remove(&self, id: &DownloadId, delete_file: bool) -> Result<()>;   // row gone (Removed event); part file always deleted; the finished file only with delete_file
      pub fn restart(&self, id: &DownloadId) -> Result<()>;      // not running: partial data dropped → QUEUED from byte 0; running or COMPLETED → INVALID_STATE
      pub fn pause_all(&self) -> Result<()>;
      pub fn resume_all(&self) -> Result<()>;
      pub async fn shutdown(&self);                              // stop scheduling; pause everything running and wait; those rows end QUEUED
      #[doc(hidden)] pub fn simulate_crash(&self);               // tests: abort every driver without saving anything more
  }
  ```
  Every entry point returns `NOT_FOUND` for an unknown id.

- [ ] **Step 1: Failing tests** — append to `tests/manager.rs`:
```rust
/// Wait for a Progress event of `id` with at least `min` bytes.
async fn wait_bytes(rx: &mut tokio::sync::broadcast::Receiver<ManagerEvent>, id: &DownloadId, min: u64) {
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            if let Ok(ManagerEvent::Progress { id: pid, downloaded, .. }) = rx.recv().await {
                if &pid == id && downloaded >= min {
                    return;
                }
            }
        }
    })
    .await
    .expect("progress arrived");
}

/// An 8 MiB download parked at 4 000 bytes (four connections, four hung bodies).
async fn parked(s: &TestServer, m: &Manager) -> DownloadId {
    s.cfg.hang_first.store(4, Ordering::SeqCst);
    let mut rx = m.subscribe();
    let id = add(m, &s.file_url());
    wait_bytes(&mut rx, &id, 4000).await;
    id
}

#[tokio::test]
async fn pause_then_resume_completes_with_the_right_bytes() {
    let s = TestServer::start(8 * 1024 * 1024).await;
    let d = tempfile::tempdir().unwrap();
    let m = manager(d.path());
    let id = parked(&s, &m).await;
    m.pause(&id).unwrap();
    wait_for(&m, &id, DownloadStatus::Paused).await;
    let saved: u64 = m.segments(&id).unwrap().iter().map(|x| x.downloaded).sum();
    assert!(saved >= 4000, "the paused state was saved: {saved}");
    assert!(d.path().join("file.mdm.part").exists());
    s.cfg.hang_first.store(0, Ordering::SeqCst);
    m.resume(&id).unwrap();
    wait_for(&m, &id, DownloadStatus::Completed).await;
    assert_eq!(sha256_file(&d.path().join("file")), sha256_bytes(&s.data));
}

#[tokio::test]
async fn a_queued_download_pauses_without_starting() {
    let s = TestServer::start(1024).await;
    let d = tempfile::tempdir().unwrap();
    let m = manager(d.path());
    let id = m.add(NewDownload { url: s.file_url(), start_paused: true, ..Default::default() }).unwrap().id;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(m.get(&id).unwrap().unwrap().status, DownloadStatus::Paused);
    assert_eq!(s.cfg.requests.load(Ordering::SeqCst), 0);
    m.resume(&id).unwrap();
    wait_for(&m, &id, DownloadStatus::Completed).await;
}

#[tokio::test]
async fn cancel_deletes_the_partial_data() {
    let s = TestServer::start(8 * 1024 * 1024).await;
    let d = tempfile::tempdir().unwrap();
    let m = manager(d.path());
    let id = parked(&s, &m).await;
    m.cancel(&id).unwrap();
    wait_for(&m, &id, DownloadStatus::Cancelled).await;
    assert!(!d.path().join("file.mdm.part").exists());
    assert!(m.segments(&id).unwrap().is_empty());
}

#[tokio::test]
async fn remove_while_running_deletes_the_row_and_the_part_file() {
    let s = TestServer::start(8 * 1024 * 1024).await;
    let d = tempfile::tempdir().unwrap();
    let m = manager(d.path());
    let id = parked(&s, &m).await;
    let mut rx = m.subscribe();
    m.remove(&id, false).unwrap();
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            if let Ok(ManagerEvent::Removed { id: rid }) = rx.recv().await {
                if rid == id {
                    return;
                }
            }
        }
    })
    .await
    .unwrap();
    assert!(m.get(&id).unwrap().is_none());
    assert!(!d.path().join("file.mdm.part").exists());
}

#[tokio::test]
async fn remove_a_completed_download_keeps_or_deletes_the_file_on_request() {
    let s = TestServer::start(1024).await;
    let d = tempfile::tempdir().unwrap();
    let m = manager(d.path());
    let keep = add(&m, &s.file_url());
    wait_for(&m, &keep, DownloadStatus::Completed).await;
    m.remove(&keep, false).unwrap();
    assert!(d.path().join("file").exists());
    let gone = add(&m, &s.file_url());
    let row = wait_for(&m, &gone, DownloadStatus::Completed).await;
    let name = row.filename.unwrap();
    m.remove(&gone, true).unwrap();
    assert!(!d.path().join(name).exists());
    assert!(m.get(&gone).unwrap().is_none());
}

#[tokio::test]
async fn restart_starts_a_failed_download_from_zero() {
    let s = TestServer::start(1024 * 1024).await;
    let d = tempfile::tempdir().unwrap();
    let m = manager(d.path());
    // A probe-level failure is immediate (the probe does not retry): HEAD is
    // refused and the GET bytes=0-0 fallback answers 503. Segment-level 503s
    // would retry ten times with the default 1 s base delay — minutes.
    s.cfg.head_allowed.store(false, Ordering::SeqCst);
    s.cfg.fail_first.store(1000, Ordering::SeqCst);
    let id = add(&m, &s.file_url());
    let row = wait_for(&m, &id, DownloadStatus::Failed).await;
    assert_eq!(row.error_code.as_deref(), Some("HTTP_STATUS"));
    s.cfg.head_allowed.store(true, Ordering::SeqCst);
    s.cfg.fail_first.store(0, Ordering::SeqCst);
    m.restart(&id).unwrap();
    wait_for(&m, &id, DownloadStatus::Completed).await;
    assert_eq!(sha256_file(&d.path().join("file")), sha256_bytes(&s.data));
    assert_eq!(m.restart(&id).unwrap_err().code(), "INVALID_STATE");
}

#[tokio::test]
async fn shutdown_saves_running_downloads_and_queues_them_for_next_launch() {
    let s = TestServer::start(8 * 1024 * 1024).await;
    let d = tempfile::tempdir().unwrap();
    let m = manager(d.path());
    let id = parked(&s, &m).await;
    tokio::time::timeout(Duration::from_secs(20), m.shutdown()).await.unwrap();
    let row = m.get(&id).unwrap().unwrap();
    assert_eq!(row.status, DownloadStatus::Queued);
    assert!(row.downloaded >= 4000);
}

#[tokio::test]
async fn unknown_ids_are_not_found() {
    let d = tempfile::tempdir().unwrap();
    let m = manager(d.path());
    let x = DownloadId::new();
    for e in [m.pause(&x), m.resume(&x), m.cancel(&x), m.remove(&x, false), m.restart(&x)] {
        assert_eq!(e.unwrap_err().code(), "NOT_FOUND");
    }
}
```

- [ ] **Step 2: Run to verify it fails** (methods missing).

- [ ] **Step 3: Implement** in `manager.rs` `impl Manager`:
```rust
    /// Pause: a running download stops and saves its state; a queued one simply waits.
    pub fn pause(&self, id: &DownloadId) -> Result<()> {
        if let Some(slot) = self.inner.slot(id) {
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
    pub fn resume(&self, id: &DownloadId) -> Result<()> {
        if self.inner.slot(id).is_some() {
            return Ok(());
        }
        let row = self.inner.require(id)?;
        match row.status {
            DownloadStatus::Paused | DownloadStatus::Failed | DownloadStatus::Cancelled => {
                self.inner.set_status(id, DownloadStatus::Queued, None)?;
                Inner::schedule(&self.inner);
                Ok(())
            }
            DownloadStatus::Queued => Ok(()),
            s => Err(CoreError::InvalidState(format!("cannot resume a {} download", s.as_str()))),
        }
    }

    /// Cancel: stop and delete the partial data.
    pub fn cancel(&self, id: &DownloadId) -> Result<()> {
        if let Some(slot) = self.inner.slot(id) {
            slot.signal(INTENT_CANCEL);
            return Ok(());
        }
        let row = self.inner.require(id)?;
        match row.status {
            DownloadStatus::Completed => Err(CoreError::InvalidState("the download is complete".into())),
            DownloadStatus::Cancelled => Ok(()),
            _ => {
                self.inner.discard_partial(&row)?;
                self.inner.set_status(id, DownloadStatus::Cancelled, None)
            }
        }
    }

    /// Remove from the list; `delete_file` also deletes a finished file.
    pub fn remove(&self, id: &DownloadId, delete_file: bool) -> Result<()> {
        if let Some(slot) = self.inner.slot(id) {
            slot.delete_file.store(delete_file, Ordering::SeqCst);
            slot.signal(INTENT_REMOVE);
            return Ok(());
        }
        let row = self.inner.require(id)?;
        self.inner.remove_now(&row, delete_file)
    }

    /// Start over from byte 0 (after SOURCE_CHANGED, for example).
    pub fn restart(&self, id: &DownloadId) -> Result<()> {
        let row = self.inner.require(id)?;
        if self.inner.slot(id).is_some() {
            return Err(CoreError::InvalidState("pause the download before restarting it".into()));
        }
        if row.status == DownloadStatus::Completed {
            return Err(CoreError::InvalidState("the download is complete".into()));
        }
        self.inner.discard_partial(&row)?;
        self.inner.set_status(id, DownloadStatus::Queued, None)?;
        Inner::schedule(&self.inner);
        Ok(())
    }

    /// Pause everything queued or running.
    pub fn pause_all(&self) -> Result<()> {
        for row in self.list()? {
            if matches!(row.status, DownloadStatus::Queued | DownloadStatus::Probing | DownloadStatus::Downloading) {
                self.pause(&row.id)?;
            }
        }
        Ok(())
    }

    /// Resume everything paused.
    pub fn resume_all(&self) -> Result<()> {
        for row in self.list()? {
            if row.status == DownloadStatus::Paused {
                self.resume(&row.id)?;
            }
        }
        Ok(())
    }

    /// Before the app exits: stop scheduling, pause everything running, wait
    /// until each has saved its state. Those rows are QUEUED again, so they
    /// continue at the next launch.
    pub async fn shutdown(&self) {
        self.inner.closing.store(true, Ordering::SeqCst);
        let slots: Vec<Slot> = self.inner.running.lock().unwrap().values().cloned().collect();
        for s in slots {
            s.signal(INTENT_SHUTDOWN);
        }
        let tasks = std::mem::take(&mut *self.inner.tasks.lock().unwrap());
        for t in tasks {
            let _ = t.await;
        }
    }

    /// Tests only: stop every driver the way a crash would, saving nothing more.
    #[doc(hidden)]
    pub fn simulate_crash(&self) {
        self.inner.closing.store(true, Ordering::SeqCst);
        for t in self.inner.tasks.lock().unwrap().drain(..) {
            t.abort();
        }
    }
```
(`Slot` must be nameable here: it is `pub(crate)` in the same module — fine.)

- [ ] **Step 4: Run and gate** (manager tests twice), **Step 5: Commit**

```bash
git add -A
git commit -m "feat(core): pause, resume, cancel, remove, restart and a clean shutdown

A signal reaches a running download through its engine control, or its
probe if it is still asking the server; a waiting one just changes
state. Closing the app pauses and saves every running download and
queues it for the next launch.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 11: Crash recovery and the automatic fresh start

**Files:**
- Test: `crates/mdm-core/tests/manager.rs` (behaviour already written in Tasks 9–10; this task proves it end to end and fixes what the proofs expose)
- Modify (only if a test exposes a bug): `crates/mdm-core/src/manager.rs`, `crates/mdm-core/src/store.rs`

**Interfaces:**
- Consumes: `Manager::{open, simulate_crash, pause, resume, restart}`, `Store::open` on the same file.
- Produces: no new API. Pins: (1) a crash leaves the row `DOWNLOADING` with durable segments; the next `Manager::open` resumes it from those segments and completes it; (2) a resume refused because the server lost range support starts over once, with a `Notice`, and completes; (3) `SOURCE_CHANGED` is NOT restarted automatically: `FAILED` with that code until `restart`.

- [ ] **Step 1: Write the tests** — append to `tests/manager.rs`:
```rust
#[tokio::test]
async fn a_crash_resumes_from_the_saved_segments_at_the_next_launch() {
    let s = TestServer::start(12 * 1024 * 1024).await;
    s.cfg.chunk_delay_ms.store(40, Ordering::SeqCst); // ~2 s for the whole file
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("state").join("mdm.db");
    let a = Manager::open(&db, d.path()).unwrap();
    a.set_settings(Settings { max_connections: 4, ..a.settings() }).unwrap();
    let id = add(&a, &s.file_url());
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    while a.get(&id).unwrap().unwrap().downloaded == 0 {
        assert!(tokio::time::Instant::now() < deadline, "no durable progress was saved");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    a.simulate_crash();
    // The aborted driver dropped its handle, which pauses the engine; give the
    // engine's own task a moment to stop writing before a new one opens the file.
    tokio::time::sleep(Duration::from_millis(500)).await;
    drop(a);
    {
        let store = Store::open(&db).unwrap();
        let row = store.get(&id).unwrap().unwrap();
        assert_eq!(row.status, DownloadStatus::Downloading, "a crash saves no status");
        assert!(store.load_segments(&id).unwrap().iter().map(|x| x.downloaded).sum::<u64>() > 0);
    }
    s.cfg.chunk_delay_ms.store(0, Ordering::SeqCst);
    let before = s.cfg.requests.load(Ordering::SeqCst);
    let b = Manager::open(&db, d.path()).unwrap();
    wait_for(&b, &id, DownloadStatus::Completed).await;
    assert_eq!(sha256_file(&d.path().join("file")), sha256_bytes(&s.data));
    assert!(s.cfg.requests.load(Ordering::SeqCst) > before);
}

#[tokio::test]
async fn a_server_that_lost_ranges_is_restarted_once_as_one_stream() {
    let s = TestServer::start(8 * 1024 * 1024).await;
    let d = tempfile::tempdir().unwrap();
    let m = manager(d.path());
    let id = parked(&s, &m).await;
    m.pause(&id).unwrap();
    wait_for(&m, &id, DownloadStatus::Paused).await;
    s.cfg.hang_first.store(0, Ordering::SeqCst);
    s.cfg.ranges.store(false, Ordering::SeqCst);
    let mut rx = m.subscribe();
    m.resume(&id).unwrap();
    wait_for(&m, &id, DownloadStatus::Completed).await;
    assert_eq!(sha256_file(&d.path().join("file")), sha256_bytes(&s.data));
    let mut noticed = false;
    while let Ok(ev) = rx.try_recv() {
        noticed |= matches!(ev, ManagerEvent::Notice { id: ref n, .. } if n == &id);
    }
    assert!(noticed, "the user was told the download started over");
}

#[tokio::test]
async fn a_changed_file_waits_for_the_user_to_restart() {
    let s = TestServer::start(8 * 1024 * 1024).await;
    let d = tempfile::tempdir().unwrap();
    let m = manager(d.path());
    let id = parked(&s, &m).await;
    m.pause(&id).unwrap();
    wait_for(&m, &id, DownloadStatus::Paused).await;
    s.cfg.hang_first.store(0, Ordering::SeqCst);
    *s.cfg.etag.lock().unwrap() = "\"v2\"".into();
    m.resume(&id).unwrap();
    let row = wait_for(&m, &id, DownloadStatus::Failed).await;
    assert_eq!(row.error_code.as_deref(), Some("SOURCE_CHANGED"));
    m.restart(&id).unwrap();
    wait_for(&m, &id, DownloadStatus::Completed).await;
    assert_eq!(sha256_file(&d.path().join("file")), sha256_bytes(&s.data));
}

#[tokio::test]
async fn settings_survive_a_restart_of_the_app() {
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("mdm.db");
    let a = Manager::open(&db, d.path()).unwrap();
    a.set_settings(Settings { max_parallel: 5, max_connections: 12, ..a.settings() }).unwrap();
    drop(a);
    let b = Manager::open(&db, Path::new("/somewhere/else")).unwrap();
    let s = b.settings();
    assert_eq!((s.max_parallel, s.max_connections, s.download_dir.as_path()), (5, 12, d.path()));
}
```

- [ ] **Step 2: Run** — `cargo test -p mdm-core --test manager` three times. Expected: all pass. A failure here is a bug in the manager or the engine until proven otherwise: fix the code, never the assertion. The one allowed test change is making a wait more patient.

- [ ] **Step 3: Gate and commit**

```bash
git add -A
git commit -m "test(core): crash recovery, lost ranges and a changed file, end to end

A crash leaves the saved segments; the next launch continues from them.
A server that stopped honouring ranges gets one fresh start as a single
stream, with a notice; a changed file waits for the user.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 12: `docs/CORE.md`, README, final gates

**Files:**
- Create: `docs/CORE.md`
- Modify: `README.md` (Status), `docs/superpowers/plans/plan-2-engine-backlog.md` (mark what this plan closed)

- [ ] **Step 1: `docs/CORE.md`**

```markdown
# mdm-core

The download core: a SQLite store and a manager over `mdm-engine`. No Tauri, no UI — the desktop
app (Plan 3) turns `Manager` methods into commands and forwards `ManagerEvent`s to the window.

## The store

One file (`mdm.db` in the app-data folder), schema versioned by `PRAGMA user_version` (1 today).
Tables: `downloads` (one row per download, status CHECK-constrained), `segments` (the durable
segment map of an unfinished download; `start_byte` / `end_byte` because `start` / `end` are SQL
keywords), `settings` (one JSON value). A row's `downloaded` is the sum of its saved segments, or
its size once completed.

## The manager

- **Queue:** FIFO by creation time, at most `max_parallel` (default 3) running.
- **Driving a download:** PROBING → the engine starts (resuming when the row has a size and saved
  segments) → DOWNLOADING → COMPLETED or FAILED with the engine's error code. The durable segment
  snapshot is saved every second (only bytes that reached the disk).
- **Pause / resume / cancel / remove / restart:** a running download is reached through its engine
  control; a waiting one only changes state. Cancel deletes the partial data; remove deletes the row
  (and the finished file only on request); restart starts from byte 0.
- **Crash and quit:** at launch, rows left PROBING / DOWNLOADING go back to QUEUED and continue from
  their saved segments. `shutdown()` pauses and saves every running download and queues it for the
  next launch.
- **Starting over by itself, once:** a resume refused because the server lost range support, the saved
  state does not fit (`INVALID_RESUME`) or the part file is gone — and a download whose server stops
  honouring ranges mid-way — restart from byte 0 with a `Notice`. A changed file (`SOURCE_CHANGED`)
  never does: it waits in FAILED for the user's `restart`.

## Events

`Added`, `Updated` (the whole row after any change), `Progress` (bytes, speed, ETA, segment map,
~4 per second per running download), `Removed`, `Notice`. JSON: `{"type": "progress", "id": …,
"speedBps": …}`.

## Tests

`cargo test -p mdm-core` — the store against in-memory and file databases; the manager against the
in-process test server (`crates/mdm-test-server`), including a simulated crash and a relaunch.
```

- [ ] **Step 2: README Status** — replace the Status paragraph with:
```markdown
**Status:** the download engine (`crates/mdm-engine`) and the download core (`crates/mdm-core`:
SQLite store, queue, pause / resume / cancel, crash recovery) are complete and tested. The desktop
app, browser extension and torrent support follow (see `docs/superpowers/`). Try the engine:

    cargo run -p mdm-engine --example fetch -- https://example.com/big.iso
```

- [ ] **Step 3: Backlog** — in `plan-2-engine-backlog.md`, add at the top: "Closed by Plan 2: the retry reset, credential strip, engine-owned headers, durable snapshot, exclusive part names, blocking writes, hidden range support, empty unknown-length stream, bare 206, StopOnDrop cancel, checked resume math, pause-after-last-byte, INTERNAL code, the `== 6` pin, the speed assertion. Still open:" and keep only the items not closed (the mid-download 200 fallback is closed by the manager's automatic fresh start; the error mapping of header CR/LF, `filename*` charset, escaped quotes, UTF-8 cut test, cookie `;`, test-server 416, crash-test byte count, action SHA pins, CONTRIBUTING.md, pnpm-workspace.yaml remain).

- [ ] **Step 4: Full gates**, twice for stability: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "docs(core): CORE.md, README status, the backlog after Plan 2

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

## Self-review against the spec and the backlog

- **Spec §3 retry (+ owner decision):** Task 2. **Durability (§3 crash paragraph, final-review #6):** Task 4 (engine) + Task 9 (the manager persists `durable_segments`) + Task 11 (crash proof). **Security (#4):** Task 5. **Exclusive part names (#5):** Task 5. **Blocking writes:** Task 4. **Behaviour gaps:** Task 6 + Task 3; the mid-download 200 → single stream is the manager's automatic fresh start (Task 9, pinned in Task 11). **Error codes:** Task 3 (`INTERNAL`).
- **Spec §2 storage:** Task 7 (tables, CHECKs, `user_version`) and Task 8 (segments, settings); column renames recorded in Global Constraints. **Spec §3 concurrency limit** (`max_parallel_downloads` 3, FIFO): Task 9. **Spec §6 settings fields** that belong to the core (download folder, max connections, max parallel, user agent, proxy): Task 8; torrent and browser-integration settings arrive with their plans.
- **Out of this plan (Plan 3):** Tauri commands / events, the React UI, tray, notifications, single instance. **Plans 4–6:** bridge + extension, torrent, release.
- **Type consistency:** `SegmentState { idx, start, end, downloaded }` is the engine's; the store maps it to `start_byte` / `end_byte`. `DownloadHandle::filename()` (Task 5) and `control()` (Task 3) are what `drive_inner` (Task 9) calls. `Progress.durable_segments` (Task 4) is what `forward_progress` saves. Intents are defined once in Task 9 and used by Task 10. `CoreError::code()` passes engine codes through, so the UI maps one code list.
- **Placeholders:** none. Timing-sensitive tests say which wait may be made more patient; no assertion may be loosened.
