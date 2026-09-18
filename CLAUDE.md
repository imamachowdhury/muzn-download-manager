# Muzn Download Manager — working rules for agents

- Spec: docs/superpowers/specs/2026-09-18-muzn-download-manager-design.md. Plans: docs/superpowers/plans/.
- Product name "Muzn Download Manager" in user-facing text; identifiers `mdm`. UI English only. MIT.
- crates/mdm-engine and crates/mdm-torrent never depend on Tauri, SQLite or UI code.
- Gates before any commit: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
  `cargo test --workspace`; JS (once present): `pnpm tsc --noEmit`, `pnpm eslint . --max-warnings=0`, `pnpm vitest run`.
- Never weaken a test to pass. New behaviour gets a regression test naming the decision and date.
- Edit source with the editor tools, never shell one-liners.
- Owner writes Banglish → answer in Banglish; code, comments and UI stay English.
- No new subsystem (video sniffing, FTP, scheduler, categories…) without an explicit owner prompt — see the spec's "Out" list.
