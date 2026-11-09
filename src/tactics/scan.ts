// Runs a tactic scan in the background. State lives in the store, so the scan
// keeps going (and reporting) while you use the rest of the app.
import { useStore } from '../store';
import { api, errorText, library, nextJobId } from '../lib/ipc';
import { mainline, toPgn } from '../chess/tree';
import type { Strictness } from '../lib/types';

export const tacticsChanged = new EventTarget();

export async function startScan(opts: { strictness: Strictness; onlyMine: boolean; rescan: boolean; maxGames: number; gameIds?: number[] }) {
  const s = useStore.getState();
  if (s.scan) return;
  const job = nextJobId();
  s.setScan({ job, progress: null });
  let lastFound = 0;
  try {
    await api.analysisStop(null);
    const result = await api.tacticsScan(job, opts, (progress) => {
      useStore.getState().setScan({ job, progress });
      if (progress.found !== lastFound) {
        lastFound = progress.found;
        tacticsChanged.dispatchEvent(new Event('change'));
      }
    });
    const n = result.found;
    s.toast(result.cancelled ? `Generation stopped. ${result.games} completed game(s), ${n} puzzle(s). Unfinished games can be retried.` : result.failed ? `${n} puzzle(s) generated. ${result.failed} game(s) could not be processed; try again.` : result.games === 0 ? 'No unprocessed games. Existing puzzles are ready to practice.' : `Scan finished: ${n} tactic${n === 1 ? '' : 's'} from ${result.games} game${result.games === 1 ? '' : 's'}.`);
  } catch (e) {
    s.toast(errorText(e), 'error');
  } finally {
    useStore.getState().setScan(null);
    tacticsChanged.dispatchEvent(new Event('change'));
    // Scans save reviews, which the Games list shows.
    library.dispatchEvent(new Event('change'));
  }
}

export function stopScan() {
  const scan = useStore.getState().scan;
  if (scan) api.cancel(scan.job);
}

export function scanText(p: { gameIndex: number; games: number; label: string; stage: string; done: number; total: number; found: number } | null) {
  if (!p) return 'Starting the engine…';
  const step = p.stage === 'review' ? `evaluating ${p.done}/${p.total}` : p.stage === 'search' ? `checking move ${p.done + 1}` : '';
  return `Game ${Math.min(p.gameIndex + 1, p.games)} of ${p.games}${p.label ? ` · ${p.label}` : ''}${step ? ` · ${step}` : ''} · ${p.found} found`;
}

let preparingCurrent = false;
/** Save the exact current game, then scan only that game and train its puzzles. */
export async function startCurrentGameScan(strictness: Strictness = 'personal') {
  const s = useStore.getState();
  if (s.scan || preparingCurrent) return;
  if (mainline(s.tree).length < 3) { s.toast('Open a game with at least two moves first.'); return; }
  preparingCurrent = true;
  const tree = s.tree;
  try {
    const id = s.gameId !== null && !s.dirty ? s.gameId : await api.gameSave(s.gameId, toPgn(tree));
    if (useStore.getState().tree !== tree) { s.toast('The game changed. Generate puzzles again from the current game.'); return; }
    s.markSaved(id);
    await startScan({ strictness, onlyMine: false, rescan: false, maxGames: 1, gameIds: [id] });
    const report = await api.reviewGet(id).catch(() => null);
    const current = useStore.getState();
    if (report && current.tree === tree && current.gameId === id) current.setReport(report);
  } catch (e) { s.toast(errorText(e), 'error'); }
  finally { preparingCurrent = false; }
}
