// File and clipboard helpers. Paths always come from a native dialog or a native drop.
import { api } from './ipc';
import { useStore } from '../store';
import { fenError } from '../chess/position';
import { toPgn } from '../chess/tree';

export const looksLikeFen = (text: string) => /^\s*([pnbrqkPNBRQK1-8]+\/){7}[pnbrqkPNBRQK1-8]+\s+[wb]\b/.test(text);

/** Loads the first game into the analyzer; with several games, imports all into the library. */
export async function openPgnText(text: string, source = 'pasted'): Promise<void> {
  const s = useStore.getState();
  const trimmed = text.trim();
  if (!trimmed) throw new Error('Nothing to open.');
  if (looksLikeFen(trimmed) && !trimmed.includes('[')) {
    const err = s.loadFen(trimmed.split('\n')[0]);
    if (err) throw new Error(err);
    s.toast('Position loaded from FEN.');
    return;
  }
  const games = (trimmed.match(/^\s*\[Event\s/gm) ?? []).length;
  if (games > 1) {
    const summary = await api.importPgn(trimmed);
    s.toast(`${summary.imported} game(s) imported to the library, ${summary.duplicates} already there${summary.failed ? `, ${summary.failed} unreadable` : ''}.`);
    if (summary.firstId !== null) {
      const g = await api.game(summary.firstId);
      s.loadPgn(g.pgn, { game: g, gameId: g.id, report: await api.reviewGet(g.id).catch(() => null) });
    }
    return;
  }
  const warning = s.loadPgn(trimmed);
  s.toast(warning ? `Loaded with a problem: ${warning}` : `Game loaded from ${source}.`, warning ? 'error' : 'info');
}

export async function openPgnFile(): Promise<void> {
  const { open } = await import('@tauri-apps/plugin-dialog');
  const path = await open({ multiple: false, filters: [{ name: 'Chess games', extensions: ['pgn', 'fen', 'epd', 'txt'] }] });
  if (typeof path === 'string') await loadDroppedPaths([path]);
}

export async function loadDroppedPaths(paths: string[]): Promise<void> {
  const s = useStore.getState();
  for (const path of paths) {
    const ext = path.split('.').pop()?.toLowerCase() ?? '';
    if (!['pgn', 'fen', 'epd', 'txt'].includes(ext)) {
      s.toast(`Can't open .${ext} files. Drop a PGN or FEN file.`, 'error');
      continue;
    }
    const text = await api.readText(path);
    await openPgnText(text, path.split('/').pop() ?? 'file');
  }
}

export async function savePgnFile(): Promise<void> {
  const s = useStore.getState();
  const { save } = await import('@tauri-apps/plugin-dialog');
  const white = s.tree.headers.find(([k]) => k === 'White')?.[1] ?? 'game';
  const path = await save({ defaultPath: `${white.replace(/[^\w.-]+/g, '_')}.pgn`, filters: [{ name: 'PGN', extensions: ['pgn'] }] });
  if (!path) return;
  await api.writeText(path.endsWith('.pgn') ? path : `${path}.pgn`, toPgn(s.tree));
  s.toast('PGN saved.');
}

export async function copyText(text: string, what: string): Promise<void> {
  try {
    const { writeText } = await import('@tauri-apps/plugin-clipboard-manager');
    await writeText(text);
  } catch {
    await navigator.clipboard.writeText(text);
  }
  useStore.getState().toast(`${what} copied to the clipboard.`);
}

export async function readClipboard(): Promise<string> {
  try {
    const { readText } = await import('@tauri-apps/plugin-clipboard-manager');
    return (await readText()) ?? '';
  } catch {
    return navigator.clipboard.readText();
  }
}

export function validateFen(fen: string): string | null {
  return fenError(fen);
}
