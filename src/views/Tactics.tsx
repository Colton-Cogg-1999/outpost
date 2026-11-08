import { useCallback, useEffect, useRef, useState } from 'react';
import { useStore } from '../store';
import { Board, type Mark } from '../board/Board';
import { Icon, Panel, Progress } from '../components/ui';
import { api, errorText, nextJobId } from '../lib/ipc';
import { playUci, uciLineToSan, uciSquares } from '../chess/position';
import { mainline } from '../chess/tree';
import { AnswerText } from './AiPanel';
import { openGame } from './Games';
import { scanText, startScan, stopScan, tacticsChanged } from '../tactics/scan';
import type { MoveCheck, Strictness, Tactic, TacticStats } from '../lib/types';
import { makeSquare } from 'chessops/util';

type Phase = 'loading' | 'setup' | 'solving' | 'wrong' | 'opponent' | 'solved' | 'revealed' | 'empty';

interface Play {
  tactic: Tactic;
  fen: string;
  /** Index in tactic.moves of the solver's next move (1, 3, 5, …). */
  index: number;
  last: [string, string] | null;
  mistakes: number;
  hints: number;
  started: number;
  recorded: boolean;
  hintSquare: string | null;
  badSquare: string | null;
}

export const prettyTheme = (t: string) =>
  t
    .replace(/([A-Z])/g, ' $1')
    .replace(/^./, (c) => c.toUpperCase())
    .replace(/In(\d)/, ' in $1')
    .replace('F2 F7', 'f2/f7')
    .replace('X Ray', 'X-ray');

const squares = (uci: string): [string, string] => [uci.slice(0, 2), uci.slice(2, 4)];

function gameLine(t: Tactic) {
  const moveNo = Math.floor(t.ply / 2) + 1;
  const who = t.ply % 2 === 0 ? `${moveNo}.` : `${moveNo}…`;
  return { moveNo, who };
}

