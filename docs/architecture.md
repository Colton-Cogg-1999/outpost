# Architecture

OUTPOST is a Tauri 2 application: a Rust process owns everything native, and a React/TypeScript interface runs in the system webview (WebKitGTK on Linux). There is no server, no account, and no background network activity.

```
┌──────────────────────────── webview (React + TS) ────────────────────────────┐
│ App shell · Analyze · Games · Tactics · Engines · AI · Settings · Import      │
│ chess/tree.ts  game tree, variations, PGN (chessops)                          │
│ board/Board.tsx  custom board: drag, click, promotion, arrows, annotations    │
│ vision/detect.ts  experimental screenshot → position (local, no inference)    │
│ store.ts  zustand state · lib/ipc.ts typed commands + Channels                │
└──────────────────────────────┬────────────────────────────────────────────────┘
                               │ Tauri IPC: invoke() + Channel streams
┌──────────────────────────────┴────────────── Rust (src-tauri/src) ───────────┐
│ commands.rs   every command the UI may call; input validation; job cancel     │
│ chess/        shakmaty wrappers, SEE, PGN parse + normalise (pgn-reader)      │
│ engine/       uci.rs parser · process.rs spawn/handshake · actor.rs search     │
│               state machine · mod.rs registry + manager · discover · builtin  │
│ analysis/     classify.rs heuristics · review.rs whole-game review + cache    │
│ games.rs      library, filters, dedup  · import/ chesscom.rs lichess.rs      │
│ tactics/      generator.rs + cook.rs (lichess-puzzler port, AGPL) · mod.rs   │
│               scan jobs, storage, move checking, spaced repetition            │
│ ai/           cli.rs (Claude Code, Codex) · http.rs (APIs, Ollama) ·          │
│               context.rs (validated prompt context, move checking)            │
│ db.rs         SQLite + migrations   · secrets.rs OS keyring   · paths.rs     │
└───────────────────────────────────────────────────────────────────────────────┘
```

## Who is authoritative

- **Rules**: the frontend uses `chessops` for instant legal moves on the board. The backend uses `shakmaty` (same author, same rules) to validate everything that crosses a trust boundary: FENs before they reach an engine, imported PGN, tactic moves, and moves in AI answers.
- **Notation**: PGN imported into the library is replayed move by move and *regenerated* from the validated moves, keeping variations, comments and NAGs, so stored notation can never disagree with the board.
- **Evaluations**: only an engine produces evaluations. AI prompts receive engine lines that the backend re-derives from UCI moves; lines with illegal moves are dropped.

## Engines

Each running engine is one tokio task (`engine/actor.rs`) owning one subprocess, started directly with no shell and no arguments. The actor's guarantees:

- A new request sends `stop`, waits for the old search's `bestmove`, and only then sends `position` + `go`. Output from a superseded search is dropped and never forwarded.
- Every event carries the request's `searchId`, and the UI also ignores events for positions it no longer shows.
- Requests are replayed with shakmaty first. Illegal moves or positions are rejected before the engine sees them (Stockfish 19 terminates on invalid input). Finished positions (mate, stalemate) are answered without the engine.
- Info lines are aggregated per MultiPV and flushed at most every 120 ms. Scores are stored both raw (side to move) and from White's view. Aspiration-window bound lines are ignored.
- An engine that ignores `stop` for 8 s is killed. An engine that exits is reported, and the next request restarts it.
- On application exit every engine gets `stop` + `quit` and is killed if it doesn't comply. `kill_on_drop` is the backstop.
- Option names and values are stripped of control characters, so a value cannot inject a second UCI command.

Live analysis uses one long-lived process per engine. Game review starts a **dedicated** process, so it never shares hash or state with live analysis. Comparison mode runs a second engine's own process and labels its lines separately, keeping its raw scale and depth.

Finished searches at depth ≥ 12 and every review evaluation are cached in `evals`, keyed by position (FEN without move counters) and engine name. Cached results are shown immediately and replaced when the live search goes deeper.

## Data

