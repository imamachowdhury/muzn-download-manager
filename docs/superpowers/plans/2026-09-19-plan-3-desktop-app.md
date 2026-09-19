# Plan 3 — The desktop app: Tauri v2 shell and React UI over `mdm-core`

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn `mdm_core::Manager` into a real desktop app — a Tauri v2 window with a React UI (download list with the segment map, add dialog with a live probe, detail panel, settings), a system tray, OS notifications and a single running instance — plus the small core items Plan 2's reviews left for this plan.

**Architecture:** A new workspace crate `src-tauri` (package `mdm-app`, binary `mdm`) opens the manager in Tauri's `setup` hook, exposes every manager action as a thin async Tauri command returning `Result<T, ApiError>` (`{code, message}`), and forwards `ManagerEvent`s to the window as two Tauri events (`download:progress`, `download:status`) plus `download:resync` when the UI fell behind. The React UI (Vite, TypeScript, zustand) talks ONLY through a `Backend` interface; the real one wraps `invoke`/`listen`, a fake one (in-memory) drives the Vitest suite and the plain-browser dev mode, so every screen can be tested and looked at without a Rust build. Pure logic — event reduction, formatting, error messages, path and clipboard rules — lives in small functions with their own tests on both sides.

**Tech Stack:** Rust stable, Tauri 2 (`tray-icon` feature), tauri-plugin-single-instance / dialog / opener / notification / clipboard-manager / autostart (all 2.x); React 19, TypeScript (strict), Vite, zustand 5, @tanstack/react-virtual 3, Vitest + jsdom + Testing Library, ESLint 9 flat config with typescript-eslint and react-hooks; pnpm 12 (the `packageManager` field).

**Spec:** `docs/superpowers/specs/2026-09-18-muzn-download-manager-design.md` — §2 (architecture, repo layout, storage path), §6 (desktop UI), §7 (CI), §8 phase 4 (the UI half). Backlog this plan draws from: `docs/superpowers/plans/plan-2-engine-backlog.md`. How the core behaves: `docs/CORE.md`.

## Global Constraints

- Product name in user-facing strings: **Muzn Download Manager**; identifiers `mdm`; Tauri identifier and native host id `com.muzn.mdm`. UI is English only. MIT.
- `mdm-engine` and `mdm-core` never depend on Tauri or any UI crate. Only `src-tauri` does.
- **The UI never touches the file system or the network.** Every action is a Tauri command, every update a Tauri event. Opening a file, revealing it in its folder, picking a folder and reading the clipboard all happen in Rust commands; the UI never passes a file path to be opened — it passes a download id and Rust resolves the path from the row.
- App data folder (spec §2): `<OS data dir>/muzn-dm` — `%APPDATA%\muzn-dm`, `~/.local/share/muzn-dm`, `~/Library/Application Support/muzn-dm`; database `mdm.db` inside it. Default download folder: the OS Downloads folder, else the home folder.
- Events: `download:progress` (payload = the `ManagerEvent::Progress` JSON), `download:status` (every other `ManagerEvent`: added / updated / removed / notice), `download:resync` (no payload: the forwarder lagged; the UI reloads the list). The manager already publishes progress at most every 250 ms (`mdm_engine::PROGRESS_INTERVAL`), which is the spec's per-download throttle — the app adds no second throttle.
- Command errors are `{ "code": "<STABLE_CODE>", "message": "<text>" }`; `code` is `CoreError::code()` (engine codes pass through) or `INVALID_STATE` / `INTERNAL` from the app layer. The UI maps codes to plain English in ONE place (`src/lib/errors.ts`).
- Look (spec §6): follows the OS light / dark (`prefers-color-scheme`), one accent (Muzn teal `#0f9b8e`, dark mode `#2cc4b4`), 13 px body, no text below 12 px, visible `:focus-visible` ring, one dialog primitive and one toast primitive — never `window.confirm` / `alert` / `prompt`. Colours are CSS custom properties on `:root`; components never use a raw hex.
- Window: 1100 × 700 default, minimum 760 × 480. Everything must work and look right at 1366 × 768 (the owner's laptop) and at the 760 px minimum width.
- Gates before every commit: `pnpm build` (the Tauri crate embeds `dist/`, so it must exist before any cargo command), `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `pnpm tsc --noEmit`, `pnpm eslint . --max-warnings=0`, `pnpm vitest run`. Bash needs `export PATH="$HOME/.cargo/bin:$PATH"`. Never weaken a test; a changed behaviour gets a regression test naming the decision and date. No `#[allow(...)]`, no `eslint-disable`.
- Commit messages tell the story and end with `Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>`. Source edits through the editor tools, never `sed -i` / `node -e`. This repository's commits use the GitHub noreply address already configured in the repo; never change `user.email`.
- No new subsystem beyond this plan (browser integration = Plan 4, torrent = Plan 5, installers / updater = Plan 6). The spec's "Out" list stands.

## Decisions made while planning

- **Branch:** `plan-3-app`, cut from `plan-2-core` (PR #2 is green and waits for the owner's merge word). When PR #2 merges, rebase `plan-3-app` onto `main` before opening PR #3.
- **`src/` at the repo root is the React app, `src-tauri/` the Rust app crate** (spec §2 layout). `src-tauri` joins the Cargo workspace, so `cargo test --workspace` builds it — and Tauri's `generate_context!` refuses to compile unless `dist/` exists; every gate run and CI therefore does `pnpm build` first.
- **The spec's "one zustand store" becomes a vanilla zustand store created per app (`createDownloadsStore()`) and handed out through React context** together with the backend, so every test gets a fresh store and a fake backend.
- **A fake backend is part of the product code** (`src/api/fake.ts`): Vitest uses it, and `pnpm dev` in a plain browser (no Tauri) runs the UI against it with a few demo rows that move — that is how the screens are looked at and screenshotted without building Rust.
- **Close button hides to the tray** (IDM behaviour) while a tray icon exists and the new setting `closeToTray` (default on) is set; Quit is on the tray menu. Without a tray (some Linux desktops) the close button quits.
- **Completion notification is a plain OS notification** ("Download complete" + file name), setting `notifyOnComplete` (default on). The spec's Open / Open folder notification buttons are not offered: tauri-plugin-notification's action buttons are mobile-only. The completed row and the detail panel carry Open and Show in folder instead.
- **Per-download connection count is NOT in the add dialog** (spec §6 lists it): the store has no column for it and `DownloadSpec` takes the engine's setting. Connections stay one global setting in v1; recorded here so nobody adds half of it.
- **"Folder (last used)"** in the add dialog is remembered in the browser's `localStorage` (`mdm.lastDir`), a per-machine UI convenience; the settings' download folder is the fallback.
- **Left-rail filters:** All / Downloading (queued, probing, downloading, paused) / Completed / Failed. "Torrents" arrives with Plan 5. CANCELLED rows show under All only.
- **The detail panel has no "log"** (spec §6): the core keeps no per-download log. It shows URL, path, size, status, the error in plain English and the segment table.
- **Launch at startup** is the OS's own state through tauri-plugin-autostart (read and written by two commands), not a stored setting — one place per fact.
- **From the backlog, in this plan:** `ProbeInfo`-for-the-UI (a new serialisable `ProbePreview` and `Manager::probe`), the panicking-driver slot leak, the metadata-after-completion FAILED, save-on-change. Left in the backlog: the fsync inside the ticker, Windows path normalisation, the crash test's byte count, the parser nits, pinning Actions by SHA, `CONTRIBUTING.md`.

## File structure

```
Cargo.toml                          + member "src-tauri"; workspace deps tauri, tauri-build, plugins
package.json                        the React app (scripts dev/build/tauri/test/lint); drop npm "workspaces"
pnpm-workspace.yaml                 NEW: packages ['.'], onlyBuiltDependencies
index.html, vite.config.ts, tsconfig.json, eslint.config.js   NEW
app-icon.svg                        NEW: source for `pnpm tauri icon`
.github/workflows/ci.yml            + Linux WebKit deps, pnpm build before cargo, JS job
crates/mdm-core/
  src/model.rs                      + ProbePreview                                   (T1)
  src/settings.rs                   + close_to_tray, notify_on_complete              (T1)
  src/error.rs                      + CoreError::Internal ("INTERNAL")               (T1)
  src/manager.rs                    + probe(), parse_http_url, guarded(), file_len, should_persist (T1)
  tests/manager.rs                  + probe tests                                    (T1)
src-tauri/                          NEW (T3–T4)
  Cargo.toml, build.rs, tauri.conf.json, capabilities/default.json, icons/
  src/main.rs                       calls mdm_app::run()
  src/lib.rs                        run(): plugins, setup, window close, exit → shutdown
  src/paths.rs                      AppPaths (data dir, db, download dir)
  src/api.rs                        ApiError + pure helpers (completed_path, clipboard_link)
  src/commands.rs                   #[tauri::command] wrappers
  src/events.rs                     event_name, completed_name, spawn_forwarder
  src/tray.rs                       tray icon + menu, show_main
src/                                NEW React app (T2, T5–T9)
  main.tsx, App.tsx, styles.css
  api/types.ts, api/backend.ts, api/tauri.ts, api/fake.ts, api/demo.ts
  state/downloads.ts, state/context.tsx
  lib/format.ts, lib/errors.ts, lib/keys.ts
  ui/Dialog.tsx, ui/confirm.tsx, ui/toast.tsx
  components/TopBar.tsx, FilterRail.tsx, DownloadList.tsx, DownloadRowView.tsx,
             SegmentBar.tsx, DetailPanel.tsx, AddDialog.tsx, SettingsDialog.tsx
  test/setup.ts, test/render.tsx
  **/*.test.ts(x)
docs/APP.md                         NEW: how the desktop app is wired (T10)
docs/superpowers/plans/plan-2-engine-backlog.md   closed items moved (T10)
README.md                           status + how to run the app (T10)
```

---

### Task 1: Core items the app needs (probe preview, panic guard, completion size, save-on-change, two settings)

**Files:**
- Modify: `crates/mdm-core/Cargo.toml` (add `futures-util.workspace = true` to `[dependencies]`)
- Modify: `crates/mdm-core/src/model.rs` (add `ProbePreview`)
- Modify: `crates/mdm-core/src/error.rs` (add `Internal`)
- Modify: `crates/mdm-core/src/settings.rs` (add two fields)
- Modify: `crates/mdm-core/src/manager.rs` (`probe`, `parse_http_url`, `guarded`, `file_len`, `should_persist`, their use)
- Test: `crates/mdm-core/tests/manager.rs`, `crates/mdm-core/tests/store.rs`, unit tests in `manager.rs`

**Interfaces:**
- Consumes: `mdm_engine::Engine::probe(&Url, &RequestExtras) -> Result<Probe, EngineError>` (fields `final_url: Url, size: Option<u64>, ranges: bool, mime: Option<String>, filename: String`); `RequestExtras` derives `Default` with `headers: Vec<(String, String)>`.
- Produces:
  - `mdm_core::ProbePreview { final_url: String, filename: String, size: Option<u64>, resumable: bool, mime: Option<String> }` — `Serialize`, camelCase JSON (`finalUrl`, `filename`, `size`, `resumable`, `mime`).
  - `Manager::probe(&self, url: &str, referrer: Option<&str>) -> Result<ProbePreview>` (async).
  - `Settings.close_to_tray: bool` (JSON `closeToTray`, default `true`), `Settings.notify_on_complete: bool` (JSON `notifyOnComplete`, default `true`).
  - `CoreError::Internal(String)` with `code() == "INTERNAL"`.

- [ ] **Step 1: Write the failing tests**

Append to `crates/mdm-core/tests/manager.rs`:

```rust
#[tokio::test]
async fn probe_previews_a_url_without_adding_it() {
    let s = TestServer::start(3 * 1024 * 1024).await;
    let d = tempfile::tempdir().unwrap();
    let m = manager(d.path());
    let p = m.probe(&s.file_url(), None).await.unwrap();
    assert_eq!(p.filename, "file");
    assert_eq!(p.size, Some(3 * 1024 * 1024));
    assert!(p.resumable);
    assert!(p.final_url.ends_with("/file"));
    assert!(m.list().unwrap().is_empty(), "a probe never adds a row");
}

#[tokio::test]
async fn probe_says_not_resumable_without_ranges() {
    let s = TestServer::start(1024 * 1024).await;
    s.cfg.ranges.store(false, Ordering::SeqCst);
    let d = tempfile::tempdir().unwrap();
    let m = manager(d.path());
    let p = m.probe(&s.file_url(), None).await.unwrap();
    assert!(!p.resumable);
}

#[tokio::test]
async fn probe_refuses_what_add_refuses_and_passes_engine_codes_through() {
    let s = TestServer::start(1024).await;
    let d = tempfile::tempdir().unwrap();
    let m = manager(d.path());
    assert_eq!(
        m.probe("ftp://example.com/x", None).await.unwrap_err().code(),
        "INVALID_URL"
    );
    assert_eq!(
        m.probe(&s.status_url(404), None).await.unwrap_err().code(),
        "HTTP_STATUS"
    );
}

#[test]
fn a_probe_preview_serialises_in_camel_case() {
    let p = ProbePreview {
        final_url: "https://x/y".into(),
        filename: "y".into(),
        size: None,
        resumable: false,
        mime: None,
    };
    let v = serde_json::to_value(&p).unwrap();
    assert_eq!(v["finalUrl"], "https://x/y");
    assert!(v["size"].is_null());
    assert_eq!(v["resumable"], false);
}
```

`serde_json` is not yet a dev-dependency of `mdm-core`'s tests — it is a normal dependency, so `serde_json::` is reachable from integration tests already (normal deps are visible to `tests/`).

Append to `crates/mdm-core/tests/store.rs`:

```rust
#[test]
fn settings_saved_before_plan_3_get_the_new_switches_on() {
    // Plan 3 (2026-09-19): close-to-tray and the completion notification
    // default ON, also for settings stored before the fields existed.
    let s = Store::open_in_memory().unwrap();
    let old = r#"{"downloadDir":"/d","maxConnections":4,"maxParallel":2,"userAgent":null,"proxy":{"mode":"system"}}"#;
    let parsed: Settings = serde_json::from_str(old).unwrap();
    assert!(parsed.close_to_tray);
    assert!(parsed.notify_on_complete);
    s.save_settings(&parsed).unwrap();
    assert_eq!(s.load_settings(Path::new("/x")).unwrap(), parsed);
}
```

(`tests/store.rs` already imports `std::path::Path` and `mdm_core::*`; if `serde_json` is not in scope there, refer to it by its full path as written.)

Add to the `#[cfg(test)] mod tests` at the bottom of `crates/mdm-core/src/manager.rs`:

```rust
    #[tokio::test]
    async fn a_panicking_driver_becomes_an_internal_error() {
        // Plan 2 backlog: a panic used to unwind past `finish()` and leak the
        // queue slot forever. The guard turns it into an INTERNAL failure.
        let r = guarded(async {
            if "boom".len() == 4 {
                panic!("boom");
            }
            Ok(())
        })
        .await;
        let e = r.unwrap_err();
        assert_eq!(e.code(), "INTERNAL");
        assert!(e.to_string().contains("boom"), "{e}");
    }

    #[tokio::test]
    async fn a_guarded_driver_passes_its_result_through() {
        assert!(guarded(async { Ok(()) }).await.is_ok());
        let e = guarded(async { Err(CoreError::NotFound("x".into())) })
            .await
            .unwrap_err();
        assert_eq!(e.code(), "NOT_FOUND");
    }

    #[test]
    fn a_finished_files_length_is_read_or_left_unknown() {
        // Plan 2 backlog: a metadata error after a successful download must
        // not turn it into a FAILED one; the size just stays unknown.
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("f");
        std::fs::write(&f, b"abc").unwrap();
        assert_eq!(file_len(&f), Some(3));
        assert_eq!(file_len(&d.path().join("missing")), None);
    }

    fn seg(downloaded: u64) -> SegmentState {
        SegmentState {
            idx: 0,
            start: 0,
            end: 99,
            downloaded,
        }
    }

    #[test]
    fn progress_is_saved_only_when_the_durable_state_moved() {
        // Plan 2 backlog: a stalled download must not rewrite the same
        // snapshot every second.
        let long = PERSIST_INTERVAL;
        let short = PERSIST_INTERVAL / 2;
        let d = Status::Downloading;
        assert!(should_persist(d, long, &[seg(5)], &[seg(0)]));
        assert!(!should_persist(d, long, &[seg(5)], &[seg(5)]), "unchanged");
        assert!(!should_persist(d, short, &[seg(5)], &[seg(0)]), "too soon");
        assert!(
            !should_persist(Status::Paused, long, &[seg(5)], &[seg(0)]),
            "a pause is saved by the driver from the outcome"
        );
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `export PATH="$HOME/.cargo/bin:$PATH"; cargo test -p mdm-core`
Expected: compile errors — `probe`, `ProbePreview`, `guarded`, `file_len`, `should_persist`, `close_to_tray` do not exist.

- [ ] **Step 3: Implement**

`crates/mdm-core/Cargo.toml` `[dependencies]`: add `futures-util.workspace = true`. Add `tempfile.workspace = true` is already a dev-dependency (unit tests in `src/` can use it).

`crates/mdm-core/src/error.rs` — add a variant after `Json` and its code:

```rust
    /// A bug: something that should never happen (a panicking task…).
    #[error("internal error: {0}")]
    Internal(String),
```

and in `code()`: `Self::Internal(_) => "INTERNAL",`.

`crates/mdm-core/src/model.rs` — append:

```rust
/// What the add dialog shows before a download exists: the server's answer
/// to a probe, without adding anything.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbePreview {
    /// URL after redirects.
    pub final_url: String,
    /// The server's (sanitised) file name.
    pub filename: String,
    /// Total size when the server states it.
    pub size: Option<u64>,
    /// Size known and byte ranges honoured: segmented, pausable, resumable.
    pub resumable: bool,
    /// MIME type.
    pub mime: Option<String>,
}
```

`crates/mdm-core/src/settings.rs` — add two fields at the end of `Settings`:

```rust
    /// The window's close button hides the app to the tray (when there is one).
    pub close_to_tray: bool,
    /// Show an OS notification when a download completes.
    pub notify_on_complete: bool,
```

and in `Default`: `close_to_tray: true, notify_on_complete: true,`. (`#[serde(default)]` on the struct fills them for settings saved before this plan.)

`crates/mdm-core/src/manager.rs`:

1. Imports: add `use std::any::Any;`, `use std::future::Future;`, `use std::panic::AssertUnwindSafe;`, `use futures_util::FutureExt;`.

2. Replace the URL checks at the top of `Manager::add` with a call to a new free function and use it from `probe` too:

```rust
/// An http(s) URL, or INVALID_URL.
fn parse_http_url(s: &str) -> Result<Url> {
    let url = Url::parse(s.trim()).map_err(|e| CoreError::InvalidUrl(format!("{s}: {e}")))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(CoreError::InvalidUrl(format!(
            "unsupported scheme {}",
            url.scheme()
        )));
    }
    Ok(url)
}
```

In `add`: `let url = parse_http_url(&new.url)?;` then `new.url = url.to_string();` as before.

3. Add to `impl Manager` (after `set_settings`):

```rust
    /// Ask the server about a URL without adding it — the add dialog's live
    /// preview. Uses the current settings' engine (proxy, user agent).
    pub async fn probe(&self, url: &str, referrer: Option<&str>) -> Result<ProbePreview> {
        let url = parse_http_url(url)?;
        let mut extras = RequestExtras::default();
        if let Some(r) = referrer.map(str::trim).filter(|r| !r.is_empty()) {
            extras.headers.push(("Referer".into(), r.to_owned()));
        }
        let engine = self.inner.engine.lock().unwrap().clone();
        let p = engine.probe(&url, &extras).await?;
        Ok(ProbePreview {
            final_url: p.final_url.to_string(),
            filename: p.filename,
            resumable: p.ranges && p.size.is_some(),
            size: p.size,
            mime: p.mime,
        })
    }
```

4. The panic guard — add as free functions and use it in `drive`:

```rust
/// Run a driver future; a panic inside it becomes an INTERNAL error instead of
/// unwinding past `finish()` and leaking the queue slot (Plan 2 backlog).
async fn guarded<F: Future<Output = Result<()>>>(f: F) -> Result<()> {
    match AssertUnwindSafe(f).catch_unwind().await {
        Ok(r) => r,
        Err(p) => Err(CoreError::Internal(panic_message(p.as_ref()))),
    }
}

fn panic_message(p: &(dyn Any + Send)) -> String {
    if let Some(s) = p.downcast_ref::<&str>() {
        format!("a download task panicked: {s}")
    } else if let Some(s) = p.downcast_ref::<String>() {
        format!("a download task panicked: {s}")
    } else {
        "a download task panicked".into()
    }
}
```

In `drive`, change `if let Err(e) = drive_inner(&inner, &id, &slot).await {` to `if let Err(e) = guarded(drive_inner(&inner, &id, &slot)).await {`.

5. Completion size — add:

```rust
/// A finished file's length, or `None` if it cannot be read. A finished
/// download is never turned into a FAILED one by a metadata error (Plan 2
/// backlog); its size just stays unknown.
fn file_len(path: &Path) -> Option<u64> {
    match std::fs::metadata(path) {
        Ok(m) => Some(m.len()),
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "reading a finished file's size failed");
            None
        }
    }
}
```

and in `drive_inner`'s `Outcome::Completed` arm replace

```rust
                if info.size.is_none() {
                    let len = std::fs::metadata(&path)?.len();
                    inner.store.set_size(id, len, now_ms())?;
                }
```

with

```rust
                if info.size.is_none() {
                    if let Some(len) = file_len(&path) {
                        inner.store.set_size(id, len, now_ms())?;
                    }
                }
```

6. Save-on-change — add:

```rust
/// Whether a progress update is written to the store: only while downloading,
/// at most once per `PERSIST_INTERVAL`, and only when the durable segments
/// moved since the last save (a stalled download writes nothing; Plan 2 backlog).
fn should_persist(
    status: Status,
    since_last: Duration,
    durable: &[SegmentState],
    last_saved: &[SegmentState],
) -> bool {
    status == Status::Downloading && since_last >= PERSIST_INTERVAL && durable != last_saved
}
```

and rewrite `forward_progress`:

```rust
async fn forward_progress(inner: Arc<Inner>, id: DownloadId, mut rx: watch::Receiver<Progress>) {
    let mut last_save = Instant::now();
    // The starting state is what the store already holds (the resume
    // segments, or nothing saved for a fresh start).
    let mut last_saved = rx.borrow().durable_segments.clone();
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
        if should_persist(p.status, last_save.elapsed(), &p.durable_segments, &last_saved) {
            match inner.store.save_segments(&id, &p.durable_segments) {
                Ok(()) => last_saved = p.durable_segments,
                Err(e) => tracing::warn!(%id, error = %e, "saving progress failed"),
            }
            last_save = Instant::now();
        }
    }
}
```

(`Status` must be `PartialEq` and `Copy` for `should_persist(p.status, …)`; it is already compared with `==` in the old code. If it is not `Copy`, pass `p.status.clone()`.)

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p mdm-core` — Expected: all pass (the existing 60-odd core tests plus the new ones).
Then the full gates: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`.

- [ ] **Step 5: Update `docs/CORE.md`**

In "The manager" add one bullet after "Queue":

```markdown
- **Probe without adding:** `probe(url, referrer)` asks the server about a URL and returns a
  `ProbePreview` (final URL, file name, size, `resumable`, MIME) — the add dialog's live preview.
  Same URL rules and engine error codes as `add`; nothing is stored.
```

In "Durable progress" replace "The manager saves that durable snapshot to the store about once a second (`PERSIST_INTERVAL`)" with "The manager saves that durable snapshot to the store at most once a second (`PERSIST_INTERVAL`), and only when it changed". Add a bullet at the end of "The manager":

```markdown
- **A panicking driver** fails its row with INTERNAL and frees its queue slot; it never leaks the
  slot. A finished download whose size cannot be read afterwards stays COMPLETED with an unknown size.
```

- [ ] **Step 6: Commit**

```bash
git add crates/mdm-core docs/CORE.md
git commit -m "feat(core): probe preview for the add dialog, a panic guard, save only on change

The desktop app needs to show a URL's size and range support before the user
adds it: Manager::probe returns a serialisable ProbePreview through the same
URL rules and engine codes as add. Three Plan 2 backlog items close here: a
panicking driver now fails its row with INTERNAL instead of leaking its queue
slot, a metadata error after a finished download no longer marks it FAILED,
and the progress forwarder writes the durable snapshot only when it moved.
Settings gain closeToTray and notifyOnComplete (default on, also for settings
saved earlier).

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 2: Frontend toolchain — Vite + React + TypeScript + Vitest + ESLint, and the JS CI job

**Files:**
- Modify: `package.json`
- Create: `pnpm-workspace.yaml`, `index.html`, `vite.config.ts`, `tsconfig.json`, `eslint.config.js`, `.gitignore` additions
- Create: `src/main.tsx`, `src/App.tsx`, `src/styles.css`, `src/test/setup.ts`, `src/App.test.tsx`
- Modify: `.github/workflows/ci.yml` (new `web` job)

**Interfaces:**
- Produces: `pnpm dev` (Vite on port 1420, strict), `pnpm build` (→ `dist/`), `pnpm test` (= `vitest run`), `pnpm lint` (= `eslint . --max-warnings=0`), `pnpm typecheck` (= `tsc --noEmit`), `pnpm tauri` (the Tauri CLI, used from Task 3). Test setup file `src/test/setup.ts` (jest-dom matchers, `ResizeObserver` stub, cleanup). CSS tokens on `:root` that every later component uses: `--bg`, `--surface`, `--surface-2`, `--border`, `--text`, `--text-muted`, `--accent`, `--accent-text`, `--danger`, `--warn`, `--ok`, `--radius-sm` (4px), `--radius` (6px), `--radius-lg` (10px).

- [ ] **Step 1: package.json and pnpm workspace**

Replace `package.json` with (the `workspaces` field did nothing for pnpm — Plan 2 backlog):

```json
{
  "name": "muzn-download-manager",
  "private": true,
  "type": "module",
  "packageManager": "pnpm@12.4.2",
  "scripts": {
    "dev": "vite",
    "build": "vite build",
    "preview": "vite preview",
    "tauri": "tauri",
    "test": "vitest run",
    "typecheck": "tsc --noEmit",
    "lint": "eslint . --max-warnings=0"
  }
}
```

Create `pnpm-workspace.yaml`:

```yaml
packages:
  - "."
onlyBuiltDependencies:
  - esbuild
  - "@tauri-apps/cli"
```

Install (versions are whatever is current; the lockfile pins them):

```bash
pnpm add react react-dom zustand @tanstack/react-virtual @tauri-apps/api
pnpm add -D typescript vite @vitejs/plugin-react vitest jsdom @testing-library/react @testing-library/user-event @testing-library/jest-dom @types/react @types/react-dom eslint @eslint/js typescript-eslint eslint-plugin-react-hooks globals @tauri-apps/cli
```

If pnpm reports ignored build scripts for anything other than the two listed, add it to `onlyBuiltDependencies` and run `pnpm install` again.

- [ ] **Step 2: Config files**

`index.html`:

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>Muzn Download Manager</title>
  </head>
  <body>
    <div id="root"></div>
    <script type="module" src="/src/main.tsx"></script>
  </body>
</html>
```

`vite.config.ts`:

```ts
import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

// Tauri expects a fixed dev port and must see Rust errors in the terminal.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**", "**/target/**", "**/crates/**"] },
  },
  envPrefix: ["VITE_", "TAURI_ENV_"],
  build: { target: "es2022", outDir: "dist", emptyOutDir: true },
  test: {
    environment: "jsdom",
    setupFiles: ["src/test/setup.ts"],
    include: ["src/**/*.test.{ts,tsx}"],
  },
});
```

`tsconfig.json`:

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "lib": ["ES2022", "DOM", "DOM.Iterable"],
    "module": "ESNext",
    "moduleResolution": "bundler",
    "jsx": "react-jsx",
    "strict": true,
    "noUnusedLocals": true,
    "noUnusedParameters": true,
    "noFallthroughCasesInSwitch": true,
    "noUncheckedIndexedAccess": true,
    "isolatedModules": true,
    "skipLibCheck": true,
    "noEmit": true,
    "types": ["vite/client"]
  },
  "include": ["src", "vite.config.ts"]
}
```

`eslint.config.js`:

```js
import js from "@eslint/js";
import tseslint from "typescript-eslint";
import reactHooks from "eslint-plugin-react-hooks";
import globals from "globals";

