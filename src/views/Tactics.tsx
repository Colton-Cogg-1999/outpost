import { useEffect, useState } from 'react';
import { useStore } from '../store';
import { Progress } from '../components/ui';
import { api } from '../lib/ipc';
import { scanText, startScan, stopScan, tacticsChanged } from '../tactics/scan';
import type { TacticStats } from '../lib/types';
import { PuzzleTrainer } from './PuzzleTrainer';

export function Tactics() {
  const [scope, setScope] = useState<'basic' | 'library'>('basic');
  const [onlyMine, setOnlyMine] = useState(false);
  const [stats, setStats] = useState<TacticStats | null>(null);
  const scan = useStore((s) => s.scan);
  useEffect(() => {
    const refresh = () => { api.tacticsStats().then(setStats).catch(() => {}); };
    refresh(); tacticsChanged.addEventListener('change', refresh);
    return () => tacticsChanged.removeEventListener('change', refresh);
  }, []);
  return <div className="practice-page">
    <div className="practice-heading"><h1>Puzzles</h1><div className="tabs" role="tablist"><button role="tab" aria-selected={scope === 'basic'} className={scope === 'basic' ? 'active' : ''} onClick={() => setScope('basic')}>Basic practice</button><button role="tab" aria-selected={scope === 'library'} className={scope === 'library' ? 'active' : ''} onClick={() => setScope('library')}>From my games{stats ? ` (${stats.tactics})` : ''}</button></div></div>
    <PuzzleTrainer key={scope} scope={scope} />
    <section className="batch-puzzles box"><div><h3>Generate puzzles from my games</h3><p className="dim small">Scan saved games that have never been processed. Completed games are skipped, even when no puzzles were found.</p></div>
      {scan ? <div><p className="small">{scanText(scan.progress)}</p><Progress value={scan.progress?.total ? scan.progress.done / scan.progress.total : 0} /><button className="btn small" onClick={stopScan}>Stop generation</button></div> : <div className="row"><label className="row small"><input type="checkbox" checked={onlyMine} onChange={(e) => setOnlyMine(e.target.checked)} />Only games I played</label><button className="btn primary" disabled={!stats?.games} onClick={() => { setScope('library'); void startScan({ strictness: 'personal', onlyMine, rescan: false, maxGames: 10000 }); }}>Generate puzzles from my games</button></div>}
    </section>
  </div>;
}