export function Tactics() {
  const toast = useStore((s) => s.toast);
  const scan = useStore((s) => s.scan);
  const [stats, setStats] = useState<TacticStats | null>(null);
  const [themes, setThemes] = useState<[string, number][]>([]);
  const [owner, setOwner] = useState<'' | 'you' | 'opponent'>('');
  const [theme, setTheme] = useState('');
  const [play, setPlay] = useState<Play | null>(null);
  const [phase, setPhase] = useState<Phase>('loading');
  const [explain, setExplain] = useState<{ text: string; moves: MoveCheck } | null>(null);
  const [explaining, setExplaining] = useState(false);
  const timers = useRef<number[]>([]);
  const recent = useRef<number[]>([]);
  const phaseRef = useRef<Phase>(phase);
  phaseRef.current = phase;

  const later = (fn: () => void, ms: number) => timers.current.push(window.setTimeout(fn, ms));
  useEffect(() => () => timers.current.forEach(clearTimeout), []);

  const refresh = useCallback(() => {
    api.tacticsStats().then(setStats).catch(() => {});
    api.tacticsThemes().then(setThemes).catch(() => {});
  }, []);

  const loadNext = useCallback(async () => {
    timers.current.forEach(clearTimeout);
    setPhase('loading');
    setExplain(null);
    try {
      const tactic = await api.tacticNext({ owner, theme, exclude: recent.current.slice(-5) });
      if (!tactic) {
        setPlay(null);
        setPhase('empty');
        return;
      }
      recent.current.push(tactic.id);
      setPlay({ tactic, fen: tactic.fen, index: 1, last: null, mistakes: 0, hints: 0, started: Date.now(), recorded: false, hintSquare: null, badSquare: null });
      setPhase('setup');
      // Replay the opponent's mistake so the position is seen as it arose.
      later(() => {
        const r = playUci(tactic.fen, tactic.moves[0]);
        if (!r) return;
        setPlay((cur) => (cur && cur.tactic.id === tactic.id ? { ...cur, fen: r.fen, last: squares(tactic.moves[0]), started: Date.now() } : cur));
        setPhase('solving');
      }, 650);
    } catch (e) {
      toast(errorText(e), 'error');
      setPhase('empty');
    }
  }, [owner, theme, toast]);

  useEffect(refresh, [refresh]);
  useEffect(() => {
    loadNext();
  }, [loadNext]);
  // New tactics arrive while a scan runs: refresh the counts, and start training once there is something.
  useEffect(() => {
    const onChange = () => {
      refresh();
      if (phaseRef.current === 'empty') loadNext();
    };
    tacticsChanged.addEventListener('change', onChange);
    return () => tacticsChanged.removeEventListener('change', onChange);
  }, [refresh, loadNext]);

  const record = async (p: Play, solved: boolean) => {
    if (p.recorded) return;
    p.recorded = true;
    try {
      await api.tacticAttempt(p.tactic.id, solved, p.mistakes, p.hints, Date.now() - p.started);
    } catch (e) {
      toast(errorText(e), 'error');
    }
    refresh();
  };

  const onMove = async (uci: string) => {
    if (!play || (phase !== 'solving' && phase !== 'wrong')) return;
    const p = play;
    try {
      const verdict = await api.tacticCheck(p.tactic.id, p.index, uci);
      if (verdict.result === 'incorrect') {
        const sq = uciSquares(uci);
        setPlay({ ...p, mistakes: p.mistakes + 1, badSquare: sq ? makeSquare(sq[1]) : null, hintSquare: null });
        setPhase('wrong');
        return;
      }
      const after = playUci(p.fen, uci)!;
      const next: Play = { ...p, fen: after.fen, last: squares(uci), badSquare: null, hintSquare: null, index: p.index + 2 };
      setPlay(next);
      if (verdict.solved) {
        setPhase('solved');
        record(next, true);
        return;
      }
      setPhase('opponent');
      const reply = verdict.reply!;
      later(() => {
        const r = playUci(after.fen, reply);
        if (r) setPlay((cur) => cur && { ...cur, fen: r.fen, last: squares(reply) });
        setPhase('solving');
      }, 420);
    } catch (e) {
      toast(errorText(e), 'error');
    }
  };

  const hint = () => {
    if (!play) return;
    const expected = play.tactic.moves[play.index];
    setPlay({ ...play, hints: play.hints + 1, hintSquare: expected.slice(0, 2) });
  };

  const reveal = () => {
    if (!play) return;
    const rest = play.tactic.moves.slice(play.index);
    let fen = play.fen;
    setPhase('revealed');
    record(play, false);
    rest.forEach((uci, i) => {
      later(() => {
        const r = playUci(fen, uci);
        if (!r) return;
        fen = r.fen;
        setPlay((cur) => cur && { ...cur, fen: r.fen, last: squares(uci), hintSquare: null, badSquare: null });
      }, 480 * (i + 1));
    });
  };

  const openInGame = async () => {
    if (!play) return;
    try {
      await openGame(play.tactic.gameId);
      const st = useStore.getState();
      const ids = mainline(st.tree);
      // ids[0] is the start; ids[ply + 1] is the position right after the mistake.
      const target = ids[Math.min(play.tactic.ply + 1, ids.length - 1)];
      if (target) st.goto(target);
    } catch (e) {
      toast(errorText(e), 'error');
    }
  };

  const askAi = async () => {
    if (!play) return;
    setExplaining(true);
    const t = play.tactic;
    const afterMistake = playUci(t.fen, t.moves[0])!;
    try {
      const answer = await api.aiAsk(
        nextJobId(),
        {
          question: 'Explain this tactic from my game: what did the last move allow, what is the key idea of the solution, and what defences does it beat? Name the tactical theme.',
          label: '[EXPLAIN TACTIC]',
          position: {
            fen: afterMistake.fen,
            startFen: t.fen,
            historyUci: [t.moves[0]],
            engines: [],
            review: [],
            tactic: { themes: t.themes, solutionUci: t.moves.slice(1), revealed: true, playedInGame: t.played },
            userColor: t.owner === 'you' ? t.solver : t.owner === 'opponent' ? (t.solver === 'white' ? 'black' : 'white') : null,
          },
          history: [],
          chatId: null,
        },
        () => {},
      );
      setExplain({ text: answer.text, moves: answer.moves });
    } catch (e) {
      toast(errorText(e), 'error');
    } finally {
      setExplaining(false);
    }
  };

  const t = play?.tactic;
  const marks: Mark[] = [];
  if (play?.hintSquare) marks.push({ square: play.hintSquare, kind: 'hint' });
  if (play?.badSquare) marks.push({ square: play.badSquare, kind: 'bad' });
  const done = phase === 'solved' || phase === 'revealed';
  const san = t ? uciLineToSan(t.fen, t.moves) : [];
  const playedSan = t?.played ? uciLineToSan(playUci(t.fen, t.moves[0])?.fen ?? t.fen, [t.played])[0] : null;
  const line = t ? gameLine(t) : null;
  const side = t?.solver === 'white' ? 'White' : 'Black';

  return (
    <div className="puzzle-layout">
      <div className="center-col">
        <div className="board-wrap">
          {play && t ? (
            <Board fen={play.fen} orientation={t.solver} lastMove={play.last} movable={phase === 'solving' || phase === 'wrong' ? t.solver : 'none'} onMove={onMove} marks={marks} label="Tactic board" />
          ) : (
            <Board fen="8/8/8/8/8/8/8/8 w - - 0 1" orientation="white" movable="none" />
          )}
        </div>
      </div>
      <div className="puzzle-side">
        <Panel id="tactic" title="Tactic" collapsible={false}>
          {phase === 'empty' && <EmptyState stats={stats} filtered={!!(owner || theme)} />}
          {play && t && line && (
            <div style={{ display: 'grid', gap: 10 }}>
              <div className={`puzzle-status ${phase === 'wrong' ? 'bad' : ''}`} aria-live="polite">
                {phase === 'setup' && `${line.who} ${san[0]}…`}
                {phase === 'solving' && `${side} to play and ${t.kind === 'mate' ? 'mate' : 'win'}`}
                {phase === 'wrong' && 'Not the move. Try again.'}
                {phase === 'opponent' && 'Opponent replies…'}
                {phase === 'solved' && (play.mistakes || play.hints ? 'Solved (with help)' : 'Solved!')}
                {phase === 'revealed' && 'Solution shown'}
                {phase === 'loading' && 'Loading…'}
              </div>
              <div className="small">
                <span className="dim">From </span>
                <b>{t.white}</b> <span className="dim">vs</span> <b>{t.black}</b>
                {t.date && <span className="dim"> · {t.date.replace(/\./g, '-')}</span>}
                <br />
                <span className="dim">After {line.who} {san[0]}{t.owner === 'opponent' ? ' (your move)' : ''}</span>
              </div>
              {done && (
                <div className="notice small">
                  {t.owner === 'you' && (playedSan ? <>You had this chance in the game and played <b>{playedSan}</b> instead.</> : <>You found this in the game.</>)}
                  {t.owner === 'opponent' && (playedSan ? <>Your move allowed this. Your opponent missed it and played <b>{playedSan}</b>.</> : <>Your move allowed this, and your opponent found it.</>)}
                  {t.owner === null && (playedSan ? <>In the game, {playedSan} was played instead.</> : <>The game continued with the solution.</>)}
                </div>
              )}
              <div className="row">
                <button className="btn" onClick={hint} disabled={done || phase !== 'solving' && phase !== 'wrong'}><Icon name="hint" />Hint</button>
                <button className="btn" onClick={reveal} disabled={done || phase === 'setup' || phase === 'opponent'}><Icon name="eye" />Solution</button>
                <button className="btn primary" onClick={loadNext}><Icon name="next" />{done ? 'Next' : 'Skip'}</button>
              </div>
              {done && (
                <>
                  <div className="theme-list">{t.themes.map((th) => <span key={th} className="chip">{prettyTheme(th)}</span>)}</div>
                  <span className="small dim">Solution: <span className="san-plain">{san.slice(1).join(' ')}</span></span>
                  <div className="row">
                    <button className="btn small" onClick={openInGame}><Icon name="analyze" />Open in game</button>
                    <button className="btn small" onClick={askAi} disabled={explaining}><Icon name="spark" />{explaining ? 'Asking…' : 'Explain with AI'}</button>
                  </div>
                  {explain && (
                    <div className="ai-msg">
                      <AnswerText text={explain.text} valid={[]} onPlay={() => {}} />
                    </div>
                  )}
                </>
              )}
              {t.attempts > 0 && !done && <span className="faint small">Review · solved cleanly {t.streak}× in a row</span>}
            </div>
          )}
        </Panel>

        <Panel id="tactic-filter" title="Training">
          <div style={{ display: 'grid', gap: 10 }}>
            <label className="field">
              <span>Whose chance</span>
              <select value={owner} onChange={(e) => setOwner(e.target.value as typeof owner)}>
                <option value="">All tactics from my games</option>
                <option value="you">My chances (found or missed)</option>
                <option value="opponent">Shots my mistakes allowed</option>
              </select>
            </label>
            <label className="field">
              <span>Theme</span>
              <select value={theme} onChange={(e) => setTheme(e.target.value)}>
                <option value="">Any theme</option>
                {themes.map(([th, n]) => <option key={th} value={th}>{prettyTheme(th)} ({n})</option>)}
              </select>
            </label>
          </div>
        </Panel>

        <ScanPanel stats={stats} scanning={!!scan} />

        {stats && stats.tactics > 0 && (
          <Panel id="tactic-stats" title="Progress">
            <div className="stat-grid">
              <div className="stat"><div className="v">{stats.tactics}</div><div className="k">Tactics</div></div>
              <div className="stat"><div className="v">{stats.missed}</div><div className="k">Missed by you</div></div>
              <div className="stat"><div className="v">{stats.due + stats.fresh}</div><div className="k">To train</div></div>
              <div className="stat"><div className="v">{stats.streak}</div><div className="k">Streak</div></div>
            </div>
            <p className="faint small" style={{ marginTop: 8 }}>Solve one cleanly and it comes back in 1, 3, 7, 21, then 60 days; miss it and it returns in ten minutes.</p>
          </Panel>
        )}
      </div>
    </div>
  );
}