export default tseslint.config(
  { ignores: ["dist", "target", "src-tauri", "crates", "node_modules"] },
  {
    files: ["**/*.{ts,tsx}"],
    extends: [js.configs.recommended, ...tseslint.configs.recommended],
    languageOptions: { globals: globals.browser },
    plugins: { "react-hooks": reactHooks },
    rules: {
      ...reactHooks.configs.recommended.rules,
      "no-restricted-globals": ["error", "confirm", "alert", "prompt"],
      "no-restricted-properties": [
        "error",
        { object: "window", property: "confirm" },
        { object: "window", property: "alert" },
        { object: "window", property: "prompt" },
      ],
    },
  },
);
```

`.gitignore` — append `node_modules/` and `dist/` if not present.

- [ ] **Step 3: Write the failing test**

`src/test/setup.ts`:

```ts
import "@testing-library/jest-dom/vitest";
import { afterEach } from "vitest";
import { cleanup } from "@testing-library/react";

afterEach(() => cleanup());

// jsdom has no ResizeObserver; the virtual list only needs it to exist.
class ResizeObserverStub {
  observe() {}
  unobserve() {}
  disconnect() {}
}
globalThis.ResizeObserver ??= ResizeObserverStub as unknown as typeof ResizeObserver;
```

`src/App.test.tsx`:

```tsx
import { render, screen } from "@testing-library/react";
import { expect, test } from "vitest";
import { App } from "./App";

test("the window carries the product name", () => {
  render(<App />);
  expect(screen.getByRole("heading", { name: "Muzn Download Manager" })).toBeInTheDocument();
});
```

Run: `pnpm test` — Expected: FAIL (`./App` does not exist).

- [ ] **Step 4: Implement the skeleton**

`src/App.tsx` (replaced by the real shell in Task 6):

```tsx
export function App() {
  return (
    <main className="app">
      <h1 className="brand">Muzn Download Manager</h1>
    </main>
  );
}
```

`src/main.tsx`:

```tsx
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import "./styles.css";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
```

`src/styles.css` — the tokens every later task uses:

```css
:root {
  color-scheme: light dark;
  --bg: #f4f6f7;
  --surface: #fbfcfc;
  --surface-2: #eef1f2;
  --border: #d8dee1;
  --text: #1c2629;
  --text-muted: #5b6b70;
  --accent: #0f9b8e;
  --accent-text: #ffffff;
  --accent-soft: #d7f0ed;
  --danger: #c4413b;
  --warn: #b7791f;
  --ok: #2f855a;
  --radius-sm: 4px;
  --radius: 6px;
  --radius-lg: 10px;
  --font: system-ui, -apple-system, "Segoe UI", Roboto, sans-serif;
  font-family: var(--font);
  font-size: 13px;
  line-height: 1.45;
  color: var(--text);
  background: var(--bg);
}

@media (prefers-color-scheme: dark) {
  :root {
    --bg: #14191b;
    --surface: #1b2224;
    --surface-2: #232c2f;
    --border: #334044;
    --text: #e4eaec;
    --text-muted: #9aabb0;
    --accent: #2cc4b4;
    --accent-text: #06201d;
    --accent-soft: #173c38;
    --danger: #ef6b64;
    --warn: #e0a84a;
    --ok: #5cc28a;
  }
}

* { box-sizing: border-box; }
html, body, #root { height: 100%; margin: 0; }
body { background: var(--bg); }
small { font-size: 12px; }
:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
button, input, select { font: inherit; color: inherit; }
.app { height: 100%; display: flex; flex-direction: column; }
.brand { font-size: 14px; margin: 0; padding: 12px 16px; }
```

- [ ] **Step 5: Run every JS gate**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm build`
Expected: 1 test passes; no type or lint errors; `dist/index.html` exists.

- [ ] **Step 6: CI — the web job**

Add to `.github/workflows/ci.yml` under `jobs:`:

```yaml
  web:
    name: Web UI
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: pnpm/action-setup@v4
      - uses: actions/setup-node@v4
        with:
          node-version: 24
          cache: pnpm
      - run: pnpm install --frozen-lockfile
      - run: pnpm typecheck
      - run: pnpm lint
      - run: pnpm test
      - run: pnpm build
```

- [ ] **Step 7: Commit**

