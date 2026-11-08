# Contributing

Patches, bug reports and ideas are welcome. A small reproducible example (a PGN, a FEN, an engine and its version) beats a description.

## Setup

```sh
# Arch / Omarchy
sudo pacman -S --needed base-devel rust nodejs npm webkit2gtk-4.1 gtk3 libsoup3
npm ci
npx tauri dev          # live-reloading app
```

With `mise`, `mise.toml` pins the toolchains (`rust = "stable"`, `node = "26"`).

## Tests

```sh
./scripts/test                                         # type-check, Vitest, cargo test
OUTPOST_TEST_STOCKFISH=/path/to/stockfish ./scripts/test # adds the real-engine tests
```

- Rust unit tests sit next to the code. Integration tests are in `src-tauri/tests/`:
  - `engine.rs` drives a scripted fake UCI engine (`tests/fixtures/fake-uci.sh`) for handshake, stale results, stop, crash and option cases, plus real Stockfish when available.
  - `review.rs` runs a whole-game review with real Stockfish.
  - `tactics.rs` generates tactics from a game with a real engine. Its opt-in `library_scan` test scans a copy of a real library (`OUTPOST_TACTICS_DB`).
  - `live_import.rs` hits the real Lichess and Chess.com APIs when `OUTPOST_LIVE_USER` is set.
- API import tests use recorded-shape fixtures in `src-tauri/tests/fixtures/` with fictional players. Tests never contact Lichess or Chess.com.
- Frontend tests (`src/**/*.test.ts(x)`) cover the board, the game tree and PGN, navigation state, review mapping, and screenshot recognition on synthetic images.

## UI previews

`node scripts/preview-shots.mjs <dir> [width height scale]` renders the interface in headless Chromium with **mocked** native calls (`src/dev/mock.ts`, loaded only by `preview.html`) and saves screenshots. It also checks drag-and-drop. Engine lines and AI text in previews are fixture placeholders. Never present them as real analysis.

## Ground rules

- Nothing leaves the machine without a user action. No telemetry, no analytics, no remote fonts.
- Engines and the rules library are authoritative. Never display an evaluation or a move the engine or rules library did not produce or validate.
- No shell interpolation. Engines and CLIs are spawned directly with fixed arguments.
- Database changes are new entries in `MIGRATIONS` (`src-tauri/src/db.rs`). Never edit a shipped migration.
- New dependencies need a GPL-3.0-compatible license. Update `THIRD_PARTY_NOTICES.md`.
- Keep the look: phosphor green, thin lines, pixel type for labels, very little colour. Readability first.

By contributing you agree your contribution is licensed under GPL-3.0-or-later.
