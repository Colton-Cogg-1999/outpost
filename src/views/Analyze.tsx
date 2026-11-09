import { useEffect, useMemo, useRef, useState } from 'react';
import { useStore } from '../store';
import { Board, type Arrow } from '../board/Board';
import { Icon } from '../components/ui';
import { EnginePanel, LinePanel, MoveList, useEngineList } from './AnalyzePanels';
import { GameDock } from './GameDock';
import { CurrentGamePuzzles, PuzzleTrainer } from './PuzzleTrainer';
import { AiPanel, type ChatPosition } from './AiPanel';
import { header, mainline, movesTo, repetitionCount, startFen, toPgn } from '../chess/tree';
import { moveFromSan, positionFromFen, statusOf, uciOf, uciSquares } from '../chess/position';
import { api, errorText, nextJobId } from '../lib/ipc';
import { copyText, openPgnFile, openPgnText, readClipboard, savePgnFile } from '../lib/files';
import { makeSquare } from 'chessops/util';

let searchCounter = 0;

/** Keeps the engine(s) analysing whatever position is on the board. */
function useLiveAnalysis(paused = false) {
  const engineOn = useStore((s) => s.engineOn);
  const engineId = useStore((s) => s.engineId);
  const compareId = useStore((s) => s.compareId);
  const current = useStore((s) => s.current);
  const fen = useStore((s) => s.tree.nodes[s.current]?.fen);
  const multipv = useStore((s) => s.prefs.multipv);
  const depth = useStore((s) => s.prefs.depthLimit);
  const scanning = useStore((s) => !!s.scan);
  const running = useRef<{ main: string | null; compare: string | null }>({ main: null, compare: null });

  useEffect(() => {
    let cancelled = false;
    const s = useStore.getState();
    s.setLive(null);
    s.setCompareLive(null);
    // An engine that is no longer selected must not keep searching an old position.
    const wanted = { main: engineOn && !scanning ? engineId : null, compare: engineOn && !scanning && compareId && compareId !== engineId ? compareId : null };
    for (const id of [running.current.main, running.current.compare]) {
      if (id && id !== wanted.main && id !== wanted.compare) api.analysisStop(id).catch(() => {});
    }
    running.current = wanted;
    if (!engineOn || !engineId || scanning || paused) {
      api.analysisStop(null).catch(() => {});
      return;
    }
    s.setEngineError(null);
    const tree = s.tree;
    const request = (id: number) => ({ searchId: id, fen: startFen(tree), moves: movesTo(tree, current), multipv, limit: depth ? { depth } : {} });
    const nodeId = current;
    // Debounce rapid navigation; the backend also discards superseded searches.
    const timer = setTimeout(() => {
      const mainId = ++searchCounter;
      api
        .analysisStart(engineId, request(mainId), (e) => {
          const st = useStore.getState();
          if (cancelled) return;
          if (e.type === 'error') {
            if (e.searchId === mainId) st.setEngineError(e.message);
            return;
          }
          if (e.snapshot.searchId !== mainId || st.current !== nodeId) return;
          const prev = st.live;
          // Keep a deeper cached result on screen until the live search catches up.
          if (e.type === 'update' && prev && prev.searchId === mainId && prev.cached && prev.depth > e.snapshot.depth) return;
          st.setLive(e.snapshot);
          const top = e.snapshot.lines[0];
          if (top) st.setNodeEval(nodeId, top.score, e.snapshot.terminal ? 999 : e.snapshot.depth);
        })
        .catch((err) => { if (!cancelled) useStore.getState().setEngineError(errorText(err)); });
      if (compareId && compareId !== engineId) {
        const cmpId = ++searchCounter;
        api
          .analysisStart(compareId, request(cmpId), (e) => {
            const st = useStore.getState();
            if (cancelled || e.type === 'error') return;
            if (e.snapshot.searchId === cmpId && st.current === nodeId) st.setCompareLive(e.snapshot);
          })
          .catch((err) => useStore.getState().toast(`Comparison engine: ${errorText(err)}`, 'error'));
      }
    }, 70);
    return () => { cancelled = true; clearTimeout(timer); };
  }, [engineOn, engineId, compareId, current, fen, multipv, depth, scanning, paused]);

  useEffect(() => () => void api.analysisStop(null).catch(() => {}), []);
}