```bash
git add package.json pnpm-lock.yaml pnpm-workspace.yaml index.html vite.config.ts tsconfig.json eslint.config.js .gitignore src .github/workflows/ci.yml
git commit -m "build(web): Vite + React 19 + TypeScript skeleton with Vitest, ESLint and a CI job

The desktop UI gets its toolchain: a strict TypeScript React app built by
Vite on Tauri's fixed port, Vitest on jsdom with Testing Library, and a flat
ESLint config that also bans confirm/alert/prompt (the app has its own dialog
primitive). The colour tokens for light and dark live on :root once. CI runs
typecheck, lint, tests and the build.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---
### Task 3: The Tauri shell — `src-tauri` opens the manager, one instance, a clean shutdown

**Files:**
- Modify: `Cargo.toml` (member `src-tauri`, workspace deps)
- Create: `src-tauri/Cargo.toml`, `src-tauri/build.rs`, `src-tauri/tauri.conf.json`, `src-tauri/capabilities/default.json`, `src-tauri/icons/*` (generated), `app-icon.svg`
- Create: `src-tauri/src/main.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/paths.rs`, `src-tauri/src/window.rs`
- Modify: `.github/workflows/ci.yml` (Rust job: Linux WebKit deps, node + pnpm, `pnpm build` before cargo)
- Modify: `CLAUDE.md` (gates line)

**Interfaces:**
- Consumes: `mdm_core::Manager::open(db: &Path, default_download_dir: &Path) -> Result<Manager>` (must run inside a tokio runtime), `Manager::shutdown(&self)` (async).
- Produces:
  - crate `mdm_app` (package `mdm-app`, binary `mdm`), `pub fn run()`.
  - `paths::AppPaths { data_dir: PathBuf, db: PathBuf, download_dir: PathBuf }`, `AppPaths::from_roots(data_root: PathBuf, downloads: Option<PathBuf>, home: PathBuf) -> AppPaths`, `AppPaths::resolve<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<AppPaths>`, `paths::DATA_FOLDER = "muzn-dm"`.
  - `window::show_main<R: Runtime>(app: &AppHandle<R>)` (unminimise, show, focus the `main` window) — Task 4's tray uses it.
  - `mdm_core::Manager` is in Tauri state (`app.state::<mdm_core::Manager>()`).

- [ ] **Step 1: Workspace and crate manifests**

Root `Cargo.toml`: `members = ["crates/*", "src-tauri"]`, and add to `[workspace.dependencies]`:

```toml
tauri = { version = "2", features = ["tray-icon"] }
tauri-build = { version = "2", features = [] }
tauri-plugin-single-instance = "2"
tauri-plugin-dialog = "2"
tauri-plugin-opener = "2"
tauri-plugin-notification = "2"
tauri-plugin-clipboard-manager = "2"
tauri-plugin-autostart = "2"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

`src-tauri/Cargo.toml`:

```toml
[package]
name = "mdm-app"
description = "Muzn Download Manager desktop app"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
authors.workspace = true
publish = false

[lib]
name = "mdm_app"
path = "src/lib.rs"

[[bin]]
name = "mdm"
path = "src/main.rs"

[build-dependencies]
tauri-build.workspace = true

[dependencies]
mdm-core = { path = "../crates/mdm-core" }
tauri.workspace = true
tauri-plugin-single-instance.workspace = true
tauri-plugin-dialog.workspace = true
tauri-plugin-opener.workspace = true
tauri-plugin-notification.workspace = true
tauri-plugin-clipboard-manager.workspace = true
tauri-plugin-autostart.workspace = true
serde.workspace = true
serde_json.workspace = true
tokio = { workspace = true, features = ["sync", "time"] }
tracing.workspace = true
tracing-subscriber.workspace = true
url.workspace = true

[dev-dependencies]
tempfile.workspace = true
```

`src-tauri/build.rs`:

```rust
fn main() {
    tauri_build::build()
}
```

- [ ] **Step 2: Tauri config, capability, icons**

`src-tauri/tauri.conf.json` (no `version`: Tauri reads the crate's):

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "Muzn Download Manager",
  "identifier": "com.muzn.mdm",
  "build": {
    "beforeDevCommand": "pnpm dev",
    "devUrl": "http://localhost:1420",
    "beforeBuildCommand": "pnpm build",
    "frontendDist": "../dist"
  },
  "app": {
    "windows": [
      {
        "label": "main",
        "title": "Muzn Download Manager",
        "width": 1100,
        "height": 700,
        "minWidth": 760,
        "minHeight": 480,
        "center": true
      }
    ],
    "security": {
      "csp": "default-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src ipc: http://ipc.localhost"
    }
  },
  "bundle": {
    "active": false,
    "icon": [
      "icons/32x32.png",
      "icons/128x128.png",
      "icons/128x128@2x.png",
      "icons/icon.icns",
      "icons/icon.ico"
    ]
  }
}
```

(`bundle.active` stays false until Plan 6 builds installers; the icon list still gives the window and tray their icon.)

`src-tauri/capabilities/default.json` — the window may only use core IPC (events) and the app's own commands; every plugin is called from Rust, so no plugin permission is granted to the page:

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "The main window: core events and the app's own commands only.",
  "windows": ["main"],
  "permissions": ["core:default"]
}
```

`app-icon.svg` (repo root; the source for every icon size):

```svg
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024">
  <rect x="64" y="64" width="896" height="896" rx="200" fill="#0f9b8e"/>
  <path d="M512 232v400M332 472l180 180 180-180" fill="none" stroke="#fff" stroke-width="88" stroke-linecap="round" stroke-linejoin="round"/>
  <path d="M312 792h400" stroke="#fff" stroke-width="88" stroke-linecap="round"/>
</svg>
```

Run: `pnpm tauri icon app-icon.svg -o src-tauri/icons` — Expected: `src-tauri/icons/` holds `32x32.png`, `128x128.png`, `128x128@2x.png`, `icon.icns`, `icon.ico` (and more). If the CLI refuses SVG input, render the SVG to a 1024 × 1024 PNG first (any tool, e.g. a headless browser screenshot) and pass the PNG.

Add `src-tauri/gen/` to `.gitignore`? No — `gen/schemas` is generated on build and Tauri recommends committing it; commit it.

- [ ] **Step 3: Write the failing test**

`src-tauri/src/paths.rs`:

```rust
//! Where the app keeps its data (spec §2) and where downloads go by default.

use std::path::PathBuf;

use tauri::{AppHandle, Manager as _, Runtime};

/// The app-data folder name under the OS data dir (spec §2).
pub const DATA_FOLDER: &str = "muzn-dm";

/// Resolved paths.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppPaths {
    /// `<OS data dir>/muzn-dm`.
    pub data_dir: PathBuf,
    /// `<data_dir>/mdm.db`.
    pub db: PathBuf,
    /// The OS Downloads folder, else the home folder.
    pub download_dir: PathBuf,
}

impl AppPaths {
    /// Build from the OS roots (pure; tested).
    pub fn from_roots(data_root: PathBuf, downloads: Option<PathBuf>, home: PathBuf) -> AppPaths {
        let data_dir = data_root.join(DATA_FOLDER);
        AppPaths {
            db: data_dir.join("mdm.db"),
            data_dir,
            download_dir: downloads.unwrap_or(home),
        }
    }

    /// Ask the OS through Tauri's path resolver.
    pub fn resolve<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<AppPaths> {
        let p = app.path();
        Ok(Self::from_roots(
            p.data_dir()?,
            p.download_dir().ok(),
            p.home_dir()?,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_lives_in_muzn_dm_under_the_os_data_dir() {
        let p = AppPaths::from_roots("/data".into(), Some("/dl".into()), "/home/u".into());
        assert_eq!(p.data_dir, PathBuf::from("/data/muzn-dm"));
        assert_eq!(p.db, PathBuf::from("/data/muzn-dm/mdm.db"));
        assert_eq!(p.download_dir, PathBuf::from("/dl"));
    }

    #[test]
    fn downloads_fall_back_to_home() {
        let p = AppPaths::from_roots("/data".into(), None, "/home/u".into());
        assert_eq!(p.download_dir, PathBuf::from("/home/u"));
    }
}
```

`src-tauri/src/lib.rs` for now:

```rust
//! Muzn Download Manager — the desktop app: Tauri commands and events over `mdm-core`.

mod paths;
```

`src-tauri/src/main.rs`:

```rust
// No console window behind the app in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    mdm_app::run();
}
```

Run: `pnpm build && cargo test -p mdm-app` — Expected: FAIL to compile (`mdm_app::run` does not exist).

- [ ] **Step 4: Implement `run`, the window helper and shutdown**

`src-tauri/src/window.rs`:

```rust
//! The main window.

use tauri::{AppHandle, Manager as _, Runtime};

/// The main window's label (tauri.conf.json).
pub const MAIN: &str = "main";

/// Bring the main window back: unminimise, show, focus.
pub fn show_main<R: Runtime>(app: &AppHandle<R>) {
    if let Some(w) = app.get_webview_window(MAIN) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}
```

`src-tauri/src/lib.rs`:

```rust
//! Muzn Download Manager — the desktop app: Tauri commands and events over `mdm-core`.

mod paths;
mod window;

use std::time::Duration;

use tauri::{AppHandle, Manager as _, RunEvent, Runtime};

use crate::paths::AppPaths;

/// How long quitting waits for running downloads to save their state.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

/// Start the app. Returns when the app exits.
pub fn run() {
    init_logging();
    let app = tauri::Builder::default()
        // Must be the first plugin: a second launch focuses this one and exits.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            window::show_main(app)
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .setup(|app| {
            let paths = AppPaths::resolve(app.handle())?;
            std::fs::create_dir_all(&paths.data_dir)?;
            tracing::info!(db = %paths.db.display(), "opening the download database");
            // The manager captures the runtime it is opened in and spawns
            // every download there; Tauri's own runtime is that runtime.
            let manager = tauri::async_runtime::block_on(async {
                mdm_core::Manager::open(&paths.db, &paths.download_dir)
            })?;
            app.manage(manager);
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("building the Muzn Download Manager window failed");
    app.run(|app, event| {
        if let RunEvent::Exit = event {
            shutdown(app);
        }
    });
}

/// Pause and save every running download so it continues at the next launch.
fn shutdown<R: Runtime>(app: &AppHandle<R>) {
    let Some(m) = app.try_state::<mdm_core::Manager>() else {
        return;
    };
    let m = m.inner().clone();
    let saved = tauri::async_runtime::block_on(async move {
        tokio::time::timeout(SHUTDOWN_GRACE, m.shutdown()).await
    });
    if saved.is_err() {
        tracing::warn!("some downloads did not save their state within {SHUTDOWN_GRACE:?}");
    }
}

fn init_logging() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}
```

(`init_logging` uses `try_init` so a second call — tests — is harmless.)

- [ ] **Step 5: Run the tests and the app**

Run: `pnpm build && cargo test -p mdm-app` — Expected: the 2 path tests pass.
Run: `cargo clippy --workspace --all-targets -- -D warnings` — Expected: clean.
Run the app once: `pnpm tauri dev` (in the background; the first build takes minutes). Expected: a window titled "Muzn Download Manager" showing the heading from Task 2; `%APPDATA%\muzn-dm\mdm.db` (Windows) now exists; launching `pnpm tauri dev`'s binary a second time (`target/debug/mdm.exe`) focuses the first window and exits. Stop the dev process afterwards.

- [ ] **Step 6: CI — the Rust job builds the web UI first**

In `.github/workflows/ci.yml`'s `rust` job, between `Swatinem/rust-cache@v2` and `cargo fmt`, add:

```yaml
      - if: runner.os == 'Linux'
        run: |
          sudo apt-get update
          sudo apt-get install -y libwebkit2gtk-4.1-dev libxdo-dev libayatana-appindicator3-dev librsvg2-dev
      - uses: pnpm/action-setup@v4
      - uses: actions/setup-node@v4
        with:
          node-version: 24
          cache: pnpm
      - run: pnpm install --frozen-lockfile
      # Tauri's generate_context! embeds dist/: it must exist before cargo builds src-tauri.
      - run: pnpm build
```

`CLAUDE.md` — replace the gates line with:

```markdown
- Gates before any commit: `pnpm build` (src-tauri embeds dist/), `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`,
  `pnpm typecheck`, `pnpm lint`, `pnpm test`.
```

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock src-tauri app-icon.svg .github/workflows/ci.yml CLAUDE.md
git commit -m "feat(app): the Tauri shell opens the download manager in one running instance

src-tauri (package mdm-app, binary mdm) joins the workspace. At setup it opens
mdm.db in <OS data dir>/muzn-dm (spec §2) on Tauri's own runtime, so the
manager spawns downloads there and every command may call it from any thread.
A second launch focuses the running window; quitting gives running downloads
five seconds to save their state so they continue at the next launch. The
page may use core events and the app's commands only - every plugin is called
from Rust. CI installs WebKitGTK on Linux and builds dist/ before cargo.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 4: Commands, events, notification, tray and close-to-tray

**Files:**
- Create: `src-tauri/src/api.rs`, `src-tauri/src/commands.rs`, `src-tauri/src/events.rs`, `src-tauri/src/tray.rs`
- Modify: `src-tauri/src/lib.rs` (modules, `invoke_handler`, forwarder, tray, window close), `src-tauri/src/window.rs` (close handling)

**Interfaces:**
- Consumes: Task 1's `Manager::probe`, `ProbePreview`, `Settings.close_to_tray`, `Settings.notify_on_complete`; `mdm_core::{Manager, ManagerEvent, DownloadRow, DownloadId, DownloadStatus, NewDownload, Settings, SegmentView, CoreError}`; Task 3's `window::show_main`.
- Produces — the command names and argument names the UI (Task 5) calls. Tauri converts snake_case arguments to camelCase in JS:

| command | JS args | returns |
|---|---|---|
| `list_downloads` | — | `DownloadRow[]` |
| `download_segments` | `{ id }` | `SegmentView[]` |
| `add_download` | `{ download: NewDownload }` | `DownloadRow` |
| `probe_url` | `{ url, referrer }` | `ProbePreview` |
| `pause_download` / `resume_download` / `cancel_download` / `restart_download` | `{ id }` | `null` |
| `remove_download` | `{ id, deleteFile }` | `null` |
| `pause_all` / `resume_all` | — | `null` |
| `get_settings` | — | `Settings` |
| `set_settings` | `{ settings }` | `Settings` (validated) |
| `open_download` / `show_download_in_folder` | `{ id }` | `null` |
| `pick_folder` | `{ current }` (string or null) | `string \| null` |
| `clipboard_url` | — | `string \| null` |
| `autostart_enabled` | — | `boolean` |
| `set_autostart` | `{ enabled }` | `boolean` (the state after the change) |

  Every command rejects with `ApiError { code, message }`. Events: `download:progress`, `download:status`, `download:resync` (constants `events::PROGRESS`, `events::STATUS`, `events::RESYNC`).

- [ ] **Step 1: Write the failing tests (pure pieces)**

`src-tauri/src/api.rs`:

```rust
//! What a command returns on failure, and the pure rules commands rely on.

use std::path::PathBuf;

use mdm_core::{CoreError, DownloadRow, DownloadStatus};
use serde::Serialize;

/// A failed command, as the UI receives it: `{ code, message }`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ApiError {
    /// Stable code (`CoreError::code()`, engine codes pass through).
    pub code: String,
    /// Plain text for the log / detail line.
    pub message: String,
}

/// Result of a command.
pub type ApiResult<T> = Result<T, ApiError>;

impl ApiError {
    /// Any code.
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_owned(),
            message: message.into(),
        }
    }
    /// The action does not fit the download's state.
    pub fn invalid_state(message: impl Into<String>) -> Self {
        Self::new("INVALID_STATE", message)
    }
    /// A failure outside the core (a plugin, the OS).
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new("INTERNAL", message)
    }
}

impl From<CoreError> for ApiError {
    fn from(e: CoreError) -> Self {
        Self::new(e.code(), e.to_string())
    }
}

/// The finished file of a completed download; anything else is INVALID_STATE.
/// The UI passes an id, never a path: this is the only way to a path.
pub fn completed_path(row: &DownloadRow) -> ApiResult<PathBuf> {
    match (row.status, &row.filename) {
        (DownloadStatus::Completed, Some(name)) => Ok(row.dir.join(name)),
        _ => Err(ApiError::invalid_state("the download has not finished")),
    }
}

/// The clipboard text, if it is exactly one http(s) link (the add dialog
/// pre-fills it); anything else is ignored.
pub fn clipboard_link(text: &str) -> Option<String> {
    let t = text.trim();
    if t.is_empty() || t.contains(char::is_whitespace) {
        return None;
    }
    let url = url::Url::parse(t).ok()?;
    matches!(url.scheme(), "http" | "https").then(|| t.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mdm_core::{DownloadId, DownloadKind};

    fn row(status: DownloadStatus, filename: Option<&str>) -> DownloadRow {
        DownloadRow {
            id: DownloadId("a".into()),
            kind: DownloadKind::Http,
            url: "https://x/y.zip".into(),
            final_url: None,
            filename: filename.map(str::to_owned),
            dir: PathBuf::from("/dl"),
            size: None,
            downloaded: 0,
            status,
            etag: None,
            last_modified: None,
            mime: None,
            referrer: None,
            headers: vec![],
            cookies: vec![],
            error_code: None,
            error_message: None,
            created_at: 0,
            updated_at: 0,
            completed_at: None,
        }
    }

    #[test]
    fn only_a_completed_download_has_a_file_to_open() {
        assert_eq!(
            completed_path(&row(DownloadStatus::Completed, Some("y.zip"))).unwrap(),
            PathBuf::from("/dl/y.zip")
        );
        for s in [DownloadStatus::Downloading, DownloadStatus::Paused, DownloadStatus::Failed] {
            assert_eq!(
                completed_path(&row(s, Some("y.zip"))).unwrap_err().code,
                "INVALID_STATE"
            );
        }
        assert!(completed_path(&row(DownloadStatus::Completed, None)).is_err());
    }

    #[test]
    fn the_clipboard_fills_the_add_dialog_only_with_one_web_link() {
        assert_eq!(
            clipboard_link("  https://x.com/a.zip \n").as_deref(),
            Some("https://x.com/a.zip")
        );
        assert_eq!(clipboard_link("http://x/a").as_deref(), Some("http://x/a"));
        assert_eq!(clipboard_link("ftp://x/a"), None);
        assert_eq!(clipboard_link("hello world"), None);
        assert_eq!(clipboard_link("https://x/a https://x/b"), None);
        assert_eq!(clipboard_link(""), None);
    }

    #[test]
    fn a_core_error_keeps_its_code() {
        let e: ApiError = CoreError::NotFound("abc".into()).into();
        assert_eq!(e.code, "NOT_FOUND");
        assert!(e.message.contains("abc"));
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v, serde_json::json!({"code": "NOT_FOUND", "message": e.message}));
    }
}
```

`src-tauri/src/events.rs` — the pure part and its tests first:

```rust
//! Manager events → window events, and the completion notification.

use mdm_core::{DownloadStatus, ManagerEvent};

/// Live progress of a running download (`ManagerEvent::Progress` JSON).
pub const PROGRESS: &str = "download:progress";
/// Every other manager event: added / updated / removed / notice.
pub const STATUS: &str = "download:status";
/// Events were lost (the forwarder lagged): the UI reloads the list.
pub const RESYNC: &str = "download:resync";

/// Which window event carries a manager event.
pub fn event_name(e: &ManagerEvent) -> &'static str {
    match e {
        ManagerEvent::Progress { .. } => PROGRESS,
        _ => STATUS,
    }
}

/// The file name to announce when this event is a download completing.
pub fn completed_name(e: &ManagerEvent) -> Option<&str> {
    match e {
        ManagerEvent::Updated { download } if download.status == DownloadStatus::Completed => {
            Some(download.filename.as_deref().unwrap_or(download.url.as_str()))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mdm_core::{DownloadId, DownloadKind, DownloadRow};
    use std::path::PathBuf;

    fn row(status: DownloadStatus) -> DownloadRow {
        DownloadRow {
            id: DownloadId("a".into()),
            kind: DownloadKind::Http,
            url: "https://x/y.zip".into(),
            final_url: None,
            filename: Some("y.zip".into()),
            dir: PathBuf::from("/dl"),
            size: Some(3),
            downloaded: 3,
            status,
            etag: None,
            last_modified: None,
            mime: None,
            referrer: None,
            headers: vec![],
            cookies: vec![],
            error_code: None,
            error_message: None,
            created_at: 0,
            updated_at: 0,
            completed_at: None,
        }
    }

    #[test]
    fn progress_and_status_travel_on_their_own_events() {
        let p = ManagerEvent::Progress {
            id: DownloadId("a".into()),
            total: None,
            downloaded: 0,
            speed_bps: 0,
            eta_secs: None,
            segments: vec![],
        };
        assert_eq!(event_name(&p), PROGRESS);
        assert_eq!(event_name(&ManagerEvent::Removed { id: DownloadId("a".into()) }), STATUS);
        assert_eq!(
            event_name(&ManagerEvent::Updated { download: row(DownloadStatus::Paused) }),
            STATUS
        );
    }

    #[test]
    fn only_a_completion_is_announced() {
        let done = ManagerEvent::Updated { download: row(DownloadStatus::Completed) };
        assert_eq!(completed_name(&done), Some("y.zip"));
        let added = ManagerEvent::Added { download: row(DownloadStatus::Completed) };
        assert_eq!(completed_name(&added), None, "added, not completing");
        let paused = ManagerEvent::Updated { download: row(DownloadStatus::Paused) };
        assert_eq!(completed_name(&paused), None);
    }
}
```

Add `pub fn hides_on_close` with its test to `src-tauri/src/window.rs`:

```rust
/// The close button hides the window (instead of quitting) only when there is
/// a tray icon to bring it back and the user wants it (Plan 3 decision).
pub fn hides_on_close(has_tray: bool, close_to_tray: bool) -> bool {
    has_tray && close_to_tray
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_a_tray_the_close_button_quits() {
        assert!(hides_on_close(true, true));
        assert!(!hides_on_close(false, true), "no tray: never hide into nothing");
        assert!(!hides_on_close(true, false), "the user turned it off");
    }
}
```

Add `mod api; mod events;` to `lib.rs`. Run: `pnpm build && cargo test -p mdm-app` — Expected: the pure tests pass as soon as they compile. They pin the rules (only a COMPLETED row has a file to open, one clean web link from the clipboard, which event carries what, when the close button hides) before the wiring in Steps 2–3 relies on them.

- [ ] **Step 2: Commands**

`src-tauri/src/commands.rs`:

```rust
//! Tauri commands: thin wrappers over `mdm_core::Manager`. Every one is
//! async so it runs on the runtime, never on the window's main thread.

use mdm_core::{
    DownloadId, DownloadRow, Manager, NewDownload, ProbePreview, SegmentView, Settings,
};
use tauri::{AppHandle, Runtime, State};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_clipboard_manager::ClipboardExt as _;
use tauri_plugin_dialog::DialogExt as _;
use tauri_plugin_opener::OpenerExt as _;

use crate::api::{clipboard_link, completed_path, ApiError, ApiResult};

fn row_of(m: &Manager, id: &DownloadId) -> ApiResult<DownloadRow> {
    m.get(id)?
        .ok_or_else(|| ApiError::new("NOT_FOUND", format!("not found: {id}")))
}

#[tauri::command]
pub async fn list_downloads(m: State<'_, Manager>) -> ApiResult<Vec<DownloadRow>> {
    Ok(m.list()?)
}

#[tauri::command]
pub async fn download_segments(
    m: State<'_, Manager>,
    id: DownloadId,
) -> ApiResult<Vec<SegmentView>> {
    Ok(m.segments(&id)?.iter().map(SegmentView::from).collect())
}

#[tauri::command]
pub async fn add_download(m: State<'_, Manager>, download: NewDownload) -> ApiResult<DownloadRow> {
    Ok(m.add(download)?)
}

#[tauri::command]
pub async fn probe_url(
    m: State<'_, Manager>,
    url: String,
    referrer: Option<String>,
) -> ApiResult<ProbePreview> {
    Ok(m.probe(&url, referrer.as_deref()).await?)
}

#[tauri::command]
pub async fn pause_download(m: State<'_, Manager>, id: DownloadId) -> ApiResult<()> {
    Ok(m.pause(&id)?)
}

#[tauri::command]
pub async fn resume_download(m: State<'_, Manager>, id: DownloadId) -> ApiResult<()> {
    Ok(m.resume(&id)?)
}

#[tauri::command]
pub async fn cancel_download(m: State<'_, Manager>, id: DownloadId) -> ApiResult<()> {
    Ok(m.cancel(&id)?)
}

#[tauri::command]
pub async fn restart_download(m: State<'_, Manager>, id: DownloadId) -> ApiResult<()> {
    Ok(m.restart(&id)?)
}

#[tauri::command]
pub async fn remove_download(
    m: State<'_, Manager>,
    id: DownloadId,
    delete_file: bool,
) -> ApiResult<()> {
    Ok(m.remove(&id, delete_file)?)
}

#[tauri::command]
pub async fn pause_all(m: State<'_, Manager>) -> ApiResult<()> {
    Ok(m.pause_all()?)
}

#[tauri::command]
pub async fn resume_all(m: State<'_, Manager>) -> ApiResult<()> {
    Ok(m.resume_all()?)
}

#[tauri::command]
pub async fn get_settings(m: State<'_, Manager>) -> ApiResult<Settings> {
    Ok(m.settings())
}

#[tauri::command]
pub async fn set_settings(m: State<'_, Manager>, settings: Settings) -> ApiResult<Settings> {
    Ok(m.set_settings(settings)?)
}

#[tauri::command]
pub async fn open_download<R: Runtime>(
    app: AppHandle<R>,
    m: State<'_, Manager>,
    id: DownloadId,
) -> ApiResult<()> {
    let path = completed_path(&row_of(&m, &id)?)?;
    app.opener()
        .open_path(path.to_string_lossy(), None::<&str>)
        .map_err(|e| ApiError::internal(e.to_string()))
}

#[tauri::command]
pub async fn show_download_in_folder<R: Runtime>(
    app: AppHandle<R>,
    m: State<'_, Manager>,
    id: DownloadId,
) -> ApiResult<()> {
    let path = completed_path(&row_of(&m, &id)?)?;
    app.opener()
        .reveal_item_in_dir(&path)
        .map_err(|e| ApiError::internal(e.to_string()))
}

#[tauri::command]
pub async fn pick_folder<R: Runtime>(
    app: AppHandle<R>,
    current: Option<String>,
) -> ApiResult<Option<String>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let mut dialog = app.dialog().file().set_title("Choose a download folder");
    if let Some(dir) = current.filter(|d| !d.is_empty()) {
        dialog = dialog.set_directory(dir);
    }
    dialog.pick_folder(move |picked| {
        let _ = tx.send(picked);
    });
    let picked = rx
        .await
        .map_err(|_| ApiError::internal("the folder dialog closed unexpectedly"))?;
    Ok(picked
        .and_then(|p| p.into_path().ok())
        .map(|p| p.display().to_string()))
}

#[tauri::command]
pub async fn clipboard_url<R: Runtime>(app: AppHandle<R>) -> ApiResult<Option<String>> {
    // An empty or non-text clipboard is not an error: there is just no link.
    Ok(app
        .clipboard()
        .read_text()
        .ok()
        .and_then(|t| clipboard_link(&t)))
}

#[tauri::command]
pub async fn autostart_enabled<R: Runtime>(app: AppHandle<R>) -> ApiResult<bool> {
    app.autolaunch()
        .is_enabled()
        .map_err(|e| ApiError::internal(e.to_string()))
}

#[tauri::command]
pub async fn set_autostart<R: Runtime>(app: AppHandle<R>, enabled: bool) -> ApiResult<bool> {
    let al = app.autolaunch();
    let r = if enabled { al.enable() } else { al.disable() };
    r.map_err(|e| ApiError::internal(e.to_string()))?;
    al.is_enabled().map_err(|e| ApiError::internal(e.to_string()))
}
```

If a plugin method name differs in the installed 2.x version (e.g. `reveal_item_in_dir`, `FilePath::into_path`), use the installed crate's equivalent — check with `cargo doc -p tauri-plugin-opener --open` or the crate source under `~/.cargo/registry` — and keep the behaviour described here.

- [ ] **Step 3: The forwarder, the notification, the tray, the close button**

Append to `src-tauri/src/events.rs` (above `#[cfg(test)]`):

```rust
use mdm_core::Manager;
use tauri::{AppHandle, Emitter as _, Runtime};
use tauri_plugin_notification::NotificationExt as _;
use tokio::sync::broadcast::error::RecvError;

/// Forward every manager event to the window, announce completions, and ask
/// the UI to reload when events were lost.
pub fn spawn_forwarder<R: Runtime>(app: AppHandle<R>, manager: Manager) {
    let mut rx = manager.subscribe();
    tauri::async_runtime::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(e) => {
                    if let Some(name) = completed_name(&e) {
                        if manager.settings().notify_on_complete {
                            notify_complete(&app, name);
                        }
                    }
                    if let Err(err) = app.emit(event_name(&e), &e) {
                        tracing::warn!(error = %err, "sending an event to the window failed");
                    }
                }
                Err(RecvError::Lagged(skipped)) => {
                    tracing::warn!(skipped, "the window fell behind; asking it to reload");
                    let _ = app.emit(RESYNC, ());
                }
                Err(RecvError::Closed) => break,
            }
        }
    });
}

fn notify_complete<R: Runtime>(app: &AppHandle<R>, name: &str) {
    if let Err(e) = app
        .notification()
        .builder()
        .title("Download complete")
        .body(name)
        .show()
    {
        tracing::warn!(error = %e, "showing the completion notification failed");
    }
}
```

(Move the `use` lines to the top of the file with the others.)

`src-tauri/src/tray.rs`:

```rust
//! The tray icon: show the window, pause / resume everything, quit.

use mdm_core::Manager;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{App, AppHandle, Manager as _, Runtime};

use crate::window::show_main;

/// Whether a tray icon exists (some Linux desktops have none).
pub struct TrayState(pub bool);

/// Build the tray icon and its menu.
pub fn build<R: Runtime>(app: &App<R>) -> tauri::Result<()> {
    let show = MenuItemBuilder::with_id("show", "Show Muzn Download Manager").build(app)?;
    let pause = MenuItemBuilder::with_id("pause_all", "Pause all").build(app)?;
    let resume = MenuItemBuilder::with_id("resume_all", "Resume all").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit").build(app)?;
    let menu = MenuBuilder::new(app)
        .items(&[&show, &pause, &resume])
        .separator()
        .item(&quit)
        .build()?;
    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("Muzn Download Manager")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => show_main(app),
            "pause_all" => on_manager(app, Manager::pause_all),
            "resume_all" => on_manager(app, Manager::resume_all),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

fn on_manager<R: Runtime>(app: &AppHandle<R>, action: fn(&Manager) -> mdm_core::Result<()>) {
    if let Some(m) = app.try_state::<Manager>() {
        if let Err(e) = action(&m) {
            tracing::warn!(error = %e, "a tray action failed");
        }
    }
}
```

(`show_menu_on_left_click` is the 2.x name; older 2.x releases call it `menu_on_left_click`.)

Add to `src-tauri/src/window.rs`:

```rust
use tauri::{Window, WindowEvent};

use crate::tray::TrayState;

/// The close button: hide to the tray when `hides_on_close` says so; otherwise
/// let the window close, which quits the app (and `run` saves the downloads).
pub fn on_window_event<R: Runtime>(window: &Window<R>, event: &WindowEvent) {
    let WindowEvent::CloseRequested { api, .. } = event else {
        return;
    };
    if window.label() != MAIN {
        return;
    }
    let app = window.app_handle();
    let has_tray = app.try_state::<TrayState>().is_some_and(|t| t.0);
    let close_to_tray = app
        .try_state::<mdm_core::Manager>()
        .is_some_and(|m| m.settings().close_to_tray);
    if hides_on_close(has_tray, close_to_tray) {
        api.prevent_close();
        let _ = window.hide();
    }
}
```

`src-tauri/src/lib.rs` — modules `api`, `commands`, `events`, `paths`, `tray`, `window`; in `setup`, after `app.manage(manager)`:

```rust
            let manager = app.state::<mdm_core::Manager>().inner().clone();
            events::spawn_forwarder(app.handle().clone(), manager);
            let has_tray = match tray::build(app) {
                Ok(()) => true,
                Err(e) => {
                    tracing::warn!(error = %e, "no tray icon; the close button will quit");
                    false
                }
            };
            app.manage(tray::TrayState(has_tray));
```

and on the builder, before `.build(...)`:

```rust
        .on_window_event(window::on_window_event)
        .invoke_handler(tauri::generate_handler![
            commands::list_downloads,
            commands::download_segments,
            commands::add_download,
            commands::probe_url,
            commands::pause_download,
            commands::resume_download,
            commands::cancel_download,
            commands::restart_download,
            commands::remove_download,
            commands::pause_all,
            commands::resume_all,
            commands::get_settings,
            commands::set_settings,
            commands::open_download,
            commands::show_download_in_folder,
            commands::pick_folder,
            commands::clipboard_url,
            commands::autostart_enabled,
            commands::set_autostart,
        ])
```

- [ ] **Step 4: Run the tests and check by hand**

Run: `pnpm build && cargo test -p mdm-app && cargo clippy --workspace --all-targets -- -D warnings` — Expected: all pass, clippy clean.
Run `pnpm tauri dev`: a tray icon appears; closing the window hides it, a left click on the tray icon brings it back, tray → Quit exits the process. (The UI still shows only the heading; Task 6 wires it.)

- [ ] **Step 5: Commit**

```bash
git add src-tauri
git commit -m "feat(app): commands and events over the manager, a tray, and close-to-tray

Every manager action becomes an async Tauri command that fails as {code,
message} with the core's stable code. The UI passes download ids, never paths:
opening a file or revealing it resolves the path from a COMPLETED row in Rust.
Manager events reach the window as download:progress and download:status, and
download:resync asks the UI to reload when the forwarder fell behind. A
completion shows an OS notification (setting, default on). The tray shows the
window, pauses or resumes everything and quits; the close button hides to the
tray only when a tray exists and the user wants it.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 5: UI data layer — types, the backend interface (Tauri + fake), the downloads store, formatting and error text

**Files:**
- Create: `src/api/types.ts`, `src/api/backend.ts`, `src/api/tauri.ts`, `src/api/fake.ts`
- Create: `src/state/downloads.ts`, `src/state/context.tsx`
- Create: `src/lib/format.ts`, `src/lib/errors.ts`
- Test: `src/state/downloads.test.ts`, `src/lib/format.test.ts`, `src/lib/errors.test.ts`, `src/api/fake.test.ts`

**Interfaces:**
- Consumes: the command / event table of Task 4.
- Produces (later tasks import exactly these):
  - `types.ts`: `DownloadStatus`, `NameValue`, `DownloadRow`, `SegmentView`, `ProgressEvent`, `ManagerEvent`, `NewDownload`, `ProbePreview`, `ProxySetting`, `Settings`, `ApiError`, `isApiError(e)`.
  - `backend.ts`: `interface Backend` (methods below), `inTauri(): boolean`.
  - `tauri.ts`: `tauriBackend: Backend`.
  - `fake.ts`: `interface FakeBackend extends Backend { rows: Map<string, DownloadRow>; settings: Settings; calls: string[]; clipboard: string | null; probeResult: ProbePreview | ApiError; folderPick: string | null; autostart: boolean; emit(e: ManagerEvent): void; resync(): void; }`, `createFakeBackend(init?: { rows?: DownloadRow[]; settings?: Partial<Settings> }): FakeBackend`, `fakeRow(over?: Partial<DownloadRow>): DownloadRow`, `fakeSettings(over?: Partial<Settings>): Settings`.
  - `downloads.ts`: `type Filter = "all" | "active" | "completed" | "failed"`, `interface Live`, `interface DownloadsState`, `type DownloadsStore`, `createDownloadsStore()`, `reduce(state, event)`, `visibleRows(rows, filter)`, `filterCounts(rows)`, `ACTIVE_STATUSES`.
  - `context.tsx`: `AppProvider({ backend, store, children })`, `useBackend()`, `useDownloadsStore()`, `useDownloads(selector)`, `useManagerSync(onNotice)`.
  - `format.ts`: `formatBytes(n)`, `formatSpeed(bps)`, `formatEta(secs)`, `percent(downloaded, total)`.
  - `errors.ts`: `errorText(code, detail?)`, `describeError(e: unknown)`, `rowErrorText(row)`.

- [ ] **Step 1: Types and the backend interface**

`src/api/types.ts`:

```ts
// Mirrors of the Rust serde shapes (mdm-core model.rs / events.rs / settings.rs).

export type DownloadStatus =
  | "QUEUED"
  | "PROBING"
  | "DOWNLOADING"
  | "PAUSED"
  | "COMPLETED"
  | "FAILED"
  | "CANCELLED"
  | "SEEDING";

export interface NameValue {
  name: string;
  value: string;
}

export interface DownloadRow {
  id: string;
  kind: "http" | "torrent";
  url: string;
  finalUrl: string | null;
  filename: string | null;
  dir: string;
  size: number | null;
  downloaded: number;
  status: DownloadStatus;
  etag: string | null;
  lastModified: string | null;
  mime: string | null;
  referrer: string | null;
  headers: NameValue[];
  cookies: NameValue[];
  errorCode: string | null;
  errorMessage: string | null;
  createdAt: number;
  updatedAt: number;
  completedAt: number | null;
}

export interface SegmentView {
  start: number;
  /** Inclusive. */
  end: number;
  downloaded: number;
}

