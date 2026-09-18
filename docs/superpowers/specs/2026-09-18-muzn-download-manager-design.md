# Muzn Download Manager — v1 design

**Date:** 2026-09-18
**Status:** approved in conversation (owner), awaiting written-spec review
**Owner decisions recorded here:** cross-platform (Windows + Linux + macOS); Tauri v2 with our
own Rust engine (approach A over an aria2 sidecar or Go/Wails); v1 = segmented HTTP download
with resume + browser link capture + BitTorrent (torrent lands as the last phase of v1, after
HTTP, UI and extension); open source, MIT.

## 1. What this is

An open-source alternative to Internet Download Manager. One small desktop binary
(Tauri v2: Rust core + React UI), a browser extension that hands downloads to it, and a
download engine we own: multi-connection segmented HTTP(S) with pause / resume / crash
recovery, plus BitTorrent via `librqbit`.

Name: **Muzn Download Manager**, short `mdm`. Product strings say "Muzn Download Manager";
identifiers (binary, crates, native host id `com.muzn.mdm`) use `mdm`. UI is English only.

### v1 scope

In:

- Segmented multi-connection HTTP/HTTPS download with resume and crash recovery
- Single-stream fallback when the server has no range support
- Browser extension (Chrome / Edge / Firefox, MV3) that captures downloads and sends them
  to the app; native messaging bridge
- BitTorrent: magnet links and `.torrent` files, through `librqbit`
- Desktop UI: download list with segment map, add dialog, detail panel, settings, tray,
  OS notifications
- Installers for Windows (msi + nsis exe), Linux (AppImage + deb), macOS (dmg); in-app
  updater from GitHub Releases
- CI on every PR, release workflow on tags

Out (explicitly, so nobody adds them by accident):

- Video sniffing / HLS / streaming capture
- FTP, SFTP, mirrors, multi-source HTTP
- Scheduler, categories / auto-folders, bandwidth scheduler, shutdown-after-download
- Global speed limit (v1.1 — the engine has the hook, the UI does not)
- "Download all links on page"
- Accounts, cloud sync, telemetry

## 2. Architecture

Three processes, one binary:

```
Browser extension (MV3, TS)
   │  native messaging: length-prefixed JSON over stdio
   ▼
mdm --native-host          thin bridge, no UI, spawned by the browser
   │  local IPC: 127.0.0.1:<random port>, bearer token from a file only this user can read
   ▼
mdm (Tauri v2 app)
   Rust core
     crates/mdm-engine    segmented HTTP download, resume, retry      (no Tauri dependency)
     crates/mdm-torrent   librqbit wrapper, same Progress shape        (no Tauri dependency)
     crates/mdm-bridge    native-messaging framing + IPC client        (no Tauri dependency)
     src-tauri/           commands, events, SQLite store, IPC server, tray, single instance
   React + TypeScript UI (Vite)  talks only through tauri invoke / listen
```

Rules that keep the boundaries honest:

- `mdm-engine` and `mdm-torrent` know nothing about Tauri, SQLite or the UI. They expose
  a handle and a progress stream. `cargo test -p mdm-engine` runs with no window and no
  network (a local test server).
- Persistence lives in `src-tauri/src/store`. The engines report state; the store writes it.
- The UI never touches the file system or the network. Every action is a Tauri command;
  every update is a Tauri event.
- The extension holds no download list and no UI beyond an options page and a badge.
  Everything the user looks at is in the app.

### Repo layout

```
muzn_download_manager/
├── Cargo.toml                 # workspace: crates/*, src-tauri
├── package.json               # pnpm workspace: ., extension
├── crates/
│   ├── mdm-engine/
│   ├── mdm-torrent/
│   └── mdm-bridge/
├── src-tauri/                 # Tauri app crate
├── src/                       # React UI
├── extension/                 # browser extension (one build, two manifests)
├── docs/
│   ├── ARCHITECTURE.md  ENGINE.md  TORRENT.md  EXTENSION.md
│   └── superpowers/specs/ plans/
├── .github/workflows/ci.yml  release.yml
├── CLAUDE.md  CONTRIBUTING.md  README.md  LICENSE (MIT)
```

### Toolchain

Rust stable (edition 2021), Tauri v2, React 19 + TypeScript + Vite, pnpm, Vitest, ESLint,
`cargo fmt` + `clippy -D warnings`. SQLite through `rusqlite` (bundled feature, no system
lib). HTTP through `reqwest` (rustls, HTTP/2, `gzip` feature OFF — content encoding and
`Range` do not mix). Async runtime tokio.

