import { useEffect, useState } from 'react';
import { useStore } from '../store';
import { api, errorText, library } from '../lib/ipc';
import type { GameSummary } from '../lib/types';
import { Icon } from '../components/ui';
import { ClearHistory } from './ClearHistory';
import { openGame } from './Games';

function displayDate(date: string | null) {
  if (!date || !/^\d{4}\.\d{2}\.\d{2}$/.test(date)) return date ?? 'Undated';
  const parsed = new Date(`${date.replaceAll('.', '-')}T12:00:00Z`);
  return Number.isNaN(parsed.getTime()) ? date : parsed.toLocaleDateString('en-US', { month: 'short', day: 'numeric', year: 'numeric', timeZone: 'UTC' });
}

/** Existing import and library actions, presented beside the board. */
export function GameDock() {
  const openImport = useStore((s) => s.openImport);
  const gameId = useStore((s) => s.gameId);
  const [games, setGames] = useState<GameSummary[]>([]);
  const [error, setError] = useState('');
  useEffect(() => {
    let active = true;
    const refresh = () => api.games({ limit: 8, sort: 'played', ascending: false }).then((p) => {
      if (active) { setGames(p.games); setError(''); }
    }).catch((e) => { if (active) setError(errorText(e)); });
    void refresh();
    library.addEventListener('change', refresh);
    return () => { active = false; library.removeEventListener('change', refresh); };
  }, []);
  const accountGame = games.find((g) => g.account);
  const account = accountGame?.account;
  return <aside className="game-dock panel" aria-label="Import and recent games">
    <h2><Icon name="download" />Import games</h2>
    <p className="dim">Browse any player’s games or open a PGN file.</p>
    <div className="import-sources">
      <button className={accountGame?.source === 'lichess' ? 'selected' : ''} onClick={() => openImport('lichess')}><Icon name="knight" />Lichess</button>
      <button className={accountGame?.source === 'chesscom' ? 'selected' : ''} onClick={() => openImport('chesscom')}><Icon name="pawn" />Chess.com</button>
      <button onClick={() => openImport('pgn')}><Icon name="file" />PGN</button>
    </div>
    <div className="account-status"><Icon name={account ? 'check' : 'pawn'} /><span>{account ? `Account: ${account}` : 'Find a chess player'}</span><button onClick={() => openImport(accountGame?.source === 'chesscom' ? 'chesscom' : 'lichess')}>{account ? 'Change' : 'Choose'}</button></div>
    <div className="recent-heading"><h3>Recent games</h3><p className="dim small">Select a saved game to analyze.</p></div>
    <div className="recent-games">
      {error && <p className="notice error">{error}</p>}
      {!error && games.length === 0 && <p className="dim small">Your imported games will appear here. Choose a source above to get started.</p>}
      {games.map((g) => <button key={g.id} className={`recent-game ${g.id === gameId ? 'selected' : ''}`} onClick={() => openGame(g.id).catch((e) => useStore.getState().toast(errorText(e), 'error'))}>
        <span className="game-radio" /><span className="game-detail"><b>{g.account ? `vs ${g.white.toLowerCase() === g.account.toLowerCase() ? g.black : g.white}` : `${g.white} vs ${g.black}`}</b><span>{displayDate(g.date)}</span><span>{g.opening ?? g.speed ?? 'Standard chess'}{g.eco ? ` (${g.eco})` : ''}</span></span><span className="game-result"><span>{g.result.replaceAll('-', '–')}</span><small>{g.timeControl?.replace(/^(\d+)\+(\d+)$/, (_, seconds: string, increment: string) => `${Number(seconds) / 60}+${increment}`) ?? g.speed ?? ''}</small></span>
      </button>)}
    </div>
    <ClearHistory />
    <button className="btn primary library-link" onClick={() => useStore.getState().setView('games')}><Icon name="games" />Open game library</button>
  </aside>;
}