export interface ProgressEvent {
  type: "progress";
  id: string;
  total: number | null;
  downloaded: number;
  speedBps: number;
  etaSecs: number | null;
  segments: SegmentView[];
}

export type ManagerEvent =
  | { type: "added"; download: DownloadRow }
  | { type: "updated"; download: DownloadRow }
  | ProgressEvent
  | { type: "removed"; id: string }
  | { type: "notice"; id: string; message: string };

export interface NewDownload {
  url: string;
  dir?: string | null;
  filename?: string | null;
  referrer?: string | null;
  headers?: NameValue[];
  cookies?: NameValue[];
  startPaused?: boolean;
}

export interface ProbePreview {
  finalUrl: string;
  filename: string;
  size: number | null;
  resumable: boolean;
  mime: string | null;
}

export type ProxySetting =
  | { mode: "system" }
  | { mode: "none" }
  | { mode: "manual"; url: string };

export interface Settings {
  downloadDir: string;
  maxConnections: number;
  maxParallel: number;
  userAgent: string | null;
  proxy: ProxySetting;
  closeToTray: boolean;
  notifyOnComplete: boolean;
}

export interface ApiError {
  code: string;
  message: string;
}

export function isApiError(e: unknown): e is ApiError {
  return (
    typeof e === "object" &&
    e !== null &&
    typeof (e as ApiError).code === "string" &&
    typeof (e as ApiError).message === "string"
  );
}
```

`src/api/backend.ts`:

```ts
import type {
  DownloadRow,
  ManagerEvent,
  NewDownload,
  ProbePreview,
  SegmentView,
  Settings,
} from "./types";

/** Everything the UI may ask of the app. The UI talks to nothing else. */
export interface Backend {
  list(): Promise<DownloadRow[]>;
  segments(id: string): Promise<SegmentView[]>;
  add(download: NewDownload): Promise<DownloadRow>;
  probe(url: string, referrer?: string | null): Promise<ProbePreview>;
  pause(id: string): Promise<void>;
  resume(id: string): Promise<void>;
  cancel(id: string): Promise<void>;
  restart(id: string): Promise<void>;
  remove(id: string, deleteFile: boolean): Promise<void>;
  pauseAll(): Promise<void>;
  resumeAll(): Promise<void>;
  getSettings(): Promise<Settings>;
  setSettings(settings: Settings): Promise<Settings>;
  openFile(id: string): Promise<void>;
  showInFolder(id: string): Promise<void>;
  pickFolder(current: string | null): Promise<string | null>;
  clipboardUrl(): Promise<string | null>;
  autostartEnabled(): Promise<boolean>;
  setAutostart(enabled: boolean): Promise<boolean>;
  /** Manager events from now on; `onResync` = events were lost, reload. Resolves to an unsubscribe. */
  subscribe(onEvent: (e: ManagerEvent) => void, onResync: () => void): Promise<() => void>;
}

/** True inside the Tauri window; false in a plain browser (`pnpm dev`) and in tests. */
export function inTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}
```

`src/api/tauri.ts`:

```ts
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Backend } from "./backend";
import type {
  DownloadRow,
  ManagerEvent,
  ProbePreview,
  SegmentView,
  Settings,
} from "./types";

// Command and argument names: src-tauri/src/commands.rs (Tauri camelCases the args).
export const tauriBackend: Backend = {
  list: () => invoke<DownloadRow[]>("list_downloads"),
  segments: (id) => invoke<SegmentView[]>("download_segments", { id }),
  add: (download) => invoke<DownloadRow>("add_download", { download }),
  probe: (url, referrer) => invoke<ProbePreview>("probe_url", { url, referrer: referrer ?? null }),
  pause: (id) => invoke<void>("pause_download", { id }),
  resume: (id) => invoke<void>("resume_download", { id }),
  cancel: (id) => invoke<void>("cancel_download", { id }),
  restart: (id) => invoke<void>("restart_download", { id }),
  remove: (id, deleteFile) => invoke<void>("remove_download", { id, deleteFile }),
  pauseAll: () => invoke<void>("pause_all"),
  resumeAll: () => invoke<void>("resume_all"),
  getSettings: () => invoke<Settings>("get_settings"),
  setSettings: (settings) => invoke<Settings>("set_settings", { settings }),
  openFile: (id) => invoke<void>("open_download", { id }),
  showInFolder: (id) => invoke<void>("show_download_in_folder", { id }),
  pickFolder: (current) => invoke<string | null>("pick_folder", { current }),
  clipboardUrl: () => invoke<string | null>("clipboard_url"),
  autostartEnabled: () => invoke<boolean>("autostart_enabled"),
  setAutostart: (enabled) => invoke<boolean>("set_autostart", { enabled }),
  async subscribe(onEvent, onResync) {
    const offs = await Promise.all([
      listen<ManagerEvent>("download:progress", (e) => onEvent(e.payload)),
      listen<ManagerEvent>("download:status", (e) => onEvent(e.payload)),
      listen("download:resync", () => onResync()),
    ]);
    return () => offs.forEach((off) => off());
  },
};
```

- [ ] **Step 2: Write the failing tests**

`src/lib/format.test.ts`:

```ts
import { expect, test } from "vitest";
import { formatBytes, formatEta, formatSpeed, percent } from "./format";

test("bytes read like a download manager's", () => {
  expect(formatBytes(null)).toBe("—");
  expect(formatBytes(0)).toBe("0 B");
  expect(formatBytes(1023)).toBe("1023 B");
  expect(formatBytes(1536)).toBe("1.50 KB");
  expect(formatBytes(10 * 1024 * 1024)).toBe("10.0 MB");
  expect(formatBytes(500 * 1024 ** 3)).toBe("500 GB");
});

test("speed and time left", () => {
  expect(formatSpeed(0)).toBe("—");
  expect(formatSpeed(2 * 1024 * 1024)).toBe("2.00 MB/s");
  expect(formatEta(null)).toBe("—");
  expect(formatEta(45)).toBe("45s");
  expect(formatEta(125)).toBe("2m 05s");
  expect(formatEta(3725)).toBe("1h 02m");
});

test("percent never passes 100 and is unknown without a size", () => {
  expect(percent(50, 200)).toBe(25);
  expect(percent(1, 3)).toBe(33);
  expect(percent(10, 5)).toBe(100);
  expect(percent(5, null)).toBeNull();
  expect(percent(5, 0)).toBeNull();
});
```

`src/lib/errors.test.ts`:

```ts
import { expect, test } from "vitest";
import { describeError, errorText, rowErrorText } from "./errors";
import { fakeRow } from "../api/fake";

test("known codes read as plain English", () => {
  expect(errorText("SOURCE_CHANGED")).toBe(
    "The file on the server changed. Restart from the beginning?",
  );
  expect(errorText("DISK_FULL")).toContain("disk space");
});

test("an unknown code falls back to the detail, then a generic line", () => {
  expect(errorText("WHAT", "server said no")).toBe("server said no");
  expect(errorText(null)).toBe("Something went wrong.");
});

test("a command failure keeps its detail for the user", () => {
  expect(describeError({ code: "INVALID_URL", message: "ftp://x: unsupported scheme ftp" })).toBe(
    "That is not a valid http or https link. (ftp://x: unsupported scheme ftp)",
  );
  expect(describeError(new Error("boom"))).toBe("boom");
  expect(describeError("plain")).toBe("plain");
});

test("a row shows its error only when it has one", () => {
  expect(rowErrorText(fakeRow())).toBeNull();
  expect(
    rowErrorText(fakeRow({ status: "FAILED", errorCode: "NETWORK", errorMessage: "reset" })),
  ).toBe("Network problem. Check the connection and resume.");
});
```

`src/state/downloads.test.ts`:

```ts
import { expect, test } from "vitest";
import { fakeRow } from "../api/fake";
import type { ProgressEvent } from "../api/types";
import { createDownloadsStore, filterCounts, visibleRows } from "./downloads";

const progress = (id: string, downloaded: number): ProgressEvent => ({
  type: "progress",
  id,
  total: 100,
  downloaded,
  speedBps: 10,
  etaSecs: 9,
  segments: [{ start: 0, end: 99, downloaded }],
});

test("load replaces the rows and keeps a valid selection", () => {
  const s = createDownloadsStore();
  s.getState().load([fakeRow({ id: "a" }), fakeRow({ id: "b" })]);
  s.getState().select("b");
  s.getState().load([fakeRow({ id: "b" })]);
  expect(Object.keys(s.getState().rows)).toEqual(["b"]);
  expect(s.getState().selected).toBe("b");
  s.getState().load([]);
  expect(s.getState().selected).toBeNull();
  expect(s.getState().loaded).toBe(true);
});

test("progress is kept only for a downloading row and dropped when it stops", () => {
  const s = createDownloadsStore();
  s.getState().load([fakeRow({ id: "a", status: "DOWNLOADING" }), fakeRow({ id: "b", status: "PAUSED" })]);
  s.getState().apply(progress("a", 40));
  s.getState().apply(progress("b", 40)); // a late event for a stopped row
  s.getState().apply(progress("zzz", 1)); // unknown row
  expect(s.getState().live.a?.downloaded).toBe(40);
  expect(s.getState().live.b).toBeUndefined();
  expect(s.getState().live.zzz).toBeUndefined();
  s.getState().apply({ type: "updated", download: fakeRow({ id: "a", status: "PAUSED", downloaded: 40 }) });
  expect(s.getState().live.a).toBeUndefined();
  expect(s.getState().rows.a?.status).toBe("PAUSED");
});

test("added and removed rows, and the selection follows a removal", () => {
  const s = createDownloadsStore();
  s.getState().load([]);
  s.getState().apply({ type: "added", download: fakeRow({ id: "n" }) });
  s.getState().select("n");
  expect(s.getState().rows.n).toBeDefined();
  s.getState().apply({ type: "removed", id: "n" });
  expect(s.getState().rows.n).toBeUndefined();
  expect(s.getState().selected).toBeNull();
});

test("filters and counts, newest first", () => {
  const rows = {
    q: fakeRow({ id: "q", status: "QUEUED", createdAt: 1 }),
    d: fakeRow({ id: "d", status: "DOWNLOADING", createdAt: 3 }),
    p: fakeRow({ id: "p", status: "PAUSED", createdAt: 2 }),
    c: fakeRow({ id: "c", status: "COMPLETED", createdAt: 4 }),
    f: fakeRow({ id: "f", status: "FAILED", createdAt: 5 }),
    x: fakeRow({ id: "x", status: "CANCELLED", createdAt: 6 }),
  };
  expect(visibleRows(rows, "all").map((r) => r.id)).toEqual(["x", "f", "c", "d", "p", "q"]);
  expect(visibleRows(rows, "active").map((r) => r.id)).toEqual(["d", "p", "q"]);
  expect(visibleRows(rows, "completed").map((r) => r.id)).toEqual(["c"]);
  expect(visibleRows(rows, "failed").map((r) => r.id)).toEqual(["f"]);
  expect(filterCounts(rows)).toEqual({ all: 6, active: 3, completed: 1, failed: 1 });
});
```

`src/api/fake.test.ts`:

```ts
import { expect, test } from "vitest";
import type { ManagerEvent } from "./types";
import { createFakeBackend, fakeRow } from "./fake";

test("the fake announces what it does, like the real manager", async () => {
  const fake = createFakeBackend({ rows: [fakeRow({ id: "a", status: "DOWNLOADING" })] });
  const seen: ManagerEvent[] = [];
  const off = await fake.subscribe((e) => seen.push(e), () => {});
  await fake.pause("a");
  const added = await fake.add({ url: "https://x/y.zip", startPaused: true });
  await fake.remove("a", false);
  off();
  await fake.resume(added.id);
  expect(seen.map((e) => e.type)).toEqual(["updated", "added", "removed"]);
  expect(added.status).toBe("PAUSED");
  expect(fake.calls).toEqual(["pause a", `add https://x/y.zip`, "remove a false", `resume ${added.id}`]);
});

test("probe answers the configured preview or rejects with it", async () => {
  const fake = createFakeBackend();
  expect((await fake.probe("https://x/y.zip")).filename).toBe("y.zip");
  fake.probeResult = { code: "HTTP_STATUS", message: "404" };
  await expect(fake.probe("https://x/z")).rejects.toEqual({ code: "HTTP_STATUS", message: "404" });
});
```

Run: `pnpm test` — Expected: FAIL (modules do not exist).

- [ ] **Step 3: Implement**

`src/lib/format.ts`:

```ts
const UNITS = ["B", "KB", "MB", "GB", "TB"] as const;

/** 1536 → "1.50 KB"; three significant digits above a kilobyte. */
export function formatBytes(n: number | null | undefined): string {
  if (n == null) return "—";
  if (n < 1024) return `${n} B`;
  let v = n;
  let i = 0;
  while (v >= 1024 && i < UNITS.length - 1) {
    v /= 1024;
    i++;
  }
  const digits = v < 10 ? 2 : v < 100 ? 1 : 0;
  return `${v.toFixed(digits)} ${UNITS[i]}`;
}

export function formatSpeed(bps: number): string {
  return bps > 0 ? `${formatBytes(bps)}/s` : "—";
}

const pad = (n: number) => String(n).padStart(2, "0");

export function formatEta(secs: number | null): string {
  if (secs == null) return "—";
  if (secs < 60) return `${secs}s`;
  const m = Math.floor(secs / 60);
  if (m < 60) return `${m}m ${pad(secs % 60)}s`;
  return `${Math.floor(m / 60)}h ${pad(m % 60)}m`;
}

/** Whole percent, capped at 100; `null` when the size is unknown. */
export function percent(downloaded: number, total: number | null): number | null {
  if (!total) return null;
  return Math.min(100, Math.floor((downloaded * 100) / total));
}
```

`src/lib/errors.ts`:

```ts
import { isApiError, type DownloadRow } from "../api/types";

// The one place stable error codes become words (Global Constraints).
const MESSAGES: Record<string, string> = {
  SOURCE_CHANGED: "The file on the server changed. Restart from the beginning?",
  HTTP_STATUS: "The server refused the download — the link may have expired.",
  NETWORK: "Network problem. Check the connection and resume.",
  TLS: "The secure connection failed.",
  DISK_FULL: "Not enough disk space. Free some space and resume.",
  IO: "The file could not be written.",
  PART_IN_USE: "Another download is writing this file.",
  RANGE_NOT_SUPPORTED: "The server does not support resuming this file.",
  INVALID_URL: "That is not a valid http or https link.",
  INVALID_RESUME: "The saved progress no longer fits this file.",
  INVALID_STATE: "That action does not fit this download right now.",
  NOT_FOUND: "That download no longer exists.",
  SETTINGS: "A setting is not valid.",
  INTERNAL: "Something went wrong inside the app.",
  DB: "The download list could not be saved.",
};

export function errorText(code: string | null | undefined, detail?: string | null): string {
  if (code && MESSAGES[code]) return MESSAGES[code];
  return detail || "Something went wrong.";
}

/** A rejected command (or any thrown value) as one line for a toast. */
export function describeError(e: unknown): string {
  if (isApiError(e)) {
    const known = MESSAGES[e.code];
    return known ? `${known} (${e.message})` : e.message;
  }
  if (e instanceof Error) return e.message;
  return String(e);
}

export function rowErrorText(row: DownloadRow): string | null {
  if (!row.errorCode) return null;
  return errorText(row.errorCode, row.errorMessage);
}
```

`src/state/downloads.ts`:

```ts
import { createStore, type StoreApi } from "zustand/vanilla";
import type { DownloadRow, DownloadStatus, ManagerEvent, SegmentView } from "../api/types";

export type Filter = "all" | "active" | "completed" | "failed";

/** Live figures of a running download (from progress events). */
export interface Live {
  total: number | null;
  downloaded: number;
  speedBps: number;
  etaSecs: number | null;
  segments: SegmentView[];
}

export interface DownloadsState {
  rows: Record<string, DownloadRow>;
  live: Record<string, Live>;
  filter: Filter;
  selected: string | null;
  loaded: boolean;
  load(rows: DownloadRow[]): void;
  apply(e: ManagerEvent): void;
  setFilter(f: Filter): void;
  select(id: string | null): void;
}

export type DownloadsStore = StoreApi<DownloadsState>;

export const ACTIVE_STATUSES: readonly DownloadStatus[] = ["QUEUED", "PROBING", "DOWNLOADING", "PAUSED"];

const FILTERS: Record<Filter, (r: DownloadRow) => boolean> = {
  all: () => true,
  active: (r) => ACTIVE_STATUSES.includes(r.status),
  completed: (r) => r.status === "COMPLETED",
  failed: (r) => r.status === "FAILED",
};

function without<T>(obj: Record<string, T>, key: string): Record<string, T> {
  if (!(key in obj)) return obj;
  const copy = { ...obj };
  delete copy[key];
  return copy;
}

/** How one manager event changes the state (pure; the store's `apply`). */
export function reduce(s: DownloadsState, e: ManagerEvent): Partial<DownloadsState> {
  switch (e.type) {
    case "added":
    case "updated": {
      const rows = { ...s.rows, [e.download.id]: e.download };
      return e.download.status === "DOWNLOADING" ? { rows } : { rows, live: without(s.live, e.download.id) };
    }
    case "progress": {
      // A progress event that arrives after its row stopped is late: ignore it.
      if (s.rows[e.id]?.status !== "DOWNLOADING") return {};
      const { total, downloaded, speedBps, etaSecs, segments } = e;
      return { live: { ...s.live, [e.id]: { total, downloaded, speedBps, etaSecs, segments } } };
    }
    case "removed":
      return {
        rows: without(s.rows, e.id),
        live: without(s.live, e.id),
        selected: s.selected === e.id ? null : s.selected,
      };
    case "notice":
      return {};
  }
}

export function createDownloadsStore(): DownloadsStore {
  return createStore<DownloadsState>()((set) => ({
    rows: {},
    live: {},
    filter: "all",
    selected: null,
    loaded: false,
    load: (rows) =>
      set((s) => {
        const byId = Object.fromEntries(rows.map((r) => [r.id, r]));
        const live = Object.fromEntries(
          Object.entries(s.live).filter(([id]) => byId[id]?.status === "DOWNLOADING"),
        );
        const selected = s.selected && byId[s.selected] ? s.selected : null;
        return { rows: byId, live, selected, loaded: true };
      }),
    apply: (e) => set((s) => reduce(s, e)),
    setFilter: (filter) => set({ filter }),
    select: (selected) => set({ selected }),
  }));
}

/** The rows a filter shows, newest first. Call inside `useMemo` — it builds a new array. */
export function visibleRows(rows: Record<string, DownloadRow>, filter: Filter): DownloadRow[] {
  return Object.values(rows)
    .filter(FILTERS[filter])
    .sort((a, b) => b.createdAt - a.createdAt || a.id.localeCompare(b.id));
}

export function filterCounts(rows: Record<string, DownloadRow>): Record<Filter, number> {
  const all = Object.values(rows);
  return {
    all: all.length,
    active: all.filter(FILTERS.active).length,
    completed: all.filter(FILTERS.completed).length,
    failed: all.filter(FILTERS.failed).length,
  };
}
```

`src/state/context.tsx`:

```tsx
import { createContext, useContext, useEffect, useMemo, type ReactNode } from "react";
import { useStore } from "zustand";
import type { Backend } from "../api/backend";
import { describeError } from "../lib/errors";
import type { DownloadsState, DownloadsStore } from "./downloads";

interface AppCtx {
  backend: Backend;
  store: DownloadsStore;
}

const Ctx = createContext<AppCtx | null>(null);

export function AppProvider(props: { backend: Backend; store: DownloadsStore; children: ReactNode }) {
  const { backend, store, children } = props;
  const value = useMemo(() => ({ backend, store }), [backend, store]);
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}

function useCtx(): AppCtx {
  const ctx = useContext(Ctx);
  if (!ctx) throw new Error("AppProvider is missing");
  return ctx;
}

export const useBackend = (): Backend => useCtx().backend;
export const useDownloadsStore = (): DownloadsStore => useCtx().store;

/** Select from the downloads state. The selector must return a stable value (no new arrays). */
export function useDownloads<T>(selector: (s: DownloadsState) => T): T {
  return useStore(useCtx().store, selector);
}

/**
 * Keep the store in step with the manager: subscribe FIRST, then load the
 * list, so nothing that happens in between is missed. `onNotice` must be a
 * stable function (a module-level one such as `toast`).
 */
export function useManagerSync(onNotice: (message: string) => void): void {
  const { backend, store } = useCtx();
  useEffect(() => {
    let cancelled = false;
    let off: (() => void) | undefined;
    const reload = () =>
      backend
        .list()
        .then((rows) => {
          if (!cancelled) store.getState().load(rows);
        })
        .catch((e: unknown) => onNotice(describeError(e)));
    backend
      .subscribe(
        (e) => {
          store.getState().apply(e);
          if (e.type === "notice") onNotice(e.message);
        },
        () => void reload(),
      )
      .then((unsubscribe) => {
        if (cancelled) unsubscribe();
        else {
          off = unsubscribe;
          void reload();
        }
      })
      .catch((e: unknown) => onNotice(describeError(e)));
    return () => {
      cancelled = true;
      off?.();
    };
  }, [backend, store, onNotice]);
}
```

`src/api/fake.ts`:

```ts
import type { Backend } from "./backend";
import type {
  ApiError,
  DownloadRow,
  ManagerEvent,
  NewDownload,
  ProbePreview,
  Settings,
} from "./types";
import { isApiError } from "./types";

export function fakeRow(over: Partial<DownloadRow> = {}): DownloadRow {
  return {
    id: "row-1",
    kind: "http",
    url: "https://example.com/file.zip",
    finalUrl: null,
    filename: "file.zip",
    dir: "/downloads",
    size: 1000,
    downloaded: 0,
    status: "QUEUED",
    etag: null,
    lastModified: null,
    mime: null,
    referrer: null,
    headers: [],
    cookies: [],
    errorCode: null,
    errorMessage: null,
    createdAt: 0,
    updatedAt: 0,
    completedAt: null,
    ...over,
  };
}

export function fakeSettings(over: Partial<Settings> = {}): Settings {
  return {
    downloadDir: "/downloads",
    maxConnections: 8,
    maxParallel: 3,
    userAgent: null,
    proxy: { mode: "system" },
    closeToTray: true,
    notifyOnComplete: true,
    ...over,
  };
}

