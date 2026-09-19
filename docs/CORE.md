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

- **Runtime:** `Manager::open` / `with_store` must be called once from inside a tokio runtime; the
  manager keeps that runtime's handle and spawns every download on it, so every other method may be
  called from any thread, inside a runtime or not (Tauri's setup hook and sync commands).
- **Queue:** FIFO by creation time, at most `max_parallel` (default 3) running.
- **Probe without adding:** `probe(url, referrer)` asks the server about a URL and returns a
  `ProbePreview` (final URL, file name, size, `resumable`, MIME) — the add dialog's live preview.
  Same URL rules and engine error codes as `add`; nothing is stored.
- **File names from callers** (`NewDownload.filename`) are sanitised on `add`
  (`mdm_engine::filename::sanitize`, and again by the engine): `../x`, `/abs/x` or `C:\x` can never
  write outside the download folder. A blank name means "use the probed one".
- **Driving a download:** PROBING → the engine starts (resuming when the row has a size and saved
  segments) → DOWNLOADING → COMPLETED or FAILED with the engine's error code. The durable segment
  snapshot is saved every second (only bytes that reached the disk).
- **Pause / resume / cancel / remove / restart:** a running download is reached through its engine
  control; a waiting one only changes state. Cancel deletes the partial data; remove deletes the row
  (and the finished file only on request); restart starts from byte 0. A COMPLETED or CANCELLED
  row owns no part file any more (renamed, or deleted at cancel time) and is not reserved, so a
  newer download of the same name may own `<name>.mdm.part`: removing (or restarting) such a row
  clears its saved segments only and never touches a part file.
- **Control signals rank:** REMOVE beats CANCEL beats SHUTDOWN beats PAUSE beats NONE — a signal
  only ever raises a slot's intent to a higher rank, so a later, weaker request (say a PAUSE after
  a CANCEL already won) can never override the earlier, stronger one. Every stop request (pause,
  cancel, remove, shutdown) also cancels any pending resume on that slot, since asking to stop is
  not compatible with a resume that arrived first. Resuming a download that is running normally —
  no stop signal pending — is a no-op: it must not silently re-queue a download that later fails on
  its own.
- **One lock, no races:** every control call and the scheduler serialize on the same `running`
  lock, so a `pause` of a download the scheduler is about to start, and the scheduler itself, can
  never interleave — one always finishes before the other looks.
- **Exclusive part names, kept across a settings change:** a fresh start never reuses or deletes
  the part file of another unfinished download in the same folder — the manager passes those other
  downloads' part files to the engine as `reserved`, and the engine names the new one `name
  (1).ext` instead of colliding with them. A settings change (`set_settings`) builds a new engine
  but shares the old one's registry of live part-file claims, so a download already running keeps
  its claim through the change.
- **Crash and quit:** at launch, rows left PROBING / DOWNLOADING go back to QUEUED and continue from
  their saved segments. `shutdown()` pauses and saves every running download and queues it for the
  next launch.
- **Starting over by itself, once:** a resume refused because the server lost range support, the saved
  state does not fit (`INVALID_RESUME`) or the part file is gone — and a download whose server stops
  honouring ranges mid-way — restart from byte 0 with a `Notice`. The mid-way case restarts as one
  plain GET (`DownloadSpec::single_stream`), because the probe already claimed ranges once and a
  segmented retry would fail the same way. A changed file (`SOURCE_CHANGED`) never does: it waits in
  FAILED for the user's `restart`. Nor does a resume whose part file another live download is
  writing (`PART_IN_USE`): the row fails with that code and nothing is deleted.
- **Retry, inside a segment** (owner, 2026-09-18): the engine fails a segment after ten attempts in
  a row that wrote no bytes; any attempt that makes progress resets that count and the backoff. The
  manager never retries on its own — a FAILED row is always the engine giving up on a segment, or a
  probe/spec error.
- **Durable progress:** the engine fsyncs the part file once a second while downloading, and once
  more at a pause or a failure, so `Outcome::Paused` / `Outcome::Failed` carry only the segments
  that survived that fsync — never bytes still sitting in the OS page cache. The manager saves that
  durable snapshot to the store at most once a second (`PERSIST_INTERVAL`), and only when it
  changed, so a crash loses at most a second or so of a running download's progress.
- **A panicking driver** fails its row with INTERNAL and frees its queue slot; it never leaks the
  slot. A finished download whose size cannot be read afterwards stays COMPLETED with an unknown size.

## Events

`Added`, `Updated` (the whole row after any change), `Progress` (bytes, speed, ETA, segment map,
~4 per second per running download), `Removed`, `Notice`. JSON: `{"type": "progress", "id": …,
"speedBps": …}`.

## Tests

`cargo test -p mdm-core` — the store against in-memory and file databases; the manager against the
in-process test server (`crates/mdm-test-server`), including a simulated crash and a relaunch.
