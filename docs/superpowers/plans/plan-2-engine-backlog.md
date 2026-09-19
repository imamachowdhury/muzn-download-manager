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
test's fixed sleep. Still open:

## Error codes

- A header value with CR/LF becomes a reqwest builder error mapped to `INVALID_URL`.

## Tests and tooling

- The crash-recovery test proves the flush window only through the final hash; count the bytes
  served on resume.
- `filename*` ignores its charset; backslash-escaped quotes are not unescaped; the UTF-8 boundary
  test uses 2-byte characters, so the cut loop is never exercised.
- Cookie values containing `;` are not escaped.
- The test server answers an invalid Range with 200, not 416.
- Pin GitHub Actions by SHA; add `CONTRIBUTING.md`; add `pnpm-workspace.yaml` when `extension/`
  arrives (npm-style `workspaces` in `package.json` does nothing for pnpm).

## Found during Plan 2's reviews

- The live-part registry's keys, and the manager's `r.dir == row.dir` comparison in
  `reserved_parts`, are not normalised — two spellings of the same Windows path (`C:\x` vs
  `c:\X\`) are treated as different folders.
- The manager's one-second save timer should become "save when the durable segments actually
  changed", so a stalled download does not write the same snapshot to the store every second.
- A panicking driver task leaks its queue slot: `finish()` never runs, so the row stays in
  `running` forever and the scheduler treats the slot as permanently occupied.
- A metadata error after a successful completion (`std::fs::metadata` in `drive_inner`, used to
  learn the size of a stream of unknown length) marks an otherwise-finished download FAILED
  instead of COMPLETED.
- The progress tick `await`s `sync_to` (the fsync) directly on the loop that also sends
  `Progress` and drives the ticker; a slow disk freezes progress reporting along with the sync
  instead of only delaying the sync.
- The crash test should prove a resume by counting bytes served, not only by comparing the final
  SHA-256 — the hash alone cannot tell a real resume from a restart that redownloaded everything.
- `ProbeInfo` has no serde derives, so the desktop app (Plan 3) cannot send a probe result across
  a Tauri command or store one anywhere JSON is needed.
