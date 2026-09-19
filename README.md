# Muzn Download Manager

An open-source download manager: multi-connection segmented HTTP downloads with
pause / resume, a browser extension that hands downloads to the app, and BitTorrent.
One small desktop app for Windows, Linux and macOS (Tauri v2, Rust engine, React UI).

**Status:** the download engine (`crates/mdm-engine`) and the download core (`crates/mdm-core`:
SQLite store, queue, pause / resume / cancel, crash recovery) are complete and tested. The desktop
app, browser extension and torrent support follow (see `docs/superpowers/`). Try the engine:

    cargo run -p mdm-engine --example fetch -- https://example.com/big.iso

## Build

    cargo test --workspace

## License

MIT
