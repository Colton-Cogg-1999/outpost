import { useEffect, useRef, useState } from 'react';
import { useStore, type ImportTab } from '../store';
import { Icon, Modal } from '../components/ui';
import { api, errorText, nextJobId } from '../lib/ipc';
import { openPgnFile, openPgnText, readClipboard } from '../lib/files';
import { openGame } from './Games';
import { fenError } from '../chess/position';
import type { HistoryGame, HistoryPage } from '../lib/types';

const TABS: { id: ImportTab; label: string }[] = [
  { id: 'lichess', label: 'Lichess' },
  { id: 'chesscom', label: 'Chess.com' },
  { id: 'pgn', label: 'PGN / FEN' },
];

export function AccountImport({ platform }: { platform: 'lichess' | 'chesscom' }) {
  const toast = useStore((s) => s.toast);
  const [username, setUsername] = useState('');
  const [ownAccount, setOwnAccount] = useState(true);
  const [month, setMonth] = useState(() => {
    const d = new Date();
    return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}`;
  });
  const [page, setPage] = useState<HistoryPage | null>(null);
  const [cursors, setCursors] = useState<(number | null)[]>([null]);
  const [busy, setBusy] = useState(false);
  const [gameRef, setGameRef] = useState('');
  const job = useRef<number | null>(null);
  const generation = useRef(0);
  useEffect(() => () => {
    generation.current++;
    if (job.current !== null) void api.cancel(job.current).catch(() => {});
  }, []);

  const reset = () => { setPage(null); setCursors([null]); };
  const browse = async (trail: (number | null)[] = [null]) => {
    const token = ++generation.current;
    const id = nextJobId();
    job.current = id;
    setBusy(true);
    try {
      const [year, m] = month.split('-').map(Number);
      const result = await api.history(id, { platform, username: username.trim(), year, month: m, cursor: trail[trail.length - 1] });
      if (generation.current !== token) return;
      setPage(result);
      setCursors(trail);
    } catch (e) {
      if (generation.current === token) toast(errorText(e), 'error');
    } finally {
      if (generation.current === token) { setBusy(false); job.current = null; }
    }
  };
  const importOne = async (game?: HistoryGame) => {
    setBusy(true);
    try {
      const r = game ? await api.importSelected({ ...game, account: ownAccount ? username.trim() : null }) : await api.lichessImportGame(gameRef);
      await openGame(r.id);
      toast(r.duplicate ? 'Already in your library — opened it.' : 'Game imported.');
      useStore.getState().openImport(null);
    } catch (e) { toast(errorText(e), 'error'); }
    finally { setBusy(false); }
  };
  return (
    <div style={{ display: 'grid', gap: 12 }}>
      <p className="dim small">Browse any player’s public games by username, then select a game to import. Browsing adds nothing to your library.</p>
      <form className="row" onSubmit={(e) => { e.preventDefault(); void browse(); }}>
        <label className="field" style={{ flex: 1 }}><span>Player username</span>
          <input value={username} disabled={busy} onChange={(e) => { setUsername(e.target.value); reset(); }} placeholder="Any player’s username" autoFocus />
        </label>
        <label className="field"><span>Month</span>
          <input type="month" value={month} disabled={busy} onChange={(e) => { setMonth(e.target.value); reset(); }} />
        </label>
        <button className="btn primary" disabled={busy || !username.trim() || !/^\d{4}-\d{2}$/.test(month)}>{busy ? 'Loading…' : 'Browse games'}</button>
      </form>
      <label className="row small"><input type="checkbox" checked={ownAccount} onChange={(e) => setOwnAccount(e.target.checked)} disabled={busy} />This is my account</label>
      {job.current !== null && <button className="btn" onClick={() => { if (job.current !== null) void api.cancel(job.current); }}>Cancel</button>}
      {page && <>
        <div style={{ maxHeight: 360, overflowY: 'auto', display: 'grid', gap: 8 }} aria-label="Game history">
          {page.games.length === 0 && <p className="dim">No supported games on this page. Try an older page or another month.</p>}
          {page.games.map((game, i) => <div className="box row" key={game.sourceId ?? i}>
            <div style={{ flex: 1 }}><b>{game.white}</b> vs <b>{game.black}</b>
              <div className="small dim">{game.date} · {game.speed ?? game.timeControl ?? 'Chess'} · {game.result}</div>
            </div>
            <button className="btn" disabled={busy} onClick={() => importOne(game)} aria-label={`Import ${game.white} vs ${game.black}`}>Import & open</button>
          </div>)}
        </div>
        <div className="row end">
          <button className="btn" disabled={busy || cursors.length === 1} onClick={() => browse(cursors.slice(0, -1))}>Newer</button>
          <span className="small dim">Page {cursors.length}</span>
          <button className="btn" disabled={busy || page.nextCursor === null} onClick={() => browse([...cursors, page.nextCursor])}>Older</button>
        </div>
      </>}
      {platform === 'lichess' && <details><summary className="small dim">Or open a game link</summary>
        <div className="row" style={{ marginTop: 8 }}>
          <input value={gameRef} onChange={(e) => setGameRef(e.target.value)} placeholder="Lichess game URL or ID" style={{ flex: 1 }} />
          <button className="btn" disabled={busy || !gameRef.trim()} onClick={() => importOne()}>Import & open</button>
        </div>
      </details>}
    </div>
  );
}

function PgnImport() {
  const toast = useStore((s) => s.toast);
  const [text, setText] = useState('');
  const [busy, setBusy] = useState(false);
  const close = () => useStore.getState().openImport(null);
  const open = async () => {
    setBusy(true);
    try {
      await openPgnText(text, 'pasted text');
      close();
    } catch (e) {
      toast(errorText(e), 'error');
    } finally {
      setBusy(false);
    }
  };
  const toLibrary = async () => {
    setBusy(true);
    try {
      const r = await api.importPgn(text);
      toast(`${r.imported} imported, ${r.duplicates} duplicates${r.failed ? `, ${r.failed} unreadable` : ''}.${r.errors.length ? ` ${r.errors[0]}` : ''}`, r.errors.length ? 'error' : 'info');
      if (r.imported + r.duplicates > 0) close();
    } catch (e) {
      toast(errorText(e), 'error');
    } finally {
      setBusy(false);
    }
  };
  const fenProblem = text.trim() && !text.includes('[') && !/\d\./.test(text) ? fenError(text.trim()) : null;
  return (
    <div style={{ display: 'grid', gap: 12 }}>
      <p className="dim small">Paste one or more PGN games, or a FEN position. You can also open .pgn files (several games per file are fine) or drop them onto the window.</p>
      <textarea rows={12} value={text} onChange={(e) => setText(e.target.value)} placeholder={'[Event "…"]\n\n1. e4 e5 2. Nf3 …\n\nor a FEN: rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1'} spellCheck={false} style={{ fontSize: 12 }} />
      {fenProblem && text.trim().split('/').length === 8 && <span className="small err">{fenProblem}</span>}
      <div className="row">
        <button className="btn" onClick={async () => setText(await readClipboard())}><Icon name="paste" />Paste</button>
        <button className="btn" onClick={() => openPgnFile().then(close).catch((e) => toast(errorText(e), 'error'))}><Icon name="folder" />Open file…</button>
        <div className="spacer" />
        <button className="btn" onClick={toLibrary} disabled={busy || !text.trim() || !text.includes('.')}>Add to library</button>
        <button className="btn primary" onClick={open} disabled={busy || !text.trim()}>Analyze</button>
      </div>
    </div>
  );
}

export function ImportDialog() {
  const tab = useStore((s) => s.importTab)!;
  const openImport = useStore((s) => s.openImport);
  return (
    <Modal
      title={
        <div className="tabs" role="tablist">
          {TABS.map((t) => (
            <button key={t.id} role="tab" aria-selected={tab === t.id} className={tab === t.id ? 'active' : ''} onClick={() => openImport(t.id)}>{t.label}</button>
          ))}
        </div>
      }
      onClose={() => openImport(null)}
    >
      {tab === 'lichess' && <AccountImport key="li" platform="lichess" />}
      {tab === 'chesscom' && <AccountImport key="cc" platform="chesscom" />}
      {(tab === 'pgn' || tab === 'fen') && <PgnImport />}
    </Modal>
  );
}
