import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useStore } from '../store';
import { Board, type Mark } from '../board/Board';
import { Icon, Panel } from '../components/ui';
import { api, errorText } from '../lib/ipc';
import { playUci, uciLineToSan } from '../chess/position';
import { mainline } from '../chess/tree';
import { openGame } from './Games';
import { AiPanel, type ChatPosition } from './AiPanel';
import { scanText, startCurrentGameScan, stopScan, tacticsChanged } from '../tactics/scan';
import { basicPuzzles, checkBasic, type PracticePuzzle } from '../tactics/basics';

type Phase = 'loading' | 'setup' | 'solving' | 'wrong' | 'opponent' | 'solved' | 'revealed' | 'empty';
type Scope = 'basic' | 'library' | 'game';
interface Play {
  tactic: PracticePuzzle; fen: string; history: string[]; index: number; last: [string, string] | null;
  mistakes: number; hints: number; started: number; recorded: boolean;
  hintSquare: string | null; badSquare: string | null;
}
export const prettyTheme = (t: string) => t.replace(/([A-Z])/g, ' $1').replace(/^./, (c) => c.toUpperCase()).replace(/In(\d)/, ' in $1').replace('F2 F7', 'f2/f7');
const squares = (uci: string): [string, string] => [uci.slice(0, 2), uci.slice(2, 4)];

