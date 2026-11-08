// DEVELOPMENT ONLY. Loaded by preview.html (never by index.html or the app bundle) to
// render the interface in an ordinary browser for layout work and screenshots.
// Every value here is fixture data; the real app gets all of it from the Rust backend.
import { mockIPC, mockWindows } from '@tauri-apps/api/mocks';
import { positionFromFen, fenOf, uciOf, START_FEN } from '../chess/position';
import { makeSan } from 'chessops/san';
import { parseUci } from 'chessops/util';
import type { NormalMove } from 'chessops/types';
import type { Chess } from 'chessops/chess';

type Ch = { onmessage: (m: unknown) => void };

function replay(fen: string | null, moves: string[]): Chess {
  const pos = positionFromFen(fen ?? START_FEN)!;
  for (const m of moves) {
    const mv = parseUci(m) as NormalMove;
    pos.play(mv);
  }
  return pos;
}

// Deterministic pseudo-evaluation so the preview is stable between runs.
function candidates(pos: Chess, n: number) {
  const out: { uci: string; san: string; score: number }[] = [];
  for (const [from, dests] of pos.allDests()) {
    for (const to of dests) {
      const move = { from, to } as NormalMove;
      const piece = pos.board.get(from)!;
      if (piece.role === 'pawn' && ((to >> 3) === 7 || (to >> 3) === 0)) move.promotion = 'queen';
      const target = pos.board.get(to);
      let s = ((from * 7 + to * 13) % 23) - 8;
      if (target && target.color !== pos.turn) s += 30;
      if (piece.role === 'knight' || piece.role === 'bishop') s += 6;
      if ([27, 28, 35, 36].includes(to)) s += 8;
      out.push({ uci: uciOf(pos, move), san: makeSan(pos, move), score: s });
    }
  }
  return out.sort((a, b) => b.score - a.score).slice(0, n);
}

function pvFrom(pos: Chess, first: string, len: number): { uci: string[]; san: string[] } {
  const p = pos.clone();
  const uci: string[] = [];
  const san: string[] = [];
  let next: string | undefined = first;
  for (let i = 0; i < len && next; i++) {
    const mv = parseUci(next) as NormalMove;
    san.push(makeSan(p, mv));
    uci.push(next);
    p.play(mv);
    next = candidates(p, 1)[0]?.uci;
  }
  return { uci, san };
}

const engines = [
  { id: 'sf', name: 'Stockfish 19 Lite', path: '/usr/bin/outpost-stockfish', author: 'the Stockfish developers (see AUTHORS file)', options: {}, availableOptions: [{ name: 'Threads', kind: 'spin', default: '1', min: 1, max: 1024, vars: [] }, { name: 'Hash', kind: 'spin', default: '16', min: 1, max: 33554432, vars: [] }, { name: 'Skill Level', kind: 'spin', default: '20', min: 0, max: 20, vars: [] }], managed: false, builtin: true, createdAt: 0, running: true, default: true, exists: true },
  { id: 'lc0', name: 'Lc0 v0.32.0', path: '/usr/bin/lc0', author: 'The LCZero Authors', options: { WeightsFile: '/home/user/nets/BT4.pb.gz' }, availableOptions: [{ name: 'WeightsFile', kind: 'string', default: '<autodiscover>', min: null, max: null, vars: [] }, { name: 'Backend', kind: 'combo', default: 'cuda-auto', min: null, max: null, vars: ['cuda-auto', 'eigen', 'blas'] }], managed: false, builtin: false, createdAt: 1, running: false, default: false, exists: true },
];

const timers: number[] = [];

