# Plan 1 — Toolchain, repo skeleton and the HTTP engine (`mdm-engine`)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A tested, Tauri-free Rust crate that downloads a file over HTTP(S) with N parallel range connections into one pre-allocated file, survives pause / crash / server hiccups, and proves it with a local test server — plus the workspace and CI everything after it builds on.

**Architecture:** Cargo workspace; `crates/mdm-engine` is a pure library (tokio + reqwest) with an I/O-free `plan` module, a `probe` step, one `segment` worker per range doing positioned writes into a single `.mdm.part` file, and a `download` orchestrator that publishes `Progress` on a `watch` channel and returns an `Outcome`. Resume and crash recovery are the same code path: `Engine::start` with `resume_from`. Tests run against an in-process `axum` server that can turn ranges off, refuse HEAD, drop connections, answer 503, change its ETag and redirect.

**Tech Stack:** Rust stable (edition 2021), tokio 1, reqwest 0.12 (rustls, HTTP/2, `gzip` OFF), tokio-util `CancellationToken`, thiserror 2, url 2, percent-encoding 2; dev: axum 0.7, proptest 1, sha2 0.10, tempfile 3. Node 24 + pnpm for the JS side later (installed here so CI's JS job exists from day one).

**Spec:** `docs/superpowers/specs/2026-09-18-muzn-download-manager-design.md` (sections 2, 3 and 8 phases 1–3)

## Global Constraints

- Product name in any user-facing string: **Muzn Download Manager**; identifiers use `mdm`.
- License: MIT. Every crate's `Cargo.toml` says `license = "MIT"`.
- `mdm-engine` has NO dependency on Tauri, SQLite or anything UI. `cargo test -p mdm-engine` needs no network and no window.
- reqwest features: `rustls-tls`, `http2`, `stream`; `default-features = false`; never enable `gzip`/`brotli`/`deflate` (content-encoding breaks `Range`).
- Segment count = `min(max_connections, ceil(size / 1 MiB))`, at least 1; `max_connections` default 8, allowed 1–32.
- Retry: transient → backoff 1, 2, 4 … capped 60 s, max 10 attempts per segment; permanent (401, 403, 404, 410, TLS) → fail at once. Stall = 30 s without bytes → reconnect that segment.
- Work stealing: when a segment finishes, split the largest remaining range if it has more than 2 MiB left.
- In-progress file is `<dir>/<filename>.mdm.part`; on completion rename to `<filename>`, clash → `name (1).ext`.
- Every `EngineError` variant has a stable `code()` string from this set: `RANGE_NOT_SUPPORTED`, `SOURCE_CHANGED`, `DISK_FULL`, `HTTP_STATUS`, `NETWORK`, `TLS`, `CANCELLED`, `INVALID_URL`, `IO`.
- CI (`.github/workflows/ci.yml`) must pass on ubuntu, windows and macos: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --workspace`.
- Commit messages tell the story (what changed and why), end with `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`.
- Source edits go through the editor tools (never `sed -i` / `node -e` one-liners).

## Decisions made while planning (not in the spec, recorded here)

- **Resume is a fresh `Engine::start` with `resume_from`.** The spec lists `resume()` on the handle; a handle whose task has ended cannot be resumed in place, and crash recovery must go through `start` anyway. One code path, tested once. `DownloadHandle` therefore has `pause()`, `cancel()` and `wait()`; the app layer (Plan 2) calls `start` again with the stored segments.
- **Positioned writes, one file, no merge step** (hydra's approach; dlman merges temp files, which doubles disk use and adds a slow final step).
- **Segment `end` is inclusive**, like HTTP `Range`. A segment is done when `start + downloaded > end`.
- **Single-stream downloads are one segment with `end = None`**; they cannot resume — `resume_from` on a single-stream download restarts from zero.

## File structure

```
muzn_download_manager/
├── Cargo.toml                          workspace + shared [workspace.dependencies]
├── rust-toolchain.toml                 stable
├── rustfmt.toml                        defaults + max_width 100
├── .github/workflows/ci.yml            fmt / clippy / test matrix
├── LICENSE                             MIT
├── README.md                           one paragraph + "status: engine under construction"
├── CLAUDE.md                           the working rules for agents in this repo
├── package.json                        pnpm workspace root (JS comes in Plan 2)
└── crates/mdm-engine/
    ├── Cargo.toml
    ├── src/
    │   ├── lib.rs                      re-exports; crate docs
    │   ├── error.rs                    EngineError + code()
    │   ├── plan.rs                     plan_segments(), SegmentState  (pure, proptest)
    │   ├── filename.rs                 filename_from(), sanitize()    (pure, unit tests)
    │   ├── request.rs                  RequestExtras (headers + cookies) → RequestBuilder
    │   ├── probe.rs                    probe(): HEAD / GET range=0-0 → Probe
    │   ├── file.rs                     PartFile: preallocate, write_at, finish (rename)
    │   ├── segment.rs                  fetch_segment(): one range, retries, stall, cancel
    │   ├── download.rs                 orchestrator: Engine::start, DownloadHandle, Progress, Outcome, work stealing
    │   └── engine.rs                   Engine, EngineConfig, Proxy, DownloadSpec
    ├── examples/fetch.rs               tiny CLI: `cargo run --example fetch -- <url> <dir>`
    └── tests/
        ├── support/mod.rs              TestServer (axum) + deterministic payload + sha256 helper
        ├── probe.rs
        ├── download.rs
        ├── resume.rs
        ├── retry.rs
        └── steal.rs
```

---

### Task 1: Toolchain on the owner's PC

**Files:** none in the repo. This task is prerequisites; it ends when the four `--version` commands print.

**Interfaces:**
- Produces: `cargo`, `rustc`, `pnpm` on PATH; MSVC link.exe for Rust on Windows.

- [ ] **Step 1: Install Rust (rustup)**

Run in PowerShell:
```powershell
winget install --id Rustlang.Rustup -e --accept-package-agreements --accept-source-agreements
```
Then open a NEW terminal (PATH changes) and run:
```powershell
rustup default stable
rustup component add rustfmt clippy
```

- [ ] **Step 2: Install the MSVC build tools (needs the owner — it is an elevated GUI installer)**

Run in PowerShell:
```powershell
winget install --id Microsoft.VisualStudio.2022.BuildTools -e --override "--passive --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
```
If winget cannot run it passively, the owner installs "Visual Studio Build Tools 2022" → workload "Desktop development with C++". Rust on Windows cannot link without it.

- [ ] **Step 3: Install pnpm**

```powershell
npm install -g pnpm@latest
```

- [ ] **Step 4: Verify**

Run (new terminal):
```powershell
rustc --version; cargo --version; cargo clippy --version; pnpm --version; node --version
```
Expected: `rustc 1.8x.x`, `cargo 1.8x.x`, `clippy 0.1.8x`, `pnpm 10.x`, `v24.19.0`. If `cargo build` later fails with `link.exe not found`, Step 2 did not complete.

---

### Task 2: Workspace skeleton, empty engine crate, CI green

**Files:**
- Create: `Cargo.toml`, `rust-toolchain.toml`, `rustfmt.toml`, `.gitignore` (extend), `LICENSE`, `README.md`, `CLAUDE.md`, `package.json`
- Create: `crates/mdm-engine/Cargo.toml`, `crates/mdm-engine/src/lib.rs`
- Create: `.github/workflows/ci.yml`

**Interfaces:**
- Produces: the workspace; `[workspace.dependencies]` every later task pulls from.

- [ ] **Step 1: Workspace `Cargo.toml`**

```toml
[workspace]
resolver = "2"
members = ["crates/*"]

[workspace.package]
version = "0.1.0"
edition = "2021"
license = "MIT"
repository = "https://github.com/imamachowdhury/muzn-download-manager"
authors = ["Imam Ahmed"]

[workspace.dependencies]
tokio = { version = "1", features = ["rt-multi-thread", "macros", "fs", "io-util", "sync", "time"] }
tokio-util = { version = "0.7", features = ["rt"] }
reqwest = { version = "0.12", default-features = false, features = ["rustls-tls", "http2", "stream"] }
futures-util = "0.3"
url = "2"
thiserror = "2"
tracing = "0.1"
percent-encoding = "2"
bytes = "1"

# dev
axum = "0.7"
proptest = "1"
sha2 = "0.10"
tempfile = "3"
tokio-stream = "0.1"

[profile.release]
lto = "thin"
codegen-units = 1
strip = true
```

- [ ] **Step 2: `rust-toolchain.toml`, `rustfmt.toml`, `.gitignore`**

`rust-toolchain.toml`:
```toml
[toolchain]
channel = "stable"
components = ["rustfmt", "clippy"]
```

`rustfmt.toml`:
```toml
max_width = 100
```

Append to `.gitignore`:
```
Cargo.lock.bak
*.mdm.part
.pnpm-store/
```
(`Cargo.lock` IS committed — this workspace builds binaries.)

- [ ] **Step 3: `LICENSE` (MIT), `README.md`, `CLAUDE.md`, `package.json`**

`LICENSE`: the standard MIT text with `Copyright (c) 2026 Imam Ahmed`.

`README.md`:
```markdown
# Muzn Download Manager

An open-source download manager: multi-connection segmented HTTP downloads with
pause / resume, a browser extension that hands downloads to the app, and BitTorrent.
One small desktop app for Windows, Linux and macOS (Tauri v2, Rust engine, React UI).

**Status:** under construction. The download engine (`crates/mdm-engine`) is being
built first; there is no app to install yet. Design: `docs/superpowers/specs/`.

## Build

    cargo test --workspace

## License

MIT
```

`CLAUDE.md`:
```markdown
# Muzn Download Manager — working rules for agents

- Spec: docs/superpowers/specs/2026-09-18-muzn-download-manager-design.md. Plans: docs/superpowers/plans/.
- Product name "Muzn Download Manager" in user-facing text; identifiers `mdm`. UI English only. MIT.
- crates/mdm-engine and crates/mdm-torrent never depend on Tauri, SQLite or UI code.
- Gates before any commit: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
  `cargo test --workspace`; JS (once present): `pnpm tsc --noEmit`, `pnpm eslint . --max-warnings=0`, `pnpm vitest run`.
- Never weaken a test to pass. New behaviour gets a regression test naming the decision and date.
- Edit source with the editor tools, never shell one-liners.
- Owner writes Banglish → answer in Banglish; code, comments and UI stay English.
- No new subsystem (video sniffing, FTP, scheduler, categories…) without an explicit owner prompt — see the spec's "Out" list.
```

`package.json`:
```json
{
  "name": "muzn-download-manager",
  "private": true,
  "packageManager": "pnpm@10.15.0",
  "workspaces": ["extension"]
}
```

- [ ] **Step 4: Engine crate with one smoke test**

`crates/mdm-engine/Cargo.toml`:
```toml
[package]
name = "mdm-engine"
description = "Segmented HTTP download engine for Muzn Download Manager"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
authors.workspace = true

[dependencies]
tokio.workspace = true
tokio-util.workspace = true
reqwest.workspace = true
futures-util.workspace = true
url.workspace = true
thiserror.workspace = true
tracing.workspace = true
percent-encoding.workspace = true
bytes.workspace = true

[dev-dependencies]
axum.workspace = true
proptest.workspace = true
sha2.workspace = true
tempfile.workspace = true
tokio-stream.workspace = true
tokio = { workspace = true, features = ["full"] }
```

`crates/mdm-engine/src/lib.rs`:
```rust
//! Segmented HTTP download engine for Muzn Download Manager.
//!
//! No Tauri, no database: callers get a [`DownloadHandle`] and a stream of
//! [`Progress`]; persistence is theirs.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

/// Engine version, for User-Agent strings.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    #[test]
    fn version_is_set() {
        assert!(!super::VERSION.is_empty());
    }
}
```

- [ ] **Step 5: Build and test locally**

Run: `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test --workspace`
Expected: 1 test passes, no warnings.

- [ ] **Step 6: CI workflow**

`.github/workflows/ci.yml`:
```yaml
name: CI
on:
  pull_request:
  push:
    branches: [main]
concurrency:
  group: ci-${{ github.ref }}
  cancel-in-progress: true
jobs:
  rust:
    name: Rust (${{ matrix.os }})
    runs-on: ${{ matrix.os }}
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, windows-latest, macos-latest]
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      - uses: Swatinem/rust-cache@v2
      - run: cargo fmt --all --check
      - run: cargo clippy --workspace --all-targets -- -D warnings
      - run: cargo test --workspace
