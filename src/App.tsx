import { lazy, Suspense, useEffect, useRef, useState } from 'react';
import { useStore, migratePrefs, type View, type UiPrefs } from './store';
import { Icon, Logo } from './components/ui';
import { api, errorText, inTauri } from './lib/ipc';
import type { AppInfo } from './lib/types';
import { Analyze } from './views/Analyze';
import { Games } from './views/Games';
const Tactics = lazy(() => import('./views/Tactics').then((m) => ({ default: m.Tactics })));
import { scanText } from './tactics/scan';
const Engines = lazy(() => import('./views/Engines').then((m) => ({ default: m.Engines })));
const Settings = lazy(() => import('./views/Settings').then((m) => ({ default: m.Settings })));
const ImportDialog = lazy(() => import('./views/ImportDialog').then((m) => ({ default: m.ImportDialog })));
import { openPgnFile, loadDroppedPaths } from './lib/files';

const NAV: { id: View; label: string; icon: string }[] = [
  { id: 'analyze', label: 'Analyze', icon: 'analyze' },
  { id: 'games', label: 'Games', icon: 'games' },
  { id: 'tactics', label: 'Puzzles', icon: 'puzzles' },
  { id: 'engines', label: 'Engines', icon: 'engines' },
  { id: 'ai', label: 'Agent', icon: 'ai' },
  { id: 'settings', label: 'Settings', icon: 'settings' },
];

function TitleBar() {
  const win = async () => (await import('@tauri-apps/api/window')).getCurrentWindow();
  const view = useStore((s) => s.view);
  const dirty = useStore((s) => s.dirty);
  return (
    <header className="titlebar" data-tauri-drag-region>
      <div className="status" data-tauri-drag-region>{view === 'analyze' && dirty ? <span>● unsaved analysis</span> : null}</div>
      <div className="title" data-tauri-drag-region>OUTPOST — Chess Analysis</div>
      <div className="controls">
        {inTauri && (
          <>
            <button aria-label="Minimize" onClick={async () => (await win()).minimize()}><Icon name="min" /></button>
            <button aria-label="Maximize" onClick={async () => (await win()).toggleMaximize()}><Icon name="max" /></button>
            <button aria-label="Close" className="close" onClick={async () => (await win()).close()}><Icon name="close" /></button>
          </>
        )}
      </div>
    </header>
  );
}

function Sidebar({ info, engineReady }: { info: AppInfo | null; engineReady: boolean | null }) {
  const view = useStore((s) => s.view);
  const setView = useStore((s) => s.setView);
  const openImport = useStore((s) => s.openImport);
  const scan = useStore((s) => s.scan);
  return (
    <aside className="sidebar">
      <Logo />
      <nav className="nav" aria-label="Sections">
        {NAV.filter((n) => n.id !== 'engines' && n.id !== 'ai').map((n) => (
          <button key={n.id} className={view === n.id ? 'active' : ''} onClick={() => setView(n.id)} aria-current={view === n.id ? 'page' : undefined}>
            <Icon name={n.icon} />
            {n.label}
            <kbd>{NAV.findIndex((item) => item.id === n.id) + 1}</kbd>
          </button>
        ))}
      </nav>
      <div className="side-section">
        <h3>Import game</h3>
        <button className="import-btn" onClick={() => openImport('lichess')}><Icon name="antenna" />From Lichess…</button>
        <button className="import-btn" onClick={() => openImport('chesscom')}><Icon name="pawn" />From Chess.com…</button>
        <button className="import-btn" onClick={() => openImport('pgn')}><Icon name="file" />From PGN / FEN…</button>
      </div>
      {scan && view !== 'tactics' && (
        <button className="side-scan small" onClick={() => setView('tactics')} title="A tactic scan is running">
          <span className="lamp on" /> {scanText(scan.progress)}
        </button>
      )}
      <div className="side-foot" title={info ? `Data: ${info.dataDir}` : undefined}>
        <span className={`lamp ${engineReady ? 'on' : engineReady === false ? 'warn' : ''}`} />
        OUTPOST v{info?.version ?? '—'}
        {engineReady === false && <span className="warn" style={{ marginLeft: 'auto' }}>no engine</span>}
      </div>
    </aside>
  );
}

