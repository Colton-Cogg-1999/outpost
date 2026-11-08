// Runs a tactic scan in the background. State lives in the store, so the scan
// keeps going (and reporting) while you use the rest of the app.
import { useStore } from '../store';
import { api, errorText, library, nextJobId } from '../lib/ipc';
import type { Strictness } from '../lib/types';

export const tacticsChanged = new EventTarget();

export async function startScan(opts: { strictness: Strictness; onlyMine: boolean; rescan: boolean; maxGames: number; gameIds?: number[] }) {
  const s = useStore.getState();
  if (s.scan) return;
  const job = nextJobId();
  s.setScan({ job, progress: null });
  let lastFound = 0;
  try {
    const result = await api.tacticsScan(job, opts, (progress) => {
      useStore.getState().setScan({ job, progress });
      if (progress.found !== lastFound) {
        lastFound = progress.found;
        tacticsChanged.dispatchEvent(new Event('change'));
      }
    });
    const n = result.found;
    s.toast(result.games === 0 ? 'Every game is already scanned.' : `Scan finished: ${n} new tactic${n === 1 ? '' : 's'} from ${result.games} game${result.games === 1 ? '' : 's'}.`);
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