```

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "chore: cargo workspace, empty mdm-engine crate and CI

The engine is a library first; Tauri and the UI come later and depend
on it, never the other way round. CI runs fmt, clippy and tests on all
three desktop platforms from the first commit.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 3: Segment planning (`plan.rs`) — pure, property-tested

**Files:**
- Create: `crates/mdm-engine/src/plan.rs`
- Modify: `crates/mdm-engine/src/lib.rs` (add `pub mod plan; pub use plan::{plan_segments, SegmentState, MIN_SEGMENT_BYTES};`)

**Interfaces:**
- Produces:
  ```rust
  pub const MIN_SEGMENT_BYTES: u64 = 1024 * 1024;
  #[derive(Clone, Debug, PartialEq, Eq)]
  pub struct SegmentState { pub idx: u32, pub start: u64, pub end: u64, pub downloaded: u64 }
  impl SegmentState { pub fn remaining(&self) -> u64; pub fn is_done(&self) -> bool; pub fn next_offset(&self) -> u64 }
  pub fn plan_segments(size: u64, max_connections: u8) -> Vec<SegmentState>;
  ```
  `end` is inclusive. Segments are contiguous, ordered by `idx`, and cover `[0, size)` exactly. `size == 0` → one segment `0..=0`? No: `size == 0` returns an empty Vec (nothing to fetch; the orchestrator creates the empty file).

- [ ] **Step 1: Write the failing tests**

`crates/mdm-engine/src/plan.rs` (tests first, at the bottom; the module body is written in Step 3):
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn small_file_is_one_segment() {
        let s = plan_segments(500_000, 8);
        assert_eq!(s.len(), 1);
        assert_eq!((s[0].start, s[0].end), (0, 499_999));
    }

    #[test]
    fn segment_count_is_bounded_by_size_and_connections() {
        assert_eq!(plan_segments(3 * MIN_SEGMENT_BYTES, 8).len(), 3);
        assert_eq!(plan_segments(100 * MIN_SEGMENT_BYTES, 8).len(), 8);
        assert_eq!(plan_segments(100 * MIN_SEGMENT_BYTES, 0).len(), 1, "0 connections means 1");
        assert_eq!(plan_segments(100 * MIN_SEGMENT_BYTES, 64).len(), 32, "capped at 32");
    }

    #[test]
    fn zero_size_plans_nothing() {
        assert!(plan_segments(0, 8).is_empty());
    }

    #[test]
    fn remaining_and_done() {
        let mut s = SegmentState { idx: 0, start: 10, end: 19, downloaded: 0 };
        assert_eq!(s.remaining(), 10);
        assert!(!s.is_done());
        s.downloaded = 10;
        assert_eq!(s.remaining(), 0);
        assert!(s.is_done());
        assert_eq!(s.next_offset(), 20);
    }

    proptest! {
        #[test]
        fn segments_cover_the_file_exactly_once(size in 1u64..(200 * MIN_SEGMENT_BYTES), conns in 1u8..=32) {
            let s = plan_segments(size, conns);
            prop_assert_eq!(s[0].start, 0);
            prop_assert_eq!(s.last().unwrap().end, size - 1);
            for (i, w) in s.windows(2).enumerate() {
                prop_assert_eq!(w[0].idx as usize, i);
                prop_assert_eq!(w[0].end + 1, w[1].start, "contiguous");
            }
            let total: u64 = s.iter().map(|x| x.end - x.start + 1).sum();
            prop_assert_eq!(total, size);
            let (min, max) = s.iter().fold((u64::MAX, 0), |(lo, hi), x| {
                let n = x.end - x.start + 1;
                (lo.min(n), hi.max(n))
            });
            prop_assert!(max - min <= 1, "near-equal sizes: {} vs {}", min, max);
        }
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p mdm-engine plan`
Expected: compile error — `plan_segments` not found.

- [ ] **Step 3: Implement**

Top of `crates/mdm-engine/src/plan.rs`:
```rust
//! Deciding how a file is split before any connection exists. Pure arithmetic,
//! so it is property-tested here and never touched by I/O.

/// A segment is never planned smaller than this (1 MiB).
pub const MIN_SEGMENT_BYTES: u64 = 1024 * 1024;

/// Hard ceiling on connections per download, whatever the setting says.
pub const MAX_CONNECTIONS: u8 = 32;

/// One byte range of a download and how much of it has been written.
///
/// `end` is inclusive, like an HTTP `Range`. The next byte to fetch is
/// `start + downloaded`; the segment is done when that passes `end`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SegmentState {
    /// Position in the plan; stable for the life of the download.
    pub idx: u32,
    /// First byte (absolute offset).
    pub start: u64,
    /// Last byte (absolute offset, inclusive).
    pub end: u64,
    /// Bytes already written, counted from `start`.
    pub downloaded: u64,
}

impl SegmentState {
    /// Bytes still to fetch.
    pub fn remaining(&self) -> u64 {
        (self.end + 1).saturating_sub(self.start + self.downloaded)
    }
    /// True once every byte up to `end` is written.
    pub fn is_done(&self) -> bool {
        self.remaining() == 0
    }
    /// Absolute offset of the next byte to fetch.
    pub fn next_offset(&self) -> u64 {
        self.start + self.downloaded
    }
}

/// Split `size` bytes into contiguous, near-equal segments.
///
/// Count = `min(max_connections, ceil(size / MIN_SEGMENT_BYTES))`, at least 1
/// and never above [`MAX_CONNECTIONS`]. A zero-size file plans nothing.
pub fn plan_segments(size: u64, max_connections: u8) -> Vec<SegmentState> {
    if size == 0 {
        return Vec::new();
    }
    let by_size = size.div_ceil(MIN_SEGMENT_BYTES).max(1);
    let wanted = u64::from(max_connections.clamp(1, MAX_CONNECTIONS));
    let count = by_size.min(wanted);
    let base = size / count;
    let extra = size % count; // the first `extra` segments get one more byte
    let mut out = Vec::with_capacity(count as usize);
    let mut start = 0u64;
    for idx in 0..count {
        let len = base + u64::from(idx < extra);
        out.push(SegmentState { idx: idx as u32, start, end: start + len - 1, downloaded: 0 });
        start += len;
    }
    out
}
```

Add to `lib.rs`:
```rust
pub mod plan;
pub use plan::{plan_segments, SegmentState, MAX_CONNECTIONS, MIN_SEGMENT_BYTES};
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p mdm-engine plan`
Expected: 5 tests pass (4 unit + 1 proptest).

- [ ] **Step 5: Commit**

