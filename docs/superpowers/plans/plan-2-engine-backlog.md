# Engine backlog carried into Plan 2

Collected from Plan 1's task reviews and final whole-branch review (2026-09-18). Plan 2 (the
desktop app) starts with these engine tasks, before the SQLite store builds on the engine.

Closed by Plan 2: the retry reset, credential strip, engine-owned headers, durable snapshot,
exclusive part names, blocking writes, hidden range support, empty unknown-length stream, bare
206, StopOnDrop cancel, checked resume math, pause-after-last-byte, INTERNAL code, the `== 6` pin,
the speed assertion (the mid-download 200 fallback is closed too, by the manager's automatic
fresh start as one plain GET — `DownloadSpec::single_stream`). The final whole-branch review
(2026-09-19) closed: sanitised caller file names (core and engine), no foreign part deletion
when a finished/cancelled row is removed, the real one-stream fallback, `PART_IN_USE` (fails,
never deletes), the `finish()` free-name-then-rename race (`FINISH_LOCK`), the manager's own
runtime handle (sync calls from outside the runtime), the SQLite busy timeout, and the crash
test's fixed sleep. Closed by Plan 3: `ProbeInfo` for the UI (a serialisable `ProbePreview` and
`Manager::probe`), the panicking-driver slot leak, the metadata-after-completion FAILED,
save-on-change, and the npm-style `workspaces` field (replaced by `pnpm-workspace.yaml`). Still
open:

## Error codes

- A header value with CR/LF becomes a reqwest builder error mapped to `INVALID_URL`.

## Tests and tooling

- The crash-recovery test proves the flush window only through the final hash; count the bytes
  served on resume.
- `filename*` ignores its charset; backslash-escaped quotes are not unescaped; the UTF-8 boundary
  test uses 2-byte characters, so the cut loop is never exercised.
- Cookie values containing `;` are not escaped.
- The test server answers an invalid Range with 200, not 416.
- Pin GitHub Actions by SHA; add `CONTRIBUTING.md`.

## Found during Plan 2's reviews

- The live-part registry's keys, and the manager's `r.dir == row.dir` comparison in
  `reserved_parts`, are not normalised — two spellings of the same Windows path (`C:\x` vs
  `c:\X\`) are treated as different folders.
- The progress tick `await`s `sync_to` (the fsync) directly on the loop that also sends
  `Progress` and drives the ticker; a slow disk freezes progress reporting along with the sync
  instead of only delaying the sync.
- The crash test should prove a resume by counting bytes served, not only by comparing the final
  SHA-256 — the hash alone cannot tell a real resume from a restart that redownloaded everything.

## Found in Plan 3's final review

- A setup failure panics invisibly in a release build (no console): show an error dialog and log to a file under the data dir.
- Downloaded files carry no Windows Mark-of-the-Web (Plan 6).
- GNOME without AppIndicator: the tray is built but invisible, so close-to-tray hides the window into nothing.
- `open_path(to_string_lossy)` mangles non-UTF-8 paths.
- No automated test of the 5 s shutdown grace.
- Unused Android / iOS / Store icon sets in `src-tauri/icons`.
- UI nits: `DownloadRowView`'s memo is defeated (a new `style` object each render); error toasts are polite, not assertive; the progressbar's aria-label is its value; `DetailPanel`'s segments `catch` is silent; `STATUS_LABEL` is imported from a component file.
- Rust nits: `row_of` builds NOT_FOUND by hand; the test row fixture is duplicated; `build.rs` has no early return off Windows; `embed-resource` is not a workspace dependency; TypeScript is pinned to 6 for typescript-eslint.
- Add an axe check (vitest-axe) to the gates.