Prerequisites on the owner's PC as of 2026-09-18: Node 24 present; Rust, pnpm and the MSVC
build tools (Visual Studio Build Tools "Desktop development with C++") are NOT installed.
The implementation plan's first task installs them.

### Storage

SQLite file `mdm.db` in the platform app-data dir (`%APPDATA%\muzn-dm`, `~/.local/share/muzn-dm`,
`~/Library/Application Support/muzn-dm`). Two tables carry the engine state; settings are a
key/value table.

```
downloads(id TEXT PK, kind TEXT CHECK(kind IN ('http','torrent')), url TEXT, final_url TEXT,
          filename TEXT, dir TEXT, size INTEGER NULL, status TEXT CHECK(...), etag TEXT,
          last_modified TEXT, mime TEXT, referrer TEXT, headers_json TEXT, cookies_json TEXT,
          torrent_info_hash TEXT NULL, error TEXT NULL, created_at, updated_at, completed_at)
segments(download_id TEXT FK, idx INTEGER, start INTEGER, end INTEGER, downloaded INTEGER,
         PRIMARY KEY(download_id, idx))
settings(key TEXT PK, value TEXT)
```

Status values: `QUEUED`, `PROBING`, `DOWNLOADING`, `PAUSED`, `COMPLETED`, `FAILED`,
`CANCELLED`, and for torrents additionally `SEEDING`. The CHECK mirrors the Rust enum;
adding a variant means a migration.

In-progress file: `<dir>/<filename>.mdm.part`. Segment progress lives in the DB only (no
sidecar json). On completion the `.part` is renamed to the final name; a clash becomes
`name (1).ext`.

## 3. HTTP engine (`mdm-engine`)

### Lifecycle

1. **Probe.** `HEAD` the URL; if the server refuses HEAD (405/403 or a body-less 200 with no
   length) fall back to `GET` with `Range: bytes=0-0`. Follow redirects (max 10) and keep the
   final URL. Read `Content-Length`, range support (`Accept-Ranges: bytes` or a `206`),
   `ETag`, `Last-Modified`, `Content-Type`, filename (`Content-Disposition` filename* /
   filename → last URL path segment, percent-decoded → `download.bin`). Filenames are
   sanitised for the OS (no path separators, no reserved Windows names, max 200 bytes).
2. **Plan.** Size known AND ranges supported → segmented. Otherwise → single stream (one
   connection, no resume; the UI shows an indeterminate bar when size is unknown).
   Segment count = `min(max_connections, ceil(size / 1 MiB))`; default 8, user setting 1–32.
   Segments are contiguous, near-equal ranges covering `[0, size)`.
3. **Allocate.** Create the `.part` file and `set_len(size)`. A disk-space failure is a
   `FAILED` before any byte moves.
4. **Fetch.** One tokio task per segment: `GET` with `Range: bytes=<start+downloaded>-<end>`,
   expect `206` with a matching `Content-Range`; anything else is a segment error. Each
   chunk is written with a positioned write at its absolute offset (no shared seek). A
   shared `reqwest::Client` gives connection pooling. The segment's `downloaded` counter
   advances after every write.
5. **Progress.** The engine publishes `Progress { total, downloaded, speed_bps, eta_secs,
   segments: Vec<SegmentState>, status }` on a `tokio::sync::watch` channel. Speed is a
   2-second moving average. Consumers sample; the engine never throttles for them.
6. **Complete.** All segments reach `end` → fsync → rename `.part` → final name → status
   `COMPLETED`.

### Pause, resume, crash

- Pause: a cancellation token; each segment finishes its current write and stops. The
  store flushes `downloaded` for every segment.
- Resume: rebuild segments from the store, re-probe the URL first. If `ETag` or
  `Last-Modified` changed, or the size differs, the engine reports
  `SourceChanged` and the UI asks "The file on the server changed. Restart from the
  beginning?". If the server now answers `200` instead of `206`, the download restarts as
  a single stream with a notice.
- Crash: identical to resume. The store flushes `downloaded` at most every 1 s, so a crash
  costs at most ~1 s of bytes per segment; those bytes are simply rewritten.

### Retry and errors

- Transient (connect error, timeout, reset, 5xx, 429): exponential backoff 1, 2, 4 … 60 s,
  max 10 attempts per segment, then the download is `FAILED` with the last error.
- Permanent (401, 403, 404, 410, TLS error): fail at once — the link expired or is wrong.
- Stall: 30 s with no bytes on a segment → drop and reconnect that segment.
- Work stealing: when a segment finishes and others still have more than 2 MiB left, the
  largest remaining range is split at its midpoint and the freed connection takes the
  second half (this is what keeps the last 10 % of an IDM download fast).
