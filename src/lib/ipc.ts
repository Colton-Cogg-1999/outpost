// Typed wrappers around Tauri commands. Outside Tauri (unit tests, browser previews)
// `invoke` rejects with a clear message instead of pretending to work.
import { Channel, invoke as tauriInvoke } from '@tauri-apps/api/core';
import type * as T from './types';

export const inTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

export function invoke<R>(cmd: string, args?: Record<string, unknown>): Promise<R> {
  if (!inTauri) return Promise.reject(new Error('Native backend unavailable (not running inside the OUTPOST app).'));
  return tauriInvoke<R>(cmd, args);
}

export function errorText(e: unknown): string {
  if (typeof e === 'string') return e;
  if (e instanceof Error) return e.message;
  return String(e);
}

export function logError(message: string) {
  if (inTauri) tauriInvoke('log_frontend', { level: 'error', message }).catch(() => {});
}

/** Send uncaught frontend errors to <data>/outpost.log. */
export function reportErrors() {
  if (!inTauri) return;
  const send = logError;
  window.addEventListener('error', (e) => send(`${e.message} @ ${e.filename}:${e.lineno}:${e.colno}`));
  window.addEventListener('unhandledrejection', (e) => send(`unhandled rejection: ${errorText(e.reason)}`));
}

/** Fired whenever the game library changes, so open lists can reload. */
export const library = new EventTarget();
const changed = () => library.dispatchEvent(new Event('change'));
function mutates<R>(p: Promise<R>): Promise<R> {
  return p.finally(changed);
}
function importing<P extends { imported: number }>(onProgress: (p: P) => void) {
  let last = 0;
  return (p: P) => {
    onProgress(p);
    if (p.imported !== last) {
      last = p.imported;
      changed();
    }
  };
}

let jobCounter = Date.now() % 1_000_000_000;
export const nextJobId = () => ++jobCounter;

function channel<E>(onEvent: (e: E) => void): Channel<E> {
  const ch = new Channel<E>();
  ch.onmessage = onEvent;
  return ch;
}

