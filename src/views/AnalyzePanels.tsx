import { Fragment, useEffect, useMemo, useState } from 'react';
import { useStore } from '../store';
import { CLASS_LABEL, ClassIcon, Icon, Panel, Toggle, formatScore } from '../components/ui';
import { EvalGraph } from './EvalGraph';
import { evalSeries, reviewByNode } from './analysis';
import { mainline, type GameTree } from '../chess/tree';
import { positionFromFen } from '../chess/position';
import type { Classification, EngineRecord, MoveReview, PvLine, Snapshot } from '../lib/types';
import { api } from '../lib/ipc';

function numberedLine(fen: string, san: string[], max = 99): { num?: string; san: string; index: number }[] {
  const pos = positionFromFen(fen);
  let number = pos?.fullmoves ?? 1;
  let white = (pos?.turn ?? 'white') === 'white';
  return san.slice(0, max).map((m, i) => {
    const item = { num: white ? `${number}.` : i === 0 ? `${number}…` : undefined, san: m, index: i };
    if (!white) number++;
    white = !white;
    return item;
  });
}

function PvRow({ line, fen, top, onPlay, onHover }: { line: PvLine; fen: string; top: boolean; onPlay: () => void; onHover: (on: boolean) => void }) {
  const items = numberedLine(fen, line.san, 8);
  return (
    <button className={`pv ${top ? 'top' : ''}`} onClick={onPlay} onMouseEnter={() => onHover(true)} onMouseLeave={() => onHover(false)} title="Play this line's first move">
      <span className="dim">{line.multipv}.</span>
      <span className="moves">
        {items.map((m) => (
          <Fragment key={m.index}>
            {m.num && <span className="dim">{m.num}</span>}
            {m.index === 0 ? <b>{m.san}</b> : m.san}{' '}
          </Fragment>
        ))}
      </span>
      <span className="score">{formatScore(line.score)}</span>
    </button>
  );
}

function formatNodes(n: number | null) {
  if (!n) return '—';
  if (n > 1e9) return `${(n / 1e9).toFixed(1)}G`;
  if (n > 1e6) return `${(n / 1e6).toFixed(1)}M`;
  if (n > 1e3) return `${(n / 1e3).toFixed(0)}k`;
  return String(n);
}

function Lines({ snap, fen, onHover }: { snap: Snapshot; fen: string; onHover: (uci: string | null) => void }) {
  const playLine = useStore((s) => s.playLine);
  if (snap.terminal) return <p className="dim">{snap.terminal === 'checkmate' ? 'Checkmate.' : snap.terminal === 'stalemate' ? 'Stalemate.' : 'Draw by insufficient material.'}</p>;
  return (
    <>
      <div className="pv-list">
        {snap.lines.map((l, i) => (
          <PvRow key={l.multipv} line={l} fen={fen} top={i === 0} onPlay={() => playLine(l.uci.slice(0, 1))} onHover={(on) => onHover(on ? l.uci[0] : null)} />
        ))}
      </div>
      <div className="engine-meta">
        <span>depth {snap.depth}{snap.seldepth ? `/${snap.seldepth}` : ''}</span>
        <span>nodes {formatNodes(snap.nodes)}</span>
        {snap.nps ? <span>{formatNodes(snap.nps)}/s</span> : null}
        {snap.lines[0]?.wdl && <span title="Win / draw / loss per mille, White's view">W/D/L {snap.lines[0].wdl.join('/')}</span>}
        {snap.cached && <span title="From the local evaluation cache; a live search is refining it">cached</span>}
      </div>
    </>
  );
}