- Disk full / permission denied: `FAILED`, the `.part` stays, resume works after space is
  freed.
- Concurrency limit: the app runs at most `max_parallel_downloads` (default 3) downloads;
  the rest wait in `QUEUED`, FIFO.

### API

```rust
pub struct Engine { /* shared client, config */ }
pub struct EngineConfig { max_connections: u8, user_agent: String, proxy: Proxy, connect_timeout: Duration }
pub enum Proxy { System, None, Manual(Url) }

pub struct DownloadSpec { url: Url, dir: PathBuf, filename: Option<String>,
                          headers: Vec<(String, String)>, cookies: Vec<Cookie>,
                          resume_from: Option<Vec<SegmentState>> }

impl Engine {
    pub async fn probe(&self, url: &Url, headers, cookies) -> Result<Probe, EngineError>;
    pub async fn start(&self, spec: DownloadSpec) -> Result<DownloadHandle, EngineError>;
}
impl DownloadHandle {
    pub fn subscribe(&self) -> watch::Receiver<Progress>;
    pub fn pause(&self); pub fn resume(&self); pub fn cancel(&self, delete_part: bool);
    pub async fn wait(self) -> Outcome;   // Completed(path) | Paused(segments) | Failed(err) | Cancelled
}
```

`EngineError` is an enum with a stable `code()` string (`RANGE_NOT_SUPPORTED`,
`SOURCE_CHANGED`, `DISK_FULL`, `HTTP_STATUS`, `NETWORK`, `TLS`, `CANCELLED`) that the UI maps
to messages.

### Tests (engine crate, no real network)

An `axum` test server in `tests/` serves a 20 MiB deterministic pseudo-random file with
switches per test: ranges on/off, drop the connection after N bytes, answer 503 for the
first K requests, change the ETag between requests, refuse HEAD. Every test asserts the
final file's SHA-256 equals the source. Cases:

- segmented happy path (8 segments), single-stream fallback, HEAD refused
- pause at 40 % then resume → same hash, no byte re-downloaded beyond the flush window
- process "crash": drop the handle, start again from stored segments → same hash
- connection drop mid-segment → retry → same hash
- 503 burst → backoff → same hash; 404 → immediate `FAILED`
- ETag changed on resume → `SOURCE_CHANGED`
- work stealing splits the largest segment and the file still hashes correctly
- proptest: random segment plans over random sizes always cover `[0, size)` exactly once

## 4. Torrent (`mdm-torrent`)

`librqbit` (pure Rust, tokio, MIT/Apache) wrapped so the rest of the app sees a torrent as
one more download:

- `TorrentEngine::new(TorrentConfig { listen_port, dht: bool, max_peers, upload_limit,
  session_dir })` owns one `librqbit::Session`, persisted in the app-data dir so resume
  survives restarts.
- `add(AddTorrent::Magnet(uri) | AddTorrent::File(bytes), dir, selected_files)` returns a
  `TorrentHandle` with the same `subscribe() -> watch::Receiver<Progress>` shape as HTTP,
  plus `files()`, `peers()`, `set_files(selection)`.