export interface FakeBackend extends Backend {
  rows: Map<string, DownloadRow>;
  settings: Settings;
  /** What was asked, in order: "pause a", "remove a true", "add <url>"… */
  calls: string[];
  clipboard: string | null;
  probeResult: ProbePreview | ApiError;
  folderPick: string | null;
  autostart: boolean;
  emit(e: ManagerEvent): void;
  resync(): void;
}

/** An in-memory stand-in for the app: tests and the plain-browser dev mode. */
export function createFakeBackend(
  init: { rows?: DownloadRow[]; settings?: Partial<Settings> } = {},
): FakeBackend {
  const listeners = new Set<(e: ManagerEvent) => void>();
  const resyncers = new Set<() => void>();
  let next = 1;
  const fake: FakeBackend = {
    rows: new Map((init.rows ?? []).map((r) => [r.id, r])),
    settings: fakeSettings(init.settings),
    calls: [],
    clipboard: null,
    probeResult: {
      finalUrl: "https://example.com/y.zip",
      filename: "y.zip",
      size: 5 * 1024 * 1024,
      resumable: true,
      mime: "application/zip",
    },
    folderPick: "/picked",
    autostart: false,
    emit(e) {
      listeners.forEach((l) => l(e));
    },
    resync() {
      resyncers.forEach((r) => r());
    },
    async list() {
      return [...fake.rows.values()];
    },
    async segments(id) {
      const r = fake.rows.get(id);
      return r?.size ? [{ start: 0, end: r.size - 1, downloaded: r.downloaded }] : [];
    },
    async add(d: NewDownload) {
      fake.calls.push(`add ${d.url}`);
      const now = Date.now();
      const row = fakeRow({
        id: `fake-${next++}`,
        url: d.url,
        filename: d.filename ?? d.url.split("/").pop() ?? "download.bin",
        dir: d.dir ?? fake.settings.downloadDir,
        size: null,
        status: d.startPaused ? "PAUSED" : "QUEUED",
        createdAt: now,
        updatedAt: now,
      });
      fake.rows.set(row.id, row);
      fake.emit({ type: "added", download: row });
      return row;
    },
    async probe(url) {
      fake.calls.push(`probe ${url}`);
      if (isApiError(fake.probeResult)) throw fake.probeResult;
      return fake.probeResult;
    },
    async pause(id) {
      fake.calls.push(`pause ${id}`);
      update(id, { status: "PAUSED" });
    },
    async resume(id) {
      fake.calls.push(`resume ${id}`);
      update(id, { status: "QUEUED", errorCode: null, errorMessage: null });
    },
    async cancel(id) {
      fake.calls.push(`cancel ${id}`);
      update(id, { status: "CANCELLED", downloaded: 0 });
    },
    async restart(id) {
      fake.calls.push(`restart ${id}`);
      update(id, { status: "QUEUED", downloaded: 0, errorCode: null, errorMessage: null });
    },
    async remove(id, deleteFile) {
      fake.calls.push(`remove ${id} ${deleteFile}`);
      if (fake.rows.delete(id)) fake.emit({ type: "removed", id });
    },
    async pauseAll() {
      fake.calls.push("pauseAll");
    },
    async resumeAll() {
      fake.calls.push("resumeAll");
    },
    async getSettings() {
      return fake.settings;
    },
    async setSettings(s) {
      fake.calls.push("setSettings");
      fake.settings = s;
      return s;
    },
    async openFile(id) {
      fake.calls.push(`open ${id}`);
    },
    async showInFolder(id) {
      fake.calls.push(`showInFolder ${id}`);
    },
    async pickFolder() {
      fake.calls.push("pickFolder");
      return fake.folderPick;
    },
    async clipboardUrl() {
      return fake.clipboard;
    },
    async autostartEnabled() {
      return fake.autostart;
    },
    async setAutostart(enabled) {
      fake.calls.push(`setAutostart ${enabled}`);
      fake.autostart = enabled;
      return enabled;
    },
    async subscribe(onEvent, onResync) {
      listeners.add(onEvent);
      resyncers.add(onResync);
      return () => {
        listeners.delete(onEvent);
        resyncers.delete(onResync);
      };
    },
  };
  function update(id: string, patch: Partial<DownloadRow>) {
    const r = fake.rows.get(id);
    if (!r) return;
    const row = { ...r, ...patch, updatedAt: Date.now() };
    fake.rows.set(id, row);
    fake.emit({ type: "updated", download: row });
  }
  return fake;
}
```

- [ ] **Step 4: Run the JS gates**

Run: `pnpm test && pnpm typecheck && pnpm lint` — Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add src
git commit -m "feat(ui): the data layer - one Backend interface, a downloads store, words for error codes

The UI reaches the app only through a Backend interface: the Tauri one wraps
invoke/listen with the command names of src-tauri, a fake one keeps rows in
memory and announces what it does the way the manager does, for tests and for
running the UI in a plain browser. A vanilla zustand store reduces manager
events (late progress for a stopped row is ignored), filters and counts rows,
and the sync hook subscribes before it loads so no event falls in between.
Byte, speed and time formatting and the code-to-English map live once.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---
### Task 6: The main window — top bar, filter rail, the virtual download list with the segment map, dialogs, toasts, shortcuts

**Files:**
- Create: `src/ui/Dialog.tsx`, `src/ui/confirm.tsx`, `src/ui/toast.tsx`
- Create: `src/lib/actions.ts`, `src/lib/keys.ts`, `src/lib/url.ts`
- Create: `src/components/TopBar.tsx`, `src/components/FilterRail.tsx`, `src/components/DownloadList.tsx`, `src/components/DownloadRowView.tsx`, `src/components/SegmentBar.tsx`
- Create: `src/api/demo.ts`, `src/test/render.tsx`
- Modify: `src/App.tsx`, `src/main.tsx`, `src/styles.css`, `src/test/setup.ts`, `src/App.test.tsx`
- Test: `src/lib/actions.test.ts`, `src/lib/keys.test.ts`, `src/App.test.tsx`

**Interfaces:**
- Consumes: everything Task 5 produces.
- Produces:
  - `ui/Dialog.tsx`: `Dialog({ title, onClose, children, footer?, width? })` — modal, focus-trapped, Escape and backdrop close, focuses `[data-autofocus]` or the first field; returns focus on close.
  - `ui/confirm.tsx`: `confirmDialog({ title, message, confirmLabel, danger?, checkbox? }): Promise<{ ok: boolean; checked: boolean }>`, `ConfirmHost`.
  - `ui/toast.tsx`: `toast(message, kind?: "info" | "ok" | "error")`, `attempt<T>(p: Promise<T>): Promise<T | undefined>` (toasts a failure), `ToastHost`, `clearToasts()`.
  - `lib/actions.ts`: `displayName(row)`, `savedPath(row)`, `canPause`, `canResume`, `canRestart`, `canCancel`, `toggle(backend, row)`, `removeWithConfirm(backend, row)`, `cancelWithConfirm(backend, row)`.
  - `lib/keys.ts`: `type Shortcut`, `shortcutFor(e, target)`, `step(list, selected, dir)`.
  - `lib/url.ts`: `isWebUrl(s)`.
  - `test/render.tsx`: `renderApp(fake?)` → `{ fake, store, user, ...RenderResult }`.
  - `App` props: none; it uses the context. `App` holds `adding` / `settingsOpen` state that Tasks 7 and 9 attach their dialogs to.
  - CSS classes later tasks reuse: `.primary-button`, `.secondary-button`, `.danger-button`, `.link-button`, `.field`, `.field-row`, `.hint`, `.error-text`, `.badge`, `.badge-<status lowercase>`.

- [ ] **Step 1: Write the failing tests**

`src/lib/keys.test.ts`:

```ts
import { expect, test } from "vitest";
import { shortcutFor, step } from "./keys";

const key = (k: string, mods: Partial<{ ctrlKey: boolean; metaKey: boolean; altKey: boolean }> = {}) => ({
  key: k,
  ctrlKey: false,
  metaKey: false,
  altKey: false,
  ...mods,
});
const body = { tagName: "BODY" };

test("IDM-style shortcuts", () => {
  expect(shortcutFor(key("n", { ctrlKey: true }), body)).toBe("add");
  expect(shortcutFor(key("N", { metaKey: true }), body)).toBe("add");
  expect(shortcutFor(key(" "), body)).toBe("toggle");
  expect(shortcutFor(key("Delete"), body)).toBe("remove");
  expect(shortcutFor(key("ArrowDown"), body)).toBe("down");
  expect(shortcutFor(key("ArrowUp"), body)).toBe("up");
  expect(shortcutFor(key("x"), body)).toBeNull();
});

test("typing in a field or pressing a button is never a shortcut, except Ctrl+N", () => {
  for (const tagName of ["INPUT", "TEXTAREA", "SELECT", "BUTTON"]) {
    expect(shortcutFor(key(" "), { tagName })).toBeNull();
    expect(shortcutFor(key("Delete"), { tagName })).toBeNull();
  }
  expect(shortcutFor(key("n", { ctrlKey: true }), { tagName: "INPUT" })).toBe("add");
  expect(shortcutFor(key(" ", { ctrlKey: true }), body)).toBeNull();
});

test("arrow steps stay inside the list", () => {
  const list = [{ id: "a" }, { id: "b" }, { id: "c" }];
  expect(step(list, null, 1)).toBe("a");
  expect(step(list, "a", 1)).toBe("b");
  expect(step(list, "c", 1)).toBe("c");
  expect(step(list, "a", -1)).toBe("a");
  expect(step(list, "gone", 1)).toBe("a");
  expect(step([], null, 1)).toBeNull();
});
```

`src/lib/actions.test.ts`:

```ts
import { expect, test } from "vitest";
import { fakeRow } from "../api/fake";
import { canCancel, canPause, canRestart, canResume, displayName, savedPath } from "./actions";

test("which action fits which status", () => {
  const s = fakeRow;
  expect(canPause(s({ status: "DOWNLOADING" }))).toBe(true);
  expect(canPause(s({ status: "QUEUED" }))).toBe(true);
  expect(canPause(s({ status: "PAUSED" }))).toBe(false);
  expect(canResume(s({ status: "PAUSED" }))).toBe(true);
  expect(canResume(s({ status: "FAILED" }))).toBe(true);
  expect(canResume(s({ status: "COMPLETED" }))).toBe(false);
  expect(canRestart(s({ status: "FAILED" }))).toBe(true);
  expect(canRestart(s({ status: "DOWNLOADING" }))).toBe(false);
  expect(canRestart(s({ status: "COMPLETED" }))).toBe(false);
  expect(canCancel(s({ status: "PAUSED" }))).toBe(true);
  expect(canCancel(s({ status: "COMPLETED" }))).toBe(false);
  expect(canCancel(s({ status: "CANCELLED" }))).toBe(false);
});

test("a row's name and where it is saved", () => {
  expect(displayName(fakeRow({ filename: "a.zip" }))).toBe("a.zip");
  expect(displayName(fakeRow({ filename: null, url: "https://x.com/dir/b.iso?x=1" }))).toBe("b.iso");
  expect(displayName(fakeRow({ filename: null, url: "https://x.com/" }))).toBe("https://x.com/");
  expect(savedPath(fakeRow({ dir: "/dl", filename: "a.zip" }))).toBe("/dl/a.zip");
  expect(savedPath(fakeRow({ dir: "C:\\Users\\u\\Downloads", filename: "a.zip" }))).toBe(
    "C:\\Users\\u\\Downloads\\a.zip",
  );
  expect(savedPath(fakeRow({ dir: "/dl/", filename: "a.zip" }))).toBe("/dl/a.zip");
  expect(savedPath(fakeRow({ dir: "/dl", filename: null }))).toBe("/dl");
});
```

`src/App.test.tsx` (replaces Task 2's):

```tsx
import { act, screen, within } from "@testing-library/react";
import { expect, test } from "vitest";
import { fakeRow } from "./api/fake";
import { renderApp } from "./test/render";

const rows = [
  fakeRow({ id: "d", filename: "movie.mkv", status: "DOWNLOADING", size: 1000, createdAt: 3 }),
  fakeRow({ id: "p", filename: "paused.iso", status: "PAUSED", size: 1000, downloaded: 400, createdAt: 2 }),
  fakeRow({ id: "c", filename: "done.zip", status: "COMPLETED", size: 1000, downloaded: 1000, createdAt: 1 }),
];

test("the list shows every download, newest first", async () => {
  renderApp(undefined, rows);
  const options = await screen.findAllByRole("option");
  expect(options.map((o) => within(o).getByTestId("name").textContent)).toEqual([
    "movie.mkv",
    "paused.iso",
    "done.zip",
  ]);
  expect(screen.getByRole("heading", { name: "Muzn Download Manager" })).toBeInTheDocument();
});

test("the filter rail counts and filters", async () => {
  const { user } = renderApp(undefined, rows);
  await screen.findAllByRole("option");
  await user.click(screen.getByRole("button", { name: /Completed 1/ }));
  expect(screen.getAllByRole("option")).toHaveLength(1);
  await user.click(screen.getByRole("button", { name: /Downloading 2/ }));
  expect(screen.getAllByRole("option")).toHaveLength(2);
});

test("select a row, then pause and resume it from the toolbar and with Space", async () => {
  const { user, fake } = renderApp(undefined, rows);
  await user.click(await screen.findByText("movie.mkv"));
  expect(screen.getByRole("option", { name: /movie\.mkv/ })).toHaveAttribute("aria-selected", "true");
  await user.click(screen.getByRole("button", { name: "Pause" }));
  expect(fake.calls).toContain("pause d");
  await within(screen.getByRole("option", { name: /movie\.mkv/ })).findByText("Paused");
  (document.activeElement as HTMLElement | null)?.blur();
  await user.keyboard(" ");
  expect(fake.calls).toContain("resume d");
});

test("Delete asks first, and can also delete a finished file", async () => {
  const { user, fake } = renderApp(undefined, rows);
  await user.click(await screen.findByText("done.zip"));
  (document.activeElement as HTMLElement | null)?.blur();
  await user.keyboard("{Delete}");
  const dialog = await screen.findByRole("dialog", { name: "Remove download" });
  await user.click(within(dialog).getByRole("checkbox", { name: "Also delete the file from disk" }));
  await user.click(within(dialog).getByRole("button", { name: "Remove" }));
  expect(fake.calls).toContain("remove c true");
  expect(screen.queryByText("done.zip")).not.toBeInTheDocument();
});

test("Escape keeps the download", async () => {
  const { user, fake } = renderApp(undefined, rows);
  await user.click(await screen.findByText("paused.iso"));
  (document.activeElement as HTMLElement | null)?.blur();
  await user.keyboard("{Delete}");
  await screen.findByRole("dialog");
  await user.keyboard("{Escape}");
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(fake.calls.some((c) => c.startsWith("remove"))).toBe(false);
});

test("live progress moves the bar and a notice becomes a toast", async () => {
  const { fake } = renderApp(undefined, rows);
  await screen.findAllByRole("option");
  act(() => {
    fake.emit({
      type: "progress",
      id: "d",
      total: 1000,
      downloaded: 250,
      speedBps: 2048,
      etaSecs: 7,
      segments: [
        { start: 0, end: 499, downloaded: 250 },
        { start: 500, end: 999, downloaded: 0 },
      ],
    });
    fake.emit({ type: "notice", id: "d", message: "Starting over from the beginning" });
  });
  const row = screen.getByRole("option", { name: /movie\.mkv/ });
  expect(within(row).getByRole("progressbar")).toHaveAttribute("aria-valuenow", "25");
  expect(within(row).getByText("2.00 KB/s")).toBeInTheDocument();
  expect(await screen.findByText("Starting over from the beginning")).toBeInTheDocument();
});

test("pause all and resume all reach the app", async () => {
  const { user, fake } = renderApp(undefined, rows);
  await screen.findAllByRole("option");
  await user.click(screen.getByRole("button", { name: "Pause all" }));
  await user.click(screen.getByRole("button", { name: "Resume all" }));
  expect(fake.calls).toEqual(expect.arrayContaining(["pauseAll", "resumeAll"]));
});

test("a resync reloads the list", async () => {
  const { fake } = renderApp(undefined, rows);
  await screen.findAllByRole("option");
  fake.rows.set("n", fakeRow({ id: "n", filename: "new.bin", createdAt: 9 }));
  act(() => fake.resync());
  expect(await screen.findByText("new.bin")).toBeInTheDocument();
});
```

Run: `pnpm test` — Expected: FAIL (modules missing).

- [ ] **Step 2: Test helpers and the jsdom layout stub**

Append to `src/test/setup.ts`:

```ts
// jsdom does no layout: every element measures 0 × 0, so the virtual list
// would think its viewport is empty. Give elements a desktop-sized box.
Object.defineProperties(HTMLElement.prototype, {
  offsetHeight: { configurable: true, get: () => 600 },
  offsetWidth: { configurable: true, get: () => 1000 },
});
```

and in its `afterEach`, also call `clearToasts()` (import from `../ui/toast`).

`src/test/render.tsx`:

```tsx
import { render } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { App } from "../App";
import { createFakeBackend, type FakeBackend } from "../api/fake";
import type { DownloadRow } from "../api/types";
import { AppProvider } from "../state/context";
import { createDownloadsStore } from "../state/downloads";

/** The whole app over a fake backend (seeded with `rows` when given). */
export function renderApp(fake?: FakeBackend, rows: DownloadRow[] = []) {
  const backend = fake ?? createFakeBackend({ rows });
  const store = createDownloadsStore();
  const user = userEvent.setup();
  const result = render(
    <AppProvider backend={backend} store={store}>
      <App />
    </AppProvider>,
  );
  return { ...result, fake: backend, store, user };
}
```

- [ ] **Step 3: Pure helpers**

`src/lib/url.ts`:

```ts
/** An absolute http(s) URL. */
export function isWebUrl(s: string): boolean {
  try {
    const u = new URL(s);
    return u.protocol === "http:" || u.protocol === "https:";
  } catch {
    return false;
  }
}
```

`src/lib/keys.ts`:

```ts
export type Shortcut = "add" | "toggle" | "remove" | "up" | "down";

interface KeyLike {
  key: string;
  ctrlKey: boolean;
  metaKey: boolean;
  altKey: boolean;
}

const TYPING = ["INPUT", "TEXTAREA", "SELECT", "BUTTON"];

/** Ctrl/Cmd+N add; Space pause/resume; Delete remove; arrows move the selection. */
export function shortcutFor(e: KeyLike, target: { tagName: string } | null): Shortcut | null {
  const mod = e.ctrlKey || e.metaKey;
  if (mod && !e.altKey && e.key.toLowerCase() === "n") return "add";
  if (TYPING.includes(target?.tagName ?? "")) return null;
  if (mod || e.altKey) return null;
  switch (e.key) {
    case " ":
      return "toggle";
    case "Delete":
      return "remove";
    case "ArrowUp":
      return "up";
    case "ArrowDown":
      return "down";
    default:
      return null;
  }
}

/** The id one step up (-1) or down (+1) from `selected`, clamped to the list. */
export function step(list: { id: string }[], selected: string | null, dir: 1 | -1): string | null {
  if (list.length === 0) return null;
  const at = list.findIndex((r) => r.id === selected);
  if (at < 0) return list[0]!.id;
  const next = Math.min(list.length - 1, Math.max(0, at + dir));
  return list[next]!.id;
}
```

`src/lib/actions.ts`:

```ts
import type { Backend } from "../api/backend";
import type { DownloadRow } from "../api/types";
import { confirmDialog } from "../ui/confirm";
import { attempt } from "../ui/toast";

export function displayName(row: DownloadRow): string {
  if (row.filename) return row.filename;
  try {
    const last = new URL(row.url).pathname.split("/").filter(Boolean).pop();
    if (last) return decodeURIComponent(last);
  } catch {
    // fall through to the URL itself
  }
  return row.url;
}

/** Folder + file name with the folder's own separator. */
export function savedPath(row: DownloadRow): string {
  if (!row.filename) return row.dir;
  const sep = row.dir.includes("\\") ? "\\" : "/";
  const dir = row.dir.endsWith(sep) ? row.dir.slice(0, -1) : row.dir;
  return `${dir}${sep}${row.filename}`;
}

export const canPause = (r: DownloadRow) => ["QUEUED", "PROBING", "DOWNLOADING"].includes(r.status);
export const canResume = (r: DownloadRow) => ["PAUSED", "FAILED", "CANCELLED"].includes(r.status);
export const canRestart = (r: DownloadRow) => ["PAUSED", "FAILED", "CANCELLED"].includes(r.status);
export const canCancel = (r: DownloadRow) => !["COMPLETED", "CANCELLED"].includes(r.status);

/** Space / the toolbar: pause a running download, resume a stopped one. */
export async function toggle(backend: Backend, row: DownloadRow): Promise<void> {
  if (canPause(row)) await attempt(backend.pause(row.id));
  else if (canResume(row)) await attempt(backend.resume(row.id));
}

export async function removeWithConfirm(backend: Backend, row: DownloadRow): Promise<void> {
  const finished = row.status === "COMPLETED";
  const answer = await confirmDialog({
    title: "Remove download",
    message: finished
      ? `Remove "${displayName(row)}" from the list?`
      : `Remove "${displayName(row)}" from the list? The partly downloaded data is deleted.`,
    confirmLabel: "Remove",
    danger: true,
    checkbox: finished ? "Also delete the file from disk" : undefined,
  });
  if (answer.ok) await attempt(backend.remove(row.id, answer.checked));
}

export async function cancelWithConfirm(backend: Backend, row: DownloadRow): Promise<void> {
  const answer = await confirmDialog({
    title: "Cancel download",
    message: `Cancel "${displayName(row)}"? The partly downloaded data is deleted.`,
    confirmLabel: "Cancel download",
    danger: true,
  });
  if (answer.ok) await attempt(backend.cancel(row.id));
}
```

- [ ] **Step 4: Primitives — dialog, confirm, toast**

`src/ui/Dialog.tsx`:

```tsx
import { useEffect, useId, useRef, type KeyboardEvent, type ReactNode } from "react";
import { createPortal } from "react-dom";

const FOCUSABLE =
  'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

interface DialogProps {
  title: string;
  onClose(): void;
  children: ReactNode;
  footer?: ReactNode;
  width?: number;
}

/** The one modal: focus trapped inside, Escape / backdrop close, focus returns on close. */
export function Dialog({ title, onClose, children, footer, width = 520 }: DialogProps) {
  const ref = useRef<HTMLDivElement>(null);
  const titleId = useId();

  useEffect(() => {
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const el = ref.current;
    const first =
      el?.querySelector<HTMLElement>("[data-autofocus]") ?? el?.querySelector<HTMLElement>(FOCUSABLE);
    (first ?? el)?.focus();
    return () => previous?.focus();
  }, []);

  function onKeyDown(e: KeyboardEvent<HTMLDivElement>) {
    if (e.key === "Escape") {
      e.stopPropagation();
      onClose();
      return;
    }
    if (e.key !== "Tab" || !ref.current) return;
    const items = [...ref.current.querySelectorAll<HTMLElement>(FOCUSABLE)];
    const first = items[0];
    const last = items[items.length - 1];
    if (!first || !last) return;
    if (e.shiftKey && document.activeElement === first) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      first.focus();
    }
  }

  return createPortal(
    <div
      className="overlay"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div
        ref={ref}
        className="dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
        style={{ width }}
        onKeyDown={onKeyDown}
      >
        <h2 id={titleId} className="dialog-title">
          {title}
        </h2>
        <div className="dialog-body">{children}</div>
        {footer && <div className="dialog-footer">{footer}</div>}
      </div>
    </div>,
    document.body,
  );
}
```

`src/ui/toast.tsx`:

```tsx
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import { describeError } from "../lib/errors";