function useReview() {
  const [progress, setProgress] = useState<{ done: number; total: number } | null>(null);
  const job = useRef<number | null>(null);
  useEffect(() => () => { if (job.current !== null) void api.cancel(job.current).catch(() => {}); }, []);
  const run = async () => {
    const s = useStore.getState();
    if (job.current !== null) {
      api.cancel(job.current);
      return;
    }
    if (s.scan) { s.toast('Wait for puzzle generation to finish first.'); return; }
    const line = mainline(s.tree);
    const moves = movesTo(s.tree, line[line.length - 1]);
    if (!moves.length) return;
    const wasOn = s.engineOn;
    s.setEngineOn(false);
    const id = nextJobId();
    job.current = id;
    setProgress({ done: 0, total: moves.length + 1 });
    try {
      const report = await api.reviewStart(id, { engineId: s.engineId, startFen: startFen(s.tree), moves, gameId: s.gameId }, (e) => {
        if (e.type === 'progress' && useStore.getState().tree === s.tree) {
          setProgress({ done: e.done, total: e.total });
          const ids = mainline(useStore.getState().tree);
          if (ids[e.ply]) useStore.getState().setNodeEval(ids[e.ply], e.score, 1);
        }
      });
      if (useStore.getState().tree === s.tree) useStore.getState().setReport(report);
      useStore.getState().toast(`Review complete${s.gameId ? ' and saved with the game' : ''}.`);
    } catch (e) {
      const msg = errorText(e);
      if (!/cancel/i.test(msg)) useStore.getState().toast(msg, 'error');
    } finally {
      job.current = null;
      setProgress(null);
      if (wasOn) useStore.getState().setEngineOn(true);
    }
  };
  return { progress, run };
}