export function EnginePanel({ engines, onHover }: { engines: EngineRecord[]; onHover: (uci: string | null) => void }) {
  const s = useStore();
  const node = s.tree.nodes[s.current];
  const live = s.live && s.live.fen === node.fen ? s.live : null;
  const compare = s.compare && s.compare.fen === node.fen ? s.compare : null;
  const report = s.report;
  const series = useMemo(() => evalSeries(s.tree, report, s.nodeEvals), [s.tree, report, s.nodeEvals]);
  const engine = engines.find((e) => e.id === s.engineId);
  const blackToMove = node.fen.split(' ')[1] === 'b';
  const top = live?.lines[0];
  const badge = live ? formatScore(top?.score, live.terminal) : s.engineOn ? '…' : 'off';

  return (
    <Panel
      id="engine"
      className="engine-panel"
      title={
        <div className="engine-head" style={{ display: 'contents' }}>
          <span className="name">
            <Icon name="engines" />
            {engines.length > 1 ? (
              <select value={s.engineId ?? ''} onChange={(e) => s.setEngine(e.target.value)} aria-label="Engine">
                {engines.map((e) => <option key={e.id} value={e.id}>{e.name}</option>)}
              </select>
            ) : (
              <span>{engine?.name ?? 'No engine'}</span>
            )}
            {live && <span className="dim">(depth {live.depth})</span>}
          </span>
        </div>
      }
      right={
        <>
          <button className={`btn small icon-only ${s.engineOn ? 'on' : ''}`} onClick={() => s.setEngineOn(!s.engineOn)} title="Toggle engine (Space)" aria-pressed={s.engineOn} disabled={!engine}>
            <Icon name="power" />
          </button>
          <span className={`eval-badge ${!live ? 'off' : top && top.score.value < 0 ? 'black' : ''}`} title={blackToMove ? 'Black to move · scores from White’s view' : 'White to move · scores from White’s view'}>{badge}</span>
        </>
      }
    >
      {!engine && (
        <div className="notice">
          No chess engine yet. <button className="btn small" onClick={() => s.setView('engines')}>Set up Stockfish</button>
        </div>
      )}
      {engine && s.engineError && <div className="notice error">{s.engineError}</div>}
      {engine && !s.engineOn && !s.engineError && <p className="dim small">Engine paused. Press <kbd>Space</kbd> or the power button to analyse.</p>}
      {engine && s.engineOn && !live && !s.engineError && <p className="dim small">Starting {engine.name}<span className="blink">_</span></p>}
      {live && <Lines snap={live} fen={node.fen} onHover={onHover} />}
      <div className="row" style={{ marginTop: 6 }}>
        <span className="small dim">Lines</span>
        <button className="btn small icon-only" onClick={() => s.setPrefs({ multipv: Math.max(1, s.prefs.multipv - 1) })} aria-label="Fewer lines"><Icon name="minus" /></button>
        <span className="mono-num small">{s.prefs.multipv}</span>
        <button className="btn small icon-only" onClick={() => s.setPrefs({ multipv: Math.min(8, s.prefs.multipv + 1) })} aria-label="More lines"><Icon name="plus" /></button>
        <select value={s.prefs.depthLimit ?? 0} onChange={(e) => s.setPrefs({ depthLimit: Number(e.target.value) || null })} aria-label="Search limit" style={{ height: 24, padding: '0 6px', fontSize: 11 }}>
          <option value={0}>∞ infinite</option>
          {[12, 16, 20, 24, 30, 40].map((d) => <option key={d} value={d}>depth {d}</option>)}
        </select>
        <div className="spacer" />
        {engines.length > 1 && (
          <select value={s.compareId ?? ''} onChange={(e) => s.setCompare(e.target.value || null)} aria-label="Compare with engine" style={{ height: 24, padding: '0 6px', fontSize: 11 }}>
            <option value="">compare: off</option>
            {engines.filter((e) => e.id !== s.engineId).map((e) => <option key={e.id} value={e.id}>vs {e.name}</option>)}
          </select>
        )}
      </div>
      {s.compareId && (
        <div className="compare-block">
          <div className="row">
            <span className="label">{compare?.engine ?? engines.find((e) => e.id === s.compareId)?.name} — independent search</span>
            <div className="spacer" />
            {compare && <span className="mono-num">{formatScore(compare.lines[0]?.score, compare.terminal)}</span>}
          </div>
          {compare ? <Lines snap={compare} fen={node.fen} onHover={onHover} /> : <p className="dim small">Waiting for the second engine…</p>}
          <p className="heuristic-note">Separate engines, separate searches: scores use each engine's own scale, and depth means different amounts of work for different engines.</p>
        </div>
      )}
      <EvalGraph points={series} current={s.current} onSelect={s.goto} />
    </Panel>
  );
}

