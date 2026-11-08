import { useCallback, useEffect, useState } from 'react';
import { useStore } from '../store';
import { Icon, Modal } from '../components/ui';
import { api, errorText } from '../lib/ipc';
import type { EngineRecord, UciOption } from '../lib/types';

const HIDDEN = new Set(['multipv', 'uci_chess960', 'uci_analysemode', 'ponder', 'uci_showwdl']);

function OptionField({ option, value, onChange }: { option: UciOption; value: string | undefined; onChange: (v: string | undefined) => void }) {
  const current = value ?? option.default ?? '';
  const label = <span>{option.name}{value !== undefined && <span className="warn"> •</span>}</span>;
  switch (option.kind) {
    case 'check':
      return (
        <label className="field">
          {label}
          <select value={current} onChange={(e) => onChange(e.target.value)}>
            <option value="true">true</option>
            <option value="false">false</option>
          </select>
        </label>
      );
    case 'spin':
      return (
        <label className="field">
          {label}
          <input type="number" min={option.min ?? undefined} max={option.max ?? undefined} value={current} onChange={(e) => onChange(e.target.value)} />
        </label>
      );
    case 'combo':
      return (
        <label className="field">
          {label}
          <select value={current} onChange={(e) => onChange(e.target.value)}>
            {option.vars.map((v) => <option key={v} value={v}>{v}</option>)}
          </select>
        </label>
      );
    case 'string':
      return (
        <label className="field">
          {label}
          <div className="row" style={{ flexWrap: 'nowrap' }}>
            <input type="text" value={current} onChange={(e) => onChange(e.target.value)} style={{ flex: 1 }} />
            {/file|path|weights|network|book/i.test(option.name) && (
              <button
                className="btn small"
                title="Choose a file"
                onClick={async () => {
                  const { open } = await import('@tauri-apps/plugin-dialog');
                  const p = await open({ multiple: false });
                  if (typeof p === 'string') onChange(p);
                }}
              >
                <Icon name="folder" />
              </button>
            )}
          </div>
        </label>
      );
    default:
      return null;
  }
}

function EngineCard({ engine, onChanged }: { engine: EngineRecord; onChanged: () => void }) {
  const toast = useStore((s) => s.toast);
  const [editing, setEditing] = useState<Record<string, string> | null>(null);
  const visible = engine.availableOptions.filter((o) => o.kind !== 'button' && !HIDDEN.has(o.name.toLowerCase()));
  const save = async () => {
    try {
      await api.engineSetOptions(engine.id, editing ?? {});
      toast(`${engine.name}: options saved. The engine restarts with them.`);
      setEditing(null);
      onChanged();
    } catch (e) {
      toast(errorText(e), 'error');
    }
  };
  return (
    <div className="box">
      <div className="row">
        <span className={`lamp ${engine.running ? 'on' : ''}`} title={engine.running ? 'Running' : 'Idle'} />
        <h3 style={{ fontSize: 13 }}>{engine.name}</h3>
        {engine.default && <span className="chip">default</span>}
        {engine.builtin && <span className="chip" title="Ships with OUTPOST: Stockfish 19 with the small network, about 5 MB of memory idle and 60 MB while searching on one thread (about 22 MB more per extra thread)">built in</span>}
        {engine.managed && <span className="chip" title="Installed by OUTPOST into its data folder">managed</span>}
        <div className="spacer" />
        {!engine.default && <button className="btn small" onClick={() => api.engineSetDefault(engine.id).then(onChanged)}>Make default</button>}
      </div>
      <span className="faint small" style={{ wordBreak: 'break-all' }}>{engine.path}</span>
      {!engine.exists && <span className="err small">The executable is missing. Remove this entry or re-add the engine.</span>}
      {engine.author && <span className="dim small">by {engine.author}</span>}
      {Object.keys(engine.options).length > 0 && (
        <span className="small dim">Custom: {Object.entries(engine.options).map(([k, v]) => `${k}=${v}`).join(', ')}</span>
      )}
      <div className="row">
        <button className="btn small" onClick={() => setEditing({ ...engine.options })} disabled={!visible.length}>Options ({visible.length})</button>
        <button className="btn small" onClick={() => api.engineRestart(engine.id).then(() => { toast(`${engine.name} restarted.`); onChanged(); }).catch((e) => toast(errorText(e), 'error'))}><Icon name="refresh" />Restart</button>
        {engine.running && <button className="btn small" onClick={() => api.engineShutdown(engine.id).then(onChanged)}><Icon name="stop" />Stop</button>}
        <div className="spacer" />
        {!engine.builtin && <button
          className="btn small danger"
          onClick={async () => {
            if (!window.confirm(engine.managed ? `Remove ${engine.name} and delete its downloaded files?` : `Remove ${engine.name} from OUTPOST? The program itself is not deleted.`)) return;
            await api.engineRemove(engine.id).catch((e) => toast(errorText(e), 'error'));
            onChanged();
          }}
        >
          <Icon name="trash" />Remove
        </button>}
      </div>
      {editing && (
        <Modal
          title={`${engine.name} options`}
          onClose={() => setEditing(null)}
          footer={
            <>
              <button className="btn ghost" onClick={() => setEditing({})}>Reset all to engine defaults</button>
              <div className="spacer" />
              <button className="btn" onClick={() => setEditing(null)}>Cancel</button>
              <button className="btn primary" onClick={save}>Save</button>
            </>
          }
        >
          <p className="dim small">
            Threads and Hash default to OUTPOST's global limits (Settings) unless set here. Leela Chess Zero needs a network file: set <code>WeightsFile</code>.
          </p>
          <div className="grid2">
            {visible.map((o) => (
              <OptionField
                key={o.name}
                option={o}
                value={editing[o.name]}
                onChange={(v) => {
                  const next = { ...editing };
                  if (v === undefined || v === (o.default ?? '')) delete next[o.name];
                  else next[o.name] = v;
                  setEditing(next);
                }}
              />
            ))}
          </div>
        </Modal>
      )}
    </div>
  );
}

