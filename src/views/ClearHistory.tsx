import { useState } from 'react';
import { Icon, Modal } from '../components/ui';
import { api, errorText } from '../lib/ipc';
import { useStore } from '../store';
import { useGameChat } from './gameChat';
import { tacticsChanged } from '../tactics/scan';

export function ClearHistory({ disabled = false }: { disabled?: boolean }) {
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const pending = useGameChat((s) => s.pending);
  const scanning = useStore((s) => !!s.scan);
  const clear = async () => {
    setBusy(true);
    try {
      const count = await api.clearHistory();
      const s = useStore.getState();
      // Keep an unsaved analysis on the board, detached from its deleted library row.
      if (s.dirty) useStore.setState({ gameId: null, game: null, playerColor: null, report: null });
      else { const view = s.view; s.newGame(); s.setView(view); }
      useGameChat.setState({ conversations: {}, pending: null });
      tacticsChanged.dispatchEvent(new Event('change'));
      s.toast(`Cleared ${count} saved game${count === 1 ? '' : 's'}.`);
      setOpen(false);
    } catch (e) { useStore.getState().toast(errorText(e), 'error'); }
    finally { setBusy(false); }
  };
  return <>
    <button className="btn small ghost" disabled={disabled || scanning || !!pending} onClick={() => setOpen(true)}><Icon name="trash" />Clear history</button>
    {open && <Modal title="Clear game history?" onClose={() => { if (!busy) setOpen(false); }}>
      <p>Delete all saved games, their reviews, generated puzzles, and remembered chess accounts from this device? Agent settings and saved conversations stay.</p>
      <div className="row end"><button className="btn" disabled={busy} onClick={() => setOpen(false)}>Cancel</button><button className="btn danger" disabled={busy} onClick={clear}>{busy ? 'Clearing…' : 'Clear game history'}</button></div>
    </Modal>}
  </>;
}