```bash
git add crates/mdm-engine/src/plan.rs crates/mdm-engine/src/lib.rs
git commit -m "feat(engine): segment planning as a pure, property-tested function

Contiguous near-equal ranges, count bounded by connections and by a
1 MiB floor per segment, so a small file never opens eight sockets.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 4: Filename resolution and sanitising (`filename.rs`)

**Files:**
- Create: `crates/mdm-engine/src/filename.rs`
- Modify: `crates/mdm-engine/src/lib.rs` (add `pub mod filename;`)

**Interfaces:**
- Produces:
  ```rust
  pub fn filename_from(content_disposition: Option<&str>, url: &url::Url) -> String; // sanitised, never empty
  pub fn sanitize(name: &str) -> String;
  pub const DEFAULT_FILENAME: &str = "download.bin";
  ```

- [ ] **Step 1: Write the failing tests**

Bottom of `crates/mdm-engine/src/filename.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use url::Url;

    fn u(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn prefers_rfc5987_filename_star() {
        let cd = r#"attachment; filename="fallback.txt"; filename*=UTF-8''r%C3%A9sum%C3%A9.pdf"#;
        assert_eq!(filename_from(Some(cd), &u("https://x/y.zip")), "résumé.pdf");
    }

    #[test]
    fn then_quoted_filename() {
        let cd = r#"attachment; filename="report 2026.pdf""#;
        assert_eq!(filename_from(Some(cd), &u("https://x/y.zip")), "report 2026.pdf");
    }

    #[test]
    fn then_bare_filename() {
        assert_eq!(filename_from(Some("inline; filename=a.iso"), &u("https://x/y")), "a.iso");
    }

    #[test]
    fn then_url_path_percent_decoded() {
        assert_eq!(filename_from(None, &u("https://x/dl/My%20File.tar.gz?token=1")), "My File.tar.gz");
    }

    #[test]
    fn then_default() {
        assert_eq!(filename_from(None, &u("https://x/")), DEFAULT_FILENAME);
        assert_eq!(filename_from(Some("attachment"), &u("https://x")), DEFAULT_FILENAME);
    }

    #[test]
    fn sanitize_strips_separators_and_reserved_names() {
        assert_eq!(sanitize("a/b\\c:d*e?f\"g<h>i|j"), "a_b_c_d_e_f_g_h_i_j");
        assert_eq!(sanitize("  ..hidden.. "), "hidden");
        assert_eq!(sanitize("CON"), "_CON");
        assert_eq!(sanitize("com1.txt"), "_com1.txt");
        assert_eq!(sanitize("bad\u{0}name\n"), "bad_name_");
        assert_eq!(sanitize(""), DEFAULT_FILENAME);
        assert_eq!(sanitize("..."), DEFAULT_FILENAME);
    }

    #[test]
    fn sanitize_caps_length_keeping_extension() {
        let long = format!("{}.tar.gz", "x".repeat(300));
        let out = sanitize(&long);
        assert!(out.len() <= MAX_FILENAME_BYTES);
        assert!(out.ends_with(".tar.gz"));
    }

    #[test]
    fn sanitize_cap_respects_utf8_boundaries() {
        let long = "é".repeat(150); // 300 bytes
        let out = sanitize(&long);
        assert!(out.len() <= MAX_FILENAME_BYTES);
        assert!(std::str::from_utf8(out.as_bytes()).is_ok());
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p mdm-engine filename`
Expected: compile error — `filename_from` not found.

- [ ] **Step 3: Implement**

Top of `crates/mdm-engine/src/filename.rs`:
```rust
//! Picking a file name for a download and making it safe for every OS.

use percent_encoding::percent_decode_str;
use url::Url;

/// Used when neither the server nor the URL names the file.
pub const DEFAULT_FILENAME: &str = "download.bin";

/// Longest name we produce, in bytes (Windows and most Linux FS allow 255).
pub const MAX_FILENAME_BYTES: usize = 200;

const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Resolve a file name: `Content-Disposition` (`filename*`, then `filename`),
/// then the URL's last path segment, then [`DEFAULT_FILENAME`]. Always sanitised.
pub fn filename_from(content_disposition: Option<&str>, url: &Url) -> String {
    if let Some(cd) = content_disposition {
        if let Some(name) = from_content_disposition(cd) {
            return sanitize(&name);
        }
    }
    let from_url = url
        .path_segments()
        .and_then(|s| s.last().map(str::to_owned))
        .filter(|s| !s.is_empty())
        .map(|s| percent_decode_str(&s).decode_utf8_lossy().into_owned());
    match from_url {
        Some(name) => sanitize(&name),
        None => DEFAULT_FILENAME.to_owned(),
    }
}

fn from_content_disposition(cd: &str) -> Option<String> {
    let params: Vec<(String, String)> = cd
        .split(';')
        .skip(1)
        .filter_map(|p| {
            let (k, v) = p.split_once('=')?;
            Some((k.trim().to_ascii_lowercase(), v.trim().to_owned()))
        })
        .collect();
    // RFC 5987: filename*=charset'lang'percent-encoded
    if let Some((_, v)) = params.iter().find(|(k, _)| k == "filename*") {
        let encoded = v.rsplit('\'').next().unwrap_or(v);
        let decoded = percent_decode_str(encoded).decode_utf8_lossy().into_owned();
        if !decoded.is_empty() {
            return Some(decoded);
        }
    }
    if let Some((_, v)) = params.iter().find(|(k, _)| k == "filename") {
        let v = v.trim_matches('"');
        if !v.is_empty() {
            return Some(v.to_owned());
        }
    }
    None
}

/// Make `name` safe: no path separators or control characters, no leading or
/// trailing dots and spaces, no Windows reserved device names, at most
/// [`MAX_FILENAME_BYTES`] (cut on a char boundary, keeping the extension).
pub fn sanitize(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    out = out.trim_matches(|c: char| c == '.' || c == ' ').to_owned();
    if out.is_empty() {
        return DEFAULT_FILENAME.to_owned();
    }
    let stem = out.split('.').next().unwrap_or("").to_ascii_uppercase();
    if RESERVED.contains(&stem.as_str()) {
        out.insert(0, '_');
    }
    if out.len() > MAX_FILENAME_BYTES {
        out = truncate_keeping_extension(&out);
    }
    out
}

fn truncate_keeping_extension(name: &str) -> String {
    // Extension = everything after the first dot in the last 20 bytes, if any
    // ("x.tar.gz" keeps ".tar.gz"; a 100-byte "extension" is not one).
    let ext_start = name
        .char_indices()
        .filter(|(i, c)| *c == '.' && name.len() - *i <= 20)
        .map(|(i, _)| i)
        .next();
    let (stem, ext) = match ext_start {
        Some(i) => (&name[..i], &name[i..]),
        None => (name, ""),
    };
    let budget = MAX_FILENAME_BYTES.saturating_sub(ext.len()).max(1);
    let mut cut = budget.min(stem.len());
    while !stem.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}{}", &stem[..cut], ext)
}
```

Add `pub mod filename;` to `lib.rs`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p mdm-engine filename`
Expected: 8 tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/mdm-engine/src/filename.rs crates/mdm-engine/src/lib.rs
git commit -m "feat(engine): file name from Content-Disposition or URL, sanitised for every OS

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 5: Errors, request extras, engine config (`error.rs`, `request.rs`, `engine.rs`)

**Files:**
- Create: `crates/mdm-engine/src/error.rs`, `crates/mdm-engine/src/request.rs`, `crates/mdm-engine/src/engine.rs`
- Modify: `crates/mdm-engine/src/lib.rs`

**Interfaces:**
- Produces:
  ```rust
  // error.rs
  #[derive(Debug, thiserror::Error)]
  pub enum EngineError {
      #[error("invalid URL: {0}")] InvalidUrl(String),
      #[error("server does not support byte ranges")] RangeNotSupported,
      #[error("the file on the server changed since the download started")] SourceChanged,
      #[error("not enough disk space")] DiskFull,
      #[error("HTTP {status}")] HttpStatus { status: u16 },
      #[error("network error: {0}")] Network(String),
      #[error("TLS error: {0}")] Tls(String),
      #[error("cancelled")] Cancelled,
      #[error("I/O error: {0}")] Io(#[from] std::io::Error),
  }
  impl EngineError { pub fn code(&self) -> &'static str; pub fn is_transient(&self) -> bool; }
  impl From<reqwest::Error> for EngineError;

  // request.rs
  #[derive(Clone, Debug, Default)]
  pub struct RequestExtras { pub headers: Vec<(String, String)>, pub cookies: Vec<Cookie> }
  #[derive(Clone, Debug)] pub struct Cookie { pub name: String, pub value: String }
  impl RequestExtras { pub fn apply(&self, rb: reqwest::RequestBuilder) -> reqwest::RequestBuilder; }

  // engine.rs
  #[derive(Clone, Debug)] pub enum Proxy { System, None, Manual(url::Url) }
  #[derive(Clone, Debug)] pub struct EngineConfig { pub max_connections: u8, pub user_agent: String, pub proxy: Proxy, pub connect_timeout: std::time::Duration }
  impl Default for EngineConfig;  // 8, "MuznDownloadManager/<VERSION>", System, 20 s
  #[derive(Clone)] pub struct Engine { pub(crate) client: reqwest::Client, pub(crate) cfg: EngineConfig }
  impl Engine { pub fn new(cfg: EngineConfig) -> Result<Engine, EngineError>; pub fn config(&self) -> &EngineConfig; }
  ```

- [ ] **Step 1: Write the failing tests**

Bottom of `error.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_stable_strings() {
        assert_eq!(EngineError::RangeNotSupported.code(), "RANGE_NOT_SUPPORTED");
        assert_eq!(EngineError::SourceChanged.code(), "SOURCE_CHANGED");
        assert_eq!(EngineError::DiskFull.code(), "DISK_FULL");
        assert_eq!(EngineError::HttpStatus { status: 404 }.code(), "HTTP_STATUS");
        assert_eq!(EngineError::Network("x".into()).code(), "NETWORK");
        assert_eq!(EngineError::Cancelled.code(), "CANCELLED");
    }

    #[test]
    fn transient_vs_permanent() {
        assert!(EngineError::Network("reset".into()).is_transient());
        assert!(EngineError::HttpStatus { status: 503 }.is_transient());
        assert!(EngineError::HttpStatus { status: 429 }.is_transient());
        assert!(!EngineError::HttpStatus { status: 404 }.is_transient());
        assert!(!EngineError::HttpStatus { status: 403 }.is_transient());
        assert!(!EngineError::Tls("bad cert".into()).is_transient());
        assert!(!EngineError::DiskFull.is_transient());
        assert!(!EngineError::Cancelled.is_transient());
    }

    #[test]
    fn enospc_maps_to_disk_full() {
        let e = std::io::Error::from_raw_os_error(ENOSPC_CODE);
        assert_eq!(EngineError::from_io(e).code(), "DISK_FULL");
    }
}
```

Bottom of `request.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_headers_and_joins_cookies() {
        let x = RequestExtras {
            headers: vec![("Referer".into(), "https://a/".into())],
            cookies: vec![
                Cookie { name: "s".into(), value: "1".into() },
                Cookie { name: "t".into(), value: "2".into() },
            ],
        };
        let client = reqwest::Client::new();
        let req = x.apply(client.get("https://example.invalid/")).build().unwrap();
        assert_eq!(req.headers().get("referer").unwrap(), "https://a/");
        assert_eq!(req.headers().get("cookie").unwrap(), "s=1; t=2");
    }

    #[test]
    fn no_cookies_means_no_cookie_header() {
        let client = reqwest::Client::new();
        let req = RequestExtras::default().apply(client.get("https://example.invalid/")).build().unwrap();
        assert!(req.headers().get("cookie").is_none());
    }
}
```

Bottom of `engine.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config() {
        let c = EngineConfig::default();
        assert_eq!(c.max_connections, 8);
        assert!(c.user_agent.starts_with("MuznDownloadManager/"));
        assert!(matches!(c.proxy, Proxy::System));
    }

    #[test]
    fn engine_builds_with_manual_proxy() {
        let cfg = EngineConfig {
            proxy: Proxy::Manual(url::Url::parse("http://127.0.0.1:8080").unwrap()),
            ..Default::default()
        };
        assert!(Engine::new(cfg).is_ok());
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p mdm-engine error request engine`
Expected: compile errors — modules missing.

- [ ] **Step 3: Implement `error.rs`**

```rust
//! One error type for the whole engine, with a stable code the UI can map.

/// Everything that can stop a download.
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    /// The URL could not be parsed or has an unsupported scheme.
    #[error("invalid URL: {0}")]
    InvalidUrl(String),
    /// Resume was asked for but the server no longer answers ranges.
    #[error("server does not support byte ranges")]
    RangeNotSupported,
    /// ETag / Last-Modified / size differ from the original probe.
    #[error("the file on the server changed since the download started")]
    SourceChanged,
    /// ENOSPC while pre-allocating or writing.
    #[error("not enough disk space")]
    DiskFull,
    /// A non-success HTTP status.
    #[error("HTTP {status}")]
    HttpStatus {
        /// The status code.
        status: u16,
    },
    /// Connect / read / reset / timeout.
    #[error("network error: {0}")]
    Network(String),
    /// Certificate or handshake failure — never retried.
    #[error("TLS error: {0}")]
    Tls(String),
    /// The caller cancelled.
    #[error("cancelled")]
    Cancelled,
    /// Any other I/O failure.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(windows)]
pub(crate) const ENOSPC_CODE: i32 = 112; // ERROR_DISK_FULL
#[cfg(not(windows))]
pub(crate) const ENOSPC_CODE: i32 = 28; // ENOSPC

impl EngineError {
    /// Stable identifier for UI mapping and logs.
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidUrl(_) => "INVALID_URL",
            Self::RangeNotSupported => "RANGE_NOT_SUPPORTED",
            Self::SourceChanged => "SOURCE_CHANGED",
            Self::DiskFull => "DISK_FULL",
            Self::HttpStatus { .. } => "HTTP_STATUS",
            Self::Network(_) => "NETWORK",
            Self::Tls(_) => "TLS",
            Self::Cancelled => "CANCELLED",
            Self::Io(_) => "IO",
        }
    }

    /// Worth retrying with backoff?
    pub fn is_transient(&self) -> bool {
        match self {
            Self::Network(_) => true,
            Self::HttpStatus { status } => *status == 429 || (500..=599).contains(status),
            _ => false,
        }
    }

    /// Map an I/O error, turning a full disk into [`EngineError::DiskFull`].
    pub fn from_io(e: std::io::Error) -> Self {
        if e.raw_os_error() == Some(ENOSPC_CODE) {
            Self::DiskFull
        } else {
            Self::Io(e)
        }
    }
}

impl From<reqwest::Error> for EngineError {
    fn from(e: reqwest::Error) -> Self {
        if e.is_builder() {
            return Self::InvalidUrl(e.to_string());
        }
        if let Some(status) = e.status() {
            return Self::HttpStatus { status: status.as_u16() };
        }
        let text = e.to_string();
        // reqwest exposes no is_tls(); rustls errors carry these words.
        if text.contains("certificate") || text.contains("tls") || text.contains("TLS") {
            return Self::Tls(text);
        }
        Self::Network(text)
    }
}
```

- [ ] **Step 4: Implement `request.rs`**

```rust
//! Per-download request decorations: extra headers and browser cookies.

use reqwest::RequestBuilder;

/// A cookie the browser extension captured for the download URL.
#[derive(Clone, Debug)]
pub struct Cookie {
    /// Cookie name.
    pub name: String,
    /// Cookie value.
    pub value: String,
}

/// Headers and cookies applied to every request of one download.
#[derive(Clone, Debug, Default)]
pub struct RequestExtras {
    /// Sent verbatim (Referer, Authorization …).
    pub headers: Vec<(String, String)>,
    /// Joined into one `Cookie` header.
    pub cookies: Vec<Cookie>,
}

impl RequestExtras {
    /// Decorate a request.
    pub fn apply(&self, mut rb: RequestBuilder) -> RequestBuilder {
        for (k, v) in &self.headers {
            rb = rb.header(k, v);
        }
        if !self.cookies.is_empty() {
            let joined = self
                .cookies
                .iter()
                .map(|c| format!("{}={}", c.name, c.value))
                .collect::<Vec<_>>()
                .join("; ");
            rb = rb.header(reqwest::header::COOKIE, joined);
        }
        rb
    }
}
```

- [ ] **Step 5: Implement `engine.rs`**

```rust
//! The engine: one shared HTTP client and its settings.

use std::time::Duration;

use crate::error::EngineError;

/// How outbound connections are routed.
#[derive(Clone, Debug)]
pub enum Proxy {
    /// Honour the OS / environment proxy settings (reqwest default).
    System,
    /// Connect directly, ignoring any proxy.
    None,
    /// Use this proxy for every request.
    Manual(url::Url),
}

/// Engine-wide settings.
#[derive(Clone, Debug)]
pub struct EngineConfig {
    /// Connections per download (1–32).
    pub max_connections: u8,
    /// `User-Agent` sent on every request.
    pub user_agent: String,
    /// Proxy policy.
    pub proxy: Proxy,
    /// TCP connect timeout.
    pub connect_timeout: Duration,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            max_connections: 8,
            user_agent: format!("MuznDownloadManager/{}", crate::VERSION),
            proxy: Proxy::System,
            connect_timeout: Duration::from_secs(20),
        }
    }
}

/// A configured engine. Cheap to clone; all clones share one connection pool.
#[derive(Clone)]
pub struct Engine {
    pub(crate) client: reqwest::Client,
    pub(crate) cfg: EngineConfig,
}

impl Engine {
    /// Build the shared client.
    pub fn new(cfg: EngineConfig) -> Result<Engine, EngineError> {
        let mut b = reqwest::Client::builder()
            .user_agent(cfg.user_agent.clone())
            .connect_timeout(cfg.connect_timeout)
            .redirect(reqwest::redirect::Policy::limited(10))
            .no_gzip()
            .no_brotli()
            .no_deflate();
        b = match &cfg.proxy {
            Proxy::System => b,
            Proxy::None => b.no_proxy(),
            Proxy::Manual(u) => b.proxy(reqwest::Proxy::all(u.as_str())?),
        };
        Ok(Engine { client: b.build()?, cfg })
    }

    /// The settings this engine was built with.
    pub fn config(&self) -> &EngineConfig {
        &self.cfg
    }
}
```

Note: `no_gzip()` / `no_brotli()` / `no_deflate()` exist on the builder even when the features are off; calling them documents the intent and guards against a future feature flip.

`lib.rs` additions:
```rust
pub mod engine;
pub mod error;
pub mod request;
pub use engine::{Engine, EngineConfig, Proxy};
pub use error::EngineError;
pub use request::{Cookie, RequestExtras};
```

- [ ] **Step 6: Run tests**

Run: `cargo test -p mdm-engine`
Expected: all pass (plan 5 + filename 8 + error 3 + request 2 + engine 2 + smoke 1).

- [ ] **Step 7: Commit**

```bash
git add crates/mdm-engine/src
git commit -m "feat(engine): error codes, request extras and the shared client

Every failure carries a stable code for the UI; transient vs permanent
is decided in one place so retry logic never guesses from strings.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 6: The test server (`tests/support/mod.rs`)

**Files:**
- Create: `crates/mdm-engine/tests/support/mod.rs`
- Create: `crates/mdm-engine/tests/support_smoke.rs`

**Interfaces:**
- Produces (used by every integration test after this):
  ```rust
  pub struct TestServer { pub base: String /* http://127.0.0.1:PORT */, pub data: Arc<Vec<u8>>, pub cfg: Arc<ServerCfg> }
  pub struct ServerCfg {
      pub ranges: AtomicBool,            // default true
      pub head_allowed: AtomicBool,      // default true
      pub fail_first: AtomicU32,         // answer 503 to this many GET /file requests, default 0
      pub drop_after: AtomicU64,         // 0 = never; else close the body after this many bytes of EACH response
      pub etag: Mutex<String>,           // default "\"v1\""
      pub requests: AtomicU32,           // GET /file counter
      pub content_disposition: Mutex<Option<String>>,
  }
  impl TestServer {
      pub async fn start(size: usize) -> TestServer;      // deterministic payload of `size` bytes
      pub fn file_url(&self) -> String;                    // /file
      pub fn redirect_url(&self) -> String;                // /redirect -> 302 -> /file
      pub fn status_url(&self, code: u16) -> String;       // /status/<code>
  }
  pub fn payload(size: usize) -> Vec<u8>;                 // same bytes every run (LCG)
  pub fn sha256_file(path: &Path) -> String;
  pub fn sha256_bytes(b: &[u8]) -> String;
  ```
  Routes: `GET|HEAD /file` (honours `Range` when `ranges`; 206 + `Content-Range`; `Accept-Ranges: bytes` when `ranges`; `ETag`; `Last-Modified: Thu, 18 Sep 2026 10:00:00 GMT`; `Content-Disposition` when set), `GET /redirect`, `GET /status/:code`.

- [ ] **Step 1: Write the smoke test (fails: module missing)**

`crates/mdm-engine/tests/support_smoke.rs`:
```rust
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

    let part = c.get(s.file_url()).header("Range", "bytes=100-199").send().await.unwrap();
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

    s.cfg.ranges.store(false, std::sync::atomic::Ordering::SeqCst);
    let r = c.get(s.file_url()).header("Range", "bytes=0-9").send().await.unwrap();
    assert_eq!(r.status(), 200, "ranges off → 200 with the whole body");
    assert!(r.headers().get("accept-ranges").is_none());

    s.cfg.head_allowed.store(false, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(c.head(s.file_url()).send().await.unwrap().status(), 405);

    s.cfg.fail_first.store(2, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(c.get(s.file_url()).send().await.unwrap().status(), 503);
    assert_eq!(c.get(s.file_url()).send().await.unwrap().status(), 503);
    assert_eq!(c.get(s.file_url()).send().await.unwrap().status(), 200);

    s.cfg.drop_after.store(300, std::sync::atomic::Ordering::SeqCst);
    let r = c.get(s.file_url()).send().await.unwrap();
    assert!(r.bytes().await.is_err(), "body must fail after 300 bytes");

    assert_eq!(c.get(s.status_url(404)).send().await.unwrap().status(), 404);
    let r = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().unwrap()
        .get(s.redirect_url()).send().await.unwrap();
    assert_eq!(r.status(), 302);
}

#[test]
fn payload_is_deterministic() {
    assert_eq!(payload(64), payload(64));
    assert_eq!(sha256_bytes(&payload(64)), sha256_bytes(&payload(64)));
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p mdm-engine --test support_smoke`
Expected: compile error — `support` module not found.

- [ ] **Step 3: Implement the server**

`crates/mdm-engine/tests/support/mod.rs`:
```rust
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
        let state = AppState { data: data.clone(), cfg: cfg.clone() };
        let app = Router::new()
            .route("/file", get(file))
            .route("/redirect", get(redirect))
            .route("/status/:code", get(status))
            .with_state(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        TestServer { base: format!("http://{addr}"), data, cfg }
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
    let range = if ranges { headers.get(header::RANGE).and_then(|v| parse_range(v.to_str().ok()?, total)) } else { None };

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
        rb = rb.header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{total}"));
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
            Err(std::io::Error::new(std::io::ErrorKind::ConnectionReset, "test drop")),
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
    let end: u64 = if b.is_empty() { total - 1 } else { b.parse().ok()? };
    (start <= end && end < total).then_some((start, end))
}

async fn redirect() -> Response {
    (StatusCode::FOUND, [(header::LOCATION, "/file")]).into_response()
}

async fn status(AxPath(code): AxPath<u16>) -> Response {
    StatusCode::from_u16(code).unwrap_or(StatusCode::IM_A_TEAPOT).into_response()
}

/// Deterministic pseudo-random bytes (LCG), same on every run and platform.
pub fn payload(size: usize) -> Vec<u8> {
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    (0..size)
        .map(|_| {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
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
```

Add `bytes` to the engine's dev-dependencies list: it is already a normal dependency (Task 2), so tests can use it. `futures-util` likewise.

- [ ] **Step 4: Run**

Run: `cargo test -p mdm-engine --test support_smoke`
Expected: 3 tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/mdm-engine/tests
git commit -m "test(engine): in-process HTTP server with switches for ranges, HEAD, 503, drops, ETag

Every engine behaviour after this is proven against this server, so the
suite needs no network and runs the same on every platform.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 7: Probe (`probe.rs`)

**Files:**
- Create: `crates/mdm-engine/src/probe.rs`
- Create: `crates/mdm-engine/tests/probe.rs`
- Modify: `crates/mdm-engine/src/lib.rs` (`pub mod probe; pub use probe::Probe;`)

**Interfaces:**
- Consumes: `Engine { client, cfg }` (Task 5), `RequestExtras::apply` (Task 5), `filename_from` (Task 4), `EngineError` (Task 5).
- Produces:
  ```rust
  #[derive(Clone, Debug)]
  pub struct Probe {
      pub final_url: url::Url,          // after redirects
      pub size: Option<u64>,
      pub ranges: bool,                 // server answered 206 or Accept-Ranges: bytes
      pub etag: Option<String>,
      pub last_modified: Option<String>,
      pub mime: Option<String>,         // Content-Type without parameters
      pub filename: String,             // sanitised
  }
  impl Engine { pub async fn probe(&self, url: &url::Url, extras: &RequestExtras) -> Result<Probe, EngineError>; }
  ```

- [ ] **Step 1: Write the failing tests**

`crates/mdm-engine/tests/probe.rs`:
```rust
mod support;

use std::sync::atomic::Ordering;

use mdm_engine::{Engine, EngineConfig, EngineError, RequestExtras};
use support::*;
use url::Url;

fn engine() -> Engine {
    Engine::new(EngineConfig::default()).unwrap()
}

#[tokio::test]
async fn ranges_on_via_head() {
    let s = TestServer::start(50_000).await;
    let p = engine().probe(&Url::parse(&s.file_url()).unwrap(), &RequestExtras::default()).await.unwrap();
    assert_eq!(p.size, Some(50_000));
    assert!(p.ranges);
    assert_eq!(p.etag.as_deref(), Some("\"v1\""));
    assert_eq!(p.last_modified.as_deref(), Some("Thu, 18 Sep 2026 10:00:00 GMT"));
    assert_eq!(p.mime.as_deref(), Some("application/octet-stream"));
    assert_eq!(p.filename, "file");
    assert_eq!(s.cfg.requests.load(Ordering::SeqCst), 0, "HEAD was enough, no GET");
}

#[tokio::test]
async fn ranges_off() {
    let s = TestServer::start(50_000).await;
    s.cfg.ranges.store(false, Ordering::SeqCst);
    let p = engine().probe(&Url::parse(&s.file_url()).unwrap(), &RequestExtras::default()).await.unwrap();
    assert_eq!(p.size, Some(50_000));
    assert!(!p.ranges);
}

#[tokio::test]
async fn head_refused_falls_back_to_get_range() {
    let s = TestServer::start(50_000).await;
    s.cfg.head_allowed.store(false, Ordering::SeqCst);
    let p = engine().probe(&Url::parse(&s.file_url()).unwrap(), &RequestExtras::default()).await.unwrap();
    assert_eq!(p.size, Some(50_000));
    assert!(p.ranges, "206 to bytes=0-0 proves range support");
    assert_eq!(s.cfg.requests.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn follows_redirects_and_keeps_final_url() {
    let s = TestServer::start(10).await;
    let p = engine().probe(&Url::parse(&s.redirect_url()).unwrap(), &RequestExtras::default()).await.unwrap();
    assert!(p.final_url.as_str().ends_with("/file"));
    assert_eq!(p.filename, "file");
}

#[tokio::test]
async fn content_disposition_names_the_file() {
    let s = TestServer::start(10).await;
    *s.cfg.content_disposition.lock().unwrap() = Some("attachment; filename=\"a b.zip\"".into());
    let p = engine().probe(&Url::parse(&s.file_url()).unwrap(), &RequestExtras::default()).await.unwrap();
    assert_eq!(p.filename, "a b.zip");
}

#[tokio::test]
async fn http_error_is_reported() {
    let s = TestServer::start(10).await;
    let e = engine().probe(&Url::parse(&s.status_url(404)).unwrap(), &RequestExtras::default()).await.unwrap_err();
    assert!(matches!(e, EngineError::HttpStatus { status: 404 }), "{e:?}");
}

#[tokio::test]
async fn rejects_non_http_scheme() {
    let e = engine().probe(&Url::parse("ftp://x/y").unwrap(), &RequestExtras::default()).await.unwrap_err();
    assert_eq!(e.code(), "INVALID_URL");
}

#[tokio::test]
async fn sends_extras_without_breaking_the_request() {
    let s = TestServer::start(10).await;
    let x = RequestExtras { headers: vec![("X-Test".into(), "1".into())], cookies: vec![] };
    engine().probe(&Url::parse(&s.file_url()).unwrap(), &x).await.unwrap();
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p mdm-engine --test probe`
Expected: compile error — `Engine::probe` not found.

- [ ] **Step 3: Implement**

`crates/mdm-engine/src/probe.rs`:
```rust
//! Learning what a URL is before downloading it: size, range support,
//! validators, name. `HEAD` first; a `GET` with `Range: bytes=0-0` when the
//! server refuses HEAD or hides the length.

use reqwest::header::{
    HeaderName, ACCEPT_RANGES, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE,
    ETAG, LAST_MODIFIED, RANGE,
};
use reqwest::{Response, StatusCode};
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
    /// Probe a URL. Follows redirects, never downloads more than one byte.
    pub async fn probe(&self, url: &Url, extras: &RequestExtras) -> Result<Probe, EngineError> {
        if !matches!(url.scheme(), "http" | "https") {
            return Err(EngineError::InvalidUrl(format!("unsupported scheme {}", url.scheme())));
        }
        if let Ok(r) = extras.apply(self.client.head(url.clone())).send().await {
            if r.status().is_success() {
                if let Some(size) = header_u64(&r, CONTENT_LENGTH) {
                    let ranges = accepts_ranges(&r);
                    return Ok(build(r, Some(size), ranges));
                }
            }
        }
        let r = extras
            .apply(self.client.get(url.clone()))
            .header(RANGE, "bytes=0-0")
            .send()
            .await?;
        let status = r.status();
        if !status.is_success() {
            return Err(EngineError::HttpStatus { status: status.as_u16() });
        }
        let (size, ranges) = if status == StatusCode::PARTIAL_CONTENT {
            (content_range_total(&r), true)
        } else {
            (header_u64(&r, CONTENT_LENGTH), accepts_ranges(&r))
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
    r.headers().get(name).and_then(|v| v.to_str().ok()).map(str::to_owned)
}

fn header_u64(r: &Response, name: HeaderName) -> Option<u64> {
    header_str(r, name).and_then(|v| v.trim().parse().ok())
}

fn accepts_ranges(r: &Response) -> bool {
    header_str(r, ACCEPT_RANGES).map(|v| v.eq_ignore_ascii_case("bytes")).unwrap_or(false)
}

/// `Content-Range: bytes 0-0/12345` → 12345; `*` → None.
fn content_range_total(r: &Response) -> Option<u64> {
    header_str(r, CONTENT_RANGE)?.rsplit('/').next()?.trim().parse().ok()
}
```

`lib.rs`: add `pub mod probe; pub use probe::Probe;`.

- [ ] **Step 4: Run**

Run: `cargo test -p mdm-engine --test probe`
Expected: 8 tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/mdm-engine/src/probe.rs crates/mdm-engine/src/lib.rs crates/mdm-engine/tests/probe.rs
git commit -m "feat(engine): probe a URL for size, range support, validators and name

HEAD first, GET bytes=0-0 when HEAD is refused or hides the length -
never more than one byte moves before the user sees what the file is.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 8: The part file (`file.rs`)

**Files:**
- Create: `crates/mdm-engine/src/file.rs`
- Modify: `crates/mdm-engine/src/lib.rs` (`pub mod file; pub use file::{PartFile, PART_SUFFIX};`)

**Interfaces:**
- Produces:
  ```rust
  pub const PART_SUFFIX: &str = ".mdm.part";
  pub struct PartFile { /* std::fs::File, paths */ }
  impl PartFile {
      pub fn open(dir: &Path, filename: &str, size: Option<u64>) -> Result<PartFile, EngineError>;
          // creates <dir>/<filename>.mdm.part (and `dir`) if missing, else opens it read-write untruncated;
          // when creating and `size` is Some, pre-allocates with set_len (ENOSPC → DiskFull)
      pub fn write_at(&self, offset: u64, buf: &[u8]) -> std::io::Result<()>;   // full write, positioned
      pub fn sync(&self) -> std::io::Result<()>;
      pub fn part_path(&self) -> &Path;
      pub fn finish(self) -> Result<PathBuf, EngineError>;   // fsync + rename to <dir>/<filename>, clash → "name (1).ext"
      pub fn remove(self) -> Result<(), EngineError>;        // delete the .part
  }
  ```

- [ ] **Step 1: Write the failing tests**

Bottom of `crates/mdm-engine/src/file.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_and_preallocates() {
        let d = tempfile::tempdir().unwrap();
        let f = PartFile::open(d.path(), "a.bin", Some(4096)).unwrap();
        assert!(f.part_path().ends_with("a.bin.mdm.part"));
        assert_eq!(std::fs::metadata(f.part_path()).unwrap().len(), 4096);
    }

    #[test]
    fn creates_missing_dir() {
        let d = tempfile::tempdir().unwrap();
        let sub = d.path().join("x").join("y");
        PartFile::open(&sub, "a.bin", None).unwrap();
        assert!(sub.join("a.bin.mdm.part").exists());
    }

    #[test]
    fn write_at_positions_bytes_and_reopen_keeps_them() {
        let d = tempfile::tempdir().unwrap();
        let f = PartFile::open(d.path(), "a.bin", Some(10)).unwrap();
        f.write_at(7, b"xyz").unwrap();
        f.write_at(0, b"ab").unwrap();
        drop(f);
        let again = PartFile::open(d.path(), "a.bin", Some(10)).unwrap();
        let bytes = std::fs::read(again.part_path()).unwrap();
        assert_eq!(&bytes[0..2], b"ab");
        assert_eq!(&bytes[7..10], b"xyz");
        assert_eq!(bytes.len(), 10, "reopen must not truncate");
    }

    #[test]
    fn finish_renames_and_resolves_clashes() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.tar.gz"), b"old").unwrap();
        let f = PartFile::open(d.path(), "a.tar.gz", Some(3)).unwrap();
        f.write_at(0, b"new").unwrap();
        let out = f.finish().unwrap();
        assert_eq!(out.file_name().unwrap(), "a.tar (1).gz");
        assert_eq!(std::fs::read(&out).unwrap(), b"new");
        assert!(!d.path().join("a.tar.gz.mdm.part").exists());
    }

    #[test]
    fn finish_without_extension() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("README"), b"").unwrap();
        std::fs::write(d.path().join("README (1)"), b"").unwrap();
        let f = PartFile::open(d.path(), "README", None).unwrap();
        assert_eq!(f.finish().unwrap().file_name().unwrap(), "README (2)");
    }

    #[test]
    fn remove_deletes_part() {
        let d = tempfile::tempdir().unwrap();
        let f = PartFile::open(d.path(), "a", None).unwrap();
        let p = f.part_path().to_owned();
        f.remove().unwrap();
        assert!(!p.exists());
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p mdm-engine file::`
Expected: compile error.

- [ ] **Step 3: Implement**

```rust
//! The single pre-allocated `.mdm.part` file every segment writes into.

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

use crate::error::EngineError;

/// Suffix of an in-progress download.
pub const PART_SUFFIX: &str = ".mdm.part";

/// An open part file; cheap to share behind an `Arc`.
pub struct PartFile {
    file: File,
    dir: PathBuf,
    filename: String,
    part_path: PathBuf,
}

impl PartFile {
    /// Open or create `<dir>/<filename>.mdm.part`. A new file with a known
    /// size is pre-allocated so a full disk fails here, not mid-download.
    pub fn open(dir: &Path, filename: &str, size: Option<u64>) -> Result<PartFile, EngineError> {
        std::fs::create_dir_all(dir).map_err(EngineError::from_io)?;
        let part_path = dir.join(format!("{filename}{PART_SUFFIX}"));
        let existed = part_path.exists();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&part_path)
            .map_err(EngineError::from_io)?;
        if !existed {
            if let Some(n) = size {
                file.set_len(n).map_err(EngineError::from_io)?;
            }
        }
        Ok(PartFile { file, dir: dir.to_owned(), filename: filename.to_owned(), part_path })
    }

    /// Write all of `buf` at `offset` without touching a shared cursor.
    pub fn write_at(&self, offset: u64, buf: &[u8]) -> std::io::Result<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::FileExt;
            self.file.write_all_at(buf, offset)
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::FileExt;
            let mut done = 0usize;
            while done < buf.len() {
                let n = self.file.seek_write(&buf[done..], offset + done as u64)?;
                if n == 0 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::WriteZero,
                        "seek_write wrote 0 bytes",
                    ));
                }
                done += n;
            }
            Ok(())
        }
    }

    /// Flush to disk.
    pub fn sync(&self) -> std::io::Result<()> {
        self.file.sync_all()
    }

    /// Where the bytes are while downloading.
    pub fn part_path(&self) -> &Path {
        &self.part_path
    }

    /// fsync, then rename to the final name; an existing file is never overwritten.
    pub fn finish(self) -> Result<PathBuf, EngineError> {
        self.file.sync_all().map_err(EngineError::from_io)?;
        let target = free_name(&self.dir, &self.filename);
        drop(self.file); // Windows will not rename an open file
        std::fs::rename(&self.part_path, &target).map_err(EngineError::from_io)?;
        Ok(target)
    }

    /// Delete the part file (cancel with "delete partial data").
    pub fn remove(self) -> Result<(), EngineError> {
        drop(self.file);
        std::fs::remove_file(&self.part_path).map_err(EngineError::from_io)
    }
}

/// `name.ext` → `name (1).ext`, `name (2).ext` … until one does not exist.
fn free_name(dir: &Path, filename: &str) -> PathBuf {
    let first = dir.join(filename);
    if !first.exists() {
        return first;
    }
    let (stem, ext) = match filename.rfind('.') {
        Some(i) if i > 0 => (&filename[..i], &filename[i..]),
        _ => (filename, ""),
    };
    (1u32..)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|p| !p.exists())
        .expect("an unused name exists")
}
```

- [ ] **Step 4: Run**

Run: `cargo test -p mdm-engine file::`
Expected: 6 tests pass (on Windows too — the `seek_write` path).

- [ ] **Step 5: Commit**

```bash
git add crates/mdm-engine/src/file.rs crates/mdm-engine/src/lib.rs
git commit -m "feat(engine): one pre-allocated part file with positioned writes and clash-safe finish

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 9: The segment worker (`segment.rs`) — one range, retries, stall, cancel

**Files:**
- Create: `crates/mdm-engine/src/segment.rs`
- Create: `crates/mdm-engine/tests/segment.rs`
- Modify: `crates/mdm-engine/tests/support/mod.rs` (add the `hang_first` switch)
- Modify: `crates/mdm-engine/src/engine.rs` (add `retry_base_delay`, `stall_timeout` to `EngineConfig`)
- Modify: `crates/mdm-engine/src/lib.rs` (`pub mod segment;`)

**Interfaces:**
- Consumes: `PartFile::write_at` (Task 8), `RequestExtras` (Task 5), `EngineError::is_transient` (Task 5), `SegmentState` (Task 3).
- Produces:
  ```rust
  // engine.rs additions
  pub struct EngineConfig { /* existing */ pub retry_base_delay: Duration /* 1 s */, pub stall_timeout: Duration /* 30 s */ }

  // segment.rs
  pub const RETRY_MAX_ATTEMPTS: u32 = 10;
  pub const RETRY_CAP: Duration = Duration::from_secs(60);
  pub const UNKNOWN_END: u64 = u64::MAX;
  pub struct SegmentRuntime { pub idx: u32, pub start: u64, pub end: AtomicU64, pub downloaded: AtomicU64 }
  impl SegmentRuntime {
      pub fn new(idx: u32, start: u64, end: Option<u64>, downloaded: u64) -> Self;
      pub fn from_state(s: &SegmentState) -> Self;
      pub fn snapshot(&self) -> SegmentState;
      pub fn next_offset(&self) -> u64; pub fn remaining(&self) -> Option<u64>; pub fn is_done(&self) -> bool;
  }
  pub struct SegmentJob { pub client: reqwest::Client, pub url: url::Url, pub extras: RequestExtras, pub file: Arc<PartFile>,
                          pub seg: Arc<SegmentRuntime>, pub ranged: bool, pub cancel: CancellationToken,
                          pub retry_base_delay: Duration, pub stall_timeout: Duration }
  pub async fn fetch_segment(job: SegmentJob) -> Result<(), EngineError>;
  ```
  `ranged = false` means a plain GET from byte 0 (single stream): the worker writes sequentially and, when the stream ends, stores the real `end`.

- [ ] **Step 1: Write the failing tests**

`crates/mdm-engine/tests/segment.rs`:
```rust
mod support;

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use mdm_engine::segment::{fetch_segment, SegmentJob, SegmentRuntime};
use mdm_engine::{EngineError, PartFile, RequestExtras};
use support::*;
use tokio_util::sync::CancellationToken;
use url::Url;

fn job(s: &TestServer, dir: &std::path::Path, seg: Arc<SegmentRuntime>, ranged: bool) -> (SegmentJob, Arc<PartFile>) {
    let file = Arc::new(PartFile::open(dir, "out", Some(s.data.len() as u64)).unwrap());
    let j = SegmentJob {
        client: reqwest::Client::new(),
        url: Url::parse(&s.file_url()).unwrap(),
        extras: RequestExtras::default(),
        file: file.clone(),
        seg,
        ranged,
        cancel: CancellationToken::new(),
        retry_base_delay: Duration::from_millis(1),
        stall_timeout: Duration::from_millis(300),
    };
    (j, file)
}

#[tokio::test]
async fn writes_exactly_its_range() {
    let s = TestServer::start(100_000).await;
    let d = tempfile::tempdir().unwrap();
    let seg = Arc::new(SegmentRuntime::new(0, 1000, Some(4999), 0));
    let (j, file) = job(&s, d.path(), seg.clone(), true);
    fetch_segment(j).await.unwrap();
    assert!(seg.is_done());
    assert_eq!(seg.downloaded.load(Ordering::SeqCst), 4000);
    let bytes = std::fs::read(file.part_path()).unwrap();
    assert_eq!(&bytes[1000..5000], &s.data[1000..5000]);
    assert!(bytes[..1000].iter().all(|b| *b == 0), "nothing before the range");
    assert!(bytes[5000..].iter().all(|b| *b == 0), "nothing after the range");
}

#[tokio::test]
async fn resumes_from_downloaded_offset() {
    let s = TestServer::start(100_000).await;
    let d = tempfile::tempdir().unwrap();
    let seg = Arc::new(SegmentRuntime::new(0, 0, Some(9999), 6000));
    let (j, file) = job(&s, d.path(), seg.clone(), true);
    fetch_segment(j).await.unwrap();
    let bytes = std::fs::read(file.part_path()).unwrap();
    assert_eq!(&bytes[6000..10000], &s.data[6000..10000]);
    assert!(bytes[..6000].iter().all(|b| *b == 0), "did not refetch the first 6000");
}

#[tokio::test]
async fn already_done_segment_makes_no_request() {
    let s = TestServer::start(10_000).await;
    let d = tempfile::tempdir().unwrap();
    let seg = Arc::new(SegmentRuntime::new(0, 0, Some(999), 1000));
    let (j, _) = job(&s, d.path(), seg, true);
    fetch_segment(j).await.unwrap();
    assert_eq!(s.cfg.requests.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn single_stream_learns_the_end() {
    let s = TestServer::start(12_345).await;
    let d = tempfile::tempdir().unwrap();
    let seg = Arc::new(SegmentRuntime::new(0, 0, None, 0));
    let (j, file) = job(&s, d.path(), seg.clone(), false);
    fetch_segment(j).await.unwrap();
    assert_eq!(seg.end.load(Ordering::SeqCst), 12_344);
    assert_eq!(sha256_file(file.part_path()), sha256_bytes(&s.data));
}

#[tokio::test]
async fn retries_after_connection_drop_and_503() {
    let s = TestServer::start(200_000).await;
    s.cfg.drop_after.store(50_000, Ordering::SeqCst);
    s.cfg.fail_first.store(2, Ordering::SeqCst);
    let d = tempfile::tempdir().unwrap();
    let seg = Arc::new(SegmentRuntime::new(0, 0, Some(199_999), 0));
    let (j, file) = job(&s, d.path(), seg, true);
    fetch_segment(j).await.unwrap();
    assert_eq!(sha256_file(file.part_path()), sha256_bytes(&s.data));
    assert!(s.cfg.requests.load(Ordering::SeqCst) >= 6, "2 x 503 + 4 partial bodies");
}

#[tokio::test]
async fn gives_up_after_max_attempts() {
    let s = TestServer::start(200_000).await;
    s.cfg.drop_after.store(10, Ordering::SeqCst); // 20 000 attempts would be needed
    let d = tempfile::tempdir().unwrap();
    let seg = Arc::new(SegmentRuntime::new(0, 0, Some(199_999), 0));
    let (j, _) = job(&s, d.path(), seg, true);
    let e = fetch_segment(j).await.unwrap_err();
    assert_eq!(e.code(), "NETWORK");
    assert_eq!(s.cfg.requests.load(Ordering::SeqCst), 10);
}

#[tokio::test]
async fn permanent_error_fails_at_once() {
    let s = TestServer::start(10).await;
    let d = tempfile::tempdir().unwrap();
    let seg = Arc::new(SegmentRuntime::new(0, 0, Some(9), 0));
    let (mut j, _) = job(&s, d.path(), seg, true);
    j.url = Url::parse(&s.status_url(404)).unwrap();
    let e = fetch_segment(j).await.unwrap_err();
    assert!(matches!(e, EngineError::HttpStatus { status: 404 }));
}

#[tokio::test]
async fn range_ignored_by_server_is_range_not_supported() {
    let s = TestServer::start(10_000).await;
    s.cfg.ranges.store(false, Ordering::SeqCst);
    let d = tempfile::tempdir().unwrap();
    let seg = Arc::new(SegmentRuntime::new(0, 100, Some(199), 0));
    let (j, _) = job(&s, d.path(), seg, true);
    assert_eq!(fetch_segment(j).await.unwrap_err().code(), "RANGE_NOT_SUPPORTED");
}

#[tokio::test]
async fn stall_reconnects() {
    let s = TestServer::start(100_000).await;
    s.cfg.hang_first.store(1, Ordering::SeqCst); // first body hangs after 1 000 bytes
    let d = tempfile::tempdir().unwrap();
    let seg = Arc::new(SegmentRuntime::new(0, 0, Some(99_999), 0));
    let (j, file) = job(&s, d.path(), seg, true);
    fetch_segment(j).await.unwrap();
    assert_eq!(sha256_file(file.part_path()), sha256_bytes(&s.data));
    assert_eq!(s.cfg.requests.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn cancel_stops_and_keeps_progress() {
    let s = TestServer::start(2_000_000).await;
    s.cfg.hang_first.store(1, Ordering::SeqCst);
    let d = tempfile::tempdir().unwrap();
    let seg = Arc::new(SegmentRuntime::new(0, 0, Some(1_999_999), 0));
    let (j, _) = job(&s, d.path(), seg.clone(), true);
    let cancel = j.cancel.clone();
    let task = tokio::spawn(fetch_segment(j));
    tokio::time::sleep(Duration::from_millis(100)).await; // 1 000 bytes in, then hanging
    cancel.cancel();
    let e = task.await.unwrap().unwrap_err();
    assert_eq!(e.code(), "CANCELLED");
    assert_eq!(seg.downloaded.load(Ordering::SeqCst), 1000);
}

#[tokio::test]
async fn shrinking_end_stops_the_worker_early() {
    let s = TestServer::start(3_000_000).await;
    s.cfg.hang_first.store(1, Ordering::SeqCst);
    let d = tempfile::tempdir().unwrap();
    let seg = Arc::new(SegmentRuntime::new(0, 0, Some(2_999_999), 0));
    let (j, file) = job(&s, d.path(), seg.clone(), true);
    let task = tokio::spawn(fetch_segment(j));
    tokio::time::sleep(Duration::from_millis(100)).await;
    seg.end.store(1_999, Ordering::SeqCst); // a stealer took [2000, end]
    task.await.unwrap().unwrap(); // stall → reconnect for 1000-1999 → done
    assert!(seg.is_done());
    let bytes = std::fs::read(file.part_path()).unwrap();
    assert_eq!(&bytes[..2000], &s.data[..2000]);
}
```

- [ ] **Step 2: Add the `hang_first` switch to the test server**

In `tests/support/mod.rs`, add to `ServerCfg`: `pub hang_first: AtomicU32,` (default `AtomicU32::new(0)`, doc: "the first N GET bodies send 1 000 bytes then hang forever"), add `use futures_util::StreamExt;` at the top, and in `file()` replace the body construction with:
```rust
    let drop_after = cfg.drop_after.load(Ordering::SeqCst);
    let hang = cfg
        .hang_first
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
        .is_ok();
    let body = if hang {
        let good = slice[..slice.len().min(1000)].to_vec();
        Body::from_stream(
            stream::iter(vec![Ok::<_, std::io::Error>(bytes::Bytes::from(good))])
                .chain(stream::pending()),
        )
    } else if drop_after > 0 && drop_after < len {
        let good = slice[..drop_after as usize].to_vec();
        Body::from_stream(stream::iter(vec![
            Ok::<_, std::io::Error>(bytes::Bytes::from(good)),
            Err(std::io::Error::new(std::io::ErrorKind::ConnectionReset, "test drop")),
        ]))
    } else {
        Body::from(slice)
    };
```

- [ ] **Step 3: Run to verify it fails**

Run: `cargo test -p mdm-engine --test segment`
Expected: compile error — `mdm_engine::segment` missing.

- [ ] **Step 4: Extend `EngineConfig`**

In `engine.rs` add two fields with docs and defaults:
```rust
    /// First retry delay; doubles per attempt up to `segment::RETRY_CAP`. Tests shorten it.
    pub retry_base_delay: Duration,
    /// No bytes for this long = the connection is dead; reconnect.
    pub stall_timeout: Duration,
```
Defaults: `retry_base_delay: Duration::from_secs(1)`, `stall_timeout: Duration::from_secs(30)`.

- [ ] **Step 5: Implement `segment.rs`**

```rust
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

/// Attempts per segment before the download fails.
pub const RETRY_MAX_ATTEMPTS: u32 = 10;
/// Longest backoff between attempts.
pub const RETRY_CAP: Duration = Duration::from_secs(60);
/// `end` of a single-stream segment until the stream tells us.
pub const UNKNOWN_END: u64 = u64::MAX;

/// Live counters for one segment, shared between the worker, the progress
/// ticker and the work stealer.
pub struct SegmentRuntime {
    /// Position in the plan.
    pub idx: u32,
    /// First byte.
    pub start: u64,
    /// Last byte, inclusive; [`UNKNOWN_END`] for a single stream. A stealer may lower it.
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
        let downloaded = downloaded.min(end + 1 - self.start);
        SegmentState { idx: self.idx, start: self.start, end, downloaded }
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
/// [`EngineError::Cancelled`].
pub async fn fetch_segment(job: SegmentJob) -> Result<(), EngineError> {
    let mut attempt: u32 = 0;
    loop {
        if job.cancel.is_cancelled() {
            return Err(EngineError::Cancelled);
        }
        match attempt_once(&job).await {
            Ok(()) => return Ok(()),
            Err(e) if e.is_transient() && attempt + 1 < RETRY_MAX_ATTEMPTS => {
                attempt += 1;
                let delay = job
                    .retry_base_delay
                    .saturating_mul(1 << (attempt - 1).min(20))
                    .min(RETRY_CAP);
                tracing::debug!(idx = job.seg.idx, attempt, ?delay, error = %e, "segment retry");
                tokio::select! {
                    _ = job.cancel.cancelled() => return Err(EngineError::Cancelled),
                    _ = tokio::time::sleep(delay) => {}
                }
            }
            Err(e) => return Err(e),
        }
    }
}

async fn attempt_once(job: &SegmentJob) -> Result<(), EngineError> {
    let seg = &job.seg;
    let next = seg.next_offset();
    let end = seg.end.load(Ordering::SeqCst);
    if end != UNKNOWN_END && next > end {
        return Ok(());
    }
    let mut rb = job.extras.apply(job.client.get(job.url.clone()));
    if job.ranged {
        rb = rb.header(RANGE, format!("bytes={next}-{end}"));
    }
    let resp = tokio::select! {
        _ = job.cancel.cancelled() => return Err(EngineError::Cancelled),
        r = rb.send() => r?,
    };
    let status = resp.status();
    if job.ranged {
        if status != StatusCode::PARTIAL_CONTENT {
            return Err(if status.is_success() {
                EngineError::RangeNotSupported
            } else {
                EngineError::HttpStatus { status: status.as_u16() }
            });
        }
        let starts_at = resp
            .headers()
            .get(CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("bytes ")?.split('-').next()?.parse::<u64>().ok());
        if starts_at != Some(next) {
            return Err(EngineError::Network(format!(
                "Content-Range starts at {starts_at:?}, wanted {next}"
            )));
        }
    } else if !status.is_success() {
        return Err(EngineError::HttpStatus { status: status.as_u16() });
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
        seg.downloaded.store(pos - seg.start, Ordering::SeqCst);
        if end != UNKNOWN_END && pos > end {
            break;
        }
    }
    let end = seg.end.load(Ordering::SeqCst);
    if end == UNKNOWN_END {
        seg.end.store(pos.saturating_sub(1).max(seg.start), Ordering::SeqCst);
        return Ok(());
    }
    if pos <= end {
        return Err(EngineError::Network(format!("connection closed at {pos}, wanted {end}")));
    }
    Ok(())
}
```

`lib.rs`: `pub mod segment;`

- [ ] **Step 6: Run**

Run: `cargo test -p mdm-engine --test segment` then `cargo clippy --all-targets -- -D warnings`
Expected: 11 tests pass, clippy clean.

- [ ] **Step 7: Commit**

```bash
git add crates/mdm-engine/src crates/mdm-engine/tests
git commit -m "feat(engine): segment worker with backoff retries, stall reconnect, cancel and shrinkable end

A segment cut short by a stealer stops cleanly; a server that ignores
Range is reported as RANGE_NOT_SUPPORTED instead of corrupting the file
with a 200 body at the wrong offset.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 10: The orchestrator (`download.rs`) — start, progress, complete

**Files:**
- Create: `crates/mdm-engine/src/download.rs`
- Create: `crates/mdm-engine/tests/download.rs`
- Modify: `crates/mdm-engine/src/lib.rs`

**Interfaces:**
- Consumes: `Engine::probe` (Task 7), `PartFile` (Task 8), `plan_segments` (Task 3), `SegmentRuntime`, `SegmentJob`, `fetch_segment` (Task 9).
- Produces:
  ```rust
  pub const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);
  pub const STEAL_MIN_REMAINING: u64 = 2 * 1024 * 1024;

  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub enum Status { Downloading, Paused, Completed, Failed, Cancelled }

  #[derive(Clone, Debug)]
  pub struct Progress { pub total: Option<u64>, pub downloaded: u64, pub speed_bps: u64, pub eta_secs: Option<u64>,
                        pub segments: Vec<SegmentState>, pub status: Status }

  #[derive(Debug)]
  pub enum Outcome { Completed(PathBuf), Paused(Vec<SegmentState>), Failed { error: EngineError, segments: Vec<SegmentState> }, Cancelled }

  #[derive(Clone, Debug)]
  pub struct Resume { pub segments: Vec<SegmentState>, pub size: u64, pub etag: Option<String>, pub last_modified: Option<String> }

  #[derive(Clone, Debug)]
  pub struct DownloadSpec { pub url: Url, pub dir: PathBuf, pub filename: Option<String>, pub extras: RequestExtras, pub resume_from: Option<Resume> }

  pub struct DownloadHandle { /* … */ }
  impl DownloadHandle {
      pub fn probe(&self) -> &Probe;
      pub fn part_path(&self) -> &Path;
      pub fn subscribe(&self) -> watch::Receiver<Progress>;
      pub fn pause(&self);             // workers stop; wait() → Outcome::Paused(segments)
      pub fn cancel(&self);            // workers stop; wait() → Outcome::Cancelled; the .part is LEFT for the caller to delete
      pub async fn wait(self) -> Outcome;
  }
  impl Engine { pub async fn start(&self, spec: DownloadSpec) -> Result<DownloadHandle, EngineError>; }
  ```
  `start` awaits the probe, so a bad URL / 404 is an `Err` from `start`, not an `Outcome`. With `resume_from`: `!probe.ranges` → `RangeNotSupported`; size / ETag (or Last-Modified when both ETags are absent) differ → `SourceChanged`; missing `.part` → `Io(NotFound)`. The caller restarts fresh on any of these. Work stealing is wired in this task (`steal()` is called when a worker finishes) and tested in Task 12.

- [ ] **Step 1: Write the failing tests**

`crates/mdm-engine/tests/download.rs`:
```rust
mod support;