export function Engines({ onChange }: { onChange: (count: number) => void }) {
  const toast = useStore((s) => s.toast);
  const setEngine = useStore((s) => s.setEngine);
  const [engines, setEngines] = useState<EngineRecord[]>([]);
  const [found, setFound] = useState<{ path: string; label: string }[] | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(() => {
    api.engines().then((list) => {
      setEngines(list);
      onChange(list.length);
      const def = list.find((e) => e.default);
      if (def) setEngine(def.id);
    }).catch((e) => toast(errorText(e), 'error'));
  }, [onChange, setEngine, toast]);
  useEffect(load, [load]);

  const add = async (path: string) => {
    setBusy(true);
    try {
      const rec = await api.engineAdd(path);
      toast(`${rec.name} added.`);
      setFound((f) => f?.filter((c) => c.path !== path) ?? null);
      load();
    } catch (e) {
      toast(errorText(e), 'error');
    } finally {
      setBusy(false);
    }
  };

  const browse = async () => {
    const { open } = await import('@tauri-apps/plugin-dialog');
    const path = await open({ multiple: false, title: 'Choose a UCI engine executable' });
    if (typeof path === 'string') add(path);
  };

  return (
    <div className="page">
      <div className="page-head">
        <div>
          <h1>Engines</h1>
          <p className="lead">
            OUTPOST comes with Stockfish 19 Lite: full Stockfish search with the small official network, so it uses about 5 MB of memory idle and about 60 MB
            while searching on one thread (about 22 MB more per extra thread). You can add other UCI engines (full Stockfish, Leela Chess Zero, Berserk, …) and compare their recommendations on the Analyze page.
          </p>
        </div>
      </div>
      <div className="cards">
        {engines.map((e) => <EngineCard key={e.id} engine={e} onChanged={load} />)}
        {engines.length === 0 && (
          <div className="notice">
            This build of OUTPOST has no built-in engine. Builds made with <code>./scripts/build</code> include Stockfish 19 Lite; for a development checkout run{' '}
            <code>./scripts/build-engine</code> once. You can also add any UCI engine below.
          </div>
        )}
      </div>
      <div className="cards">
        <div className="box">
          <h3>Add an engine</h3>
          <p className="dim small">Engines are started directly (never through a shell) and must answer the UCI handshake before they are added.</p>
          <div className="row">
            <button className="btn" onClick={() => api.enginesDetect().then(setFound).catch((e) => toast(errorText(e), 'error'))}><Icon name="refresh" />Detect installed engines</button>
            <button className="btn" onClick={browse} disabled={busy}><Icon name="folder" />Choose executable…</button>
          </div>
          {found && found.length === 0 && <p className="dim small">Nothing new found in PATH or the usual install folders.</p>}
          {found?.map((c) => (
            <div key={c.path} className="row">
              <span>{c.label}</span>
              <span className="faint small" style={{ wordBreak: 'break-all' }}>{c.path}</span>
              <div className="spacer" />
              <button className="btn small" onClick={() => add(c.path)} disabled={busy}>Add</button>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