export function LinePanel() {
  const s = useStore();
  const node = s.tree.nodes[s.current];
  const live = s.live && s.live.fen === node.fen ? s.live : null;
  const line = live?.lines[0];
  if (!line || !line.san.length) return null;
  const items = numberedLine(node.fen, line.san, 16);
  return (
    <Panel id="line" title={<h2>✦ Line (<span className="san-plain">{line.san[0]}</span>)</h2>}>
      <div className="line-moves">
        {items.map((m) => (
          <Fragment key={m.index}>
            {m.num && <span className="num">{m.num}</span>}
            <button onClick={() => s.playLine(line.uci.slice(0, m.index + 1))} title="Play the line up to this move">{m.san}</button>
          </Fragment>
        ))}
      </div>
    </Panel>
  );
}

// ───────────── move list ─────────────

function Variation({ tree, start, current, goto, depth = 0 }: { tree: GameTree; start: string; current: string; goto: (id: string) => void; depth?: number }) {
  const out: React.ReactNode[] = [];
  let id: string | undefined = start;
  let first = true;
  while (id) {
    const n: GameTree['nodes'][string] = tree.nodes[id];
    const [, turn, , , , full] = tree.nodes[n.parent!].fen.split(' ');
    const white = turn === 'w';
    const num = Number(full) || 1;
    out.push(
      <Fragment key={id}>
        {(white || first) && <span className="dim">{white ? `${num}.` : `${num}…`}</span>}
        <button className={id === current ? 'current' : ''} onClick={() => goto(n.id)}>{n.san}</button>{' '}
      </Fragment>,
    );
    for (const alt of tree.nodes[n.id].children.slice(1)) {
      out.push(<span key={`v${alt}`}>({depth < 3 ? <Variation tree={tree} start={alt} current={current} goto={goto} depth={depth + 1} /> : '…'}) </span>);
    }
    first = false;
    id = n.children[0];
  }
  return <>{out}</>;
}

function MoveCell({ id, review, showEval, evalText }: { id?: string; review?: MoveReview; showEval: boolean; evalText?: string }) {
  const s = useStore();
  if (!id) return <span />;
  const n = s.tree.nodes[id];
  return (
    <button className={`mv ${id === s.current ? 'current' : ''}`} onClick={() => s.goto(id)} title={review ? `${CLASS_LABEL[review.classification]}${review.bestSan && review.bestSan !== review.san ? ` — best was ${review.bestSan}` : ''}` : undefined}>
      <span>{n.san}</span>
      {review && review.classification !== 'good' && <ClassIcon c={review.classification} />}
      {showEval && evalText && <span className="faint small">{evalText}</span>}
    </button>
  );
}

const ORDER: Classification[] = ['brilliant', 'great', 'best', 'good', 'inaccuracy', 'mistake', 'blunder', 'forced'];

