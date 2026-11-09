# Third-party software, data and notices

OUTPOST's own code is licensed under the **GNU General Public License v3.0 or later** (see `LICENSE`). That choice follows from its dependencies: the chess rules libraries it links (`shakmaty`, `pgn-reader`, `chessops`) are GPL-3.0-or-later.

The tactic generator (`src-tauri/src/tactics/generator.rs`, `cook.rs`, `util.rs`) is a port of [lichess-puzzler](https://github.com/ornicar/lichess-puzzler) (© Thibault Duplessis and contributors) and stays under the **GNU Affero General Public License v3.0 or later** (`licenses/AGPL-3.0.txt`). GPL-3.0 §13 allows combining the two. The combined program is therefore distributed with AGPL-3.0's network-interaction terms applying to it as a whole. For a desktop app nobody runs as a network service, that changes nothing in practice. If you build a server from this code, offer its source to its users.

The audit below was made on 2026-11-07 against the locked dependency tree (`Cargo.lock`, `package-lock.json`), counting normal and build dependencies for x86_64 Linux. Regenerate it after changing dependencies. Commands are at the end.

## Chess engine (a separate program shipped with OUTPOST)

| Program | License | How OUTPOST uses it |
| --- | --- | --- |
| Stockfish 19 Lite, built from [stockfish.js](https://github.com/nmrugg/stockfish.js) (commit `0dc3d0ac03705353dad6659c0dad4597cdee4021`), a fork of [Stockfish](https://github.com/official-stockfish/Stockfish) by the Stockfish developers, maintained by Nathan Rugg | GPL-3.0 | `scripts/build-engine` fetches that pinned commit and compiles it natively with `LITE_NET=yes`, so only the small network is used. The result is installed next to the OUTPOST binary as `outpost-stockfish`, with `outpost-stockfish.COPYING.txt`, `.AUTHORS.txt` and `.SOURCE.txt`. OUTPOST talks to it over UCI as a separate process. The engine identifies itself as "Stockfish 19 Lite" to mark that it is modified (GPL-3.0 §5a). |
| Any other UCI engine (Lc0, full Stockfish, Berserk, …) | Their own | Users register executables they installed themselves. |

Packages that include the engine meet GPL-3.0 §6 by pointing to the exact corresponding source: the commit above plus the network file below. `outpost-stockfish.SOURCE.txt` records both.

## Chess pieces

The default board silhouettes in `public/pieces/reference/` are reconstructed as SVG paths from the supplied `OUTPOST Chess Analysis Dashboard.png` visual reference.

The project also includes the Cburnett SVG piece set by Colin M.L. Burnett, obtained from [lichess-org/lila](https://github.com/lichess-org/lila/tree/a1590345c9c3a31b6f9c6705eefeeb7dab32a78e/public/piece/cburnett) at commit `a1590345c9c3a31b6f9c6705eefeeb7dab32a78e`. Lichess records the set as GPLv2 or later in its [COPYING.md](https://github.com/lichess-org/lila/blob/a1590345c9c3a31b6f9c6705eefeeb7dab32a78e/COPYING.md); OUTPOST distributes the SVGs under GPLv3 or later, with cream and charcoal palette adjustments. Their source is in `public/pieces/cburnett/` and the GPLv3 license text is in `LICENSE`.

## Data

| Data | License | Notes |
| --- | --- | --- |
| Stockfish network `nn-61e7af4bb97d.nnue` (1.1 MB) | GPL-3.0 (as part of Stockfish) | Downloaded at build time from the Stockfish project's network server (`tests.stockfishchess.org`), checked against a pinned SHA-256, and embedded in the engine binary. |

OUTPOST ships no puzzle database. Tactics are generated locally from the user's own games.

## Fonts (bundled in the interface)

The fonts are under the SIL Open Font License 1.1, delivered through Fontsource. Full license texts are in `licenses/`.

- **Noto Sans**: Copyright The Noto Project Authors.
- **IBM Plex Mono**: Copyright 2017 IBM Corp.

## Rust crates (431, linked into the binary)

| License expression | Crates |
| --- | --- |
| MIT and/or Apache-2.0 (incl. dual and triple forms with Zlib/0BSD/BSL/ISC/Unlicense) | ~395 |
| Unicode-3.0 (ICU4X data and support crates) | 18 |
| BSD-2/3-Clause, ISC, Zlib (sole or combined) | ~13 |
| MPL-2.0: `cssparser`, `cssparser-macros`, `dtoa-short`, `selectors`, `option-ext` | 5 |
| CDLA-Permissive-2.0: `webpki-roots` (certificate data) | 1 |
| GPL-3.0-or-later: `shakmaty`, `pgn-reader` | 2 |

All of these are compatible with distribution under GPL-3.0-or-later and AGPL-3.0-or-later. MPL-2.0 files stay under MPL-2.0 (§3.3 permits combination with GPL); their source is available from crates.io.

Direct dependencies: `tauri`, `tauri-plugin-dialog`, `tauri-plugin-clipboard-manager` (MIT/Apache-2.0); `shakmaty`, `pgn-reader` (GPL-3.0-or-later); `rusqlite` with bundled SQLite (MIT; SQLite is public domain); `reqwest` + `rustls` (MIT/Apache-2.0/ISC); `tokio`, `serde`, `serde_json`, `anyhow`, `thiserror`, `futures-util`, `sha2`, `hex`, `uuid`, `tracing`, `tracing-subscriber` (MIT and/or Apache-2.0); `keyring` (MIT/Apache-2.0).

## JavaScript packages (shipped in the interface bundle)

| Package | License |
| --- | --- |
| chessops 0.15.1 | GPL-3.0-or-later |
| @badrap/result 0.3.1 | MIT |
| react 19.3.1, react-dom 19.3.1, scheduler 0.28.0 | MIT |
| zustand 5.1.0 | MIT |
| @tauri-apps/api 2.12.4, plugin-dialog 2.8.4, plugin-clipboard-manager 2.5.0 | MIT OR Apache-2.0 |
| @fontsource/noto-sans, silkscreen, vt323, ibm-plex-mono | OFL-1.1 (fonts) |

Development-only tools (not shipped): Vite, TypeScript, Vitest, jsdom, Testing Library, Playwright (MIT / Apache-2.0).

## Trademarks and affiliation

"Lichess" and "Chess.com" are used only to name the services OUTPOST can import public games from, through their documented public APIs. OUTPOST is not affiliated with, endorsed by, or sponsored by either. No Lichess or Chess.com logos or classification symbols are used. The icons, classification glyphs and logo are original; the chess pieces are credited above. "Claude", "ChatGPT", "Codex", "Gemini" and "Ollama" name optional AI providers the user may connect to with their own accounts.

## Regenerating this audit

```sh
cd src-tauri && cargo metadata --format-version 1 --filter-platform x86_64-unknown-linux-gnu > /tmp/meta.json
# then list packages reachable from `outpost` through normal/build edges, grouped by `license`
node -e '/* walk package.json "dependencies" from the root; print name, version, license */'
```