export function installMocks() {
  mockWindows('main');
  let aiSettings = { active: 'claude', providers: { claude: { model: 'sonnet' } }, skillLevel: 'club player (~1500)', saveChats: false, length: 'normal' };
  mockIPC(async (cmd, args: any) => {
    switch (cmd) {
      case 'app_info':
        return { version: '0.1.0', dataDir: '/home/user/.local/share/outpost', os: 'linux', arch: 'x86_64', cores: 12, engineDefaults: { threads: 6, hashMb: 256 } };
      case 'settings_get':
        return { engineDefaults: { threads: 6, hashMb: 256 }, review: { depth: 16, movetimeMs: null, multipv: 2, classify: { bestTolerance: 1, inaccuracy: 5, mistake: 10, blunder: 15, greatGap: 10, decidedAbove: 95, sacrificeMin: 2, brilliantMaxLoss: 2, brilliantMinAfter: 45 } }, ai: aiSettings };
      case 'settings_set':
        if (args.key === 'ai') aiSettings = args.value;
        return null;
      case 'engines_list':
        return engines;
      case 'analysis_stop':
        timers.splice(0).forEach(clearTimeout);
        return null;
      case 'analysis_start': {
        const { request, onEvent, engineId } = args as { request: any; onEvent: Ch; engineId: string };
        const pos = replay(request.fen, request.moves);
        const fen = fenOf(pos);
        const cands = candidates(pos, request.multipv);
        const white = pos.turn === 'white';
        const base = engineId === 'lc0' ? 22 : 34;
        [8, 14, 18, 22].forEach((depth, k) => {
          timers.push(window.setTimeout(() => {
            const lines = cands.map((c, i) => {
              const cp = (base - i * 6 - (3 - k)) * (white ? 1 : 1);
              const pv = pvFrom(pos, c.uci, 10);
              return { multipv: i + 1, depth, score: { kind: 'cp', value: cp }, rawScore: { kind: 'cp', value: white ? cp : -cp }, wdl: [120 - i * 10, 840, 40 + i * 10], san: pv.san, uci: pv.uci };
            });
            const snapshot = { searchId: request.searchId, engine: engineId === 'lc0' ? 'Lc0 v0.32.0' : 'Stockfish 19', fen, depth, seldepth: depth + 9, nodes: 61_000 * 2 ** depth / 16, nps: 1_450_000, timeMs: depth * 90, hashfull: depth * 12, lines, terminal: null, cached: false };
            onEvent.onmessage({ type: k === 3 && request.limit?.depth ? 'done' : 'update', snapshot, bestMove: cands[0]?.uci ?? null });
          }, 120 + k * 260));
        });
        return engineId === 'lc0' ? 'Lc0 v0.32.0' : 'Stockfish 19';
      }
      case 'review_start': {
        const { moves, startFen, onEvent } = args;
        const classes = ['best', 'best', 'best', 'best', 'great', 'good', 'brilliant', 'best', 'inaccuracy', 'good', 'mistake', 'best', 'blunder', 'best', 'best', 'good'];
        const evals = [22, 28, 21, 26, 21, 24, 36, 30, 10, 14, -24, -20, -118, -100, -92, -95, -90];
        const positions: unknown[] = [];
        let pos = replay(startFen, []);
        const reviews: Record<string, unknown>[] = [];
        for (let i = 0; i <= moves.length; i++) {
          const cp = evals[i] ?? -90;
          const best = candidates(pos, 2);
          positions.push({ fen: fenOf(pos), depth: 16, lines: best.map((b, j) => ({ multipv: j + 1, depth: 16, score: { kind: 'cp', value: cp - j * 9 }, rawScore: { kind: 'cp', value: cp }, wdl: null, san: [b.san], uci: [b.uci] })), terminal: null, cached: i < 4 });
          if (i < moves.length) {
            const mv = parseUci(moves[i]) as NormalMove;
            const san = makeSan(pos, mv);
            const cls = classes[i] ?? 'good';
            const after = evals[i + 1] ?? -90;
            reviews.push({ ply: i + 1, color: i % 2 ? 'black' : 'white', san, uci: moves[i], bestSan: cls === 'best' || cls === 'great' || cls === 'brilliant' ? san : best[0]?.san, bestUci: best[0]?.uci, bestLine: [best[0]?.san ?? ''], evalBefore: { kind: 'cp', value: cp }, evalAfter: { kind: 'cp', value: after }, classification: cls, winBefore: 53, winAfter: 51, winLoss: cls === 'blunder' ? 21.4 : cls === 'mistake' ? 11.2 : cls === 'inaccuracy' ? 6.1 : 0.4, cpLoss: Math.abs(cp - after), accuracy: 90, reasons: cls === 'brilliant' ? ['Sound sacrifice: offers about 2 pawn(s) of material by static exchange, yet keeps 55% expected score.'] : cls === 'blunder' ? ['Gives up 21.4% expected score.'] : ['Engine’s first choice.'], sacrifice: cls === 'brilliant' ? 2 : null });
            pos.play(mv);
            pos = pos.clone();
          }
          const done = i + 1;
          timers.push(window.setTimeout(() => onEvent.onmessage({ type: 'progress', done, total: moves.length + 1, cached: 4, ply: i, score: { kind: 'cp', value: cp } }), 40 * i));
        }
        await new Promise((r) => setTimeout(r, 40 * moves.length + 100));
        const counts = (c: string) => reviews.filter((r) => r.color === c).reduce((m: Record<string, number>, r) => ((m[r.classification as string] = (m[r.classification as string] ?? 0) + 1), m), {});
        return { engine: 'Stockfish 19', settings: { depth: 16, movetimeMs: null, multipv: 2, classify: {} }, startFen: fenOf(replay(startFen, [])), positions, moves: reviews, white: { acpl: 18, accuracy: 88.4, moves: 8, counts: counts('white') }, black: { acpl: 41, accuracy: 79.2, moves: 8, counts: counts('black') }, turningPoints: [{ ply: 11, swing: -12.3 }, { ply: 13, swing: -21.4 }], createdAt: 0 };
      }
      case 'review_get':
        return null;
      case 'ai_providers':
        return {
          providers: [
            { id: 'claude', name: 'Claude Code', kind: 'cli', needsKey: false, defaultBaseUrl: null, description: 'Uses your Claude subscription through the official Claude Code CLI you are signed in to.' },
            { id: 'codex', name: 'Codex (ChatGPT)', kind: 'cli', needsKey: false, defaultBaseUrl: null, description: 'Uses your ChatGPT plan through the official Codex CLI (`codex login`).' },
            { id: 'anthropic', name: 'Anthropic API', kind: 'api', needsKey: true, defaultBaseUrl: 'https://api.anthropic.com', description: 'Claude models with an API key.' },
            { id: 'ollama', name: 'Ollama (local)', kind: 'local', needsKey: false, defaultBaseUrl: 'http://127.0.0.1:11434', description: 'Models running on this computer.' },
          ],
          keys: { anthropic: { source: 'none', storeAvailable: true } },
          settings: aiSettings,
        };
      case 'ai_ask': {
        const text =
          'This is a classical Ruy López. White has developed smoothly with Nf3 and Bb5, keeping pressure on e5, while Black’s …Nf6 develops and prepares …d6. ' +
          'The position is roughly equal: White’s plans include d4 to open the centre, Re1 to support e4, and c3 to prepare d4. For Black, …Be7, …O-O and …b5 are standard.\n' +
          'KEY MOVES: d4, Re1, c3';
        const parts = text.match(/.{1,24}/gs) ?? [];
        for (const p of parts) {
          await new Promise((r) => setTimeout(r, 15));
          (args.onEvent as Ch).onmessage({ type: 'delta', text: p });
        }
        const pos = positionFromFen(args.position.fen)!;
        const keyMoves = ['d4', 'Re1', 'c3', 'O-O']
          .map((s) => {
            for (const c of candidates(pos, 99)) if (c.san.replace(/[+#]/, '') === s) return { san: c.san, uci: c.uci };
            return null;
          })
          .filter(Boolean);
        return { text, moves: { keyMoves, mentioned: [], rejected: [] }, provider: 'claude', model: 'sonnet', chatId: null, cancelled: false };
      }
      case 'games_list':
        return {
          total: 4,
          games: [
            { id: 1, source: 'lichess', account: 'station_7', url: null, white: 'station_7', black: 'PolarNight', whiteElo: 1654, blackElo: 1701, result: '1-0', date: '2026.11.03', playedAt: 1793750400, timeControl: '300+3', speed: 'blitz', event: 'Rated blitz game', eco: 'C84', opening: 'Ruy Lopez: Closed', plyCount: 71, favorite: true, analyzed: true, importedAt: 0 },
            { id: 2, source: 'chesscom', account: 'station_7', url: null, white: 'MacReady82', black: 'station_7', whiteElo: 1588, blackElo: 1610, result: '0-1', date: '2026.11.01', playedAt: 1793577600, timeControl: '600', speed: 'rapid', event: 'Live Chess', eco: 'B90', opening: 'Sicilian Defense: Najdorf', plyCount: 94, favorite: false, analyzed: false, importedAt: 0 },
            { id: 3, source: 'pgn', account: null, url: null, white: 'Kasparov, G.', black: 'Topalov, V.', whiteElo: 2812, blackElo: 2700, result: '1-0', date: '1999.01.20', playedAt: null, timeControl: null, speed: null, event: 'Hoogovens', eco: 'B07', opening: 'Pirc Defense', plyCount: 87, favorite: true, analyzed: true, importedAt: 0 },
            { id: 4, source: 'lichess', account: 'station_7', url: null, white: 'station_7', black: 'Blair_U31', whiteElo: 1660, blackElo: 1642, result: '1/2-1/2', date: '2026.10.29', playedAt: 1793232000, timeControl: '180+0', speed: 'blitz', event: 'Rated blitz game', eco: 'D02', opening: 'London System', plyCount: 120, favorite: false, analyzed: false, importedAt: 0 },
          ],
        };
      case 'accounts_list':
        return [{ platform: 'lichess', username: 'station_7', lastSync: 1793750400, newestGameAt: 1793750400 }, { platform: 'chesscom', username: 'station_7', lastSync: 1793577600, newestGameAt: 1793577600 }];
      case 'tactics_stats':
        return { games: 48, scanned: 25, tactics: 14, yours: 9, missed: 6, due: 2, fresh: 9, attempts: 11, solvedFirstTry: 3, streak: 2 };
      case 'tactics_themes':
        return [['oneMove', 8], ['advantage', 7], ['hangingPiece', 5], ['fork', 3], ['mate', 3], ['mateIn2', 2], ['pin', 2], ['short', 4]];
      case 'tactic_next':
        return { id: 7, gameId: 1, ply: 9, fen: 'rn1qkbnr/ppp2p1p/3p2p1/4N3/2B1P1b1/2N5/PPPP1PPP/R1BQK2R b KQkq - 0 5', moves: ['g4d1', 'c4f7', 'e8e7', 'c3d5'], solver: 'white', kind: 'mate', cp: 999999999, themes: ['mateIn2', 'mate', 'attackingF2F7', 'short'], owner: 'you', played: null, attempts: 0, solved: 0, streak: 0, dueAt: 0, white: 'station_7', black: 'PolarNight', date: '2026.11.04', url: null };
      case 'tactic_check':
        return { result: 'correct', reply: 'e8e7', solved: false, alternativeMate: false };
      case 'tactics_scan':
        return { gameIndex: 0, games: 0, gameId: 0, label: '', stage: '', done: 0, total: 0, found: 0, finished: true };
      case 'chats_list':
        return [];
      case 'plugin:clipboard-manager|read_text':
        return '';
      default:
        return null;
    }
  });
}
