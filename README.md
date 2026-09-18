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