export function MoveList({ reviewing, onReview }: { reviewing: { done: number; total: number } | null; onReview: () => void }) {
  const s = useStore();
  const [tab, setTab] = useState<'moves' | 'review'>('moves');
  const reviews = useMemo(() => reviewByNode(s.tree, s.report), [s.tree, s.report]);
  const line = useMemo(() => mainline(s.tree), [s.tree]);
  const rows: { num: number; white?: string; black?: string; vars: string[] }[] = [];
  for (const id of line.slice(1)) {
    const parent = s.tree.nodes[s.tree.nodes[id].parent!];
    const [, turn, , , , full] = parent.fen.split(' ');
    const num = Number(full) || 1;
    const last = rows[rows.length - 1];
    if (turn === 'w') rows.push({ num, white: id, vars: [] });
    else if (last && last.num === num && !last.black) last.black = id;
    else rows.push({ num, black: id, vars: [] });
    rows[rows.length - 1].vars.push(...parent.children.slice(1));
  }
  const evalAfter = (id?: string) => {
    if (!id) return undefined;
    const r = reviews.get(id);
    if (r) return formatScore(r.evalAfter);
    const e = s.nodeEvals[id];
    return e ? formatScore(e.score) : undefined;
  };
  const rowEval = (r: { white?: string; black?: string }) => evalAfter(r.black) ?? evalAfter(r.white);
  const selected = reviews.get(s.current);

  useEffect(() => {
    const el = document.querySelector('.movelist .mv.current');
    el?.scrollIntoView({ block: 'nearest' });
  }, [s.current]);

  const jumpToNext = (filter: (c: Classification) => boolean) => {
    const idx = line.indexOf(s.current);
    const target = [...line.slice(idx + 1), ...line.slice(0, idx + 1)].find((id) => {
      const r = reviews.get(id);
      return r && filter(r.classification);
    });
    if (target) s.goto(target);
  };

  return (
    <Panel
      id="moves"
      className="moves-panel"
      title={
        <div className="tabs" role="tablist">
          <button role="tab" className={tab === 'moves' ? 'active' : ''} aria-selected={tab === 'moves'} onClick={() => setTab('moves')}>Move list</button>
          <button role="tab" className={tab === 'review' ? 'active' : ''} aria-selected={tab === 'review'} onClick={() => setTab('review')}>Review{s.report ? '' : ''}</button>
        </div>
      }
      right={tab === 'moves' ? <Toggle on={s.prefs.showEval} onChange={(v) => s.setPrefs({ showEval: v })} label="Eval" /> : undefined}
    >
      {tab === 'moves' && (
        <div className="movelist">
          {rows.length === 0 && <p className="dim small">No moves yet. Drag a piece, click a piece then a square, or type a move below the board.</p>}
          {rows.map((r, i) => (
            <Fragment key={i}>
              <div className="mrow">
                <span className="num">{r.num}.</span>
                {r.white ? <MoveCell id={r.white} review={reviews.get(r.white)} showEval={false} /> : <span className="dim">…</span>}
                <MoveCell id={r.black} review={reviews.get(r.black ?? '')} showEval={false} />
                <span className="ev">{s.prefs.showEval ? rowEval(r) ?? '' : ''}</span>
              </div>
              {[r.white, r.black].map((id) => id && s.tree.nodes[id].comment ? <div key={`c${id}`} className="comment">{s.tree.nodes[id].comment}</div> : null)}
              {r.vars.map((v) => (
                <div key={v} className="variation-line">
                  <Variation tree={s.tree} start={v} current={s.current} goto={s.goto} />
                  <button title="Promote this variation" onClick={() => s.promoteVariation(v)}>↑</button>
                  <button title="Delete this variation" onClick={() => s.deleteVariation(v)}>×</button>
                </div>
              ))}
            </Fragment>
          ))}
        </div>
      )}
      {tab === 'review' && (
        <div className="review-summary">
          {!s.report && !reviewing && (
            <>
              <p className="dim small">Evaluate every move of the mainline, classify each one, and find the turning points. Cached evaluations are reused.</p>
              <button className="btn primary" onClick={onReview} disabled={line.length < 2}><Icon name="review" />Review game</button>
            </>
          )}
          {reviewing && (
            <div className="grid" style={{ display: 'grid', gap: 6 }}>
              <span className="small">Analysing position {reviewing.done} of {reviewing.total}…</span>
              <div className="progress"><div style={{ width: `${(reviewing.done / Math.max(1, reviewing.total)) * 100}%` }} /></div>
              <button className="btn small" onClick={onReview}>Cancel</button>
            </div>
          )}
          {s.report && !reviewing && (
            <>
              <div className="review-table">
                <span className="head">{s.report.engine}</span><span className="head">White</span><span className="head">Black</span>
                <span className="k">Mean move accuracy</span><span className="v">{s.report.white.accuracy.toFixed(1)}%</span><span className="v">{s.report.black.accuracy.toFixed(1)}%</span>
                <span className="k">Avg. centipawn loss</span><span className="v">{s.report.white.acpl.toFixed(0)}</span><span className="v">{s.report.black.acpl.toFixed(0)}</span>
                {ORDER.map((c) => (
                  <Fragment key={c}>
                    <span className="k"><ClassIcon c={c} />{CLASS_LABEL[c]}</span>
                    <span className="v">{s.report!.white.counts[c] ?? 0}</span>
                    <span className="v">{s.report!.black.counts[c] ?? 0}</span>
                  </Fragment>
                ))}
              </div>
              <div className="row">
                <button className="btn small" onClick={() => jumpToNext((c) => c === 'mistake' || c === 'blunder' || c === 'inaccuracy')}>Next error →</button>
                <button className="btn small" onClick={() => jumpToNext((c) => c === 'brilliant' || c === 'great')}>Next great move →</button>
                <div className="spacer" />
                <button className="btn small ghost" onClick={onReview} title="Run the review again with the current settings"><Icon name="refresh" />Re-run</button>
              </div>
              {s.report.turningPoints.length > 0 && (
                <div className="turning">
                  <span className="label">Turning points</span>
                  {s.report.turningPoints.map((t) => {
                    const id = line[t.ply];
                    const r = id ? reviews.get(id) : undefined;
                    if (!id || !r) return null;
                    return (
                      <button key={t.ply} onClick={() => s.goto(id)}>
                        <ClassIcon c={r.classification} />
                        <span>{Math.ceil(t.ply / 2)}{r.color === 'white' ? '.' : '…'} {r.san}</span>
                        <span className="dim small">{t.swing > 0 ? '+' : ''}{t.swing.toFixed(0)}% for White</span>
                        {r.bestSan && <span className="faint small">best {r.bestSan}</span>}
                      </button>
                    );
                  })}
                </div>
              )}
              {selected && (
                <div className="move-detail">
                  <div className="row"><ClassIcon c={selected.classification} /><b>{selected.san}</b><span className="dim">{CLASS_LABEL[selected.classification]}</span></div>
                  <dl className="facts">
                    <dt>Engine best</dt><dd>{selected.bestSan ?? '—'} {selected.bestLine.length > 1 && <span className="faint">({selected.bestLine.slice(0, 6).join(' ')})</span>}</dd>
                    <dt>Eval before → after</dt><dd>{formatScore(selected.evalBefore)} → {formatScore(selected.evalAfter)}</dd>
                    <dt>Expected score</dt><dd>{selected.winBefore.toFixed(1)}% → {selected.winAfter.toFixed(1)}% (−{selected.winLoss.toFixed(1)})</dd>
                    <dt>Centipawn loss</dt><dd>{selected.cpLoss}</dd>
                  </dl>
                  {selected.reasons.map((r, i) => <span key={i} className="small">· {r}</span>)}
                  <span className="heuristic-note">Engine scores are measured; the label is OUTPOST's heuristic (see Settings → Review).</span>
                </div>
              )}
              <span className="heuristic-note">Depth {s.report.settings.depth ?? '—'} · {s.report.positions.filter((p) => p.cached).length} cached positions reused.</span>
            </>
          )}
        </div>
      )}
    </Panel>
  );
}

export function useEngineList(): EngineRecord[] {
  const [engines, setEngines] = useState<EngineRecord[]>([]);
  const view = useStore((s) => s.view);
  useEffect(() => {
    api.engines().then(setEngines).catch(() => setEngines([]));
  }, [view]);
  return engines;
}
