import { useCallback, useEffect, useState } from 'react';
import { ClearHistory } from './ClearHistory';
import { useStore } from '../store';
import { Icon } from '../components/ui';
import { api, errorText, library, nextJobId } from '../lib/ipc';
import type { GameSummary, ImportProgress } from '../lib/types';

type Account = { platform: string; username: string; lastSync: number | null; newestGameAt: number | null };

const date = (g: GameSummary) => (g.playedAt ? new Date(g.playedAt * 1000).toISOString().slice(0, 10) : g.date?.replace(/\./g, '-') ?? '—');

export async function openGame(id: number) {
  const s = useStore.getState();
  const g = await api.game(id);
  const report = await api.reviewGet(id).catch(() => null);
  const warning = s.loadPgn(g.pgn, { game: g, gameId: g.id, report });
  if (warning) s.toast(warning, 'error');
}

export function Games() {
  const toast = useStore((s) => s.toast);
  const openImport = useStore((s) => s.openImport);
  const [games, setGames] = useState<GameSummary[]>([]);
  const [total, setTotal] = useState(0);
  const [filter, setFilter] = useState({ search: '', source: '', speed: '', result: '', favorites: false, analyzed: '' as '' | 'yes' | 'no', sort: 'played', ascending: false });
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [sync, setSync] = useState<{ key: string; job: number; progress: ImportProgress | null } | null>(null);
  const [limit, setLimit] = useState(200);

  const load = useCallback(() => {
    api
      .games({ ...filter, analyzed: filter.analyzed === '' ? null : filter.analyzed === 'yes', limit })
      .then((page) => {
        setGames(page.games);
        setTotal(page.total);
      })
      .catch((e) => toast(errorText(e), 'error'));
  }, [filter, limit, toast]);

  useEffect(() => {
    const t = setTimeout(load, 150);
    return () => clearTimeout(t);
  }, [load]);

  // Reload when anything (an import dialog, a sync, a save) changes the library.
  const [version, setVersion] = useState(0);
  useEffect(() => {
    let t: ReturnType<typeof setTimeout> | undefined;
    const bump = () => {
      clearTimeout(t);
      t = setTimeout(() => setVersion((v) => v + 1), 250);
    };
    library.addEventListener('change', bump);
    return () => {
      clearTimeout(t);
      library.removeEventListener('change', bump);
    };
  }, []);
  useEffect(() => {
    if (version) load();
  }, [version]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    api.accounts().then(setAccounts).catch(() => {});
  }, [sync, version]);

  const sortBy = (key: string) => setFilter((f) => ({ ...f, sort: key, ascending: f.sort === key ? !f.ascending : false }));

  const runSync = async (a: Account) => {
    const job = nextJobId();
    const key = `${a.platform}:${a.username}`;
    setSync({ key, job, progress: null });
    try {
      const opts = { username: a.username, incremental: true };
      const update = (p: ImportProgress) => setSync({ key, job, progress: p });
      const result = a.platform === 'lichess' ? await api.lichessImport(job, opts, update) : await api.chesscomImport(job, opts, update);
      toast(`${a.username}: ${result.imported} new game(s), ${result.duplicates} already in the library.`);
    } catch (e) {
      toast(errorText(e), 'error');
    } finally {
      setSync(null);
    }
  };

  const remove = async (g: GameSummary) => {
    if (!window.confirm(`Delete ${g.white} vs ${g.black} from the library? Its review is deleted too.`)) return;
    await api.gameDelete(g.id).catch((e) => toast(errorText(e), 'error'));
    load();
  };

  const header = (key: string, label: string) => (
    <th onClick={() => sortBy(key)} aria-sort={filter.sort === key ? (filter.ascending ? 'ascending' : 'descending') : undefined}>
      {label}{filter.sort === key ? (filter.ascending ? ' ▲' : ' ▼') : ''}
    </th>
  );

  return (
    <div className="page">
      <div className="page-head">
        <div>
          <h1>Games</h1>
          <p className="lead">Your local library: imported, pasted and saved games. Everything stays on this computer.</p>
        </div>
        <div className="spacer" />
        <ClearHistory disabled={!!sync} />
        <button className="btn primary" onClick={() => openImport('lichess')}><Icon name="plus" />Import games</button>
      </div>

      {accounts.length > 0 && (
        <div className="row">
          {accounts.map((a) => {
            const key = `${a.platform}:${a.username}`;
            const active = sync?.key === key;
            return (
              <div key={key} className="box" style={{ padding: '8px 12px', minWidth: 260 }}>
                <div className="row">
                  <Icon name={a.platform === 'lichess' ? 'antenna' : 'pawn'} />
                  <b>{a.username}</b>
                  <span className="dim small">{a.platform === 'lichess' ? 'Lichess' : 'Chess.com'}</span>
                  <div className="spacer" />
                  {active ? (
                    <button className="btn small" onClick={() => api.cancel(sync!.job)}>Stop</button>
                  ) : (
                    <button className="btn small" onClick={() => runSync(a)} disabled={!!sync} title="Fetch games newer than the last sync"><Icon name="refresh" />Sync</button>
                  )}
                </div>
                <span className="faint small">{active ? sync?.progress?.message ?? 'Starting…' : a.lastSync ? `last synced ${new Date(a.lastSync * 1000).toLocaleString()}` : 'never synced'}</span>
                {active && sync?.progress && <span className="small">{sync.progress.imported} new · {sync.progress.duplicates} known · {sync.progress.scanned} scanned</span>}
              </div>
            );
          })}
        </div>
      )}

      <div className="row">
        <input type="search" placeholder="Search players, events, openings…" value={filter.search} onChange={(e) => setFilter({ ...filter, search: e.target.value })} style={{ width: 280 }} aria-label="Search games" />
        <select value={filter.source} onChange={(e) => setFilter({ ...filter, source: e.target.value })} aria-label="Source">
          <option value="">All sources</option>
          <option value="lichess">Lichess</option>
          <option value="chesscom">Chess.com</option>
          <option value="pgn">PGN files</option>
          <option value="local">Saved analysis</option>
        </select>
        <select value={filter.speed} onChange={(e) => setFilter({ ...filter, speed: e.target.value })} aria-label="Time control">
          <option value="">All speeds</option>
          {['ultrabullet', 'bullet', 'blitz', 'rapid', 'classical', 'correspondence'].map((s) => <option key={s} value={s}>{s}</option>)}
        </select>
        <select value={filter.result} onChange={(e) => setFilter({ ...filter, result: e.target.value })} aria-label="Result">
          <option value="">Any result</option>
          <option value="1-0">1-0</option>
          <option value="0-1">0-1</option>
          <option value="1/2-1/2">½-½</option>
          <option value="*">unfinished</option>
        </select>
        <select value={filter.analyzed} onChange={(e) => setFilter({ ...filter, analyzed: e.target.value as '' | 'yes' | 'no' })} aria-label="Reviewed">
          <option value="">Reviewed or not</option>
          <option value="yes">Reviewed</option>
          <option value="no">Not reviewed</option>
        </select>
        <button className={`btn small ${filter.favorites ? 'on' : ''}`} onClick={() => setFilter({ ...filter, favorites: !filter.favorites })} aria-pressed={filter.favorites}><Icon name="star" />Favourites</button>
        <div className="spacer" />
        <span className="dim small">{total} game{total === 1 ? '' : 's'}</span>
      </div>

      <div className="table-wrap">
        <table className="list">
          <thead>
            <tr>
              <th aria-label="Favourite" />
              {header('played', 'Date')}
              {header('white', 'White')}
              {header('black', 'Black')}
              <th>Result</th>
              <th>Speed</th>
              <th>Opening</th>
              {header('moves', 'Moves')}
              <th>Source</th>
              <th aria-label="Actions" />
            </tr>
          </thead>
          <tbody>
            {games.map((g) => (
              <tr key={g.id} className="clickable" onClick={() => openGame(g.id).catch((e) => toast(errorText(e), 'error'))}>
                <td>
                  <button
                    className={`btn ghost small icon-only ${g.favorite ? 'on' : ''}`}
                    aria-label={g.favorite ? 'Unfavourite' : 'Favourite'}
                    onClick={(e) => {
                      e.stopPropagation();
                      api.gameFavorite(g.id, !g.favorite).then(load);
                    }}
                  >
                    <Icon name="star" />
                  </button>
                </td>
                <td className="mono-num">{date(g)}</td>
                <td>{g.white} <span className="faint">{g.whiteElo ?? ''}</span></td>
                <td>{g.black} <span className="faint">{g.blackElo ?? ''}</span></td>
                <td className={g.result === '1-0' ? 'result-w' : 'result-b'}>{g.result === '1/2-1/2' ? '½-½' : g.result}</td>
                <td className="dim">{g.speed ?? ''} <span className="faint">{g.timeControl ?? ''}</span></td>
                <td className="trunc dim" title={g.opening ?? ''}>{g.eco ? `${g.eco} ` : ''}{g.opening ?? ''}</td>
                <td className="mono-num">{Math.ceil(g.plyCount / 2)}</td>
                <td className="dim">{g.source === 'chesscom' ? 'Chess.com' : g.source === 'lichess' ? 'Lichess' : g.source}{g.analyzed ? ' · reviewed' : ''}</td>
                <td>
                  <button className="btn ghost small icon-only danger" aria-label="Delete game" onClick={(e) => { e.stopPropagation(); remove(g); }}><Icon name="trash" /></button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        {games.length === 0 && (
          <div style={{ padding: 30, textAlign: 'center' }} className="dim">
            {total === 0 && !filter.search ? 'No games yet. Import from Lichess or Chess.com, open a PGN file, or save an analysis.' : 'No games match these filters.'}
          </div>
        )}
      </div>
      {games.length < total && <button className="btn" onClick={() => setLimit(limit + 200)}>Show more</button>}
    </div>
  );
}