- After the data is complete the handle reports `COMPLETED`; seeding is OFF by default (the
  IDM user's expectation: finished means finished). Settings offer "seed until ratio X",
  which moves the torrent to `SEEDING` until reached.
- Entry points: paste a magnet link in the add dialog, drop a `.torrent` file on the window,
  the extension capturing a `.torrent` download, and the OS: the installer registers the
  `magnet:` protocol and the `.torrent` file association to `mdm`.
- Tests: two `librqbit` sessions on localhost with DHT off and the second session given the
  first as a direct peer; a 5 MiB file transfers and hashes correctly; pause / resume; file
  selection downloads only the chosen file.

## 5. Browser extension and bridge

### Extension (`extension/`, MV3, TypeScript, one source → Chrome/Edge manifest and Firefox manifest)

- Capture: Chrome/Edge `chrome.downloads.onDeterminingFilename`; Firefox
  `browser.downloads.onCreated`. When the item matches the capture rules the extension
  cancels and erases the browser's download and sends `{type:"download", url, filename,
  referrer, mime, size, cookies, userAgent}` to the app. Cookies come from
  `chrome.cookies.getAll({url})` so authenticated downloads keep working.
- Capture rules (options page): extension allow-list (default the IDM list — zip rar 7z
  tar gz iso exe msi dmg apk pdf mp4 mkv avi mov mp3 flac torrent …), minimum size
  (default 0), site skip-list. Holding **Alt** while clicking a link bypasses capture
  (IDM convention). `.torrent` files are always captured.
- Context menu on links: "Download with Muzn Download Manager".
- Toolbar icon: connected / disconnected; click focuses (or launches) the app. No popup.
- Native host name `com.muzn.mdm`; the app's extension IDs (store IDs plus a dev ID) are the
  only `allowed_origins`.

### Bridge (`mdm --native-host`)

- Same binary; the browser spawns it per message session. Reads the 4-byte-length-prefixed
  JSON frames from stdin, forwards each to the app over local IPC, writes the app's reply to
  stdout.
- IPC: the app listens on `127.0.0.1:<random port>` and writes `{port, token}` to
  `<app-data>/ipc.json` with owner-only permissions. The bridge reads it and sends
  `Authorization: Bearer <token>`. Only this user's processes can reach the app.
- App not running: the bridge launches `mdm --add <json>` and waits up to 5 s for the
  socket; the single-instance plugin then hands the URL to the running app.
- Registration: the installers write the native-messaging manifests for Chrome, Edge and
  Firefox (Windows: `HKCU\Software\{Google\Chrome,Microsoft\Edge,Mozilla}\NativeMessagingHosts\com.muzn.mdm`
  + a JSON file; macOS / Linux: JSON under each browser's `NativeMessagingHosts` dir).
  Settings → Browser integration shows each browser's status and has "Register again".

## 6. Desktop UI

One main window:

- Top bar: Add URL (Ctrl+N), Pause all, Resume all, Settings.
- Left rail filters: All / Downloading / Completed / Failed / Torrents.
- List (virtualised): name, size, progress bar with the **segment map** (one cell per
  segment, filled proportionally — the IDM look), speed, ETA, status. Space pauses /
  resumes the selection, Delete removes (asks whether to delete the file).
- Detail panel for the selection: URL, path, per-segment table, error, log; torrents add
  Files (tick to select), Peers, ratio.
- Add dialog: URL / magnet (clipboard auto-fills when it holds one), filename, folder
  (last used), connections, "Start now" / "Add paused". Probe runs as you type so the
  dialog shows size and range support before you confirm.
- Complete: OS notification with Open / Open folder; the row gets the same buttons.
- Tray: minimise to tray, right-click Pause all / Resume all / Quit.
- Settings: download folder, max connections, max parallel downloads, user agent, proxy
  (system / none / manual), torrent (listen port, DHT, max peers, upload limit, seed
  ratio), browser integration, launch at startup, check for updates.

State: one zustand store fed by Tauri events (`download:progress`, `download:status`); the
engine publishes continuously, the app throttles to one event per download per 250 ms.
Look: follows the OS light / dark, one accent (Muzn teal), 13 px body, visible focus ring,
one dialog primitive and one toast primitive (no `window.confirm` / `alert`).

## 7. Release and CI

- `ci.yml` on PRs and `main`: `cargo fmt --check`, `cargo clippy --all-targets -D warnings`,
  `cargo test --workspace` on ubuntu / windows / macos; `pnpm tsc --noEmit`,
  `pnpm eslint . --max-warnings=0`, `pnpm vitest run`; extension build.
- `release.yml` on `v*` tags: `tauri-apps/tauri-action` builds msi + nsis (Windows),
  AppImage + deb (Linux), dmg (macOS), attaches them and the extension zips to a draft
  GitHub Release, and publishes `latest.json` for the Tauri updater.
- Updater signing key: generated by the owner (`pnpm tauri signer generate`); the private
  key lives only in the `TAURI_SIGNING_PRIVATE_KEY` repo secret. No key in the repo.
- Windows binaries are unsigned in v1 (SmartScreen warning documented in the README).
- Store submissions (Chrome Web Store, AMO) are manual owner steps; CI produces the zips.

## 8. Implementation order (for the plan)

1. Toolchain + repo skeleton (workspace, Tauri app that opens a window, CI green on empty tests)
2. `mdm-engine`: probe → plan → segmented fetch → complete, with the test server
3. `mdm-engine`: pause / resume / crash recovery, retry, work stealing
4. `src-tauri` store + commands + events; UI list, add dialog, detail panel, settings, tray
5. Bridge + extension + installer registration
6. `mdm-torrent` + torrent UI additions + protocol / file association
7. Release workflow, updater, README, docs

Each phase ends with its tests green in CI before the next starts.
