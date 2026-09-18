# mdm-engine

The HTTP download engine. A library: no Tauri, no database, no UI.

## Flow

probe → plan → allocate → fetch (N workers) → complete

- **probe** (`probe.rs`): HEAD, or GET `Range: bytes=0-0`. Size, range support, ETag,
  Last-Modified, MIME, file name. Redirects followed; `final_url` is what workers fetch.
- **plan** (`plan.rs`): `min(max_connections (≤ 32), ceil(size / 1 MiB))` contiguous segments.
- **allocate** (`file.rs`): `<name>.mdm.part`, pre-sized. Positioned writes; no merge step.
- **fetch** (`segment.rs`): one worker per segment. `Range: bytes=<next>-<end>`, expects 206
  with a matching `Content-Range`. Transient errors back off 1, 2, 4 … 60 s. Only attempts that
  write no bytes count against the budget: ten of those in a row fail the segment, but any attempt
  that makes progress resets both the count and the backoff (owner, 2026-09-18).
  30 s (`stall_timeout`) without bytes, or without response headers, = reconnect; the
  probe's HEAD and GET wait at most as long. 4xx except 429 = fail at once; 429 and 5xx back off like
  network errors. A single stream (server without range support) restarts from byte 0 on
  every retry — a plain GET always answers from the beginning.
  Writes and fsyncs run on blocking threads.
- **complete** (`download.rs`): fsync, rename, `name (1).ext` on a clash.

Cookies and `Authorization` are dropped when a redirect leaves the original origin;
`Range`, `Host` and `Content-Length` from the caller are never sent.

A second live download of the same name gets `name (1).ext`; so does a fresh start whose chosen
name is in `DownloadSpec::reserved` — the caller's part-file paths of its other, not-yet-running
downloads, which a fresh start must neither take nor delete. A resume of a part file already
claimed by another live download is refused (`INVALID_RESUME`) instead, since its name is fixed.

## Progress, pause, resume

`Engine::start` returns a `DownloadHandle`: `subscribe()` is a `watch` channel of
`Progress` (250 ms cadence, 2 s speed window). `pause()` ends the task with
`Outcome::Paused(segments)`; the caller stores them and later calls `start` again with
`resume_from: Resume { segments, size, etag, last_modified }`. A crash is the same path.
Persist `durable_segments` — the segments as of the last fsync (every second, and once
more at a pause or failure). `segments` may count bytes still in the OS cache.
Dropping a `DownloadHandle` without `wait()` pauses its download (the task stops, the
part file stays) — it never keeps running unowned.
`handle.control()` gives a cloneable pause / cancel for use while another task awaits
`wait()`. A pause that arrives after the last byte still completes the file.
Before resuming the engine re-probes: a changed ETag / Last-Modified / size is
`SOURCE_CHANGED`; a server that stopped honouring ranges is `RANGE_NOT_SUPPORTED`; a
missing `.mdm.part` is `IO`; segments that do not run contiguously over `[0, size)`, or a
part file whose length is not `size`, are `INVALID_RESUME`. All four mean "start over" to
the caller. A fresh start discards any leftover `.mdm.part` of the same name; only a
resume reuses it.

## Work stealing

When a worker finishes, the largest segment with more than 2 MiB left is split at its
midpoint; the freed connection takes the second half. Segment `end` is atomic, so the
victim notices and stops. This is why the last 10 % does not crawl on one connection.

## Errors

`EngineError::code()` is stable: `INVALID_URL`, `RANGE_NOT_SUPPORTED`, `SOURCE_CHANGED`,
`DISK_FULL`, `HTTP_STATUS`, `NETWORK`, `TLS`, `CANCELLED`, `INVALID_RESUME`, `IO`,
`INTERNAL`.

## Tests

`cargo test -p mdm-engine`. Integration tests run an in-process axum server
(`tests/support/mod.rs`) with switches: `ranges`, `head_allowed`, `fail_first` (503s),
`drop_after`, `hang_first`, `hang_headers`, `chunk_delay_ms`, `etag`, `content_disposition`. Every
download test ends by comparing SHA-256 of the result with the served bytes.

## Try it

    cargo run -p mdm-engine --example fetch -- <url> [dir] [connections]