type Kind = "info" | "ok" | "error";
interface Toast {
  id: number;
  message: string;
  kind: Kind;
}

const toasts = createStore<{ items: Toast[] }>(() => ({ items: [] }));
let nextId = 1;

function dismiss(id: number) {
  toasts.setState((s) => ({ items: s.items.filter((t) => t.id !== id) }));
}

/** The one notice primitive (never alert()). At most four stay on screen. */
export function toast(message: string, kind: Kind = "info"): void {
  const id = nextId++;
  toasts.setState((s) => ({ items: [...s.items, { id, message, kind }].slice(-4) }));
  setTimeout(() => dismiss(id), kind === "error" ? 8000 : 4000);
}

/** Await a command; a failure becomes an error toast instead of an unhandled rejection. */
export async function attempt<T>(p: Promise<T>): Promise<T | undefined> {
  try {
    return await p;
  } catch (e) {
    toast(describeError(e), "error");
    return undefined;
  }
}

export function clearToasts(): void {
  toasts.setState({ items: [] });
}

export function ToastHost() {
  const items = useStore(toasts, (s) => s.items);
  return (
    <div className="toasts" role="status" aria-live="polite">
      {items.map((t) => (
        <div key={t.id} className={`toast toast-${t.kind}`}>
          <span>{t.message}</span>
          <button type="button" className="link-button" aria-label="Dismiss" onClick={() => dismiss(t.id)}>
            ×
          </button>
        </div>
      ))}
    </div>
  );
}
```

`src/ui/confirm.tsx`:

```tsx
import { useState } from "react";
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import { Dialog } from "./Dialog";

interface ConfirmOptions {
  title: string;
  message: string;
  confirmLabel: string;
  danger?: boolean;
  /** Offer a checkbox (e.g. "Also delete the file from disk"). */
  checkbox?: string;
}
interface ConfirmAnswer {
  ok: boolean;
  checked: boolean;
}
interface Request extends ConfirmOptions {
  resolve(a: ConfirmAnswer): void;
}

const requests = createStore<{ current: Request | null }>(() => ({ current: null }));

/** The one confirmation primitive (never window.confirm). */
export function confirmDialog(opts: ConfirmOptions): Promise<ConfirmAnswer> {
  return new Promise((resolve) => requests.setState({ current: { ...opts, resolve } }));
}

export function ConfirmHost() {
  const req = useStore(requests, (s) => s.current);
  const [checked, setChecked] = useState(false);
  if (!req) return null;
  const close = (ok: boolean) => {
    requests.setState({ current: null });
    setChecked(false);
    req.resolve({ ok, checked: ok && checked });
  };
  return (
    <Dialog
      title={req.title}
      onClose={() => close(false)}
      width={420}
      footer={
        <>
          <button type="button" className="secondary-button" onClick={() => close(false)}>
            Keep
          </button>
          <button
            type="button"
            className={req.danger ? "danger-button" : "primary-button"}
            data-autofocus
            onClick={() => close(true)}
          >
            {req.confirmLabel}
          </button>
        </>
      }
    >
      <p>{req.message}</p>
      {req.checkbox && (
        <label className="check">
          <input type="checkbox" checked={checked} onChange={(e) => setChecked(e.target.checked)} />
          {req.checkbox}
        </label>
      )}
    </Dialog>
  );
}
```

- [ ] **Step 5: The components**

`src/components/SegmentBar.tsx`:

```tsx
import type { DownloadStatus, SegmentView } from "../api/types";
import { percent } from "../lib/format";

interface Props {
  status: DownloadStatus;
  total: number | null;
  downloaded: number;
  /** The live segment map; `null` = draw one plain bar. */
  segments: SegmentView[] | null;
}

/** IDM's look: one cell per segment, each filled as far as it got. */
export function SegmentBar({ status, total, downloaded, segments }: Props) {
  const done = status === "COMPLETED";
  const pct = done ? 100 : percent(downloaded, total);
  const label = pct == null ? "Progress unknown" : `${pct}%`;
  return (
    <div
      className={`segbar${pct == null && status === "DOWNLOADING" ? " segbar-busy" : ""}`}
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={pct ?? undefined}
    >
      {!done && total && segments && segments.length > 0 ? (
        segments.map((s) => {
          const len = s.end - s.start + 1;
          return (
            <div key={s.start} className="seg" style={{ width: `${(len / total) * 100}%` }}>
              <div className="seg-fill" style={{ width: `${Math.min(100, (s.downloaded / len) * 100)}%` }} />
            </div>
          );
        })
      ) : (
        <div className="seg" style={{ width: "100%" }}>
          <div className="seg-fill" style={{ width: `${pct ?? 0}%` }} />
        </div>
      )}
    </div>
  );
}
```

`src/components/DownloadRowView.tsx`:

```tsx
import { memo, type CSSProperties } from "react";
import type { DownloadRow } from "../api/types";
import { displayName } from "../lib/actions";
import { formatBytes, formatEta, formatSpeed, percent } from "../lib/format";
import type { Live } from "../state/downloads";
import { SegmentBar } from "./SegmentBar";

export const STATUS_LABEL: Record<DownloadRow["status"], string> = {
  QUEUED: "Queued",
  PROBING: "Connecting",
  DOWNLOADING: "Downloading",
  PAUSED: "Paused",
  COMPLETED: "Completed",
  FAILED: "Failed",
  CANCELLED: "Cancelled",
  SEEDING: "Seeding",
};

interface Props {
  row: DownloadRow;
  live: Live | undefined;
  selected: boolean;
  onSelect(id: string): void;
  onOpen(row: DownloadRow): void;
  style: CSSProperties;
}

export const DownloadRowView = memo(function DownloadRowView({ row, live, selected, onSelect, onOpen, style }: Props) {
  const total = live?.total ?? row.size;
  const downloaded = live?.downloaded ?? row.downloaded;
  const pct = row.status === "COMPLETED" ? 100 : percent(downloaded, total);
  const name = displayName(row);
  return (
    <div
      role="option"
      aria-selected={selected}
      aria-label={name}
      className={`row${selected ? " row-selected" : ""}`}
      style={style}
      onClick={() => onSelect(row.id)}
      onDoubleClick={() => onOpen(row)}
    >
      <div className="cell cell-name">
        <span className="name" data-testid="name" title={name}>
          {name}
        </span>
        <small className="sub">
          {total ? `${formatBytes(downloaded)} of ${formatBytes(total)}` : formatBytes(downloaded)}
        </small>
      </div>
      <div className="cell cell-size">{formatBytes(total)}</div>
      <div className="cell cell-progress">
        <SegmentBar status={row.status} total={total} downloaded={downloaded} segments={live?.segments ?? null} />
        <small className="pct">{pct == null ? "—" : `${pct}%`}</small>
      </div>
      <div className="cell cell-speed">{live ? formatSpeed(live.speedBps) : "—"}</div>
      <div className="cell cell-eta">{live ? formatEta(live.etaSecs) : "—"}</div>
      <div className="cell cell-status">
        <span className={`badge badge-${row.status.toLowerCase()}`}>{STATUS_LABEL[row.status]}</span>
      </div>
    </div>
  );
});
```

`src/components/DownloadList.tsx`:

```tsx
import { useVirtualizer } from "@tanstack/react-virtual";
import { useCallback, useMemo, useRef } from "react";
import type { DownloadRow } from "../api/types";
import { useBackend, useDownloads, useDownloadsStore } from "../state/context";
import { visibleRows } from "../state/downloads";
import { attempt } from "../ui/toast";
import { DownloadRowView } from "./DownloadRowView";

const ROW_HEIGHT = 48;

export function DownloadList() {
  const backend = useBackend();
  const store = useDownloadsStore();
  const rowsById = useDownloads((s) => s.rows);
  const filter = useDownloads((s) => s.filter);
  const live = useDownloads((s) => s.live);
  const selected = useDownloads((s) => s.selected);
  const loaded = useDownloads((s) => s.loaded);
  const rows = useMemo(() => visibleRows(rowsById, filter), [rowsById, filter]);
  const scrollRef = useRef<HTMLDivElement>(null);
  const virtual = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 8,
  });
  const onSelect = useCallback((id: string) => store.getState().select(id), [store]);
  const onOpen = useCallback(
    (row: DownloadRow) => {
      if (row.status === "COMPLETED") void attempt(backend.openFile(row.id));
    },
    [backend],
  );

  return (
    <div className="list">
      <div className="list-head" aria-hidden="true">
        <div className="cell cell-name">Name</div>
        <div className="cell cell-size">Size</div>
        <div className="cell cell-progress">Progress</div>
        <div className="cell cell-speed">Speed</div>
        <div className="cell cell-eta">Time left</div>
        <div className="cell cell-status">Status</div>
      </div>
      <div ref={scrollRef} className="list-scroll" role="listbox" aria-label="Downloads" tabIndex={0}>
        {loaded && rows.length === 0 && (
          <p className="empty">No downloads here. Press Ctrl+N to add one.</p>
        )}
        <div style={{ height: virtual.getTotalSize(), position: "relative" }}>
          {virtual.getVirtualItems().map((item) => {
            const row = rows[item.index]!;
            return (
              <DownloadRowView
                key={row.id}
                row={row}
                live={live[row.id]}
                selected={row.id === selected}
                onSelect={onSelect}
                onOpen={onOpen}
                style={{ position: "absolute", top: 0, left: 0, right: 0, height: ROW_HEIGHT, transform: `translateY(${item.start}px)` }}
              />
            );
          })}
        </div>
      </div>
    </div>
  );
}
```

`src/components/FilterRail.tsx`:

```tsx
import { useMemo } from "react";
import { useDownloads, useDownloadsStore } from "../state/context";
import { filterCounts, type Filter } from "../state/downloads";

const LABELS: [Filter, string][] = [
  ["all", "All"],
  ["active", "Downloading"],
  ["completed", "Completed"],
  ["failed", "Failed"],
];

export function FilterRail() {
  const store = useDownloadsStore();
  const rows = useDownloads((s) => s.rows);
  const filter = useDownloads((s) => s.filter);
  const counts = useMemo(() => filterCounts(rows), [rows]);
  return (
    <nav className="rail" aria-label="Filters">
      {LABELS.map(([key, label]) => (
        <button
          key={key}
          type="button"
          className={`rail-item${filter === key ? " rail-item-on" : ""}`}
          aria-pressed={filter === key}
          onClick={() => store.getState().setFilter(key)}
        >
          <span>{label}</span> <span className="count">{counts[key]}</span>
        </button>
      ))}
    </nav>
  );
}
```

`src/components/TopBar.tsx`:

```tsx
import { useBackend, useDownloads } from "../state/context";
import { canPause, canResume, removeWithConfirm, toggle } from "../lib/actions";
import { attempt } from "../ui/toast";

interface Props {
  onAdd(): void;
  onSettings(): void;
}

export function TopBar({ onAdd, onSettings }: Props) {
  const backend = useBackend();
  const row = useDownloads((s) => (s.selected ? s.rows[s.selected] : undefined));
  return (
    <header className="topbar">
      <h1 className="brand">Muzn Download Manager</h1>
      <div className="toolbar" role="toolbar" aria-label="Actions">
        <button type="button" className="primary-button" onClick={onAdd} title="Add a download (Ctrl+N)">
          Add URL
        </button>
        <button
          type="button"
          className="secondary-button"
          disabled={!row || !canResume(row)}
          onClick={() => row && void toggle(backend, row)}
        >
          Resume
        </button>
        <button
          type="button"
          className="secondary-button"
          disabled={!row || !canPause(row)}
          onClick={() => row && void toggle(backend, row)}
        >
          Pause
        </button>
        <button
          type="button"
          className="secondary-button"
          disabled={!row}
          onClick={() => row && void removeWithConfirm(backend, row)}
        >
          Remove
        </button>
        <span className="toolbar-gap" />
        <button type="button" className="secondary-button" onClick={() => void attempt(backend.resumeAll())}>
          Resume all
        </button>
        <button type="button" className="secondary-button" onClick={() => void attempt(backend.pauseAll())}>
          Pause all
        </button>
        <button type="button" className="secondary-button" onClick={onSettings}>
          Settings
        </button>
      </div>
    </header>
  );
}
```

`src/App.tsx`:

```tsx
import { useCallback, useEffect, useState } from "react";
import { removeWithConfirm, toggle } from "./lib/actions";
import { shortcutFor, step } from "./lib/keys";
import { DownloadList } from "./components/DownloadList";
import { FilterRail } from "./components/FilterRail";
import { TopBar } from "./components/TopBar";
import { useBackend, useDownloadsStore, useManagerSync } from "./state/context";
import { visibleRows } from "./state/downloads";
import { ConfirmHost } from "./ui/confirm";
import { ToastHost, toast } from "./ui/toast";

function useShortcuts(onAdd: () => void) {
  const backend = useBackend();
  const store = useDownloadsStore();
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (document.querySelector('[role="dialog"]')) return;
      const target = e.target instanceof Element ? e.target : null;
      const sc = shortcutFor(e, target);
      if (!sc) return;
      e.preventDefault();
      const s = store.getState();
      const row = s.selected ? s.rows[s.selected] : undefined;
      switch (sc) {
        case "add":
          onAdd();
          break;
        case "toggle":
          if (row) void toggle(backend, row);
          break;
        case "remove":
          if (row) void removeWithConfirm(backend, row);
          break;
        case "up":
        case "down":
          s.select(step(visibleRows(s.rows, s.filter), s.selected, sc === "down" ? 1 : -1));
          break;
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [backend, store, onAdd]);
}

export function App() {
  useManagerSync(toast);
  const [adding, setAdding] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const openAdd = useCallback(() => setAdding(true), []);
  useShortcuts(openAdd);
  return (
    <div className="app">
      <TopBar onAdd={openAdd} onSettings={() => setSettingsOpen(true)} />
      <div className="main">
        <FilterRail />
        <div className="content">
          <DownloadList />
        </div>
      </div>
      {/* Task 7 renders the add dialog while `adding`; Task 9 the settings while `settingsOpen`. */}
      {adding && null}
      {settingsOpen && null}
      <ConfirmHost />
      <ToastHost />
    </div>
  );
}
```

(`adding` / `settingsOpen` are wired to their dialogs in Tasks 7 and 9; until then the two `null` lines keep the state used so the linter stays quiet — Task 7 and Task 9 replace them.)

`src/api/demo.ts` — the plain-browser demo (never used inside Tauri or in tests):

```ts
import type { FakeBackend } from "./fake";
import { fakeRow } from "./fake";

const MB = 1024 * 1024;

/** Seed a few rows and move the downloading ones, so `pnpm dev` in a browser looks alive. */
export function startDemo(fake: FakeBackend): FakeBackend {
  const now = Date.now();
  const seed = [
    fakeRow({ id: "demo-1", filename: "ubuntu-24.04-desktop-amd64.iso", size: 5800 * MB, downloaded: 1900 * MB, status: "DOWNLOADING", createdAt: now - 1000 }),
    fakeRow({ id: "demo-2", filename: "vacation-video.mp4", size: 820 * MB, downloaded: 300 * MB, status: "PAUSED", createdAt: now - 2000 }),
    fakeRow({ id: "demo-3", filename: "report-final.pdf", size: 3 * MB, downloaded: 3 * MB, status: "COMPLETED", createdAt: now - 3000 }),
    fakeRow({ id: "demo-4", filename: "setup-2.1.exe", size: 96 * MB, downloaded: 40 * MB, status: "FAILED", errorCode: "SOURCE_CHANGED", errorMessage: "the ETag changed", createdAt: now - 4000 }),
    fakeRow({ id: "demo-5", filename: "podcast-episode-12.mp3", size: null, downloaded: 0, status: "QUEUED", createdAt: now - 5000 }),
  ];
  seed.forEach((r) => fake.rows.set(r.id, r));
  const parts = 8;
  const progress = new Map<string, number[]>();
  setInterval(() => {
    for (const row of fake.rows.values()) {
      if (row.status !== "DOWNLOADING" || !row.size) continue;
      const size = row.size;
      const segLen = Math.ceil(size / parts);
      const done = progress.get(row.id) ?? Array.from({ length: parts }, () => 0);
      const next = done.map((d, i) => {
        const len = Math.min(segLen, size - i * segLen);
        return Math.min(len, d + Math.round(len * (0.004 + Math.random() * 0.006)));
      });
      progress.set(row.id, next);
      const downloaded = next.reduce((a, b) => a + b, 0);
      fake.emit({
        type: "progress",
        id: row.id,
        total: size,
        downloaded,
        speedBps: 6 * MB + Math.round(Math.random() * 2 * MB),
        etaSecs: Math.round((size - downloaded) / (7 * MB)),
        segments: next.map((d, i) => ({ start: i * segLen, end: Math.min(size, (i + 1) * segLen) - 1, downloaded: d })),
      });
      if (downloaded >= size) {
        const doneRow = { ...row, status: "COMPLETED" as const, downloaded: size };
        fake.rows.set(row.id, doneRow);
        fake.emit({ type: "updated", download: doneRow });
      }
    }
  }, 250);
  return fake;
}
```

(The demo starts each download's segments from zero; the row's stored `downloaded` is only shown until the first tick.)

`src/main.tsx`:

```tsx
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { inTauri } from "./api/backend";
import { startDemo } from "./api/demo";
import { createFakeBackend } from "./api/fake";
import { tauriBackend } from "./api/tauri";
import { AppProvider } from "./state/context";
import { createDownloadsStore } from "./state/downloads";
import "./styles.css";

// Inside the Tauri window: the real app. In a plain browser (`pnpm dev`): the demo.
const backend = inTauri() ? tauriBackend : startDemo(createFakeBackend());
const store = createDownloadsStore();

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <AppProvider backend={backend} store={store}>
      <App />
    </AppProvider>
  </StrictMode>,
);
```

- [ ] **Step 6: Styles**

Append to `src/styles.css` (tokens only — no raw hex below `:root`):

```css
/* Buttons have roles, not colours per place. */
.primary-button, .secondary-button, .danger-button {
  height: 30px; padding: 0 12px; border-radius: var(--radius); border: 1px solid transparent;
  cursor: pointer; white-space: nowrap;
}
.primary-button { background: var(--accent); color: var(--accent-text); }
.secondary-button { background: var(--surface); border-color: var(--border); }
.secondary-button:hover:not(:disabled) { background: var(--surface-2); }
.danger-button { background: var(--danger); color: var(--accent-text); }
.primary-button:disabled, .secondary-button:disabled, .danger-button:disabled { opacity: .5; cursor: default; }
.link-button { background: none; border: 0; padding: 0 4px; color: var(--accent); cursor: pointer; }

.topbar { display: flex; align-items: center; gap: 16px; padding: 8px 12px; background: var(--surface); border-bottom: 1px solid var(--border); }
.topbar .brand { padding: 0; white-space: nowrap; }
.toolbar { display: flex; align-items: center; gap: 6px; flex: 1; min-width: 0; overflow-x: auto; }
.toolbar-gap { flex: 1; }

.main { flex: 1; display: flex; min-height: 0; }
.rail { width: 160px; flex: none; padding: 8px; display: flex; flex-direction: column; gap: 2px; border-right: 1px solid var(--border); background: var(--surface); }
.rail-item { display: flex; justify-content: space-between; align-items: center; height: 32px; padding: 0 10px; border: 0; border-radius: var(--radius); background: none; cursor: pointer; text-align: left; }
.rail-item:hover { background: var(--surface-2); }
.rail-item-on { background: var(--accent-soft); font-weight: 600; }
.count { color: var(--text-muted); font-size: 12px; }

.content { flex: 1; min-width: 0; display: flex; flex-direction: column; }
.list { flex: 1; min-height: 0; display: flex; flex-direction: column; }
.list-head, .row { display: grid; grid-template-columns: minmax(160px, 1fr) 88px 200px 90px 72px 104px; align-items: center; column-gap: 12px; padding: 0 12px; }
.list-head { height: 30px; font-size: 12px; color: var(--text-muted); border-bottom: 1px solid var(--border); }
.list-scroll { flex: 1; overflow-y: auto; }
.row { border-bottom: 1px solid var(--border); cursor: default; }
.row:hover { background: var(--surface-2); }
.row-selected, .row-selected:hover { background: var(--accent-soft); }
.cell { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.cell-name { display: flex; flex-direction: column; }
.name { overflow: hidden; text-overflow: ellipsis; }
.sub, .pct { color: var(--text-muted); }
.cell-progress { display: flex; align-items: center; gap: 8px; }
.pct { width: 34px; text-align: right; flex: none; }
.empty { padding: 32px; text-align: center; color: var(--text-muted); }

.segbar { flex: 1; height: 10px; display: flex; gap: 1px; background: var(--surface-2); border-radius: var(--radius-sm); overflow: hidden; }
.seg { height: 100%; background: var(--border); }
.seg-fill { height: 100%; background: var(--accent); }
.segbar-busy .seg-fill { width: 30% !important; animation: busy 1.2s ease-in-out infinite; }
@keyframes busy { from { transform: translateX(-100%); } to { transform: translateX(340%); } }
@media (prefers-reduced-motion: reduce) { .segbar-busy .seg-fill { animation: none; } }

.badge { display: inline-block; padding: 1px 8px; border-radius: 999px; font-size: 12px; background: var(--surface-2); color: var(--text-muted); }
.badge-downloading, .badge-probing { background: var(--accent-soft); color: var(--text); }
.badge-completed { color: var(--ok); }
.badge-failed { color: var(--danger); }
.badge-paused { color: var(--warn); }

.overlay { position: fixed; inset: 0; display: grid; place-items: center; background: color-mix(in srgb, var(--text) 35%, transparent); z-index: 10; }
.dialog { max-width: calc(100vw - 32px); max-height: calc(100vh - 32px); overflow: auto; background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-lg); padding: 16px 20px; box-shadow: 0 12px 32px color-mix(in srgb, var(--text) 25%, transparent); }
.dialog-title { font-size: 15px; margin: 0 0 12px; }
.dialog-footer { display: flex; justify-content: flex-end; gap: 8px; margin-top: 16px; }
.check { display: flex; align-items: center; gap: 8px; }

.field { display: flex; flex-direction: column; gap: 4px; margin-bottom: 12px; }
.field > span { font-size: 12px; color: var(--text-muted); }
.field input, .field select { height: 30px; padding: 0 8px; border: 1px solid var(--border); border-radius: var(--radius); background: var(--bg); }
.field-row { display: flex; gap: 8px; }
.field-row > input { flex: 1; min-width: 0; }
.hint { font-size: 12px; color: var(--text-muted); min-height: 18px; }
.error-text { color: var(--danger); }

.toasts { position: fixed; right: 16px; bottom: 16px; display: flex; flex-direction: column; gap: 8px; z-index: 20; }
.toast { display: flex; align-items: center; gap: 12px; max-width: 420px; padding: 10px 12px; border-radius: var(--radius); background: var(--surface); border: 1px solid var(--border); box-shadow: 0 6px 18px color-mix(in srgb, var(--text) 20%, transparent); }
.toast-error { border-color: var(--danger); }
.toast-ok { border-color: var(--ok); }

