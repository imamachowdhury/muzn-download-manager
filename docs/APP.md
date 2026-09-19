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

## The wiring test

`src-tauri/tests/commands.rs` drives `commands::handler()` — the exact list `run()` registers,
nothing smaller — through `tauri::test`'s mock runtime, with the same camelCase JSON
`src/api/tauri.ts` sends. It proves three things the UI depends on: add / list / segments /
remove round-trip, failures arrive as `{code, message}`, and `set_settings` returns the core's
clamped values (a UI-visible contract, not just a Rust type check). A renamed command or
argument fails this test, not the owner's window.

On Windows, `cargo test` builds its own binaries, and `tauri_build::build()`'s manifest
embedding only reaches `[[bin]]` targets (`cargo:rustc-link-arg-bins`) — never a test binary.
Without the Common Controls v6 manifest that gives `mdm.exe` an activated `comctl32.dll`, the
old system one is missing `SetWindowSubclass` / `DefSubclassProc` / `TaskDialogIndirect`: any
test binary that reaches the dialog plugin (as `handler()` does, through `pick_folder`) fails
to start at all — `STATUS_ENTRYPOINT_NOT_FOUND`, before `main()` runs. `src-tauri/build.rs`
embeds the same manifest into test binaries too (`embed_resource::compile_for_tests`).
