import { useEffect, useMemo, useRef, useState } from 'react';
import { useStore, type ImportTab } from '../store';
import { Icon, Modal } from '../components/ui';
import { api, errorText, nextJobId } from '../lib/ipc';
import { openPgnFile, openPgnText, readClipboard } from '../lib/files';
import { openGame } from './Games';
import { fenError } from '../chess/position';
import { PieceSprite } from '../board/pieces';
import { analyseCells, findBoard, guessWhiteAtBottom, placement, plausibleCastling, propagate, startingPositionLabels, type Bounds, type Cell, type Img, type PieceCode } from '../vision/detect';
import type { ImportProgress } from '../lib/types';
import type { Role } from 'chessops/types';

const TABS: { id: ImportTab; label: string }[] = [
  { id: 'lichess', label: 'Lichess' },
  { id: 'chesscom', label: 'Chess.com' },
  { id: 'pgn', label: 'PGN / FEN' },
  { id: 'image', label: 'Screenshot (experimental)' },
];

function AccountImport({ platform }: { platform: 'lichess' | 'chesscom' }) {
  const toast = useStore((s) => s.toast);
  const close = () => useStore.getState().openImport(null);
  const [username, setUsername] = useState('');
  const [max, setMax] = useState(50);
  const [speeds, setSpeeds] = useState<string[]>([]);
  const [since, setSince] = useState('');
  const [rated, setRated] = useState(false);
  const [job, setJob] = useState<number | null>(null);
  const [progress, setProgress] = useState<ImportProgress | null>(null);
  const [gameRef, setGameRef] = useState('');
  const speedOptions = platform === 'lichess' ? ['bullet', 'blitz', 'rapid', 'classical', 'correspondence'] : ['bullet', 'blitz', 'rapid', 'daily'];

  const run = async () => {
    const id = nextJobId();
    setJob(id);
    setProgress(null);
    const options = {
      username: username.trim(),
      maxGames: max || null,
      since: since ? Math.floor(new Date(since).getTime() / 1000) : null,
      ratedOnly: rated,
      ...(platform === 'lichess' ? { perfTypes: speeds } : { timeClasses: speeds }),
    };
    try {
      const result = platform === 'lichess' ? await api.lichessImport(id, options, setProgress) : await api.chesscomImport(id, options, setProgress);
      setProgress(result);
      toast(result.message);
    } catch (e) {
      toast(errorText(e), 'error');
    } finally {
      setJob(null);
    }
  };

  const importOne = async () => {
    try {
      const r = await api.lichessImportGame(gameRef);
      await openGame(r.id);
      toast(r.duplicate ? 'Already in your library — opened it.' : 'Game imported.');
      close();
    } catch (e) {
      toast(errorText(e), 'error');
    }
  };

  return (
    <div style={{ display: 'grid', gap: 12 }}>
      <p className="dim small">
        {platform === 'lichess'
          ? 'Uses the public Lichess API. No login needed for public games; only finished games are imported.'
          : 'Uses Chess.com’s public Published-Data API: monthly archives of completed games. Never asks for your password.'}{' '}
        Requests are made one at a time. OUTPOST is not affiliated with {platform === 'lichess' ? 'Lichess' : 'Chess.com'}.
      </p>
      <div className="grid2">
        <label className="field">
          <span>Username</span>
          <input type="text" value={username} onChange={(e) => setUsername(e.target.value)} placeholder="e.g. your_handle" autoFocus />
        </label>
        <label className="field">
          <span>Latest games to import</span>
          <input type="number" min={1} max={10000} value={max} onChange={(e) => setMax(Number(e.target.value))} />
        </label>
        <label className="field">
          <span>Only games since (optional)</span>
          <input type="text" placeholder="YYYY-MM-DD" value={since} onChange={(e) => setSince(e.target.value)} />
        </label>
        <label className="field">
          <span>Rated only</span>
          <select value={rated ? 'y' : 'n'} onChange={(e) => setRated(e.target.value === 'y')}>
            <option value="n">Rated and casual</option>
            <option value="y">Rated only</option>
          </select>
        </label>
      </div>
      <div className="row">
        <span className="small dim">Time controls:</span>
        {speedOptions.map((sp) => (
          <button key={sp} className={`btn small ${speeds.includes(sp) ? 'on' : ''}`} onClick={() => setSpeeds(speeds.includes(sp) ? speeds.filter((x) => x !== sp) : [...speeds, sp])} aria-pressed={speeds.includes(sp)}>
            {sp}
          </button>
        ))}
        {speeds.length === 0 && <span className="faint small">all</span>}
      </div>
      {progress && (
        <div className="notice">
          {progress.message}
          <br />
          <span className="small">{progress.imported} imported · {progress.duplicates} already in library · {progress.skipped} skipped (variants, unfinished or filtered) · {progress.scanned} scanned</span>
        </div>
      )}
      <div className="row end">
        {job ? <button className="btn" onClick={() => api.cancel(job)}>Stop</button> : progress?.done ? <button className="btn" onClick={() => { close(); useStore.getState().setView('games'); }}>Open library</button> : null}
        <button className="btn primary" onClick={run} disabled={!!job || !username.trim()}><Icon name="antenna" />{job ? 'Importing…' : 'Import games'}</button>
      </div>
      {platform === 'lichess' && (
        <div className="box">
          <h3>Single game</h3>
          <div className="row" style={{ flexWrap: 'nowrap' }}>
            <input type="text" value={gameRef} onChange={(e) => setGameRef(e.target.value)} placeholder="https://lichess.org/AbCdEfGh or game id" style={{ flex: 1 }} />
            <button className="btn" onClick={importOne} disabled={!gameRef.trim()}><Icon name="link" />Import & open</button>
          </div>
        </div>
      )}
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

const PALETTE: PieceCode[] = ['K', 'Q', 'R', 'B', 'N', 'P', 'k', 'q', 'r', 'b', 'n', 'p'];
const ROLE: Record<string, Role> = { k: 'king', q: 'queen', r: 'rook', b: 'bishop', n: 'knight', p: 'pawn' };

function Sprite({ code }: { code: PieceCode }) {
  return <PieceSprite role={ROLE[code.toLowerCase()]} color={code === code.toUpperCase() ? 'white' : 'black'} />;
}

async function decode(bytes: ArrayBuffer): Promise<Img> {
  const bitmap = await createImageBitmap(new Blob([bytes]));
  const scale = Math.min(1, 1400 / Math.max(bitmap.width, bitmap.height));
  const canvas = document.createElement('canvas');
  canvas.width = Math.round(bitmap.width * scale);
  canvas.height = Math.round(bitmap.height * scale);
  const ctx = canvas.getContext('2d')!;
  ctx.drawImage(bitmap, 0, 0, canvas.width, canvas.height);
  const data = ctx.getImageData(0, 0, canvas.width, canvas.height);
  return { data: data.data, width: data.width, height: data.height };
}

function ImageImport() {
  const toast = useStore((s) => s.toast);
  const [img, setImg] = useState<Img | null>(null);
  const [bounds, setBounds] = useState<Bounds | null>(null);
  const [cells, setCells] = useState<Cell[] | null>(null);
  const [labels, setLabels] = useState<Record<number, PieceCode | null>>({});
  const [brush, setBrush] = useState<PieceCode | null>('P');
  const [whiteBottom, setWhiteBottom] = useState(true);
  const [turn, setTurn] = useState<'w' | 'b'>('w');
  const [castling, setCastling] = useState<string | null>(null);
  const [drag, setDrag] = useState<{ x: number; y: number } | null>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);

  const load = async (bytes: ArrayBuffer) => {
    try {
      const image = await decode(bytes);
      setImg(image);
      const b = findBoard(image);
      setBounds(b);
      setLabels({});
      setCastling(null);
      if (!b) toast('Could not find a board automatically. Drag a square around the board on the image.', 'error');
    } catch (e) {
      toast(`Could not read that image: ${errorText(e)}`, 'error');
    }
  };

  useEffect(() => {
    const w = window as unknown as { __outpostImage?: string };
    if (w.__outpostImage) {
      const path = w.__outpostImage;
      w.__outpostImage = undefined;
      api.readImage(path).then(load).catch((e) => toast(errorText(e), 'error'));
    }
    const onPaste = (e: ClipboardEvent) => {
      const file = Array.from(e.clipboardData?.files ?? []).find((f) => f.type.startsWith('image/'));
      if (file) file.arrayBuffer().then(load);
    };
    window.addEventListener('paste', onPaste);
    return () => window.removeEventListener('paste', onPaste);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (!img || !bounds) {
      setCells(null);
      return;
    }
    const c = analyseCells(img, bounds);
    setCells(c);
    setWhiteBottom(guessWhiteAtBottom(c));
    const start = startingPositionLabels(c);
    if (start) setLabels(Object.fromEntries(start.map((p, i) => [i, p])));
  }, [img, bounds]);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || !img) return;
    canvas.width = img.width;
    canvas.height = img.height;
    const ctx = canvas.getContext('2d')!;
    ctx.putImageData(new ImageData(new Uint8ClampedArray(img.data), img.width, img.height), 0, 0);
    if (bounds) {
      ctx.strokeStyle = '#9ad8a4';
      ctx.lineWidth = Math.max(2, img.width / 400);
      ctx.setLineDash([8, 6]);
      ctx.strokeRect(bounds.x, bounds.y, bounds.size, bounds.size);
    }
  }, [img, bounds]);

  const guesses = useMemo(() => (cells ? propagate(cells, labels) : []), [cells, labels]);
  const pieces = guesses.map((g) => g.piece);
  const board = cells ? placement(pieces, whiteBottom) : '';
  const rights = castling ?? (board ? plausibleCastling(board) : '-');
  const fen = `${board} ${turn} ${rights} - 0 1`;
  const problem = board ? fenError(fen) : null;
  const unresolved = guesses.filter((g) => g.uncertain).length;

  const toImage = (e: React.MouseEvent<HTMLCanvasElement>) => {
    const r = e.currentTarget.getBoundingClientRect();
    return { x: ((e.clientX - r.left) / r.width) * (img?.width ?? 1), y: ((e.clientY - r.top) / r.height) * (img?.height ?? 1) };
  };

  return (
    <div style={{ display: 'grid', gap: 12 }}>
      <p className="dim small">
        Recognises a board from a screenshot on this computer — no online service. It works best on flat 2D boards. You confirm every square: label one piece of
        each kind and OUTPOST matches similar-looking squares; dashed squares need a look.
      </p>
      <div className="row">
        <button
          className="btn"
          onClick={async () => {
            const { open } = await import('@tauri-apps/plugin-dialog');
            const path = await open({ filters: [{ name: 'Images', extensions: ['png', 'jpg', 'jpeg', 'webp', 'bmp'] }] });
            if (typeof path === 'string') api.readImage(path).then(load).catch((e) => toast(errorText(e), 'error'));
          }}
        >
          <Icon name="image" />Open image…
        </button>
        <span className="faint small">or paste an image (Ctrl+V) / drop a file on the window</span>
      </div>
      {img && (
        <div className="vision">
          <div className="crop">
            <canvas
              ref={canvasRef}
              onMouseDown={(e) => setDrag(toImage(e))}
              onMouseUp={(e) => {
                if (!drag) return;
                const p = toImage(e);
                const size = Math.max(Math.abs(p.x - drag.x), Math.abs(p.y - drag.y));
                if (size > 40) setBounds({ x: Math.min(drag.x, p.x), y: Math.min(drag.y, p.y), size });
                setDrag(null);
              }}
              style={{ cursor: 'crosshair' }}
              aria-label="Screenshot. Drag to select the board."
            />
            <span className="faint small">Drag over the image to select the board manually.</span>
          </div>
          {cells && (
            <div style={{ display: 'grid', gap: 8, alignContent: 'start' }}>
              <div className="palette" role="toolbar" aria-label="Piece to place">
                {PALETTE.map((p) => (
                  <button key={p} className={brush === p ? 'active' : ''} onClick={() => setBrush(p)} aria-label={`Place ${p}`}><Sprite code={p} /></button>
                ))}
                <button className={`erase ${brush === null ? 'active' : ''}`} onClick={() => setBrush(null)} aria-label="Empty square">∅</button>
              </div>
              <div className="editor-board">
                {guesses.map((g, i) => {
                  const light = (Math.floor(i / 8) + (i % 8)) % 2 === 0;
                  return (
                    <button key={i} className={`${light ? 'light' : 'dark'} ${g.uncertain ? 'uncertain' : ''}`} onClick={() => setLabels({ ...labels, [i]: brush })} aria-label={`Square ${i}`}>
                      {g.piece && <Sprite code={g.piece} />}
                    </button>
                  );
                })}
              </div>
              <div className="row small">
                <span className={unresolved ? 'warn' : 'dim'}>{unresolved ? `${unresolved} square(s) to check` : 'All squares resolved'}</span>
                <div className="spacer" />
                <button className="btn small" onClick={() => setWhiteBottom(!whiteBottom)}><Icon name="flip" />{whiteBottom ? 'White at bottom' : 'Black at bottom'}</button>
              </div>
              <div className="row small">
                <span className="dim">To move</span>
                <select value={turn} onChange={(e) => setTurn(e.target.value as 'w' | 'b')}>
                  <option value="w">White</option>
                  <option value="b">Black</option>
                </select>
                <span className="dim">Castling</span>
                {(['K', 'Q', 'k', 'q'] as const).map((c) => (
                  <label key={c} className="row" style={{ gap: 3 }}>
                    <input type="checkbox" checked={rights.includes(c)} onChange={(e) => {
                      const set = new Set(rights === '-' ? [] : rights.split(''));
                      if (e.target.checked) set.add(c); else set.delete(c);
                      const next = ['K', 'Q', 'k', 'q'].filter((x) => set.has(x)).join('');
                      setCastling(next || '-');
                    }} />
                    {c}
                  </label>
                ))}
              </div>
              <code className="small" style={{ wordBreak: 'break-all' }}>{fen}</code>
              {problem && <span className="small err">{problem}</span>}
              <div className="row end">
                <button
                  className="btn primary"
                  disabled={!!problem}
                  onClick={() => {
                    const err = useStore.getState().loadFen(fen);
                    if (err) toast(err, 'error');
                    else {
                      useStore.getState().openImport(null);
                      toast('Position loaded. Check it against the screenshot before relying on it.');
                    }
                  }}
                >
                  Analyze position
                </button>
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

export function ImportDialog() {
  const tab = useStore((s) => s.importTab)!;
  const openImport = useStore((s) => s.openImport);
  return (
    <Modal
      wide={tab === 'image'}
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
      {tab === 'image' && <ImageImport />}
    </Modal>
  );
}