SQLite in `$XDG_DATA_HOME/outpost/outpost.db` (`OUTPOST_DATA_DIR` overrides). Migrations are an append-only list in `db.rs`, tracked with `PRAGMA user_version`. The tables are `games`, `accounts`, `analyses`, `evals`, `engines`, `tactics`, `tactic_attempts`, `tactic_scans`, `chats`, `chat_messages`, and `settings`. Migration 2 dropped the old Lichess puzzle tables.

Duplicate detection for games: the unique key is `(source, source_id)` for API imports, plus a SHA-256 fingerprint over the start position, players, date and time, and the exact move sequence. A pasted copy of a game later imported from an API is upgraded in place rather than duplicated.

## Network

Network requests happen only when the user asks, and only to:

- `api.chess.com` and `lichess.org`: user-initiated imports and syncs. Requests are serial, carry an identifying User-Agent, and honour 429 and `Retry-After`; Lichess gets a 60 s pause after a 429.
- At build time only, `scripts/build-engine` fetches the engine source (`github.com/nmrugg/stockfish.js`, a pinned commit) and its network (`tests.stockfishchess.org`, a pinned SHA-256). The app itself never downloads executables.
- The AI provider the user configured, and only when they ask a question.

## Security notes

- Capabilities (`src-tauri/capabilities/default.json`) grant only window controls, open/save dialogs and plain-text clipboard. There is no shell plugin, and neither the fs nor the http plugin is exposed to the webview.
- The CSP allows only bundled scripts, styles, fonts and IPC.
- File commands accept only absolute paths with expected extensions and size limits. They are called with paths from native dialogs or native drops.
- API keys live in the OS credential store via `keyring`, never in SQLite or logs. Remote AI servers must use https (plain http only on loopback or private networks).
- The subscription CLIs (Claude Code, Codex) run with every tool disabled, in an empty working directory, with server-initiated approval requests declined. This is the same lock-down as OmaReader.

## Built-in engine

`engine/builtin.rs` looks for `outpost-stockfish` next to the OUTPOST executable. That is where Tauri bundles place sidecars, and where `scripts/build`, `scripts/install-user` and the PKGBUILD put it. In a checkout it falls back to `src-tauri/binaries/outpost-stockfish-<target>`. On startup the engine is probed with a UCI handshake and registered under a stable record (`managed = 2`). If it moved, the record follows it and keeps the user's options. It becomes the default unless the user chose another engine, and it cannot be removed from the UI. Default resources are small: a 16 MB hash and one thread.

## Tactics from your games

A scan (`tactics_scan`) launches a dedicated engine process and goes through the newest unscanned games:

1. **Review.** The game's saved review is reused. Otherwise `analysis::review::run` evaluates every position (cached in `evals`) and saves the review.
2. **Generate.** `tactics/generator.rs` is a port of lichess-puzzler's `generator.py`. It walks the mainline with those evaluations. Where a move swings the winning chances enough, it probes deeper with MultiPV 2. Mate lines must keep a forced mate. Advantage lines must keep at least +2. Every solver move must be clearly better than the second-best move. Two rule sets exist: `Lichess` (upstream's thresholds) and `Personal` (a 0.45 uniqueness margin and 0.5 swing, single decisive moves allowed, mates kept when already ahead). Forced moves and positions on an already-found solution are skipped, so one combination is one tactic.
3. **Tag.** `tactics/cook.rs` ports `cook.py`: mate patterns, fork, pin, skewer, discovered attack, deflection, attraction, sacrifice, endgame types, length and more. Its test cases are ported from upstream's `test.py`.
4. **Store.** Each tactic records the position before the mistake, the mistake plus the solution, the solver, and themes. It also records whether the library owner was the solver ("you") or made the mistake ("opponent"), using the game's account or synced account names, and what was actually played. Repeated tactical positions remain available in each originating game.

Training (`tactic_next` / `tactic_check` / `tactic_attempt`) serves due reviews first, then new tactics, with the owner's own chances first. The final move of a mate accepts any mating move. A clean solve schedules the next review in 1, 3, 7, 21, then 60 days; a miss brings it back in ten minutes.