/* The minimum window (760 px): drop the size and time-left columns (the detail panel has them). */
@media (max-width: 1000px) {
  .list-head, .row { grid-template-columns: minmax(140px, 1fr) 160px 84px 96px; }
  .cell-size, .cell-eta { display: none; }
  .rail { width: 136px; }
  .topbar .brand { display: none; }
}
```

The heading "Muzn Download Manager" stays in the DOM at narrow widths (hidden visually only by `display: none`, which also hides it from assistive tech — acceptable because the window title carries the name). The test for the heading runs at jsdom's default (no media query applies).

- [ ] **Step 7: Run the JS gates**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm build` — Expected: all pass.

- [ ] **Step 8: Look at it**

Run `pnpm dev` and open `http://localhost:1420` in a browser (the demo backend). Check at 1366 × 768 and at 760 × 480, in light and dark colour schemes: the demo rows show, the Ubuntu row's eight segment cells fill, selecting a row highlights it, Pause / Resume / Remove enable per selection, Delete asks, toasts appear bottom-right, nothing overflows horizontally at 760 px. Save one screenshot of each size into the PR description later (Task 10 collects them).

- [ ] **Step 9: Commit**

```bash
git add src
git commit -m "feat(ui): the main window - download list with the segment map, filters, toolbar, shortcuts

The list is virtual so thousands of rows stay smooth; each row draws IDM's
segment map from live progress (one cell per segment, filled as far as it
got), with size, speed, time left and status. The rail filters All /
Downloading / Completed / Failed with counts. The toolbar and the keyboard
(Ctrl+N, Space, Delete, arrows) share one set of actions; Delete asks first
and offers to delete a finished file. One dialog and one toast primitive
replace confirm/alert everywhere. In a plain browser the UI runs against a
moving demo so it can be looked at without building Rust.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 7: The add dialog — clipboard pre-fill, a live probe, name, folder, start now or paused

**Files:**
- Create: `src/components/AddDialog.tsx`, `src/lib/lastDir.ts`
- Modify: `src/App.tsx` (render the dialog while `adding`)
- Test: `src/components/AddDialog.test.tsx`

**Interfaces:**
- Consumes: `Backend.probe / add / clipboardUrl / pickFolder / getSettings`, `Dialog`, `toast`, `describeError`, `formatBytes`, `isWebUrl`, the store's `select`.
- Produces: `AddDialog({ onClose, initialUrl? })`; `PROBE_DELAY_MS = 400`; `lastDir(): string | null`, `rememberDir(dir: string): void` (localStorage key `mdm.lastDir`, every access in try/catch).

- [ ] **Step 1: Write the failing tests**

`src/components/AddDialog.test.tsx`:

```tsx
import { screen, within } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { createFakeBackend } from "../api/fake";
import { renderApp } from "../test/render";

afterEach(() => localStorage.clear());

async function openAdd(user: ReturnType<typeof renderApp>["user"]) {
  await user.keyboard("{Control>}n{/Control}");
  return screen.findByRole("dialog", { name: "Add a download" });
}

test("a link on the clipboard fills the URL and the probe previews it", async () => {
  const fake = createFakeBackend();
  fake.clipboard = "https://example.com/y.zip";
  const { user } = renderApp(fake);
  const dialog = await openAdd(user);
  expect(await within(dialog).findByDisplayValue("https://example.com/y.zip")).toBeInTheDocument();
  expect(await within(dialog).findByText("5.00 MB · resumable")).toBeInTheDocument();
  expect(within(dialog).getByLabelText("File name")).toHaveValue("y.zip");
  expect(within(dialog).getByLabelText("Save to")).toHaveValue("/downloads");
});

test("a refused probe says why but still lets the user add", async () => {
  const fake = createFakeBackend();
  fake.probeResult = { code: "HTTP_STATUS", message: "404 Not Found" };
  const { user } = renderApp(fake);
  const dialog = await openAdd(user);
  await user.type(within(dialog).getByLabelText("URL"), "https://example.com/gone.zip");
  expect(await within(dialog).findByText(/link may have expired/)).toBeInTheDocument();
  expect(within(dialog).getByRole("button", { name: "Start download" })).toBeEnabled();
});

test("not a web link: no probe, no add", async () => {
  const fake = createFakeBackend();
  const { user } = renderApp(fake);
  const dialog = await openAdd(user);
  await user.type(within(dialog).getByLabelText("URL"), "ftp://example.com/x");
  expect(within(dialog).getByText("Paste an http or https link.")).toBeInTheDocument();
  expect(within(dialog).getByRole("button", { name: "Start download" })).toBeDisabled();
  await new Promise((r) => setTimeout(r, 500));
  expect(fake.calls.some((c) => c.startsWith("probe"))).toBe(false);
});

test("start: the typed name and the chosen folder are sent, the row is selected, the folder remembered", async () => {
  const fake = createFakeBackend();
  const { user, store } = renderApp(fake);
  const dialog = await openAdd(user);
  await user.type(within(dialog).getByLabelText("URL"), "https://example.com/y.zip");
  await within(dialog).findByText("5.00 MB · resumable");
  const name = within(dialog).getByLabelText("File name");
  await user.clear(name);
  await user.type(name, "mine.zip");
  await user.click(within(dialog).getByRole("button", { name: "Browse…" }));
  expect(within(dialog).getByLabelText("Save to")).toHaveValue("/picked");
  await user.click(within(dialog).getByRole("button", { name: "Start download" }));
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  const row = [...fake.rows.values()][0]!;
  expect(row).toMatchObject({ url: "https://example.com/y.zip", filename: "mine.zip", dir: "/picked", status: "QUEUED" });
  expect(store.getState().selected).toBe(row.id);
  expect(localStorage.getItem("mdm.lastDir")).toBe("/picked");
});

test("add paused, and an untouched name lets the server's name win", async () => {
  const fake = createFakeBackend();
  const { user } = renderApp(fake);
  const dialog = await openAdd(user);
  await user.type(within(dialog).getByLabelText("URL"), "https://example.com/y.zip");
  await within(dialog).findByText("5.00 MB · resumable");
  await user.click(within(dialog).getByRole("button", { name: "Add paused" }));
  const row = [...fake.rows.values()][0]!;
  expect(row.status).toBe("PAUSED");
  // fake.add names the row after the URL when no name is sent
  expect(row.filename).toBe("y.zip");
});

test("the last folder is offered next time", async () => {
  localStorage.setItem("mdm.lastDir", "/elsewhere");
  const { user } = renderApp();
  const dialog = await openAdd(user);
  expect(within(dialog).getByLabelText("Save to")).toHaveValue("/elsewhere");
});
```

Run: `pnpm test` — Expected: FAIL (`AddDialog` missing; Ctrl+N opens nothing).

- [ ] **Step 2: Implement**

`src/lib/lastDir.ts`:

```ts
const KEY = "mdm.lastDir";

/** The folder the last download went to (this machine's UI convenience). */
export function lastDir(): string | null {
  try {
    return localStorage.getItem(KEY);
  } catch {
    return null;
  }
}

export function rememberDir(dir: string): void {
  try {
    if (dir) localStorage.setItem(KEY, dir);
  } catch {
    // storage unavailable: nothing to remember
  }
}
```

`src/components/AddDialog.tsx`:

```tsx
import { useEffect, useId, useState, type FormEvent, type ReactNode } from "react";
import type { ProbePreview } from "../api/types";
import { describeError } from "../lib/errors";
import { formatBytes } from "../lib/format";
import { lastDir, rememberDir } from "../lib/lastDir";
import { isWebUrl } from "../lib/url";
import { useBackend, useDownloadsStore } from "../state/context";
import { Dialog } from "../ui/Dialog";
import { toast } from "../ui/toast";

export const PROBE_DELAY_MS = 400;

type Probe =
  | { url: string; kind: "loading" }
  | { url: string; kind: "ok"; preview: ProbePreview }
  | { url: string; kind: "error"; message: string };

export function AddDialog({ onClose, initialUrl }: { onClose(): void; initialUrl?: string }) {
  const backend = useBackend();
  const store = useDownloadsStore();
  const ids = { url: useId(), name: useId(), dir: useId() };
  const [url, setUrl] = useState(initialUrl ?? "");
  const [name, setName] = useState("");
  const [nameTouched, setNameTouched] = useState(false);
  const [dir, setDir] = useState(() => lastDir() ?? "");
  const [probe, setProbe] = useState<Probe | null>(null);
  const [busy, setBusy] = useState(false);

  const target = url.trim();
  const valid = isWebUrl(target);
  // A probe answer belongs to the URL it was asked for; typing on makes it stale.
  const current = probe && probe.url === target ? probe : null;
  const preview = current?.kind === "ok" ? current.preview : null;
  const shownName = nameTouched ? name : (preview?.filename ?? "");

  useEffect(() => {
    let live = true;
    if (!lastDir()) {
      backend
        .getSettings()
        .then((s) => live && setDir((d) => d || s.downloadDir))
        .catch(() => {});
    }
    return () => {
      live = false;
    };
  }, [backend]);

  useEffect(() => {
    if (initialUrl) return;
    let live = true;
    backend
      .clipboardUrl()
      .then((link) => live && link && setUrl((u) => u || link))
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [backend, initialUrl]);

  useEffect(() => {
    if (!isWebUrl(target)) return;
    const timer = setTimeout(() => {
      setProbe({ url: target, kind: "loading" });
      backend
        .probe(target)
        .then((p) => setProbe((cur) => (cur?.url === target ? { url: target, kind: "ok", preview: p } : cur)))
        .catch((e: unknown) =>
          setProbe((cur) => (cur?.url === target ? { url: target, kind: "error", message: describeError(e) } : cur)),
        );
    }, PROBE_DELAY_MS);
    return () => clearTimeout(timer);
  }, [backend, target]);

  async function submit(startPaused: boolean) {
    if (!valid || busy) return;
    setBusy(true);
    try {
      const typed = nameTouched ? name.trim() : "";
      const row = await backend.add({ url: target, dir: dir || null, filename: typed || null, startPaused });
      rememberDir(dir);
      store.getState().select(row.id);
      onClose();
    } catch (e) {
      toast(describeError(e), "error");
      setBusy(false);
    }
  }

  async function browse() {
    try {
      const picked = await backend.pickFolder(dir || null);
      if (picked) setDir(picked);
    } catch (e) {
      toast(describeError(e), "error");
    }
  }

  function onSubmit(e: FormEvent) {
    e.preventDefault();
    void submit(false);
  }

  let hint: ReactNode = " ";
  if (target && !valid) hint = "Paste an http or https link.";
  else if (current?.kind === "loading") hint = "Checking the link…";
  else if (current?.kind === "error") hint = <span className="error-text">{current.message}</span>;
  else if (preview)
    hint = `${preview.size == null ? "Size unknown" : formatBytes(preview.size)} · ${preview.resumable ? "resumable" : "not resumable (one connection)"}`;

  return (
    <Dialog
      title="Add a download"
      onClose={onClose}
      width={560}
      footer={
        <>
          <button type="button" className="secondary-button" onClick={onClose}>
            Close
          </button>
          <button type="button" className="secondary-button" disabled={!valid || busy} onClick={() => void submit(true)}>
            Add paused
          </button>
          <button type="submit" form="add-form" className="primary-button" disabled={!valid || busy}>
            Start download
          </button>
        </>
      }
    >
      <form id="add-form" onSubmit={onSubmit}>
        <label className="field" htmlFor={ids.url}>
          <span>URL</span>
          <input
            id={ids.url}
            data-autofocus
            value={url}
            onChange={(e) => setUrl(e.target.value)}
            placeholder="https://"
            spellCheck={false}
            autoComplete="off"
          />
        </label>
        <p className="hint" aria-live="polite">
          {hint}
        </p>
        <label className="field" htmlFor={ids.name}>
          <span>File name</span>
          <input
            id={ids.name}
            value={shownName}
            onChange={(e) => {
              setName(e.target.value);
              setNameTouched(true);
            }}
            placeholder="From the server"
            spellCheck={false}
          />
        </label>
        <div className="field">
          <label htmlFor={ids.dir}>
            <span>Save to</span>
          </label>
          <div className="field-row">
            <input id={ids.dir} value={dir} readOnly />
            <button type="button" className="secondary-button" onClick={() => void browse()}>
              Browse…
            </button>
          </div>
        </div>
      </form>
    </Dialog>
  );
}
```

(The "Save to" `<span>` sits inside its `<label>` so `getByLabelText("Save to")` resolves; style `.field > label > span` like `.field > span`.)

In `src/App.tsx` replace `{adding && null}` with `{adding && <AddDialog onClose={() => setAdding(false)} />}` and import it.

- [ ] **Step 3: Run the JS gates**

Run: `pnpm test && pnpm typecheck && pnpm lint` — Expected: all pass.

- [ ] **Step 4: Look at it**

`pnpm dev` in a browser: Ctrl+N opens the dialog at 1366 × 768 and 760 × 480; typing a URL shows "Checking the link…", then the preview; Escape closes and focus returns to the list.

- [ ] **Step 5: Commit**

```bash
git add src
git commit -m "feat(ui): the add dialog - clipboard link, live probe, name, folder, start now or paused

Ctrl+N opens it with the clipboard's link already in place; 400 ms after the
typing stops the app probes the URL and shows its size and whether it can
resume, and fills the server's file name until the user types their own. A
probe answer belongs to the URL it was asked for, so a slow answer never
overwrites a newer one. The folder is the last one used on this machine,
else the settings' download folder; Browse asks the OS. The new row is
selected when the dialog closes.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 8: The detail panel — path, error in plain English, the segment table, row actions

**Files:**
- Create: `src/components/DetailPanel.tsx`
- Modify: `src/App.tsx` (panel under the list), `src/styles.css` (panel styles)
- Test: `src/components/DetailPanel.test.tsx`

**Interfaces:**
- Consumes: store `selected` / `rows` / `live`; `Backend.segments / openFile / showInFolder / restart / pause / resume`; `lib/actions` (`savedPath`, `canPause`, `canResume`, `canRestart`, `canCancel`, `toggle`, `removeWithConfirm`, `cancelWithConfirm`, `displayName`); `rowErrorText`; `STATUS_LABEL` from `DownloadRowView`.
- Produces: `DetailPanel()` (no props).

- [ ] **Step 1: Write the failing tests**

`src/components/DetailPanel.test.tsx`:

```tsx
import { act, screen, within } from "@testing-library/react";
import { expect, test } from "vitest";
import { fakeRow } from "../api/fake";
import { renderApp } from "../test/render";

const panel = () => screen.getByRole("region", { name: "Download details" });

test("nothing selected: the panel says how to see details", async () => {
  renderApp(undefined, [fakeRow({ id: "a" })]);
  expect(await screen.findByText("Select a download to see its details.")).toBeInTheDocument();
});

test("a completed download opens and shows its folder", async () => {
  const { user, fake } = renderApp(undefined, [
    fakeRow({ id: "c", filename: "done.zip", dir: "/dl", status: "COMPLETED", size: 2048, downloaded: 2048 }),
  ]);
  await user.click(await screen.findByText("done.zip", { selector: "[data-testid=name]" }));
  expect(within(panel()).getByText("/dl/done.zip")).toBeInTheDocument();
  await user.click(within(panel()).getByRole("button", { name: "Open" }));
  await user.click(within(panel()).getByRole("button", { name: "Show in folder" }));
  expect(fake.calls).toEqual(expect.arrayContaining(["open c", "showInFolder c"]));
});

test("double-clicking a completed row opens it", async () => {
  const { user, fake } = renderApp(undefined, [fakeRow({ id: "c", filename: "done.zip", status: "COMPLETED" })]);
  await user.dblClick(await screen.findByText("done.zip", { selector: "[data-testid=name]" }));
  expect(fake.calls).toContain("open c");
});

test("a changed file explains itself and restarts on request", async () => {
  const { user, fake } = renderApp(undefined, [
    fakeRow({ id: "f", filename: "setup.exe", status: "FAILED", errorCode: "SOURCE_CHANGED", errorMessage: "etag" }),
  ]);
  await user.click(await screen.findByText("setup.exe", { selector: "[data-testid=name]" }));
  expect(within(panel()).getByRole("alert")).toHaveTextContent(
    "The file on the server changed. Restart from the beginning?",
  );
  await user.click(within(panel()).getByRole("button", { name: "Restart from the beginning" }));
  expect(fake.calls).toContain("restart f");
});

test("a paused download shows its saved segments; a running one its live segments", async () => {
  const { user, fake } = renderApp(undefined, [
    fakeRow({ id: "p", filename: "paused.iso", status: "PAUSED", size: 1000, downloaded: 400 }),
    fakeRow({ id: "d", filename: "movie.mkv", status: "DOWNLOADING", size: 1000, createdAt: 5 }),
  ]);
  await user.click(await screen.findByText("paused.iso", { selector: "[data-testid=name]" }));
  const saved = await within(panel()).findAllByRole("row");
  expect(saved).toHaveLength(2); // header + one saved segment (the fake saves one)
  expect(within(saved[1]!).getByText("40%")).toBeInTheDocument();

  await user.click(screen.getByText("movie.mkv", { selector: "[data-testid=name]" }));
  act(() =>
    fake.emit({
      type: "progress",
      id: "d",
      total: 1000,
      downloaded: 300,
      speedBps: 1024,
      etaSecs: 3,
      segments: [
        { start: 0, end: 499, downloaded: 250 },
        { start: 500, end: 999, downloaded: 50 },
      ],
    }),
  );
  const rows = within(panel()).getAllByRole("row");
  expect(rows).toHaveLength(3);
  expect(within(rows[1]!).getByText("50%")).toBeInTheDocument();
  expect(within(rows[2]!).getByText("10%")).toBeInTheDocument();
});

test("cancel asks first", async () => {
  const { user, fake } = renderApp(undefined, [fakeRow({ id: "p", filename: "paused.iso", status: "PAUSED" })]);
  await user.click(await screen.findByText("paused.iso", { selector: "[data-testid=name]" }));
  await user.click(within(panel()).getByRole("button", { name: "Cancel" }));
  const dialog = await screen.findByRole("dialog", { name: "Cancel download" });
  await user.click(within(dialog).getByRole("button", { name: "Cancel download" }));
  expect(fake.calls).toContain("cancel p");
});
```

Run: `pnpm test` — Expected: FAIL.

- [ ] **Step 2: Implement**

`src/components/DetailPanel.tsx`:

```tsx
import { useEffect, useState } from "react";
import type { DownloadRow, SegmentView } from "../api/types";
import {
  canCancel,
  canPause,
  canRestart,
  canResume,
  cancelWithConfirm,
  displayName,
  removeWithConfirm,
  savedPath,
  toggle,
} from "../lib/actions";
import { rowErrorText } from "../lib/errors";
import { formatBytes, formatEta, formatSpeed, percent } from "../lib/format";
import { useBackend, useDownloads } from "../state/context";
import type { Live } from "../state/downloads";
import { attempt } from "../ui/toast";
import { STATUS_LABEL } from "./DownloadRowView";

export function DetailPanel() {
  const row = useDownloads((s) => (s.selected ? s.rows[s.selected] : undefined));
  const live = useDownloads((s) => (s.selected ? s.live[s.selected] : undefined));
  if (!row) {
    return (
      <section className="detail detail-empty" aria-label="Download details">
        Select a download to see its details.
      </section>
    );
  }
  return <DetailBody key={row.id} row={row} live={live} />;
}

function DetailBody({ row, live }: { row: DownloadRow; live: Live | undefined }) {
  const backend = useBackend();
  const running = live !== undefined;
  const [saved, setSaved] = useState<SegmentView[]>([]);

  // Stopped downloads show what the store saved; running ones their live map.
  useEffect(() => {
    if (running) return;
    let on = true;
    backend
      .segments(row.id)
      .then((s) => on && setSaved(s))
      .catch(() => {});
    return () => {
      on = false;
    };
  }, [backend, row.id, row.updatedAt, running]);

  const segments = live?.segments ?? saved;
  const error = rowErrorText(row);
  const done = row.status === "COMPLETED";
  const total = live?.total ?? row.size;
  const downloaded = live?.downloaded ?? row.downloaded;

  return (
    <section className="detail" aria-label="Download details">
      <header className="detail-head">
        <strong className="detail-name" title={displayName(row)}>
          {displayName(row)}
        </strong>
        <span className={`badge badge-${row.status.toLowerCase()}`}>{STATUS_LABEL[row.status]}</span>
        <span className="toolbar-gap" />
        {done && (
          <>
            <button type="button" className="secondary-button" onClick={() => void attempt(backend.openFile(row.id))}>
              Open
            </button>
            <button type="button" className="secondary-button" onClick={() => void attempt(backend.showInFolder(row.id))}>
              Show in folder
            </button>
          </>
        )}
        {(canPause(row) || canResume(row)) && (
          <button type="button" className="secondary-button" onClick={() => void toggle(backend, row)}>
            {canPause(row) ? "Pause" : "Resume"}
          </button>
        )}
        {canRestart(row) && row.errorCode !== "SOURCE_CHANGED" && (
          <button type="button" className="secondary-button" onClick={() => void attempt(backend.restart(row.id))}>
            Restart
          </button>
        )}
        {canCancel(row) && (
          <button type="button" className="secondary-button" onClick={() => void cancelWithConfirm(backend, row)}>
            Cancel
          </button>
        )}
        <button type="button" className="secondary-button" onClick={() => void removeWithConfirm(backend, row)}>
          Remove
        </button>
      </header>

      {error && (
        <p className="detail-error" role="alert">
          {error}{" "}
          {row.errorCode === "SOURCE_CHANGED" && (
            <button type="button" className="link-button" onClick={() => void attempt(backend.restart(row.id))}>
              Restart from the beginning
            </button>
          )}
        </p>
      )}

      <dl className="facts">
        <dt>URL</dt>
        <dd title={row.finalUrl ?? row.url}>{row.finalUrl ?? row.url}</dd>
        <dt>Saved to</dt>
        <dd title={savedPath(row)}>{savedPath(row)}</dd>
        <dt>Size</dt>
        <dd>{formatBytes(total)}</dd>
        <dt>Downloaded</dt>
        <dd>
          {formatBytes(done ? total : downloaded)}
          {running && ` · ${formatSpeed(live.speedBps)} · ${formatEta(live.etaSecs)} left`}
        </dd>
      </dl>

      {segments.length > 0 && (
        <table className="segments">
          <thead>
            <tr>
              <th>#</th>
              <th>Range</th>
              <th>Done</th>
            </tr>
          </thead>
          <tbody>
            {segments.map((s, i) => (
              <tr key={s.start}>
                <td>{i + 1}</td>
                <td>
                  {formatBytes(s.start)} – {formatBytes(s.end + 1)}
                </td>
                <td>{`${percent(s.downloaded, s.end - s.start + 1) ?? 0}%`}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </section>
  );
}
```

The fake's `segments()` answers one segment `[0, size-1]` with the row's `downloaded` (400 of 1000 → "40%"), which is what the paused-row test expects.

`src/App.tsx`: inside `.content`, after `<DownloadList />`, add `<DetailPanel />`.

Append to `src/styles.css`:

```css
.detail { flex: none; height: 230px; overflow: auto; border-top: 1px solid var(--border); background: var(--surface); padding: 10px 12px; }
.detail-empty { display: grid; place-items: center; color: var(--text-muted); }
.detail-head { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; margin-bottom: 8px; }
.detail-name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 40%; }
.detail-error { margin: 0 0 8px; color: var(--danger); }
.facts { display: grid; grid-template-columns: 90px 1fr; gap: 2px 12px; margin: 0 0 8px; }
.facts dt { color: var(--text-muted); }
.facts dd { margin: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.segments { border-collapse: collapse; font-size: 12px; }
.segments th, .segments td { padding: 2px 12px 2px 0; text-align: left; }
.segments th { color: var(--text-muted); font-weight: 500; }
@media (max-width: 1000px) { .detail { height: 200px; } .detail-name { max-width: 100%; } }
```

- [ ] **Step 3: Run the JS gates, look at it, commit**

Run: `pnpm test && pnpm typecheck && pnpm lint`. Look at the demo in a browser at both sizes: select the failed demo row (SOURCE_CHANGED) — the message and "Restart from the beginning" show; the Ubuntu row's table counts up.

```bash
git add src
git commit -m "feat(ui): the detail panel - where the file is, why it failed, how far each segment got

Selecting a download shows its URL, where it is saved, size and progress,
the error in plain English (a changed file offers 'Restart from the
beginning'), and a segment table: live for a running download, the saved
state for a stopped one. Open and Show in folder live on finished rows, and
double-clicking a finished row opens it. Cancel asks first, like Remove.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 9: Settings

**Files:**
- Create: `src/components/SettingsDialog.tsx`
- Modify: `src/App.tsx` (render while `settingsOpen`)
- Test: `src/components/SettingsDialog.test.tsx`

**Interfaces:**
- Consumes: `Backend.getSettings / setSettings / pickFolder / autostartEnabled / setAutostart`, `Dialog`, `toast`, `describeError`, `Settings`, `ProxySetting`.
- Produces: `SettingsDialog({ onClose })`; `clampSettings(s: Settings): Settings` (exported for its test: connections 1–32, parallel 1–10, blank user agent → `null`, trimmed proxy URL).

- [ ] **Step 1: Write the failing tests**

`src/components/SettingsDialog.test.tsx`:

```tsx
import { screen, within } from "@testing-library/react";
import { expect, test } from "vitest";
import { createFakeBackend, fakeSettings } from "../api/fake";
import { renderApp } from "../test/render";
import { clampSettings } from "./SettingsDialog";

async function openSettings(user: ReturnType<typeof renderApp>["user"]) {
  await user.click(screen.getByRole("button", { name: "Settings" }));
  return screen.findByRole("dialog", { name: "Settings" });
}

test("numbers are kept in range and a blank user agent means the default", () => {
  const s = clampSettings(fakeSettings({ maxConnections: 99, maxParallel: 0, userAgent: "  " }));
  expect(s).toMatchObject({ maxConnections: 32, maxParallel: 1, userAgent: null });
  expect(clampSettings(fakeSettings({ proxy: { mode: "manual", url: " http://p:8080 " } })).proxy).toEqual({
    mode: "manual",
    url: "http://p:8080",
  });
});

test("the dialog shows the current settings and saves the changes", async () => {
  const fake = createFakeBackend({ settings: { maxConnections: 8, maxParallel: 3 } });
  const { user } = renderApp(fake);
  const dialog = await openSettings(user);
  const conns = await within(dialog).findByLabelText("Connections per download");
  expect(conns).toHaveValue(8);
  await user.clear(conns);
  await user.type(conns, "16");
  await user.click(within(dialog).getByLabelText("Close button hides the app to the tray"));
  await user.click(within(dialog).getByRole("button", { name: "Save" }));
  expect(fake.settings).toMatchObject({ maxConnections: 16, closeToTray: false });
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(await screen.findByText("Settings saved")).toBeInTheDocument();
});

test("a manual proxy needs its URL", async () => {
  const { user } = renderApp();
  const dialog = await openSettings(user);
  await user.selectOptions(await within(dialog).findByLabelText("Proxy"), "manual");
  expect(within(dialog).getByRole("button", { name: "Save" })).toBeDisabled();
  await user.type(within(dialog).getByLabelText("Proxy URL"), "http://127.0.0.1:8080");
  expect(within(dialog).getByRole("button", { name: "Save" })).toBeEnabled();
});

test("launch at startup is the OS's own switch", async () => {
  const fake = createFakeBackend();
  const { user } = renderApp(fake);
  const dialog = await openSettings(user);
  const box = await within(dialog).findByLabelText("Start with the computer");
  expect(box).not.toBeChecked();
  await user.click(box);
  await user.click(within(dialog).getByRole("button", { name: "Save" }));
  expect(fake.calls).toContain("setAutostart true");
});

test("a refused save keeps the dialog open and says why", async () => {
  const fake = createFakeBackend();
  fake.setSettings = async () => {
    throw { code: "SETTINGS", message: "proxy URL: relative URL without a base" };
  };
  const { user } = renderApp(fake);
  const dialog = await openSettings(user);
  await within(dialog).findByLabelText("Connections per download");
  await user.click(within(dialog).getByRole("button", { name: "Save" }));
  expect(await screen.findByText(/proxy URL: relative URL without a base/)).toBeInTheDocument();
  expect(screen.getByRole("dialog", { name: "Settings" })).toBeInTheDocument();
});

test("the download folder is chosen through the OS dialog", async () => {
  const fake = createFakeBackend();
  const { user } = renderApp(fake);
  const dialog = await openSettings(user);
  await within(dialog).findByLabelText("Download folder");
  await user.click(within(dialog).getByRole("button", { name: "Browse…" }));
  expect(within(dialog).getByLabelText("Download folder")).toHaveValue("/picked");
});
```

Run: `pnpm test` — Expected: FAIL.

- [ ] **Step 2: Implement**

`src/components/SettingsDialog.tsx`:

```tsx
import { useEffect, useId, useState, type FormEvent } from "react";
import type { ProxySetting, Settings } from "../api/types";
import { describeError } from "../lib/errors";
import { useBackend } from "../state/context";
import { Dialog } from "../ui/Dialog";
import { toast } from "../ui/toast";

const clamp = (n: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, Math.round(n) || lo));