export const api = {
  appInfo: () => invoke<T.AppInfo>('app_info'),
  settingsGet: () => invoke<Record<string, unknown>>('settings_get'),
  settingsSet: (key: string, value: unknown) => invoke<void>('settings_set', { key, value }),
  cancel: (jobId: number) => invoke<void>('job_cancel', { jobId }),

  fenCheck: (fen: string) => invoke<{ ok: boolean; fen?: string; error?: string }>('fen_check', { fen }),
  readText: (path: string) => invoke<string>('file_read_text', { path }),
  writeText: (path: string, contents: string) => invoke<void>('file_write_text', { path, contents }),
  readImage: (path: string) => invoke<ArrayBuffer>('file_read_image', { path }),

  engines: () => invoke<T.EngineRecord[]>('engines_list'),
  enginesDetect: () => invoke<{ path: string; label: string }[]>('engines_detect'),
  engineAdd: (path: string) => invoke<T.EngineRecord>('engine_add', { path }),
  engineRemove: (id: string) => invoke<void>('engine_remove', { id }),
  engineSetOptions: (id: string, options: Record<string, string>) => invoke<T.EngineRecord>('engine_set_options', { id, options }),
  engineSetDefault: (id: string) => invoke<void>('engine_set_default', { id }),
  engineRestart: (id: string) => invoke<void>('engine_restart', { id }),
  engineShutdown: (id: string) => invoke<void>('engine_shutdown', { id }),

  analysisStart: (
    engineId: string | null,
    request: { searchId: number; fen: string | null; moves: string[]; multipv: number; limit: T.SearchLimit },
    onEvent: (e: T.SearchEvent) => void,
  ) => invoke<string>('analysis_start', { engineId, request, onEvent: channel(onEvent) }),
  analysisStop: (engineId: string | null) => invoke<void>('analysis_stop', { engineId }),

  reviewStart: (
    jobId: number,
    args: { engineId: string | null; startFen: string | null; moves: string[]; gameId: number | null },
    onEvent: (e: T.ReviewEvent) => void,
  ) => invoke<T.GameReport>('review_start', { jobId, ...args, onEvent: channel(onEvent) }),
  reviewGet: (gameId: number) => invoke<T.GameReport | null>('review_get', { gameId }),

  games: (filter: Record<string, unknown>) => invoke<{ games: T.GameSummary[]; total: number }>('games_list', { filter }),
  game: (id: number) => invoke<T.GameDetail>('game_get', { id }),
  importPgn: (text: string) => mutates(invoke<T.ImportSummary>('games_import_pgn', { text })),
  gameSave: (id: number | null, pgnText: string) => mutates(invoke<number>('game_save', { id, pgnText })),
  gameDelete: (id: number) => mutates(invoke<void>('game_delete', { id })),
  clearHistory: () => mutates(invoke<number>('games_clear_history')),
  gameFavorite: (id: number, favorite: boolean) => mutates(invoke<void>('game_favorite', { id, favorite })),
  history: (jobId: number, request: { platform: string; username: string; year: number; month: number; cursor: number | null }) =>
    invoke<T.HistoryPage>('games_history', { jobId, request }),
  importSelected: (game: T.HistoryGame) => mutates(invoke<{ id: number; duplicate: boolean }>('game_import_selected', { game })),
  accounts: () => invoke<{ platform: string; username: string; lastSync: number | null; newestGameAt: number | null }[]>('accounts_list'),
  chesscomImport: (jobId: number, options: Record<string, unknown>, onProgress: (p: T.ImportProgress) => void) =>
    mutates(invoke<T.ImportProgress>('chesscom_import', { jobId, options, onProgress: channel(importing(onProgress)) })),
  lichessImport: (jobId: number, options: Record<string, unknown>, onProgress: (p: T.ImportProgress) => void) =>
    mutates(invoke<T.ImportProgress>('lichess_import', { jobId, options, onProgress: channel(importing(onProgress)) })),
  lichessImportGame: (input: string) => mutates(invoke<{ id: number; duplicate: boolean }>('lichess_import_game', { input })),

  tacticsStats: () => invoke<T.TacticStats>('tactics_stats'),
  tacticsThemes: () => invoke<[string, number][]>('tactics_themes'),
  tacticNext: (filter: { owner?: string; theme?: string; gameId?: number | null; exclude?: number[] }) => invoke<T.Tactic | null>('tactic_next', { filter }),
  tacticsForGame: (gameId: number) => invoke<T.Tactic[]>('tactics_for_game', { gameId }),
  tacticCheck: (id: number, index: number, uci: string) => invoke<T.TacticVerdict>('tactic_check', { id, index, uci }),
  tacticAttempt: (id: number, solved: boolean, mistakes: number, hints: number, durationMs: number) =>
    invoke<T.Tactic>('tactic_attempt', { id, solved, mistakes, hints, durationMs }),
  tacticsScan: (jobId: number, settings: { strictness: T.Strictness; onlyMine: boolean; rescan: boolean; maxGames: number; gameIds?: number[] }, onProgress: (p: T.ScanProgress) => void) =>
    invoke<T.ScanProgress>('tactics_scan', { jobId, settings, onProgress: channel(onProgress) }),

  aiProviders: () => invoke<{ providers: T.ProviderInfo[]; keys: Record<string, T.KeyStatus>; settings: T.AiSettings }>('ai_providers'),
  aiStatus: (provider: string) => invoke<T.ProviderStatus>('ai_status', { provider }),
  aiKeySet: (provider: string, key: string) => invoke<void>('ai_key_set', { provider, key }),
  aiKeyDelete: (provider: string) => invoke<void>('ai_key_delete', { provider }),
  aiAsk: (
    requestId: number,
    args: { question: string; label: string | null; position: unknown; history: { role: string; content: string }[]; chatId: string | null },
    onDelta: (text: string) => void,
  ) => invoke<T.AiAnswer>('ai_ask', { requestId, ...args, onEvent: channel<{ type: 'delta'; text: string }>((e) => onDelta(e.text)) }),
  aiCheckMoves: (fen: string, text: string) => invoke<T.MoveCheck>('ai_check_moves', { fen, text }),
  chats: () => invoke<{ id: string; title: string; fen: string | null; updatedAt: number; messages: number }[]>('chats_list'),
  chatMessages: (id: string) => invoke<{ role: string; content: string; provider: string | null; model: string | null; fen: string | null; createdAt: number }[]>('chat_messages', { id }),
  chatDelete: (id: string) => invoke<void>('chat_delete', { id }),
};
