import { useEffect, useState } from 'react';
import { AiView } from './AiView';
import { useStore } from '../store';
import { Icon, Toggle } from '../components/ui';
import { api, errorText } from '../lib/ipc';
import type { AppInfo, EngineDefaults, ReviewSettings } from '../lib/types';

export function Settings({ info }: { info: AppInfo | null }) {
  const prefs = useStore((s) => s.prefs);
  const setPrefs = useStore((s) => s.setPrefs);
  const toast = useStore((s) => s.toast);
  const [engine, setEngine] = useState<EngineDefaults | null>(null);
  const [review, setReview] = useState<ReviewSettings | null>(null);

  useEffect(() => {
    api.settingsGet().then((s) => {
      setEngine(s.engineDefaults as EngineDefaults);
      setReview(s.review as ReviewSettings);
    }).catch(() => {});
  }, []);

  const saveEngine = (next: EngineDefaults) => {
    setEngine(next);
    api.settingsSet('engineDefaults', next).catch((e) => toast(errorText(e), 'error'));
  };
  const saveReview = (next: ReviewSettings) => {
    setReview(next);
    api.settingsSet('review', next).catch((e) => toast(errorText(e), 'error'));
  };
  const num = (v: string) => (v === '' ? 0 : Number(v));

  return (
    <div className="page">
      <div className="page-head">
        <div>
          <h1>Settings</h1>
          <p className="lead">Stored locally in OUTPOST's database. No account, no sync, no telemetry.</p>
        </div>
        <button className="btn" onClick={() => useStore.getState().setView('engines')}><Icon name="engines" />Manage engines</button>
      </div>
      <details className="agent-settings box" open><summary><h2>Agent</h2></summary><AiView embedded /></details>
      <div className="cards">
        <div className="box">
          <h3>Appearance</h3>
          <label className="field">
            <span>CRT effect — scanlines and glow ({Math.round(prefs.crt * 100)}%)</span>
            <input type="range" min={0} max={1} step={0.05} value={prefs.crt} onChange={(e) => setPrefs({ crt: Number(e.target.value) })} />
          </label>
          <Toggle on={prefs.translucent} onChange={(v) => setPrefs({ translucent: v })} label="Translucent window (shows the desktop; blur depends on your compositor)" />
          {prefs.translucent && (
            <label className="field">
              <span>Window opacity ({Math.round(prefs.opacity * 100)}%)</span>
              <input type="range" min={0.55} max={1} step={0.01} value={prefs.opacity} onChange={(e) => setPrefs({ opacity: Number(e.target.value) })} />
            </label>
          )}
          <Toggle on={prefs.coordinates} onChange={(v) => setPrefs({ coordinates: v })} label="Board coordinates" />
          <Toggle on={prefs.aiPanel} onChange={(v) => setPrefs({ aiPanel: v })} label="Show AI panel under the board" />
          <p className="faint small">On Hyprland, background blur comes from your compositor's <code>decoration:blur</code> settings.</p>
        </div>

        <div className="box">
          <h3>Engine resources</h3>
          {engine && (
            <>
              <label className="field">
                <span>Threads per engine (this computer has {info?.cores ?? '?'} logical cores)</span>
                <input type="number" min={1} max={info?.cores ?? 256} value={engine.threads} onChange={(e) => saveEngine({ ...engine, threads: Math.max(1, num(e.target.value)) })} />
              </label>
              <label className="field">
                <span>Hash table per engine (MB)</span>
                <select value={engine.hashMb} onChange={(e) => saveEngine({ ...engine, hashMb: num(e.target.value) })}>
                  {[16, 64, 128, 256, 512, 1024, 2048, 4096, 8192].map((m) => <option key={m} value={m}>{m >= 1024 ? `${m / 1024} GB` : `${m} MB`}</option>)}
                </select>
              </label>
              <p className="faint small">Applied the next time an engine starts. Per-engine options on the Engines page take priority.</p>
            </>
          )}
          <label className="field">
            <span>Engine arrows shown</span>
            <select value={prefs.arrowCount} onChange={(e) => setPrefs({ arrowCount: num(e.target.value) })}>
              {[1, 2, 3, 4, 5].map((n) => <option key={n} value={n}>{n === 1 ? 'Best move only' : `Top ${n} lines`}</option>)}
            </select>
          </label>
        </div>

        <div className="box">
          <h3>Game review</h3>
          {review && (
            <>
              <div className="grid2">
                <label className="field">
                  <span>Depth per position</span>
                  <input type="number" min={6} max={40} value={review.depth ?? 16} onChange={(e) => saveReview({ ...review, depth: Math.max(6, num(e.target.value)) })} />
                </label>
                <label className="field">
                  <span>Lines per position</span>
                  <input type="number" min={1} max={5} value={review.multipv} onChange={(e) => saveReview({ ...review, multipv: Math.min(5, Math.max(1, num(e.target.value))) })} />
                </label>
              </div>
              <p className="dim small">
                Labels come from the loss of expected score (win chance, from the engine evaluation via the logistic curve Lichess publishes), so the same
                centipawn loss matters less in a decided position. Thresholds, in percentage points:
              </p>
              <div className="grid3">
                {(['inaccuracy', 'mistake', 'blunder'] as const).map((k) => (
                  <label className="field" key={k}>
                    <span>{k}</span>
                    <input type="number" step={0.5} value={review.classify[k]} onChange={(e) => saveReview({ ...review, classify: { ...review.classify, [k]: num(e.target.value) } })} />
                  </label>
                ))}
                <label className="field">
                  <span>“best” tolerance</span>
                  <input type="number" step={0.5} value={review.classify.bestTolerance} onChange={(e) => saveReview({ ...review, classify: { ...review.classify, bestTolerance: num(e.target.value) } })} />
                </label>
                <label className="field">
                  <span>“great”: gap to 2nd line</span>
                  <input type="number" step={1} value={review.classify.greatGap} onChange={(e) => saveReview({ ...review, classify: { ...review.classify, greatGap: num(e.target.value) } })} />
                </label>
                <label className="field">
                  <span>“brilliant”: min. sacrifice (pawns)</span>
                  <input type="number" step={1} value={review.classify.sacrificeMin} onChange={(e) => saveReview({ ...review, classify: { ...review.classify, sacrificeMin: num(e.target.value) } })} />
                </label>
              </div>
              <p className="faint small">
                Brilliant requires a real sacrifice (material given up by static exchange, which the opponent can legally take) that the engine still rates as
                sound, in a position that was not already decided. Great means the only move that keeps the evaluation. See docs/classification.md.
              </p>
            </>
          )}
        </div>

        <div className="box">
          <h3>About</h3>
          <p className="small">OUTPOST {info?.version} · {info?.os}/{info?.arch}</p>
          <p className="dim small" style={{ wordBreak: 'break-all' }}>Data folder: {info?.dataDir}</p>
          <p className="dim small">
            Free software: GNU GPL v3 or later, with the tactic generator under the GNU AGPL v3 or later (a port of lichess-puzzler). The built-in engine
            is Stockfish 19 Lite (GPL-3.0), a separate program OUTPOST talks to over UCI. Fonts: Silkscreen, VT323 and IBM Plex Mono (SIL Open Font
            License). Not affiliated with Lichess or Chess.com.
          </p>
          <p className="faint small">Shortcuts: 1–6 sections · ←/→ moves · ↑/↓ variations · Home/End · F flip · Space engine · A arrows · Ctrl+S save · Ctrl+O open · Ctrl+V paste · Ctrl+I import</p>
        </div>
      </div>
    </div>
  );
}