use std::sync::atomic::Ordering;
use std::time::Duration;

use mdm_engine::{DownloadSpec, Engine, EngineConfig, Outcome, RequestExtras, Status};
use support::*;
use url::Url;

fn engine(conns: u8) -> Engine {
    Engine::new(EngineConfig {
        max_connections: conns,
        retry_base_delay: Duration::from_millis(1),
        stall_timeout: Duration::from_millis(300),
        ..Default::default()
    })
    .unwrap()
}

fn spec(url: &str, dir: &std::path::Path) -> DownloadSpec {
    DownloadSpec {
        url: Url::parse(url).unwrap(),
        dir: dir.to_owned(),
        filename: None,
        extras: RequestExtras::default(),
        resume_from: None,
    }
}

#[tokio::test]
async fn segmented_happy_path() {
    let s = TestServer::start(16 * 1024 * 1024).await;
    let d = tempfile::tempdir().unwrap();
    let h = engine(8).start(spec(&s.file_url(), d.path())).await.unwrap();
    assert_eq!(h.probe().size, Some(16 * 1024 * 1024));
    let rx = h.subscribe();
    let out = h.wait().await;
    let Outcome::Completed(path) = out else { panic!("{out:?}") };
    assert_eq!(path.file_name().unwrap(), "file");
    assert_eq!(sha256_file(&path), sha256_bytes(&s.data));
    let last = rx.borrow().clone();
    assert_eq!(last.status, Status::Completed);
    assert_eq!(last.downloaded, 16 * 1024 * 1024);
    assert_eq!(last.total, Some(16 * 1024 * 1024));
    assert!(last.segments.len() >= 8, "8 planned (+ any stolen)");
    assert!(last.segments.iter().all(|x| x.is_done()));
    assert!(s.cfg.requests.load(Ordering::SeqCst) >= 8);
    assert!(!d.path().join("file.mdm.part").exists());
}