/** The same trainer is used inline in Analyze and on the standalone practice page. */
export function PuzzleTrainer({ scope, gameId, embedded = false, onClose, onPosition }: {
  scope: Scope; gameId?: number | null; embedded?: boolean; onClose?: () => void;
  onPosition?: (position: ChatPosition | null) => void;
}) {
  const toast = useStore((s) => s.toast);
  const [play, setPlay] = useState<Play | null>(null);
  const [phase, setPhase] = useState<Phase>('loading');
  const [tab, setTab] = useState<'puzzle' | 'agent'>('puzzle');
  const recent = useRef<number[]>([]);
  const basicIndex = useRef(0);
  const generation = useRef(0);
  const checking = useRef(false);
  const timers = useRef<ReturnType<typeof setTimeout>[]>([]);
  const phaseRef = useRef(phase); phaseRef.current = phase;
  const clearTimers = () => { timers.current.forEach(clearTimeout); timers.current = []; };
  const later = (fn: () => void, ms: number) => { timers.current.push(setTimeout(fn, ms)); };
  const loadNext = useCallback(async () => {
    const request = ++generation.current;
    clearTimers(); checking.current = false; setPhase('loading');
    try {
      let tactic: PracticePuzzle | null;
      if (scope === 'basic') tactic = basicPuzzles[basicIndex.current++ % basicPuzzles.length];
      else if (scope === 'game') {
        const puzzles = gameId == null ? [] : await api.tacticsForGame(gameId);
        tactic = puzzles.find((t) => !recent.current.includes(t.id)) ?? puzzles.find((t) => t.id !== recent.current.at(-1)) ?? puzzles[0] ?? null;
      } else tactic = await api.tacticNext({ exclude: recent.current.slice(-5) });
      if (request !== generation.current) return;
      if (!tactic) { setPlay(null); setPhase('empty'); return; }
      recent.current = [...recent.current.slice(-4), tactic.id];
      setPlay({ tactic, fen: tactic.fen, history: [], index: tactic.basic ? 0 : 1, last: null, mistakes: 0, hints: 0, started: Date.now(), recorded: false, hintSquare: null, badSquare: null });
      if (tactic.basic) setPhase('solving');
      else {
        setPhase('setup');
        later(() => {
          const after = playUci(tactic.fen, tactic.moves[0]);
          if (!after) { toast('This puzzle has an invalid starting move.', 'error'); setPhase('empty'); return; }
          setPlay((p) => p && { ...p, fen: after.fen, history: [tactic.moves[0]], last: squares(tactic.moves[0]), started: Date.now() });
          setPhase('solving');
        }, 500);
      }
    } catch (e) { if (request === generation.current) { toast(errorText(e), 'error'); setPlay(null); setPhase('empty'); } }
  }, [scope, gameId, toast]);
  useEffect(() => { recent.current = []; basicIndex.current = 0; void loadNext(); return () => { generation.current++; clearTimers(); }; }, [loadNext]);
  useEffect(() => {
    const refresh = () => { if (phaseRef.current === 'empty') void loadNext(); };
    tacticsChanged.addEventListener('change', refresh);
    return () => tacticsChanged.removeEventListener('change', refresh);
  }, [loadNext]);
  const done = phase === 'solved' || phase === 'revealed';
  const position = useMemo<ChatPosition | null>(() => {
    if (!play || phase === 'loading' || phase === 'empty') return null;
    const t = play.tactic;
    return { fen: play.fen, label: t.basic ? `${t.title} · ${t.solver} to solve` : `Puzzle from ${t.white} vs ${t.black} · ${t.solver} to solve`, context: {
      fen: play.fen, startFen: t.fen, historyUci: play.history, engines: [], review: [],
      game: t.basic ? null : { white: t.white, black: t.black, date: t.date },
      tactic: { themes: done ? t.themes : [], solutionUci: done ? t.moves.slice(play.index) : [], revealed: done, playedInGame: null },
      userColor: t.solver,
    } };
  }, [play, phase, done]);
  useEffect(() => { onPosition?.(position); }, [position, onPosition]);
  useEffect(() => () => onPosition?.(null), [onPosition]);
  const record = async (p: Play, solved: boolean) => {
    if (p.recorded || p.tactic.basic) return;
    p.recorded = true;
    try { await api.tacticAttempt(p.tactic.id, solved, p.mistakes, p.hints, Date.now() - p.started); }
    catch (e) { toast(errorText(e), 'error'); }
  };
  const onMove = async (uci: string) => {
    if (!play || checking.current || (phase !== 'solving' && phase !== 'wrong')) return;
    checking.current = true;
    const request = generation.current, p = play;
    try {
      const verdict = p.tactic.basic ? checkBasic(p.tactic, p.fen, uci) : await api.tacticCheck(p.tactic.id, p.index, uci);
      if (request !== generation.current) return;
      if (verdict.result === 'incorrect') {
        setPlay({ ...p, mistakes: p.mistakes + 1, badSquare: uci.slice(2, 4), hintSquare: null }); setPhase('wrong'); return;
      }
      const after = playUci(p.fen, uci);
      if (!after) return;
      const next: Play = { ...p, fen: after.fen, history: [...p.history, uci], index: p.index + (verdict.solved ? 1 : 2), last: squares(uci), badSquare: null, hintSquare: null };
      setPlay(next);
      if (verdict.solved) { setPhase('solved'); void record(next, true); }
      else {
        setPhase('opponent');
        later(() => {
          const reply = verdict.reply && playUci(after.fen, verdict.reply);
          if (reply) setPlay((cur) => cur && { ...cur, fen: reply.fen, history: [...cur.history, verdict.reply!], last: squares(verdict.reply!) });
          setPhase('solving');
        }, 420);
      }
    } catch (e) { if (request === generation.current) toast(errorText(e), 'error'); }
    finally { if (request === generation.current) checking.current = false; }
  };
  const reveal = () => {
    if (!play) return;
    clearTimers(); setPhase('revealed'); void record(play, false);
    let fen = play.fen;
    play.tactic.moves.slice(play.index).forEach((uci, i) => later(() => {
      const after = playUci(fen, uci);
      if (after) { fen = after.fen; setPlay((p) => p && { ...p, fen, history: [...p.history, uci], index: play.index + i + 1, last: squares(uci), hintSquare: null, badSquare: null }); }
    }, (i + 1) * 420));
  };
  const openInGame = async () => {
    if (!play || play.tactic.basic) return;
    try { await openGame(play.tactic.gameId); const s = useStore.getState(); const id = mainline(s.tree)[play.tactic.ply + 1]; if (id) s.goto(id); }
    catch (e) { toast(errorText(e), 'error'); }
  };
  const t = play?.tactic;
  const marks: Mark[] = [];
  if (play?.hintSquare) marks.push({ square: play.hintSquare, kind: 'hint' });
  if (play?.badSquare) marks.push({ square: play.badSquare, kind: 'bad' });
  const board = <div className="board-wrap"><Board fen={play?.fen ?? '8/8/8/8/8/8/8/8 w - - 0 1'} orientation={t?.solver ?? 'white'} lastMove={play?.last} movable={phase === 'solving' || phase === 'wrong' ? t?.solver ?? 'none' : 'none'} onMove={onMove} marks={marks} label="Tactic board" /></div>;
  const controls = <div className="puzzle-controls">
    <div className={`puzzle-status ${phase === 'wrong' ? 'bad' : ''}`} aria-live="polite">
      {phase === 'loading' ? 'Loading puzzle…' : phase === 'empty' ? (scope === 'game' ? 'No qualifying puzzles in this game.' : 'No generated puzzles are ready. Generate some below, or return when a review is due.') : phase === 'setup' ? 'Watch the last move…' : phase === 'wrong' ? 'Try another move.' : phase === 'opponent' ? 'Opponent replies…' : phase === 'solved' ? 'Solved!' : phase === 'revealed' ? 'Solution shown' : `${t?.solver === 'white' ? 'White' : 'Black'} to play and ${t?.kind === 'mate' ? 'mate' : 'win'}`}
    </div>
    {play && t && <>
      <p className="small dim">{t.basic ? `Basic practice · ${basicPuzzles.findIndex((p) => p.id === t.id) + 1} / ${basicPuzzles.length}` : `${t.white} vs ${t.black} · move ${Math.floor(t.ply / 2) + 1}`}</p>
      <div className="row">
        <button className="btn small" disabled={done || phase !== 'solving' && phase !== 'wrong'} onClick={() => setPlay({ ...play, hints: play.hints + 1, hintSquare: t.moves[play.index]?.slice(0, 2) ?? null })}><Icon name="hint" />Hint</button>
        <button className="btn small" disabled={done || phase === 'setup' || phase === 'opponent' || phase === 'loading'} onClick={reveal}>Solution</button>
        <button className="btn small" onClick={() => void loadNext()}>{done ? 'Next puzzle' : 'Skip puzzle'}<Icon name="next" /></button>
      </div>
      {done && <><div className="theme-list">{t.themes.map((th) => <span key={th} className="chip">{prettyTheme(th)}</span>)}</div><p className="small">Solution: {uciLineToSan(t.fen, t.moves).slice(t.basic ? 0 : 1).join(' ')}</p>{!embedded && !t.basic && <button className="btn small" onClick={openInGame}>Open in game</button>}</>}
    </>}
  </div>;
  if (embedded) return <div className="inline-trainer"><div className="row"><span className="label">Puzzle from this game</span><span className="spacer" /><button className="btn small" onClick={onClose}>Back to game</button></div>{board}{controls}</div>;
  return <div className="puzzle-layout"><div className="center-col">{board}</div><div className="puzzle-side">
    <div className="tabs" role="tablist" aria-label="Puzzle sidebar">
      <button id="puzzle-controls-tab" role="tab" aria-controls="puzzle-controls-panel" aria-selected={tab === 'puzzle'} className={tab === 'puzzle' ? 'active' : ''} onClick={() => setTab('puzzle')}>Puzzle</button>
      <button id="puzzle-agent-tab" role="tab" aria-controls="puzzle-agent-panel" aria-selected={tab === 'agent'} className={tab === 'agent' ? 'active' : ''} onClick={() => setTab('agent')}>Ask agent</button>
    </div>
    <div id="puzzle-controls-panel" role="tabpanel" aria-labelledby="puzzle-controls-tab" className="puzzle-tab-panel" hidden={tab !== 'puzzle'}><Panel title={scope === 'basic' ? 'Basic practice' : 'Puzzles from your games'} collapsible={false}>{controls}</Panel></div>
    <div id="puzzle-agent-panel" role="tabpanel" aria-labelledby="puzzle-agent-tab" className="puzzle-tab-panel" hidden={tab !== 'agent'}>
      {position && t ? <AiPanel key={`${scope}:${t.id}`} session={`puzzle:${t.id}`} position={position} /> : <p className="dim small">Load a puzzle to discuss it with your agent.</p>}
    </div>
  </div></div>;
}

