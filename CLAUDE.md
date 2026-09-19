# Muzn Download Manager — working rules for agents

- Spec: docs/superpowers/specs/2026-09-18-muzn-download-manager-design.md. Plans: docs/superpowers/plans/.
- How the app is wired: docs/APP.md; the core: docs/CORE.md; the engine: docs/ENGINE.md.
- Product name "Muzn Download Manager" in user-facing text; identifiers `mdm`. UI English only. MIT.
- crates/mdm-engine and crates/mdm-torrent never depend on Tauri, SQLite or UI code.
- Gates before any commit: `pnpm build` (src-tauri embeds dist/), `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`,
  `pnpm typecheck`, `pnpm lint`, `pnpm test`.
- Never weaken a test to pass. New behaviour gets a regression test naming the decision and date.
- Edit source with the editor tools, never shell one-liners.
- Owner writes Banglish → answer in Banglish; code, comments and UI stay English.
- No new subsystem (video sniffing, FTP, scheduler, categories…) without an explicit owner prompt — see the spec's "Out" list.