## Diagnostics

Warnings, errors and uncaught frontend errors (including error toasts) are appended to `<data>/outpost.log`, truncated past 1 MB. `OUTPOST_SELFTEST=<report>` starts the app with its window hidden and runs `src/dev/selftest.ts` inside the real webview against the real backend, then exits.


## Streamlined game workflow and resource budget (2026-11-08)

Account history is a read-only operation (`games_history`), separate from `game_import_selected`. The frontend retains one page of at most 20 games, replacing it on navigation. Lichess uses a creation-time cursor in milliseconds within the selected month. Chess.com exposes monthly archives rather than a server-paged history endpoint, so each page currently fetches that month and returns only its 20 selected rows. Responses are capped at 16 MiB before JSON decoding; malformed or oversized responses fail without adding games. This bounds retained history, but a busy Chess.com month still costs a download and temporary decoding memory. A bounded disk cache keyed by account/month and HTTP validators would be a useful next improvement. See the [Lichess API](https://lichess-org.github.io/api/) and [Chess.com Published-Data API](https://www.chess.com/news/view/published-data-api).

Live analysis defaults to one PV, depth 16, one thread and a 16 MiB hash. Additional lines and infinite search remain available. Older UI defaults migrate once; existing custom engine thread/hash settings remain respected. Stale frontend callbacks are discarded on position/search changes. Pausing analysis or leaving Analyze quits the managed interactive processes, releasing their networks, hash tables and thread state. Puzzle scans stop live analysis before launching their dedicated engine, then quit that engine when finished. A completed depth-limited search keeps its process warm while Analyze remains open; it is not a zero-memory idle state.

Measured on this Linux machine using the bundled native Stockfish Lite binary, starting position, a three-second search and 16 MiB hash:

| Threads | Sampled peak engine RSS |
| --- | --- |
| 1 | 56.6 MiB |
| 4 | 121.1 MiB |

Reproduce with `python3 scripts/measure-engine-memory.py [engine-path]`. This samples `/proc/<pid>/status` every 20 ms, so it can miss brief peaks. These figures describe the engine process only, not total application memory or a guarantee for other positions. No whole-app RSS reduction is claimed from this measurement.

The app also loads tactics, import/image recognition, engine management, AI settings and general settings on demand. The production entry JavaScript dropped from about 399 kB to 353 kB uncompressed in this pass; bundle size is not the same as RAM saved. Puzzle navigation retains only five recent IDs and clears expired timer references.

Near-zero total resident memory is not achievable with a running WebKit interface and a searching chess engine. Browser Stockfish still needs a network, hash and search working memory; moving those allocations into a WebAssembly worker changes where they are accounted for. This app already uses the system WebKit runtime rather than shipping Chromium. The largest demonstrated saving here comes from reducing thread count. Further work should measure the complete process tree (including WebKit/GPU processes), ideally PSS as well as RSS to avoid double-counting shared pages, in idle, search, import and puzzle-generation states. Offloading search could reduce local engine RAM, but would add server dependence, latency and position disclosure. A native-widget rewrite could reduce webview overhead but would be a substantial separate project.

Current-game puzzle generation saves the exact current PGN and passes a single explicit game ID. Training filters by that ID. Mainline edits invalidate stored reviews, tactics and scan markers; cached reviews are additionally checked against every replayed position. Repeated tactical positions remain associated with each originating game so scanning one game cannot silently hide its tactics because another game was scanned first.


The Analyze page trains current-game puzzles inline, pausing live engine hints until the player returns to the game. The standalone Puzzles page opens with twelve original basic mate exercises and can generate a library collection. Batch selection skips all completed scan markers, including zero-result scans and older generator versions; interrupted games without a completed marker remain eligible. Mainline edits remove the marker so the changed game can be processed again.

Agent configuration lives in Settings. The board's chat sends only typed questions and retains conversation history while navigating positions or application views. Scrollback does not follow incoming text while the reader is scrolled up. Library clearing requires confirmation, refuses to run during active jobs, and deletes game-linked reviews and puzzles while preserving settings and saved conversations.