export function CurrentGamePuzzles({ active, onPractice, disabled = false }: { active: boolean; onPractice: () => void; disabled?: boolean }) {
  const gameId = useStore((s) => s.gameId);
  const tree = useStore((s) => s.tree);
  const dirty = useStore((s) => s.dirty);
  const scan = useStore((s) => s.scan);
  const [count, setCount] = useState(0);
  const [busy, setBusy] = useState(false);
  const [checked, setChecked] = useState(false);
  useEffect(() => {
    let live = true;
    const refresh = () => {
      if (gameId == null || dirty) { setCount(0); setChecked(false); return; }
      api.tacticsForGame(gameId).then((p) => { if (live) setCount(p.length); }).catch(() => {});
    };
    refresh(); tacticsChanged.addEventListener('change', refresh);
    return () => { live = false; tacticsChanged.removeEventListener('change', refresh); };
  }, [gameId, dirty]);
  const generate = async () => {
    const session = useStore.getState().analysisSession;
    setBusy(true);
    try {
      await startCurrentGameScan();
      const current = useStore.getState();
      if (current.analysisSession !== session || current.gameId == null) return;
      const puzzles = await api.tacticsForGame(current.gameId);
      setCount(puzzles.length); setChecked(true);
      if (puzzles.length) onPractice();
    } catch (e) { useStore.getState().toast(errorText(e), 'error'); }
    finally { setBusy(false); }
  };
  return <section className="puzzle-callout panel"><h3><Icon name="puzzles" />Puzzles from this game</h3>
    <p className="dim small">{count ? `${count} puzzle${count === 1 ? '' : 's'} ready to practice here.` : checked ? 'No qualifying tactics found in this game.' : 'Find and practice tactical opportunities on this board.'}</p>
    {busy && scan ? <><p className="small">{scanText(scan.progress)}</p><button className="btn small" onClick={stopScan}>Stop generation</button></> : <button className="btn primary" disabled={disabled || !!scan || busy || active || mainline(tree).length < 3} onClick={count && !dirty ? onPractice : generate}>{active ? 'Practicing' : count && !dirty ? 'Practice puzzles' : 'Generate puzzles'}</button>}
  </section>;
}
