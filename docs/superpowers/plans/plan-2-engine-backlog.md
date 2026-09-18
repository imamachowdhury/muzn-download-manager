# Engine backlog carried into Plan 2

Collected from Plan 1's task reviews and final whole-branch review (2026-09-18). Plan 2 (the
desktop app) starts with these engine tasks, before the SQLite store builds on the engine.

## Owner decision

- **Retry budget resets on progress** (owner, 2026-09-18: "haan, progress holei retry reset koro,
  Plan 2-e dao"). `fetch_segment` resets `attempt` and the backoff when an attempt wrote at least
  one byte; only 10 attempts in a row without progress fail. Rework the two tests that would never
  fail afterwards — `gives_up_after_max_attempts` (`drop_after = 10`) and
  `permanent_mid_download_failure_reports_and_keeps_part` (`drop_after = 1`) — to bodies that drop
  with 0 bytes, or to 503s. Add a regression test: a body that drops every 50 KB over a 2 MB segment
  completes.

## Before the extension ships (Plan 3)

- **Cross-origin credential strip.** Workers request `probe.final_url` directly with the full
  `RequestExtras`, so cookies and `Authorization` reach a redirect target on another host (reqwest
  strips them only while it follows the redirect itself). Drop `Cookie`, `Authorization` and
  `Proxy-Authorization` when the origin of `final_url` differs from `spec.url`; test with a redirect
  from `127.0.0.1` to `localhost`.
- Filter `Range`, `Host` and `Content-Length` out of caller extras (they would duplicate the
  engine's own headers).

## Durability and concurrency

- **Durable snapshot.** `Progress.segments` counts bytes that may still sit in the OS page cache;
  after a power loss the store can claim bytes that never reached the disk. Run `sync_data`
  periodically (in `spawn_blocking`) and publish a "durable" snapshot taken at the last sync for the
  store to save. Amend the spec's crash paragraph (§3) accordingly.
- **Exclusive part names.** Two live downloads of the same (dir, filename) share one `.mdm.part`
  (documented today). Claim the part name with `create_new` and fall back to `name (1).ext.mdm.part`.
- **Blocking writes.** `PartFile::write_at` runs on tokio worker threads; move it to
  `spawn_blocking` / `block_in_place` before three downloads × eight connections share the runtime.

## Behaviour gaps

- A ranged request answered with 200 mid-download is permanent `RANGE_NOT_SUPPORTED`; the spec
  wants a fall back to a single stream with a notice (same on resume).
- HEAD with `Content-Length` but no `Accept-Ranges` never tries `GET bytes=0-0`, so servers that
  honour ranges without advertising them get one connection.
- A zero-byte single stream of unknown length (chunked, empty) ends with `end = 0, downloaded = 0`,
  so the download fails with "workers ended with bytes missing". One-line fix in `attempt_once`.
- A 206 without `Content-Range` costs ten retries before `NETWORK`; `RANGE_NOT_SUPPORTED` is the
  honest code.
- `StopOnDrop` stores `STOP_PAUSE` unconditionally, so `cancel()` followed by dropping the handle
  (without `wait()`) can report `Paused`; use `compare_exchange(STOP_NONE, STOP_PAUSE)`.
- `validate_resume` uses unchecked `end - start + 1` / `end + 1`; corrupted saved state with
  `end == u64::MAX` panics in debug. Use `checked_add` → `INVALID_RESUME`.
- A pause (or a dropped handle) landing just after the last byte reports `Paused` for a complete
  file; resuming finishes it. Consider checking `all_done` before `stop`.

## Error codes

- A worker panic and the internal invariant paths map to transient `NETWORK`; use `IO` or a
  dedicated internal code so the UI never offers a retry for a panic.
- A header value with CR/LF becomes a reqwest builder error mapped to `INVALID_URL`.

## Tests and tooling

- `retries_after_connection_drop_and_503` asserts `>= 6` requests; `== 6` is deterministic.
- `progress_stream_reports_bytes_and_speed` asserts no speed (rename or assert it on a slow body).
- The crash-recovery test proves the flush window only through the final hash; count the bytes
  served on resume.
- `filename*` ignores its charset; backslash-escaped quotes are not unescaped; the UTF-8 boundary
  test uses 2-byte characters, so the cut loop is never exercised.
- Cookie values containing `;` are not escaped.
- The test server answers an invalid Range with 200, not 416.
- Pin GitHub Actions by SHA; add `CONTRIBUTING.md`; add `pnpm-workspace.yaml` when `extension/`
  arrives (npm-style `workspaces` in `package.json` does nothing for pnpm).