export function Analyze() {
  const s = useStore();
  const engines = useEngineList();
  const node = s.tree.nodes[s.current];
  const [hover, setHover] = useState<string | null>(null);
  const [moveInput, setMoveInput] = useState('');
  const review = useReview();
  const [trainingSession, setTrainingSession] = useState<number | null>(null);
  const training = trainingSession === s.analysisSession;
  const [puzzlePosition, setPuzzlePosition] = useState<ChatPosition | null>(null);
  const trainingRef = useRef(training); trainingRef.current = training;
  useLiveAnalysis(training);
  useEffect(() => setTrainingSession(null), [s.current, s.analysisSession]);

  const pos = useMemo(() => positionFromFen(node.fen), [node.fen]);
  const status = useMemo(() => (pos ? statusOf(pos, repetitionCount(s.tree, s.current)) : null), [pos, s.tree, s.current]);
  const live = s.live && s.live.fen === node.fen ? s.live : null;
  const compare = s.compare && s.compare.fen === node.fen ? s.compare : null;

  const arrows: Arrow[] = [];
  if (s.prefs.engineArrows && s.engineOn && live) {
    live.lines.slice(0, s.prefs.arrowCount).forEach((l, i) => {
      const sq = l.uci[0] && uciSquares(l.uci[0]);
      if (sq) arrows.push({ from: makeSquare(sq[0]), to: makeSquare(sq[1]), kind: i === 0 ? 'best' : 'alt', weight: 1 - i / Math.max(3, s.prefs.arrowCount) });
    });
    const c = compare?.lines[0]?.uci[0] && uciSquares(compare.lines[0].uci[0]);
    if (c) arrows.push({ from: makeSquare(c[0]), to: makeSquare(c[1]), kind: 'compare' });
  }
  if (hover) {
    const sq = uciSquares(hover);
    if (sq) arrows.push({ from: makeSquare(sq[0]), to: makeSquare(sq[1]), kind: 'hint' });
  }
  const last = node.uci ? [node.uci.slice(0, 2), node.uci.slice(2, 4)] as [string, string] : null;

  // Board keyboard navigation (ignored while typing).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.target as HTMLElement).closest('input, textarea, select')) return;
      if (trainingRef.current) { if (e.key === 'Escape') setTrainingSession(null); return; }
      const st = useStore.getState();
      const keys: Record<string, () => void> = {
        ArrowLeft: st.prev,
        ArrowRight: st.next,
        ArrowUp: () => st.variation(-1),
        ArrowDown: () => st.variation(1),
        Home: st.first,
        End: st.last,
        f: st.flip,
        ' ': () => st.setEngineOn(!st.engineOn),
        a: () => st.setPrefs({ engineArrows: !st.prefs.engineArrows }),
      };
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 's') {
        e.preventDefault();
        saveToLibrary();
        return;
      }
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'v') {
        e.preventDefault();
        pasteGame();
        return;
      }
      if (e.ctrlKey || e.metaKey || e.altKey) return;
      const fn = keys[e.key];
      if (fn) {
        e.preventDefault();
        fn();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  const pasteGame = async () => {
    try {
      await openPgnText(await readClipboard(), 'clipboard');
    } catch (e) {
      useStore.getState().toast(errorText(e), 'error');
    }
  };

  const saveToLibrary = async () => {
    const st = useStore.getState();
    try {
      const id = await api.gameSave(st.gameId, toPgn(st.tree));
      st.markSaved(id);
      st.toast(st.gameId ? 'Game updated in the library.' : 'Saved to the library.');
    } catch (e) {
      st.toast(errorText(e), 'error');
    }
  };

  const submitMove = (e: React.FormEvent) => {
    e.preventDefault();
    if (!pos) return;
    const text = moveInput.trim();
    const m = moveFromSan(pos, text) ?? null;
    const ok = m ? s.play(uciOf(pos, m)) : s.play(text.toLowerCase());
    if (ok) setMoveInput('');
    else s.toast(`"${text}" is not a legal move here.`, 'error');
  };

  return (
    <div className="analyze">
      <GameDock />
      <div className="center-col">
        {training ? <PuzzleTrainer key={s.analysisSession} scope="game" gameId={s.gameId} embedded onClose={() => setTrainingSession(null)} onPosition={setPuzzlePosition} /> : <>
        <div className="player-strip"><span className={`player-dot ${s.orientation === 'white' ? 'black' : 'white'}`} /><span>{header(s.tree, s.orientation === 'white' ? 'Black' : 'White') ?? (s.orientation === 'white' ? 'Black' : 'White')}</span><span className="dim">{header(s.tree, s.orientation === 'white' ? 'BlackElo' : 'WhiteElo') ?? ''}</span></div>
        <div className="board-wrap">
          <Board
            fen={node.fen}
            orientation={s.orientation}
            lastMove={last}
            movable="both"
            onMove={(uci) => s.play(uci)}
            arrows={arrows}
            shapes={node.shapes ?? []}
            onShapes={s.setShapes}
            coordinates={s.prefs.coordinates}
          />
        </div>
        <div className="player-strip"><span className={`player-dot ${s.orientation}`} /><span>{header(s.tree, s.orientation === 'white' ? 'White' : 'Black') ?? (s.orientation === 'white' ? 'White' : 'Black')}</span><span className="dim">{header(s.tree, s.orientation === 'white' ? 'WhiteElo' : 'BlackElo') ?? ''}</span><span className="spacer" />
          <div className="board-navigation">
          <button className="btn small icon-only" onClick={s.first} title="Start (Home)" aria-label="First move"><Icon name="first" /></button>
          <button className="btn small icon-only" onClick={s.prev} title="Back (←)" aria-label="Previous move"><Icon name="prev" /></button>
          <button className="btn small icon-only" onClick={s.next} title="Forward (→)" aria-label="Next move"><Icon name="next" /></button>
          <button className="btn small icon-only" onClick={s.last} title="End (End)" aria-label="Last move"><Icon name="last" /></button>
          </div>
          <details className="board-tools"><summary title="Board tools and game files" aria-label="Board tools"><Icon name="settings" /></summary>
          <div className="board-bar">
          <button className="btn small icon-only" onClick={s.flip} title="Flip board (F)" aria-label="Flip board"><Icon name="flip" /></button>
          <button className={`btn small icon-only ${s.prefs.engineArrows ? 'on' : ''}`} onClick={() => s.setPrefs({ engineArrows: !s.prefs.engineArrows })} title="Engine arrows (A)" aria-pressed={s.prefs.engineArrows} aria-label="Toggle engine arrows"><Icon name="arrow" /></button>
          <form onSubmit={submitMove}>
            <input className="move-input" value={moveInput} onChange={(e) => setMoveInput(e.target.value)} placeholder="type a move…" aria-label="Type a move in SAN or UCI" />
          </form>
          {status?.text && <span className="status-text">{status.text}</span>}
          <div className="spacer" />

          <button className="btn small ghost" onClick={() => s.newGame()} title="New analysis from the starting position">New</button>
          <button className="btn small ghost" onClick={() => openPgnFile().catch((e) => s.toast(errorText(e), 'error'))} title="Open PGN / FEN file (Ctrl+O)"><Icon name="folder" /></button>
          <button className="btn small ghost" onClick={pasteGame} title="Paste PGN or FEN (Ctrl+V)"><Icon name="paste" /></button>
          <button className="btn small ghost" onClick={() => copyText(node.fen, 'FEN')} title="Copy FEN">FEN</button>
          <button className="btn small ghost" onClick={() => copyText(toPgn(s.tree), 'PGN')} title="Copy PGN">PGN</button>
          <button className="btn small ghost" onClick={() => savePgnFile().catch((e) => s.toast(errorText(e), 'error'))} title="Export PGN file"><Icon name="save" /></button>
          <button className="btn small" onClick={saveToLibrary} title="Save to library (Ctrl+S)"><Icon name="games" />{s.gameId ? 'Update' : 'Save'}</button>
          </div>
          </details>
        </div>
        </>}
        {s.prefs.aiPanel && <AiPanel position={training ? puzzlePosition : null} />}
      </div>
      <div className="right-col">
        {!training && <EnginePanel engines={engines} onHover={setHover} />}
        {!training && s.prefs.multipv > 1 && <LinePanel />}
        <MoveList reviewing={review.progress} onReview={() => { setTrainingSession(null); void review.run(); }} />
        <CurrentGamePuzzles disabled={!!review.progress} active={training} onPractice={() => setTrainingSession(s.analysisSession)} />
      </div>
    </div>
  );
}
