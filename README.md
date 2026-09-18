# Muzn Download Manager

An open-source download manager: multi-connection segmented HTTP downloads with
pause / resume, a browser extension that hands downloads to the app, and BitTorrent.
One small desktop app for Windows, Linux and macOS (Tauri v2, Rust engine, React UI).

**Status:** the download engine (`crates/mdm-engine`) is complete and tested — segmented
downloads, pause / resume / crash recovery, retries, work stealing. Try it:

    cargo run -p mdm-engine --example fetch -- https://example.com/big.iso

The desktop app, browser extension and torrent support follow (see `docs/superpowers/`).

## Build

    cargo test --workspace

## License

MIT