function Toasts() {
  const toasts = useStore((s) => s.toasts);
  const dismiss = useStore((s) => s.dismiss);
  return (
    <div className="toasts" aria-live="polite">
      {toasts.map((t) => (
        <div key={t.id} className={`toast ${t.kind}`} onClick={() => dismiss(t.id)}>{t.text}</div>
      ))}
    </div>
  );
}

export default function App() {
  const view = useStore((s) => s.view);
  const prefs = useStore((s) => s.prefs);
  const importTab = useStore((s) => s.importTab);
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [engineReady, setEngineReady] = useState<boolean | null>(null);
  const [dropping, setDropping] = useState(false);
  const loaded = useRef(false);

  // Load persisted preferences once, then save changes (debounced).
  useEffect(() => {
    api.appInfo().then(setInfo).catch(() => {});
    api.settingsGet()
      .then((s) => {
        const ui = (s.ui ?? {}) as Partial<UiPrefs>;
        useStore.getState().setPrefs(migratePrefs(ui));
      })
      .catch(() => {})
      .finally(() => (loaded.current = true));
  }, []);

  // Engine availability (refreshed when the section changes, e.g. after installing one).
  useEffect(() => {
    api.engines()
      .then((list) => {
        setEngineReady(list.length > 0);
        const def = list.find((e) => e.default) ?? list[0];
        if (def) useStore.getState().setEngine(def.id);
      })
      .catch(() => setEngineReady(null));
  }, [view]);

  useEffect(() => {
    if (!loaded.current) return;
    const t = setTimeout(() => api.settingsSet('ui', prefs).catch(() => {}), 400);
    return () => clearTimeout(t);
  }, [prefs]);

  useEffect(() => {
    document.documentElement.style.setProperty('--crt', String(prefs.crt));
    document.documentElement.style.setProperty('--opacity', String(prefs.opacity));
    document.body.classList.toggle('solid', !prefs.translucent);
  }, [prefs.crt, prefs.opacity, prefs.translucent]);

  // Global shortcuts (ignored while typing).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement;
      if (t.closest('input, textarea, select, [contenteditable]')) return;
      const s = useStore.getState();
      if (!e.ctrlKey && !e.metaKey && !e.altKey && /^[1-6]$/.test(e.key)) {
        s.setView(NAV[Number(e.key) - 1].id);
        e.preventDefault();
      } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'o') {
        e.preventDefault();
        openPgnFile().catch((err) => s.toast(errorText(err), 'error'));
      } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'i') {
        e.preventDefault();
        s.openImport('pgn');
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  // Native file drops (Tauri gives real paths; the webview never reads the disk itself).
  useEffect(() => {
    if (!inTauri) return;
    let unlisten: (() => void) | undefined;
    import('@tauri-apps/api/webview').then(({ getCurrentWebview }) =>
      getCurrentWebview()
        .onDragDropEvent((event) => {
          const p = event.payload;
          if (p.type === 'enter' || p.type === 'over') setDropping(true);
          else if (p.type === 'leave') setDropping(false);
          else if (p.type === 'drop') {
            setDropping(false);
            loadDroppedPaths(p.paths).catch((err) => useStore.getState().toast(errorText(err), 'error'));
          }
        })
        .then((u) => (unlisten = u)),
    );
    return () => unlisten?.();
  }, []);

  return (
    <div className={`app view-${view} ${prefs.crt > 0 ? 'crt' : ''}`}>
      <TitleBar />
      <div className="main">
        <Sidebar info={info} engineReady={engineReady} />
        <main style={{ minWidth: 0, minHeight: 0 }}>
          <Suspense fallback={<p className="dim">Loading…</p>}>
          {view === 'analyze' && <Analyze />}
          {view === 'games' && <Games />}
          {view === 'tactics' && <Tactics />}
          {view === 'engines' && <Engines onChange={(n) => setEngineReady(n > 0)} />}
          {view === 'ai' && <Settings info={info} />}
          {view === 'settings' && <Settings info={info} />}
          </Suspense>
        </main>
      </div>
      {importTab && <Suspense fallback={null}><ImportDialog /></Suspense>}
      {dropping && <div className="drop-hint">DROP PGN · FEN</div>}
      <Toasts />
    </div>
  );
}