#[tokio::test]
async fn single_stream_when_ranges_off() {
    let s = TestServer::start(3 * 1024 * 1024).await;
    s.cfg.ranges.store(false, Ordering::SeqCst);
    let d = tempfile::tempdir().unwrap();
    let h = engine(8).start(spec(&s.file_url(), d.path())).await.unwrap();
    let rx = h.subscribe();
    let Outcome::Completed(path) = h.wait().await else { panic!() };
    assert_eq!(sha256_file(&path), sha256_bytes(&s.data));
    assert_eq!(rx.borrow().segments.len(), 1);
    assert_eq!(s.cfg.requests.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn head_refused_is_still_segmented() {
    let s = TestServer::start(4 * 1024 * 1024).await;
    s.cfg.head_allowed.store(false, Ordering::SeqCst);
    let d = tempfile::tempdir().unwrap();
    let h = engine(4).start(spec(&s.file_url(), d.path())).await.unwrap();
    let rx = h.subscribe();
    let Outcome::Completed(path) = h.wait().await else { panic!() };
    assert_eq!(sha256_file(&path), sha256_bytes(&s.data));
    assert!(rx.borrow().segments.len() >= 4);
}

#[tokio::test]
async fn zero_byte_file_completes_immediately() {
    let s = TestServer::start(0).await;
    let d = tempfile::tempdir().unwrap();
    let h = engine(8).start(spec(&s.file_url(), d.path())).await.unwrap();
    let Outcome::Completed(path) = h.wait().await else { panic!() };
    assert_eq!(std::fs::metadata(&path).unwrap().len(), 0);
}

#[tokio::test]
async fn explicit_filename_and_clash() {
    let s = TestServer::start(1000).await;
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("x.bin"), b"old").unwrap();
    let mut sp = spec(&s.file_url(), d.path());
    sp.filename = Some("x.bin".into());
    let Outcome::Completed(path) = engine(2).start(sp).await.unwrap().wait().await else { panic!() };
    assert_eq!(path.file_name().unwrap(), "x (1).bin");
    assert_eq!(std::fs::read(d.path().join("x.bin")).unwrap(), b"old");
}

#[tokio::test]
async fn redirect_downloads_from_final_url() {
    let s = TestServer::start(2 * 1024 * 1024).await;
    let d = tempfile::tempdir().unwrap();
    let h = engine(2).start(spec(&s.redirect_url(), d.path())).await.unwrap();
    assert!(h.probe().final_url.as_str().ends_with("/file"));
    let Outcome::Completed(path) = h.wait().await else { panic!() };
    assert_eq!(sha256_file(&path), sha256_bytes(&s.data));
}

#[tokio::test]
async fn probe_failure_is_an_error_from_start() {
    let s = TestServer::start(10).await;
    let d = tempfile::tempdir().unwrap();
    let e = engine(2).start(spec(&s.status_url(404), d.path())).await.unwrap_err();
    assert_eq!(e.code(), "HTTP_STATUS");
}

#[tokio::test]
async fn survives_503_burst_and_connection_drops() {
    let s = TestServer::start(2 * 1024 * 1024).await;
    s.cfg.fail_first.store(3, Ordering::SeqCst);
    s.cfg.drop_after.store(300_000, Ordering::SeqCst);
    let d = tempfile::tempdir().unwrap();
    let Outcome::Completed(path) = engine(2).start(spec(&s.file_url(), d.path())).await.unwrap().wait().await else { panic!() };
    assert_eq!(sha256_file(&path), sha256_bytes(&s.data));
}

#[tokio::test]
async fn permanent_mid_download_failure_reports_and_keeps_part() {
    let s = TestServer::start(2 * 1024 * 1024).await;
    let d = tempfile::tempdir().unwrap();
    s.cfg.drop_after.store(1, Ordering::SeqCst); // every body dies → 10 attempts → NETWORK
    let h = engine(2).start(spec(&s.file_url(), d.path())).await.unwrap();
    let part = h.part_path().to_owned();
    let Outcome::Failed { error, segments } = h.wait().await else { panic!() };
    assert_eq!(error.code(), "NETWORK");
    assert_eq!(segments.len(), 2);
    assert!(part.exists(), "the part file stays for a later resume");
}

#[tokio::test]
async fn progress_stream_reports_bytes_and_speed() {
    let s = TestServer::start(8 * 1024 * 1024).await;
    let d = tempfile::tempdir().unwrap();
    let h = engine(4).start(spec(&s.file_url(), d.path())).await.unwrap();
    let mut rx = h.subscribe();
    let first = rx.borrow_and_update().clone();
    assert_eq!(first.status, Status::Downloading);
    assert_eq!(first.total, Some(8 * 1024 * 1024));
    let _ = h.wait().await;
    let last = rx.borrow().clone();
    assert_eq!(last.status, Status::Completed);
    assert_eq!(last.downloaded, 8 * 1024 * 1024);
    assert_eq!(last.eta_secs, Some(0));
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p mdm-engine --test download`
Expected: compile error — `DownloadSpec` etc. missing.

- [ ] **Step 3: Implement**

`crates/mdm-engine/src/download.rs`:
```rust
//! The orchestrator: probe, plan, run one worker per segment, publish
//! progress, steal work from the slowest segment, finish the file.

use std::collections::VecDeque;
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
    /// Current status.
    pub status: Status,
}

/// How a download ended.
#[derive(Debug)]
pub enum Outcome {
    /// The final path.
    Completed(PathBuf),
    /// Persist these and pass them back as [`Resume::segments`].
    Paused(Vec<SegmentState>),
    /// A permanent error; the part file is kept for a later resume.
    Failed {
        /// Why.
        error: EngineError,
        /// State at the time of failure.
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
}

const STOP_NONE: u8 = 0;
const STOP_PAUSE: u8 = 1;
const STOP_CANCEL: u8 = 2;

/// A running download.
pub struct DownloadHandle {
    probe: Probe,
    part_path: PathBuf,
    rx: watch::Receiver<Progress>,
    cancel: CancellationToken,
    stop: Arc<AtomicU8>,
    join: JoinHandle<Outcome>,
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
    /// Progress stream; the current value is available at once.
    pub fn subscribe(&self) -> watch::Receiver<Progress> {
        self.rx.clone()
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
    /// Wait for the end.
    pub async fn wait(self) -> Outcome {
        self.join.await.unwrap_or_else(|_| Outcome::Failed {
            error: EngineError::Network("download task panicked".into()),
            segments: Vec::new(),
        })
    }
}

impl Engine {
    /// Probe, then start downloading. Errors here mean nothing was started.
    pub async fn start(&self, spec: DownloadSpec) -> Result<DownloadHandle, EngineError> {
        let probe = self.probe(&spec.url, &spec.extras).await?;
        let filename = spec.filename.clone().unwrap_or_else(|| probe.filename.clone());
        let part_path = spec.dir.join(format!("{filename}{PART_SUFFIX}"));

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
                (r.segments.iter().map(|s| Arc::new(SegmentRuntime::from_state(s))).collect(), true)
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
                (Some(size), false) => (vec![Arc::new(SegmentRuntime::new(0, 0, Some(size - 1), 0))], false),
                (None, _) => (vec![Arc::new(SegmentRuntime::new(0, 0, None, 0))], false),
            },
        };

        let file = Arc::new(PartFile::open(&spec.dir, &filename, probe.size)?);
        let run = Run {
            engine: self.clone(),
            url: probe.final_url.clone(),
            extras: spec.extras.clone(),
            file,
            segments: Mutex::new(segments),
            ranged,
            total: probe.size,
            cancel: CancellationToken::new(),
            stop: Arc::new(AtomicU8::new(STOP_NONE)),
        };
        let (tx, rx) = watch::channel(run.progress(0, Status::Downloading));
        let cancel = run.cancel.clone();
        let stop = run.stop.clone();
        let join = tokio::spawn(run.run(tx));
        Ok(DownloadHandle { probe, part_path, rx, cancel, stop, join })
    }
}

struct Run {
    engine: Engine,
    url: Url,
    extras: RequestExtras,
    file: Arc<PartFile>,
    segments: Mutex<Vec<Arc<SegmentRuntime>>>,
    ranged: bool,
    total: Option<u64>,
    cancel: CancellationToken,
    stop: Arc<AtomicU8>,
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
        let mut v: Vec<SegmentState> =
            self.segments.lock().unwrap().iter().map(|s| s.snapshot()).collect();
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
        Progress { total: self.total, downloaded, speed_bps, eta_secs, segments, status }
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
        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    let p = self.progress(0, Status::Downloading);
                    let speed = meter.push(p.downloaded);
                    tx.send_replace(Progress { speed_bps: speed, ..p });
                }
                res = set.join_next() => match res {
                    None => break,
                    Some(Ok(Ok(()))) => {
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
                            failure = Some(EngineError::Network("worker panicked".into()));
                            self.cancel.cancel();
                        }
                    }
                }
            }
        }

        let segments = self.snapshot();
        let outcome = if let Some(error) = failure {
            Outcome::Failed { error, segments }
        } else {
            match self.stop.load(Ordering::SeqCst) {
                STOP_PAUSE => Outcome::Paused(segments),
                STOP_CANCEL => Outcome::Cancelled,
                _ if segments.iter().all(|s| s.is_done()) => {
                    match Arc::try_unwrap(self.file) {
                        Ok(file) => match file.finish() {
                            Ok(path) => Outcome::Completed(path),
                            Err(error) => Outcome::Failed { error, segments },
                        },
                        Err(_) => Outcome::Failed {
                            error: EngineError::Network("part file still in use".into()),
                            segments,
                        },
                    }
                }
                _ => Outcome::Failed {
                    error: EngineError::Network("workers ended with bytes missing".into()),
                    segments,
                },
            }
        };
        let status = match &outcome {
            Outcome::Completed(_) => Status::Completed,
            Outcome::Paused(_) => Status::Paused,
            Outcome::Failed { .. } => Status::Failed,
            Outcome::Cancelled => Status::Cancelled,
        };
        let final_progress = Progress {
            segments: match &outcome {
                Outcome::Completed(_) => {
                    // `self.file` may be gone; rebuild from the last snapshot.
                    let mut v = self.segments_after_finish();
                    v.sort_by_key(|s| s.idx);
                    v
                }
                _ => self.snapshot(),
            },
            ..self.progress_without_file(status)
        };
        tx.send_replace(final_progress);
        outcome
    }

    fn segments_after_finish(&self) -> Vec<SegmentState> {
        self.segments.lock().unwrap().iter().map(|s| s.snapshot()).collect()
    }

    fn progress_without_file(&self, status: Status) -> Progress {
        let segments = self.segments_after_finish();
        let downloaded = segments.iter().map(|s| s.downloaded).sum();
        Progress {
            total: self.total,
            downloaded,
            speed_bps: 0,
            eta_secs: self.total.map(|t| t.saturating_sub(downloaded)).map(|left| if left == 0 { 0 } else { u64::MAX }),
            segments,
            status,
        }
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
```

Note on `self.file` after `Arc::try_unwrap`: `run(self)` moves `self.file` into `try_unwrap` inside the match, so the compiler will reject the later `self.snapshot()` use of `self.segments`? No — `segments` is a different field; partial moves out of `self` are allowed because `Run` has no `Drop` impl. `progress_without_file` deliberately reads only `segments` and `total`. Do NOT add a `Drop` impl to `Run`.

`eta_secs` in the final progress: `Some(0)` when complete, `u64::MAX` is wrong for a paused download — replace that line with `eta_secs: self.total.filter(|t| *t <= downloaded).map(|_| 0),` so it is `Some(0)` only when everything is downloaded and `None` otherwise. (Written out here so the implementer does not copy the placeholder above.)

`lib.rs`:
```rust
pub mod download;
pub use download::{DownloadHandle, DownloadSpec, Outcome, Progress, Resume, Status, PROGRESS_INTERVAL, STEAL_MIN_REMAINING};
```

- [ ] **Step 4: Run**

Run: `cargo test -p mdm-engine --test download` then the whole crate `cargo test -p mdm-engine` and `cargo clippy --all-targets -- -D warnings`.
Expected: 10 new tests pass; everything earlier still green; clippy clean (fix any `needless_return` / `clone_on_copy` it flags — do not `allow` them).

- [ ] **Step 5: Commit**

```bash
git add crates/mdm-engine/src/download.rs crates/mdm-engine/src/lib.rs crates/mdm-engine/tests/download.rs
git commit -m "feat(engine): orchestrator - probe, plan, workers, progress stream, finish

Engine::start returns a handle with a watch-channel of Progress; the
outcome tells the caller exactly what to persist (Paused segments) or
delete (Cancelled). Work stealing is wired here and pinned in Task 12.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 11: Pause, resume, crash recovery, changed source (`tests/resume.rs`)

**Files:**
- Create: `crates/mdm-engine/tests/resume.rs`
- Modify (only if a test exposes a bug): `crates/mdm-engine/src/download.rs`

**Interfaces:**
- Consumes: everything from Task 10. No new public API; this task pins behaviour.

- [ ] **Step 1: Write the tests**

`crates/mdm-engine/tests/resume.rs`:
```rust
mod support;

use std::sync::atomic::Ordering;
use std::time::Duration;

use mdm_engine::{DownloadSpec, Engine, EngineConfig, Outcome, RequestExtras, Resume, SegmentState};
use support::*;
use url::Url;

const SIZE: usize = 8 * 1024 * 1024;

fn engine() -> Engine {
    Engine::new(EngineConfig {
        max_connections: 4,
        retry_base_delay: Duration::from_millis(1),
        stall_timeout: Duration::from_millis(400),
        ..Default::default()
    })
    .unwrap()
}

fn spec(s: &TestServer, dir: &std::path::Path, resume: Option<Resume>) -> DownloadSpec {
    DownloadSpec {
        url: Url::parse(&s.file_url()).unwrap(),
        dir: dir.to_owned(),
        filename: None,
        extras: RequestExtras::default(),
        resume_from: resume,
    }
}

/// Start with every first body hanging after 1 000 bytes, wait until all
/// four segments have those bytes, pause. Returns the paused segments.
async fn start_and_pause(s: &TestServer, dir: &std::path::Path) -> Vec<SegmentState> {
    s.cfg.hang_first.store(4, Ordering::SeqCst);
    let h = engine().start(spec(s, dir, None)).await.unwrap();
    let mut rx = h.subscribe();
    loop {
        rx.changed().await.unwrap();
        if rx.borrow().downloaded >= 4000 {
            break;
        }
    }
    h.pause();
    let Outcome::Paused(segs) = h.wait().await else { panic!("expected Paused") };
    s.cfg.hang_first.store(0, Ordering::SeqCst);
    segs
}

fn resume(s: &TestServer, segs: Vec<SegmentState>) -> Resume {
    Resume {
        segments: segs,
        size: s.data.len() as u64,
        etag: Some(s.cfg.etag.lock().unwrap().clone()),
        last_modified: Some("Thu, 18 Sep 2026 10:00:00 GMT".into()),
    }
}

#[tokio::test]
async fn pause_then_resume_completes_with_the_right_bytes() {
    let s = TestServer::start(SIZE).await;
    let d = tempfile::tempdir().unwrap();
    let segs = start_and_pause(&s, d.path()).await;
    assert_eq!(segs.len(), 4);
    let paused_bytes: u64 = segs.iter().map(|x| x.downloaded).sum();
    assert!(paused_bytes >= 4000 && paused_bytes < SIZE as u64);
    assert!(d.path().join("file.mdm.part").exists());

    let h = engine().start(spec(&s, d.path(), Some(resume(&s, segs)))).await.unwrap();
    let first = h.subscribe().borrow().clone();
    assert!(first.downloaded >= paused_bytes, "progress starts where it stopped");
    let Outcome::Completed(path) = h.wait().await else { panic!() };
    assert_eq!(sha256_file(&path), sha256_bytes(&s.data));
}

#[tokio::test]
async fn crash_recovery_tolerates_an_unflushed_window() {
    let s = TestServer::start(SIZE).await;
    let d = tempfile::tempdir().unwrap();
    let mut segs = start_and_pause(&s, d.path()).await;
    // Pretend the store was ~1 s behind: forget the last 500 bytes of every segment.
    for x in &mut segs {
        x.downloaded = x.downloaded.saturating_sub(500);
    }
    let Outcome::Completed(path) = engine().start(spec(&s, d.path(), Some(resume(&s, segs)))).await.unwrap().wait().await else { panic!() };
    assert_eq!(sha256_file(&path), sha256_bytes(&s.data));
}

#[tokio::test]
async fn changed_etag_is_source_changed() {
    let s = TestServer::start(SIZE).await;
    let d = tempfile::tempdir().unwrap();
    let segs = start_and_pause(&s, d.path()).await;
    let r = resume(&s, segs);
    *s.cfg.etag.lock().unwrap() = "\"v2\"".into();
    let e = engine().start(spec(&s, d.path(), Some(r))).await.unwrap_err();
    assert_eq!(e.code(), "SOURCE_CHANGED");
}

#[tokio::test]
async fn changed_size_is_source_changed() {
    let s = TestServer::start(SIZE).await;
    let d = tempfile::tempdir().unwrap();
    let segs = start_and_pause(&s, d.path()).await;
    let mut r = resume(&s, segs);
    r.size += 1;
    assert_eq!(engine().start(spec(&s, d.path(), Some(r))).await.unwrap_err().code(), "SOURCE_CHANGED");
}

#[tokio::test]
async fn server_that_lost_ranges_is_range_not_supported() {
    let s = TestServer::start(SIZE).await;
    let d = tempfile::tempdir().unwrap();
    let segs = start_and_pause(&s, d.path()).await;
    let r = resume(&s, segs);
    s.cfg.ranges.store(false, Ordering::SeqCst);
    assert_eq!(engine().start(spec(&s, d.path(), Some(r))).await.unwrap_err().code(), "RANGE_NOT_SUPPORTED");
}

#[tokio::test]
async fn missing_part_file_is_an_io_error() {
    let s = TestServer::start(SIZE).await;
    let d = tempfile::tempdir().unwrap();
    let segs = start_and_pause(&s, d.path()).await;
    std::fs::remove_file(d.path().join("file.mdm.part")).unwrap();
    assert_eq!(engine().start(spec(&s, d.path(), Some(resume(&s, segs)))).await.unwrap_err().code(), "IO");
}

#[tokio::test]
async fn cancel_leaves_the_part_file_for_the_caller() {
    let s = TestServer::start(SIZE).await;
    let d = tempfile::tempdir().unwrap();
    s.cfg.hang_first.store(4, Ordering::SeqCst);
    let h = engine().start(spec(&s, d.path(), None)).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    h.cancel();
    let part = h.part_path().to_owned();
    assert!(matches!(h.wait().await, Outcome::Cancelled));
    assert!(part.exists());
    std::fs::remove_file(part).unwrap();
}
```

- [ ] **Step 2: Run**

Run: `cargo test -p mdm-engine --test resume`
Expected: 7 tests pass. If `pause_then_resume…` hangs, the loop waiting for `downloaded >= 4000` never sees 4000 — check that `hang_first` is 4 and that each hung body sends exactly 1 000 bytes before pending.

- [ ] **Step 3: Commit**

```bash
git add crates/mdm-engine/tests/resume.rs
git commit -m "test(engine): pause/resume, crash recovery, changed source, lost ranges, missing part

Resume is a fresh start with the stored segments; the same path serves a
crash, so a stale flush window only costs a rewrite of identical bytes.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 12: Work stealing pinned (`tests/steal.rs`)

**Files:**
- Create: `crates/mdm-engine/tests/steal.rs`
- Modify: `crates/mdm-engine/tests/support/mod.rs` (add `chunk_delay_ms`)

**Interfaces:**
- Consumes: `Run::steal` (Task 10, private, exercised through `Engine::start`).

- [ ] **Step 1: Add a `chunk_delay_ms` switch to the test server**

In `ServerCfg`: `pub chunk_delay_ms: AtomicU64,` (default 0; doc: "stream bodies in 64 KiB chunks with this pause between them, so a test can watch a download in flight"). In `file()`, before the `hang` / `drop_after` body selection, add a branch that wins when `chunk_delay_ms > 0` and neither hang nor drop applies:
```rust
    let delay = cfg.chunk_delay_ms.load(Ordering::SeqCst);
    let body = if hang {
        /* unchanged */
    } else if drop_after > 0 && drop_after < len {
        /* unchanged */
    } else if delay > 0 {
        let chunks: Vec<Vec<u8>> = slice.chunks(64 * 1024).map(|c| c.to_vec()).collect();
        Body::from_stream(stream::unfold(chunks.into_iter(), move |mut it| async move {
            let c = it.next()?;
            tokio::time::sleep(Duration::from_millis(delay)).await;
            Some((Ok::<_, std::io::Error>(bytes::Bytes::from(c)), it))
        }))
    } else {
        Body::from(slice)
    };
```
Add `use std::time::Duration;` to the support module.

- [ ] **Step 2: Write the tests**

`crates/mdm-engine/tests/steal.rs`:
```rust
mod support;

use std::sync::atomic::Ordering;
use std::time::Duration;

use mdm_engine::{DownloadSpec, Engine, EngineConfig, Outcome, PartFile, RequestExtras, Resume, SegmentState};
use support::*;
use url::Url;

const MIB: u64 = 1024 * 1024;

#[tokio::test]
async fn a_finished_worker_takes_half_of_the_largest_remaining_range() {
    let s = TestServer::start(12 * MIB as usize).await;
    s.cfg.chunk_delay_ms.store(3, Ordering::SeqCst); // ~190 chunks → ~0.6 s for the big segment
    let d = tempfile::tempdir().unwrap();
    // A skewed plan as if resumed: 1 MiB + 11 MiB. The small one finishes first.
    drop(PartFile::open(d.path(), "file", Some(12 * MIB)).unwrap());
    let segs = vec![
        SegmentState { idx: 0, start: 0, end: MIB - 1, downloaded: 0 },
        SegmentState { idx: 1, start: MIB, end: 12 * MIB - 1, downloaded: 0 },
    ];
    let engine = Engine::new(EngineConfig {
        max_connections: 2,
        retry_base_delay: Duration::from_millis(1),
        stall_timeout: Duration::from_secs(5),
        ..Default::default()
    })
    .unwrap();
    let h = engine
        .start(DownloadSpec {
            url: Url::parse(&s.file_url()).unwrap(),
            dir: d.path().to_owned(),
            filename: None,
            extras: RequestExtras::default(),
            resume_from: Some(Resume {
                segments: segs,
                size: 12 * MIB,
                etag: Some("\"v1\"".into()),
                last_modified: Some("Thu, 18 Sep 2026 10:00:00 GMT".into()),
            }),
        })
        .await
        .unwrap();
    let rx = h.subscribe();
    let Outcome::Completed(path) = h.wait().await else { panic!() };
    assert_eq!(sha256_file(&path), sha256_bytes(&s.data));
    let last = rx.borrow().clone();
    assert!(last.segments.len() >= 3, "a stolen segment appeared: {:?}", last.segments);
    assert!(last.segments.iter().all(|x| x.is_done()));
    // Stolen segments are contiguous with their victims: sorted by start, no gaps, no overlap.
    let mut by_start = last.segments.clone();
    by_start.sort_by_key(|x| x.start);
    for w in by_start.windows(2) {
        assert_eq!(w[0].end + 1, w[1].start, "{:?}", by_start);
    }
    assert_eq!(by_start.last().unwrap().end, 12 * MIB - 1);
}

#[tokio::test]
async fn nothing_is_stolen_when_less_than_two_mib_remain() {
    let s = TestServer::start(3 * MIB as usize).await;
    let d = tempfile::tempdir().unwrap();
    let engine = Engine::new(EngineConfig { max_connections: 2, ..Default::default() }).unwrap();
    let h = engine
        .start(DownloadSpec {
            url: Url::parse(&s.file_url()).unwrap(),
            dir: d.path().to_owned(),
            filename: None,
            extras: RequestExtras::default(),
            resume_from: None,
        })
        .await
        .unwrap();
    let rx = h.subscribe();
    let Outcome::Completed(_) = h.wait().await else { panic!() };
    assert_eq!(rx.borrow().segments.len(), 2, "1.5 MiB halves never qualify");
}
```

- [ ] **Step 3: Run**

Run: `cargo test -p mdm-engine --test steal`
Expected: 2 tests pass. If the first reports only 2 segments, the small segment did not finish before the big one dropped under 2 MiB remaining — raise `chunk_delay_ms` to 5.

- [ ] **Step 4: Commit**

```bash
git add crates/mdm-engine/tests
git commit -m "test(engine): work stealing splits the largest remaining range and keeps the file contiguous

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 13: Example CLI, engine docs, final gates

**Files:**
- Create: `crates/mdm-engine/examples/fetch.rs`
- Create: `docs/ENGINE.md`
- Modify: `README.md` (status line + how to try the engine)

**Interfaces:**
- Consumes: the public API of Tasks 3–10.

- [ ] **Step 1: The example**

`crates/mdm-engine/examples/fetch.rs`:
```rust
//! Try the engine for real: `cargo run --example fetch -- <url> [dir] [connections]`

use std::path::PathBuf;

use mdm_engine::{DownloadSpec, Engine, EngineConfig, Outcome, RequestExtras};

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let Some(url) = args.next() else {
        eprintln!("usage: fetch <url> [dir] [connections]");
        std::process::exit(2);
    };
    let dir = PathBuf::from(args.next().unwrap_or_else(|| ".".into()));
    let conns: u8 = args.next().and_then(|c| c.parse().ok()).unwrap_or(8);

    let engine = Engine::new(EngineConfig { max_connections: conns, ..Default::default() }).unwrap();
    let handle = match engine
        .start(DownloadSpec {
            url: url.parse().expect("a valid http(s) URL"),
            dir,
            filename: None,
            extras: RequestExtras::default(),
            resume_from: None,
        })
        .await
    {
        Ok(h) => h,
        Err(e) => {
            eprintln!("{} ({})", e, e.code());
            std::process::exit(1);
        }
    };
    let p = handle.probe();
    println!("{}  size={:?}  ranges={}", p.filename, p.size, p.ranges);

    let mut rx = handle.subscribe();
    let printer = tokio::spawn(async move {
        while rx.changed().await.is_ok() {
            let pr = rx.borrow().clone();
            let pct = pr.total.map(|t| if t == 0 { 100.0 } else { pr.downloaded as f64 * 100.0 / t as f64 });
            let bars: String = pr
                .segments
                .iter()
                .map(|s| {
                    let len = s.end - s.start + 1;
                    match s.downloaded * 8 / len.max(1) {
                        8.. => '█',
                        n => "▁▂▃▄▅▆▇".chars().nth(n as usize).unwrap_or('▁'),
                    }
                })
                .collect();
            print!(
                "\r{:>6.2}%  {:>8.1} KiB/s  eta {:>4}s  {} {:?}      ",
                pct.unwrap_or(0.0),
                pr.speed_bps as f64 / 1024.0,
                pr.eta_secs.map_or("?".into(), |e| e.to_string()),
                bars,
                pr.status
            );
        }
    });
    let out = handle.wait().await;
    let _ = printer.await;
    println!();
    match out {
        Outcome::Completed(path) => println!("saved {}", path.display()),
        Outcome::Failed { error, .. } => {
            eprintln!("failed: {} ({})", error, error.code());
            std::process::exit(1);
        }
        other => println!("{other:?}"),
    }
}
```

- [ ] **Step 2: Run it against the real internet once (manual check, not CI)**

Run: `cargo run -p mdm-engine --example fetch -- https://speed.hetzner.de/100MB.bin %TEMP% 8`
Expected: a progress line with eight bars filling, then `saved …\100MB.bin`. Then verify: `certutil -hashfile %TEMP%\100MB.bin SHA256` prints a hash (the file is random data; the point is a clean finish with no `.mdm.part` left behind). If Hetzner is down, use `https://proof.ovh.net/files/100Mb.dat`.

- [ ] **Step 3: `docs/ENGINE.md`**

```markdown
# mdm-engine

The HTTP download engine. A library: no Tauri, no database, no UI.

## Flow

probe → plan → allocate → fetch (N workers) → complete

- **probe** (`probe.rs`): HEAD, or GET `Range: bytes=0-0`. Size, range support, ETag,
  Last-Modified, MIME, file name. Redirects followed; `final_url` is what workers fetch.
- **plan** (`plan.rs`): `min(max_connections, ceil(size / 1 MiB))` contiguous segments.
- **allocate** (`file.rs`): `<name>.mdm.part`, pre-sized. Positioned writes; no merge step.
- **fetch** (`segment.rs`): one worker per segment. `Range: bytes=<next>-<end>`, expects 206
  with a matching `Content-Range`. Transient errors back off 1, 2, 4 … 60 s, ten attempts.
  30 s without bytes = reconnect. 4xx = fail at once.
- **complete** (`download.rs`): fsync, rename, `name (1).ext` on a clash.

## Progress, pause, resume

`Engine::start` returns a `DownloadHandle`: `subscribe()` is a `watch` channel of
`Progress` (250 ms cadence, 2 s speed window). `pause()` ends the task with
`Outcome::Paused(segments)`; the caller stores them and later calls `start` again with
`resume_from: Resume { segments, size, etag, last_modified }`. A crash is the same path.
Before resuming the engine re-probes: a changed ETag / Last-Modified / size is
`SOURCE_CHANGED`; a server that stopped honouring ranges is `RANGE_NOT_SUPPORTED`; a
missing `.mdm.part` is `IO`. All three mean "start over" to the caller.

## Work stealing

When a worker finishes, the largest segment with more than 2 MiB left is split at its
midpoint; the freed connection takes the second half. Segment `end` is atomic, so the
victim notices and stops. This is why the last 10 % does not crawl on one connection.

## Errors

`EngineError::code()` is stable: `INVALID_URL`, `RANGE_NOT_SUPPORTED`, `SOURCE_CHANGED`,
`DISK_FULL`, `HTTP_STATUS`, `NETWORK`, `TLS`, `CANCELLED`, `IO`.

## Tests

`cargo test -p mdm-engine`. Integration tests run an in-process axum server
(`tests/support/mod.rs`) with switches: `ranges`, `head_allowed`, `fail_first` (503s),
`drop_after`, `hang_first`, `chunk_delay_ms`, `etag`, `content_disposition`. Every
download test ends by comparing SHA-256 of the result with the served bytes.

## Try it

    cargo run -p mdm-engine --example fetch -- <url> [dir] [connections]
```

- [ ] **Step 4: README status**

Replace the README's **Status** paragraph with:
```markdown
**Status:** the download engine (`crates/mdm-engine`) is complete and tested — segmented
downloads, pause / resume / crash recovery, retries, work stealing. Try it:

    cargo run -p mdm-engine --example fetch -- https://example.com/big.iso

The desktop app, browser extension and torrent support follow (see `docs/superpowers/`).
```

- [ ] **Step 5: Full gates**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: all green. Fix anything clippy flags in the code, never with `#[allow]`.

- [ ] **Step 6: Commit and push**

```bash
git add -A
git commit -m "docs(engine): ENGINE.md, fetch example, README status

The engine phase is done: a real file downloads over eight connections
with the example, and every behaviour in the spec is pinned by a test.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```
Then create the GitHub repo (owner's account; ask first if not already created): `gh repo create imamachowdhury/muzn-download-manager --public --source=. --push`. CI must be green on `main` before Plan 2 starts.

---

## Self-review against the spec

- **Spec §3 lifecycle** probe → Task 7; plan → Task 3; allocate → Task 8; fetch → Task 9; progress → Task 10; complete → Task 8 + 10. ✔
- **§3 pause / resume / crash**: Task 10 (`pause`, `resume_from`, `SourceChanged`, `RangeNotSupported`), Task 11 pins all of it. ✔ Deviation recorded: resume is a fresh `start`, not `handle.resume()`.
- **§3 retry & errors**: backoff, max 10, permanent codes, stall → Task 9; work stealing → Task 10 + 12; disk full → `from_io` mapping in Task 5, pre-allocation in Task 8; concurrency limit (`max_parallel_downloads`) is a queue concern → Plan 2 (app), not the engine. ✔
- **§3 API**: `Engine::new / probe / start`, `DownloadHandle::subscribe / pause / cancel / wait`, `EngineError::code()` ✔. `cancel(delete_part)` became `cancel()` + the caller deletes `part_path()` — recorded in Decisions.
- **§3 tests list**: segmented, single stream, HEAD refused, pause/resume, crash, drop, 503, 404, ETag change, work stealing, proptest — every one has a named test above. ✔
- **§2 storage**: SQLite is Plan 2. The engine hands over `SegmentState` and validators, which is exactly what the `segments` table stores. ✔
- **Placeholder scan**: the one placeholder-ish line (`eta_secs … u64::MAX`) in Task 10 is called out with its replacement in the same step. No TBDs.
- **Type consistency**: `SegmentState { idx, start, end, downloaded }` (Task 3) is what `SegmentRuntime::from_state / snapshot` (Task 9), `Resume.segments` and `Progress.segments` (Task 10) and every test use. `EngineConfig` gains `retry_base_delay` / `stall_timeout` in Task 9 and every later test constructs it with `..Default::default()`. `TestServer` switches are added in the task that first needs them (`hang_first` Task 9, `chunk_delay_ms` Task 12).

## Reference notes taken from other projects (for Plans 2–5, not this plan)

- **dlman** (Tauri + Rust + SQLite): WXT for the extension (one source → Chrome/Firefox), `browser.alarms` 30 s reconnect, probe-time auth detection + credential prompt on 401/403 (v1.1), final CDN URL cached for resume (we keep `final_url`).
- **hydra** (Rust): I/O-free core (`plan`/`sched`/`intervals`) with a simulator — the shape `plan.rs` follows; range stealing; positioned writes.
- **Motrix**: native host as its own crate (`protocol` / `ticket` / `launcher`) — the shape `mdm-bridge` follows.
- **IDM's install folder** (file names and plain-text configs only): native-messaging manifest `com.tonec.idm` with per-browser `allowed_origins` / `allowed_extensions` — identical to the spec's bridge; Alt-click = let the browser download, a configurable "force capture" key (Insert) — add the force key to Plan 3; `defexclist.txt` = URL **glob** exclusion patterns (`http://*.example.com/*.mp3`), so the extension's skip-list should accept globs, not just hosts; "Sites Logins" per host (v1.1); toggles for the start-download and download-complete dialogs (Plan 2 settings); kernel drivers (`idmwfp*.sys`) for video capture — deliberately out of scope.