/** UX clean-up before saving; the core validates again (its checks are the real ones). */
export function clampSettings(s: Settings): Settings {
  const proxy: ProxySetting = s.proxy.mode === "manual" ? { mode: "manual", url: s.proxy.url.trim() } : s.proxy;
  return {
    ...s,
    maxConnections: clamp(s.maxConnections, 1, 32),
    maxParallel: clamp(s.maxParallel, 1, 10),
    userAgent: s.userAgent?.trim() ? s.userAgent.trim() : null,
    proxy,
  };
}

export function SettingsDialog({ onClose }: { onClose(): void }) {
  const backend = useBackend();
  const id = useId();
  const [form, setForm] = useState<Settings | null>(null);
  const [autostart, setAutostart] = useState<boolean | null>(null);
  const [initialAutostart, setInitialAutostart] = useState<boolean | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let on = true;
    backend
      .getSettings()
      .then((s) => on && setForm(s))
      .catch((e: unknown) => toast(describeError(e), "error"));
    backend
      .autostartEnabled()
      .then((a) => {
        if (!on) return;
        setAutostart(a);
        setInitialAutostart(a);
      })
      .catch(() => {});
    return () => {
      on = false;
    };
  }, [backend]);

  const set = <K extends keyof Settings>(key: K, value: Settings[K]) => setForm((f) => (f ? { ...f, [key]: value } : f));
  const proxyUrlMissing = form?.proxy.mode === "manual" && !form.proxy.url.trim();

  async function save(e: FormEvent) {
    e.preventDefault();
    if (!form || proxyUrlMissing) return;
    setBusy(true);
    try {
      await backend.setSettings(clampSettings(form));
      if (autostart !== null && autostart !== initialAutostart) await backend.setAutostart(autostart);
      toast("Settings saved", "ok");
      onClose();
    } catch (err) {
      toast(describeError(err), "error");
      setBusy(false);
    }
  }

  async function browse() {
    if (!form) return;
    try {
      const picked = await backend.pickFolder(form.downloadDir || null);
      if (picked) set("downloadDir", picked);
    } catch (err) {
      toast(describeError(err), "error");
    }
  }

  return (
    <Dialog
      title="Settings"
      onClose={onClose}
      width={560}
      footer={
        <>
          <button type="button" className="secondary-button" onClick={onClose}>
            Close
          </button>
          <button type="submit" form="settings-form" className="primary-button" disabled={!form || busy || proxyUrlMissing}>
            Save
          </button>
        </>
      }
    >
      {!form ? (
        <p className="hint">Loading…</p>
      ) : (
        <form id="settings-form" onSubmit={save}>
          <div className="field">
            <label htmlFor={`${id}-dir`}>
              <span>Download folder</span>
            </label>
            <div className="field-row">
              <input id={`${id}-dir`} value={form.downloadDir} readOnly />
              <button type="button" className="secondary-button" onClick={() => void browse()}>
                Browse…
              </button>
            </div>
          </div>
          <div className="field-row">
            <label className="field" htmlFor={`${id}-conns`}>
              <span>Connections per download</span>
              <input
                id={`${id}-conns`}
                type="number"
                min={1}
                max={32}
                value={form.maxConnections}
                onChange={(e) => set("maxConnections", e.target.valueAsNumber)}
              />
            </label>
            <label className="field" htmlFor={`${id}-par`}>
              <span>Downloads at once</span>
              <input
                id={`${id}-par`}
                type="number"
                min={1}
                max={10}
                value={form.maxParallel}
                onChange={(e) => set("maxParallel", e.target.valueAsNumber)}
              />
            </label>
          </div>
          <label className="field" htmlFor={`${id}-ua`}>
            <span>User agent</span>
            <input
              id={`${id}-ua`}
              value={form.userAgent ?? ""}
              placeholder="Muzn Download Manager's own"
              onChange={(e) => set("userAgent", e.target.value)}
            />
          </label>
          <div className="field-row">
            <label className="field" htmlFor={`${id}-proxy`}>
              <span>Proxy</span>
              <select
                id={`${id}-proxy`}
                value={form.proxy.mode}
                onChange={(e) => {
                  const mode = e.target.value as ProxySetting["mode"];
                  set("proxy", mode === "manual" ? { mode, url: "" } : { mode });
                }}
              >
                <option value="system">The system's</option>
                <option value="none">None</option>
                <option value="manual">Manual</option>
              </select>
            </label>
            {form.proxy.mode === "manual" && (
              <label className="field" htmlFor={`${id}-purl`}>
                <span>Proxy URL</span>
                <input
                  id={`${id}-purl`}
                  value={form.proxy.url}
                  placeholder="http://127.0.0.1:8080"
                  onChange={(e) => set("proxy", { mode: "manual", url: e.target.value })}
                />
              </label>
            )}
          </div>
          <label className="check">
            <input type="checkbox" checked={form.closeToTray} onChange={(e) => set("closeToTray", e.target.checked)} />
            Close button hides the app to the tray
          </label>
          <label className="check">
            <input
              type="checkbox"
              checked={form.notifyOnComplete}
              onChange={(e) => set("notifyOnComplete", e.target.checked)}
            />
            Notify me when a download completes
          </label>
          {autostart !== null && (
            <label className="check">
              <input type="checkbox" checked={autostart} onChange={(e) => setAutostart(e.target.checked)} />
              Start with the computer
            </label>
          )}
        </form>
      )}
    </Dialog>
  );
}
```

(`.check` labels need a little vertical rhythm: add `.dialog .check { margin: 6px 0; }` to `styles.css`.)

In `src/App.tsx` replace `{settingsOpen && null}` with `{settingsOpen && <SettingsDialog onClose={() => setSettingsOpen(false)} />}`.

- [ ] **Step 3: Run the JS gates, look at it, commit**

Run: `pnpm test && pnpm typecheck && pnpm lint`. Look in a browser at both sizes (the dialog must fit 760 × 480 with its own scroll).

```bash
git add src
git commit -m "feat(ui): settings - folder, connections, parallel downloads, user agent, proxy, tray, notifications, startup

The settings dialog edits what the core stores (the core validates again on
save and a refusal keeps the dialog open with the reason) plus the OS's own
launch-at-startup switch. A manual proxy cannot be saved without its URL;
numbers are kept in range and a blank user agent means the app's own.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 10: The command wiring test, docs, and the check on screen

**Files:**
- Modify: `src-tauri/Cargo.toml` (dev-dependency `tauri` with `test`), `src-tauri/src/commands.rs` (`handler()`), `src-tauri/src/lib.rs` (use `handler()`)
- Create: `src-tauri/tests/commands.rs`
- Create: `docs/APP.md`
- Modify: `README.md`, `docs/superpowers/plans/plan-2-engine-backlog.md`, `CLAUDE.md` (pointer to APP.md)

**Interfaces:**
- Consumes: everything above.
- Produces: `mdm_app::commands::handler<R: Runtime>() -> impl Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync + 'static` — the one command list, used by `run()` and by the wiring test.

- [ ] **Step 1: One command list, shared**

In `src-tauri/src/commands.rs` add:

```rust
/// Every command the window may call — the one list `run()` registers and the
/// wiring test drives. Names and argument names are the UI's contract
/// (src/api/tauri.ts).
pub fn handler<R: Runtime>() -> impl Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        list_downloads,
        download_segments,
        add_download,
        probe_url,
        pause_download,
        resume_download,
        cancel_download,
        restart_download,
        remove_download,
        pause_all,
        resume_all,
        get_settings,
        set_settings,
        open_download,
        show_download_in_folder,
        pick_folder,
        clipboard_url,
        autostart_enabled,
        set_autostart,
    ]
}
```

and in `lib.rs` replace the `invoke_handler(tauri::generate_handler![...])` block with `.invoke_handler(commands::handler())`. Make `commands` a `pub mod` so the integration test can reach `handler`.

- [ ] **Step 2: Write the wiring test**

`src-tauri/Cargo.toml` `[dev-dependencies]`: add `tauri = { workspace = true, features = ["test"] }` and `tokio = { workspace = true, features = ["full"] }`.

`src-tauri/tests/commands.rs` — drives the real command list through Tauri's mock runtime with the exact JSON the UI sends (camelCase arguments), so a renamed command or argument fails here, not in the owner's hands:

```rust
use mdm_app::commands::handler;
use serde_json::{json, Value};
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{get_ipc_response, mock_builder, mock_context, noop_assets, INVOKE_KEY};
use tauri::webview::InvokeRequest;
use tauri::{Manager as _, WebviewWindowBuilder};

fn app(dir: &std::path::Path) -> (tauri::App<tauri::test::MockRuntime>, tauri::WebviewWindow<tauri::test::MockRuntime>) {
    let app = mock_builder()
        .invoke_handler(handler())
        .build(mock_context(noop_assets()))
        .unwrap();
    let db = dir.join("mdm.db");
    let manager = tauri::async_runtime::block_on(async { mdm_core::Manager::open(&db, dir) }).unwrap();
    app.manage(manager);
    let webview = WebviewWindowBuilder::new(&app, "main", Default::default()).build().unwrap();
    (app, webview)
}

fn call(w: &tauri::WebviewWindow<tauri::test::MockRuntime>, cmd: &str, body: Value) -> Result<Value, Value> {
    get_ipc_response(
        w,
        InvokeRequest {
            cmd: cmd.into(),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            url: "http://tauri.localhost".parse().unwrap(),
            body: InvokeBody::Json(body),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_string(),
        },
    )
    .map(|r| r.deserialize::<Value>().unwrap())
}

#[test]
fn the_ui_contract_add_list_pause_remove() {
    let d = tempfile::tempdir().unwrap();
    let (_app, w) = app(d.path());
    let row = call(
        &w,
        "add_download",
        json!({ "download": { "url": "http://127.0.0.1:9/never.bin", "startPaused": true } }),
    )
    .unwrap();
    let id = row["id"].as_str().unwrap().to_owned();
    assert_eq!(row["status"], "PAUSED");
    let list = call(&w, "list_downloads", json!({})).unwrap();
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(call(&w, "download_segments", json!({ "id": id })).unwrap(), json!([]));
    call(&w, "remove_download", json!({ "id": id, "deleteFile": false })).unwrap();
    assert_eq!(call(&w, "list_downloads", json!({})).unwrap(), json!([]));
}

#[test]
fn failures_arrive_as_code_and_message() {
    let d = tempfile::tempdir().unwrap();
    let (_app, w) = app(d.path());
    let e = call(&w, "add_download", json!({ "download": { "url": "ftp://x/y" } })).unwrap_err();
    assert_eq!(e["code"], "INVALID_URL");
    assert!(e["message"].as_str().unwrap().contains("ftp"));
    let e = call(&w, "pause_download", json!({ "id": "no-such-id" })).unwrap_err();
    assert_eq!(e["code"], "NOT_FOUND");
    let e = call(&w, "open_download", json!({ "id": "no-such-id" })).unwrap_err();
    assert_eq!(e["code"], "NOT_FOUND");
}

#[test]
fn settings_round_trip_in_camel_case() {
    let d = tempfile::tempdir().unwrap();
    let (_app, w) = app(d.path());
    let mut s = call(&w, "get_settings", json!({})).unwrap();
    assert_eq!(s["closeToTray"], true);
    s["maxConnections"] = json!(99);
    let saved = call(&w, "set_settings", json!({ "settings": s })).unwrap();
    assert_eq!(saved["maxConnections"], 32, "the core clamps");
}
```

The `tauri::test` API moves a little between 2.x releases (`InvokeRequest` fields, `INVOKE_KEY`): if a field or name differs, follow the installed version's `tauri::test` docs (`cargo doc -p tauri --features test`) and keep the three tests' assertions exactly as written. If `get_ipc_response` returns the error as a `serde_json::Value` already, drop the `.deserialize` on the error side accordingly.

Run: `pnpm build && cargo test -p mdm-app` — Expected: PASS (the list moved into `handler()` is the one `run()` uses, so this proves the live wiring).

- [ ] **Step 3: Docs**

`docs/APP.md`:

```markdown
# The desktop app

`src-tauri` (package `mdm-app`, binary `mdm`) is a thin layer over `mdm_core::Manager`;
`src/` is the React UI. Neither holds download logic.

## Wiring

- **Startup:** `run()` resolves `<OS data dir>/muzn-dm/mdm.db` and the OS Downloads folder,
  opens the manager on Tauri's runtime (the manager spawns every download there), starts the
  event forwarder and the tray. A second launch focuses the running window.
- **Commands** (`src-tauri/src/commands.rs`, one list in `handler()`): every manager action, the
  probe preview, settings, open / show a finished file by id, the folder picker, the clipboard
  link, launch at startup. Failures are `{code, message}` with the core's stable code.
- **Events:** `download:progress` (live progress, ~4 per second per running download),
  `download:status` (added / updated / removed / notice), `download:resync` (events were lost —
  the UI reloads the list). The UI subscribes before it loads the list.
- **Quit:** the tray's Quit or closing the window with close-to-tray off; running downloads get
  five seconds to save and continue at the next launch.
- **Security:** the page may use core events and the app's own commands only
  (`capabilities/default.json`); plugins are called from Rust. The UI never passes a path to be
  opened — Rust resolves it from a COMPLETED row.

## The UI

- Talks only through `src/api/backend.ts` (`Backend`). `tauri.ts` is the real one; `fake.ts` an
  in-memory one used by every test and by `pnpm dev` in a plain browser (with `demo.ts`'s moving
  rows), so screens can be built and looked at without Rust.
- State: one vanilla zustand store per app (`state/downloads.ts`) reduced from manager events.
- One dialog (`ui/Dialog.tsx`), one confirmation (`ui/confirm.tsx`), one toast (`ui/toast.tsx`);
  error codes become words only in `lib/errors.ts`; toolbar, keyboard and detail panel share
  `lib/actions.ts`.
- Shortcuts: Ctrl+N add, Space pause / resume, Delete remove, arrows move the selection.

## Run it

    pnpm install
    pnpm tauri dev          # the app
    pnpm dev                # the UI alone in a browser, against the demo

Gates: `pnpm build`, `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo test --workspace`, `pnpm typecheck`, `pnpm lint`, `pnpm test`.
```

`README.md` — replace the **Status** paragraph and the Build section:

```markdown
**Status:** the download engine, the download core (SQLite store, queue, pause / resume /
cancel, crash recovery) and the desktop app (download list with the segment map, add dialog
with a live preview, detail panel, settings, tray, notifications) are complete and tested. The
browser extension, torrent support and installers follow (see `docs/superpowers/`).

## Build and run

Needs Rust (stable), Node 24 and pnpm; on Linux also WebKitGTK 4.1
(`libwebkit2gtk-4.1-dev libxdo-dev libayatana-appindicator3-dev librsvg2-dev`).

    pnpm install
    pnpm tauri dev

Tests: `pnpm build && cargo test --workspace && pnpm test`. How the app is wired: `docs/APP.md`.
```

`docs/superpowers/plans/plan-2-engine-backlog.md` — add a line after "Still open:" listing what Plan 3 closed: `ProbeInfo` for the UI (`ProbePreview` + `Manager::probe`), the panicking-driver slot leak, the metadata-after-completion FAILED, save-on-change, the npm-style `workspaces` field (replaced by `pnpm-workspace.yaml`); delete those items from the lists below it.

`CLAUDE.md` — add `- How the app is wired: docs/APP.md; the core: docs/CORE.md; the engine: docs/ENGINE.md.` under the spec line.

- [ ] **Step 4: Every gate, then the check on screen**

Run all: `pnpm build && cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && pnpm typecheck && pnpm lint && pnpm test`.

Then look at it:

1. `pnpm dev` in a browser (the demo): screenshots at 1366 × 768 and 760 × 480, light and dark — the list, a selected row with the detail panel, the add dialog, the settings dialog. Nothing overflows sideways at 760 px; every text is at least 12 px; the focus ring shows when tabbing.
2. `pnpm tauri dev` (the real app): the window opens with an empty list; add a real file (e.g. a Linux ISO or any public file of 50 MB+), watch the segment map fill, pause and resume it, let it finish — the notification appears, Open and Show in folder work, the file's SHA-256 matches the published one. Close the window → it hides to the tray; tray → Quit; start again → the list is still there.

Record what was looked at in the PR description ("checked: …"), with the screenshots.

- [ ] **Step 5: Commit**

```bash
git add src-tauri docs README.md CLAUDE.md
git commit -m "test(app): the command wiring through Tauri's mock runtime; docs for the desktop app

The command list lives in one handler() that run() registers and a test
drives with the exact JSON the UI sends, so a renamed command or argument
fails in CI instead of in the window. docs/APP.md explains the wiring, the
fake backend and how to run it; the README and the backlog catch up.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

## Not in this plan (stays in `plan-2-engine-backlog.md` or later plans)

- Browser extension, native host, `mdm --add` (Plan 4); torrents and the Torrents filter (Plan 5);
  installers, updater, release workflow, code signing (Plan 6).
- The engine's fsync inside the progress ticker, Windows path normalisation of the registry and of
  `reserved_parts`, the crash test's byte count, the parser nits, pinning Actions by SHA,
  `CONTRIBUTING.md`.
