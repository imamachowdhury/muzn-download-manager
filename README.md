# Muzn Download Manager

An open-source download manager: multi-connection segmented HTTP downloads with
pause / resume, a browser extension that hands downloads to the app, and BitTorrent.
One small desktop app for Windows, Linux and macOS (Tauri v2, Rust engine, React UI).

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

## License

MIT