function EmptyState({ stats, filtered }: { stats: TacticStats | null; filtered: boolean }) {
  if (filtered) return <p className="dim">Nothing matches these filters right now. Choose “All tactics” or another theme.</p>;
  if (!stats || stats.games === 0) return <p className="dim">Import your games (Lichess or Chess.com) and scan them: every position where a move handed someone a forced win becomes a tactic to train.</p>;
  if (stats.tactics === 0) return <p className="dim">No tactics yet. Scan your games below to find them.</p>;
  return <p className="dim">You are up to date: nothing is due. Scan more games, or come back later for reviews.</p>;
}

function ScanPanel({ stats, scanning }: { stats: TacticStats | null; scanning: boolean }) {
  const scan = useStore((s) => s.scan);
  const [maxGames, setMaxGames] = useState(25);
  const [strictness, setStrictness] = useState<Strictness>('personal');
  const [onlyMine, setOnlyMine] = useState(true);
  const remaining = stats ? Math.max(0, stats.games - stats.scanned) : 0;
  const p = scan?.progress ?? null;
  const fraction = p && p.games ? (p.gameIndex + (p.total ? (p.stage === 'review' ? 0.7 * p.done / p.total : 0.7 + 0.3 * (p.done + 1) / p.total) : 0)) / p.games : 0;

  return (
    <Panel id="tactic-scan" title="Find tactics in my games">
      <div style={{ display: 'grid', gap: 10 }}>
        <p className="dim small">
          {stats ? <>{stats.scanned} of {stats.games} game{stats.games === 1 ? '' : 's'} scanned{remaining ? `, ${remaining} to go` : ''}. </> : null}
          Each game is reviewed by the built-in engine, then searched with the lichess-puzzler method: only lines where one move wins are kept.
        </p>
        {scanning ? (
          <>
            <span className="small">{scanText(p)}</span>
            <Progress value={fraction} />
            <div className="row"><button className="btn" onClick={stopScan}>Stop</button></div>
          </>
        ) : (
          <>
            <div className="grid2">
              <label className="field">
                <span>Games (newest first)</span>
                <select value={maxGames} onChange={(e) => setMaxGames(Number(e.target.value))}>
                  {[10, 25, 50, 100, 500].map((n) => <option key={n} value={n}>{n}</option>)}
                </select>
              </label>
              <label className="field">
                <span>Rules</span>
                <select value={strictness} onChange={(e) => setStrictness(e.target.value as Strictness)}>
                  <option value="personal">Personal</option>
                  <option value="lichess">Lichess standard</option>
                </select>
              </label>
            </div>
            <label className="row small" style={{ gap: 8 }}>
              <input type="checkbox" checked={onlyMine} onChange={(e) => setOnlyMine(e.target.checked)} />
              Only games from my synced accounts
            </label>
            <div className="row">
              <button className="btn primary" onClick={() => startScan({ strictness, onlyMine, rescan: false, maxGames })} disabled={!stats || remaining === 0}>
                <Icon name="antenna" />Scan {Math.min(maxGames, remaining || maxGames)} game{Math.min(maxGames, remaining || maxGames) === 1 ? '' : 's'}
              </button>
              {stats && stats.scanned > 0 && (
                <button className="btn" onClick={() => startScan({ strictness, onlyMine, rescan: true, maxGames })} title="Scan already-scanned games again, e.g. with other rules">Rescan</button>
              )}
            </div>
            <span className="faint small">Takes 10–60 s per game; you can keep using OUTPOST meanwhile. Reviews are saved with each game.</span>
          </>
        )}
      </div>
    </Panel>
  );
}
