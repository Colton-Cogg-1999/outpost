import { useCallback, useEffect, useState } from 'react';
import { useStore } from '../store';
import { Icon, Toggle } from '../components/ui';
import { api, errorText } from '../lib/ipc';
import type { AiSettings, KeyStatus, ProviderInfo, ProviderStatus } from '../lib/types';

export function AiView({ embedded = false }: { embedded?: boolean }) {
  const toast = useStore((s) => s.toast);
  const [providers, setProviders] = useState<ProviderInfo[]>([]);
  const [keys, setKeys] = useState<Record<string, KeyStatus>>({});
  const [settings, setSettings] = useState<AiSettings | null>(null);
  const [statuses, setStatuses] = useState<Record<string, ProviderStatus | 'checking'>>({});
  const [keyInput, setKeyInput] = useState<Record<string, string>>({});
  const [chats, setChats] = useState<{ id: string; title: string; updatedAt: number; messages: number }[]>([]);
  const [openChat, setOpenChat] = useState<{ id: string; messages: { role: string; content: string }[] } | null>(null);

  const load = useCallback(() => {
    api.aiProviders().then((r) => {
      setProviders(r.providers);
      setKeys(r.keys);
      setSettings(r.settings);
    }).catch((e) => toast(errorText(e), 'error'));
    api.chats().then(setChats).catch(() => {});
  }, [toast]);
  useEffect(load, [load]);

  const save = async (next: AiSettings) => {
    setSettings(next);
    try {
      await api.settingsSet('ai', next);
    } catch (e) {
      toast(errorText(e), 'error');
      load();
    }
  };

  const check = async (id: string) => {
    setStatuses((s) => ({ ...s, [id]: 'checking' }));
    try {
      const st = await api.aiStatus(id);
      setStatuses((s) => ({ ...s, [id]: st }));
    } catch (e) {
      setStatuses((s) => ({ ...s, [id]: { id, ready: false, detail: errorText(e), plan: null, models: [] } }));
    }
  };

  const setKey = async (id: string) => {
    try {
      await api.aiKeySet(id, keyInput[id] ?? '');
      setKeyInput((k) => ({ ...k, [id]: '' }));
      toast('Key saved to the system credential store.');
      load();
      check(id);
    } catch (e) {
      toast(errorText(e), 'error');
    }
  };

  if (!settings) return <div className="page"><p className="dim">Loading…</p></div>;
  const cfg = (id: string) => settings.providers[id] ?? {};
  const setCfg = (id: string, patch: Record<string, unknown>) => save({ ...settings, providers: { ...settings.providers, [id]: { ...cfg(id), ...patch } } });

  return (
    <div className={embedded ? 'agent-settings-content' : 'page'}>
      <div className="page-head">
        <div>
          {!embedded && <h1>Agent settings</h1>}
          <p className="lead">
            Optional. Ask why a move is a blunder, what the plans are, or why two engines disagree. The engine and the rules library stay authoritative: the model
            receives validated facts, and any move it suggests is checked before you can play it. Nothing is sent unless you ask a question.
          </p>
        </div>
      </div>

      <div className="cards">
        {providers.map((p) => {
          const st = statuses[p.id];
          const active = settings.active === p.id;
          const key = keys[p.id];
          return (
            <div key={p.id} className="box" style={active ? { borderColor: 'var(--phosphor-lo)' } : undefined}>
              <div className="row">
                <input type="radio" name="provider" checked={active} onChange={() => save({ ...settings, active: p.id })} aria-label={`Use ${p.name}`} />
                <h3 style={{ fontSize: 13 }}>{p.name}</h3>
                <span className="chip">{p.kind === 'cli' ? 'subscription' : p.kind === 'local' ? 'local' : 'API key'}</span>
                <div className="spacer" />
                <button className="btn small" onClick={() => check(p.id)} disabled={st === 'checking'}>{st === 'checking' ? 'Checking…' : 'Check'}</button>
              </div>
              <p className="dim small">{p.description}</p>
              {st && st !== 'checking' && (
                <p className={`small ${st.ready ? '' : 'warn'}`}>
                  <span className={`lamp ${st.ready ? 'on' : 'warn'}`} style={{ display: 'inline-block', marginRight: 6 }} />
                  {st.detail}{st.plan ? ` Plan: ${st.plan}.` : ''}
                </p>
              )}
              {(p.needsKey || p.id === 'compatible') && (
                <div style={{ display: 'grid', gap: 6 }}>
                  <span className="small dim">
                    {key?.source === 'keyring' ? 'API key stored in the system credential store.' : key?.source === 'environment' ? 'Using the key from the environment.' : p.needsKey ? 'No key yet.' : 'Key optional.'}
                    {key && !key.storeAvailable && <span className="warn"> No credential store is available; use an environment variable instead.</span>}
                  </span>
                  <div className="row" style={{ flexWrap: 'nowrap' }}>
                    <input type="password" placeholder="Paste API key" value={keyInput[p.id] ?? ''} onChange={(e) => setKeyInput({ ...keyInput, [p.id]: e.target.value })} style={{ flex: 1 }} autoComplete="off" aria-label={`${p.name} API key`} />
                    <button className="btn small" onClick={() => setKey(p.id)} disabled={!keyInput[p.id]}>Save</button>
                    {key?.source === 'keyring' && <button className="btn small danger" onClick={() => api.aiKeyDelete(p.id).then(load)}><Icon name="trash" /></button>}
                  </div>
                </div>
              )}
              {p.defaultBaseUrl && p.kind !== 'api' && (
                <label className="field">
                  <span>Server URL</span>
                  <input type="text" defaultValue={cfg(p.id).baseUrl ?? p.defaultBaseUrl} onBlur={(e) => setCfg(p.id, { baseUrl: e.target.value })} />
                </label>
              )}
              <label className="field">
                <span>Model</span>
                <div className="row" style={{ flexWrap: 'nowrap' }}>
                  {st && st !== 'checking' && st.models.length > 0 ? (
                    <select value={cfg(p.id).model ?? ''} onChange={(e) => setCfg(p.id, { model: e.target.value })} style={{ flex: 1 }}>
                      {p.kind !== 'cli' && <option value="">Choose a model…</option>}
                      {st.models.map((m) => <option key={m.id} value={m.id}>{m.label}</option>)}
                    </select>
                  ) : (
                    <input type="text" placeholder={p.kind === 'cli' ? 'default' : 'press Check to list models'} defaultValue={cfg(p.id).model ?? ''} onBlur={(e) => setCfg(p.id, { model: e.target.value })} style={{ flex: 1 }} />
                  )}
                </div>
              </label>
            </div>
          );
        })}
      </div>

      <div className="cards">
        <div className="box">
          <h3>Answers</h3>
          <label className="field">
            <span>Your level (shapes the explanations)</span>
            <select value={settings.skillLevel} onChange={(e) => save({ ...settings, skillLevel: e.target.value })}>
              {['beginner (~800)', 'improving player (~1200)', 'club player (~1500)', 'strong club player (~1900)', 'expert / master (2200+)'].map((l) => <option key={l} value={l}>{l}</option>)}
            </select>
          </label>
          <label className="field">
            <span>Length</span>
            <select value={settings.length} onChange={(e) => save({ ...settings, length: e.target.value as AiSettings['length'] })}>
              <option value="short">Short</option>
              <option value="normal">Normal</option>
              <option value="detailed">Detailed</option>
            </select>
          </label>
          <Toggle on={settings.saveChats} onChange={(v) => save({ ...settings, saveChats: v })} label="Save conversations locally" />
          <button className="btn small ghost" onClick={() => save({ ...settings, active: null })} disabled={!settings.active}>Turn AI off</button>
        </div>
        <div className="box">
          <h3>What is sent</h3>
          <p className="dim small">
            When you ask, OUTPOST sends: the current FEN, the moves that led to it, legal moves and simple material facts from the rules library, the engine lines
            on screen, review facts for the move in question, player names and ratings from the PGN headers, and your question. No files, no library, no account
            data. Subscription CLIs run with all tools disabled in an empty folder. API keys live in the OS credential store, never in OUTPOST's database.
          </p>
        </div>
        <div className="box">
          <h3>Saved conversations</h3>
          {chats.length === 0 && <p className="dim small">{settings.saveChats ? 'None yet.' : 'Saving is off.'}</p>}
          {chats.map((c) => (
            <div key={c.id} className="row">
              <button className="btn ghost small" style={{ flex: 1, justifyContent: 'flex-start', overflow: 'hidden' }} onClick={() => api.chatMessages(c.id).then((m) => setOpenChat({ id: c.id, messages: m }))}>
                {c.title || 'Untitled'}
              </button>
              <span className="faint small">{new Date(c.updatedAt * 1000).toLocaleDateString()}</span>
              <button className="btn ghost small icon-only" aria-label="Delete conversation" onClick={() => api.chatDelete(c.id).then(load)}><Icon name="trash" /></button>
            </div>
          ))}
          {openChat && (
            <div className="ai-thread" style={{ maxHeight: 360 }}>
              {openChat.messages.map((m, i) => (
                <div key={i} className={`ai-msg ${m.role}`}><div className="text">{m.role === 'user' ? `› ${m.content}` : m.content}</div></div>
              ))}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
