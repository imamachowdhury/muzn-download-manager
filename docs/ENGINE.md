# mdm-engine

The HTTP download engine. A library: no Tauri, no database, no UI.

## Flow

probe → plan → allocate → fetch (N workers) → complete

- **probe** (`probe.rs`): HEAD, or GET `Range: bytes=0-0`. Size, range support, ETag,
  Last-Modified, MIME, file name. Redirects followed; `final_url` is what workers fetch.
- **plan** (`plan.rs`): `min(max_connections (≤ 32), ceil(size / 1 MiB))` contiguous segments.
- **allocate** (`file.rs`): `<name>.mdm.part`, pre-sized. Positioned writes; no merge step.
- **fetch** (`segment.rs`): one worker per segment. `Range: bytes=<next>-<end>`, expects 206
  with a matching `Content-Range`. Transient errors back off 1, 2, 4 … 60 s, ten attempts.
  30 s without bytes = reconnect. 4xx except 429 = fail at once; 429 and 5xx back off like
  network errors. A single stream (server without range support) restarts from byte 0 on
  every retry — a plain GET always answers from the beginning.
- **complete** (`download.rs`): fsync, rename, `name (1).ext` on a clash.

## Progress, pause, resume

`Engine::start` returns a `DownloadHandle`: `subscribe()` is a `watch` channel of
`Progress` (250 ms cadence, 2 s speed window). `pause()` ends the task with
`Outcome::Paused(segments)`; the caller stores them and later calls `start` again with
`resume_from: Resume { segments, size, etag, last_modified }`. A crash is the same path.
Before resuming the engine re-probes: a changed ETag / Last-Modified / size is
`SOURCE_CHANGED`; a server that stopped honouring ranges is `RANGE_NOT_SUPPORTED`; a
missing `.mdm.part` is `IO`. All three mean "start over" to the caller. A fresh start
discards any leftover `.mdm.part` of the same name; only a resume reuses it.

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
