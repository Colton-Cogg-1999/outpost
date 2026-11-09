import { Fragment, useEffect, useRef, useState } from 'react';
import { useStore } from '../store';
import { useGameChat, type ChatMessage, type ChatSession } from './gameChat';
import { CLASS_LABEL, Icon, Panel } from '../components/ui';
import { api, errorText, nextJobId } from '../lib/ipc';
import { header, mainline, movesTo, startFen, type GameTree } from '../chess/tree';
import { reviewByNode } from './analysis';
import type { AiSettings, GameReport, Snapshot, ValidatedMove } from '../lib/types';

/** Builds the structured context the backend validates and renders for the model. */
export function positionContext(tree: GameTree, current: string, live: Snapshot | null, compare: Snapshot | null, report: GameReport | null, userColor: 'white' | 'black' | null = null) {
  const node = tree.nodes[current];
  const engines = [live, compare]
    .filter((s): s is Snapshot => !!s && s.fen === node.fen && !s.terminal)
    .map((s) => ({ engine: s.engine, depth: s.depth, lines: s.lines.map((l) => ({ uci: l.uci, score: l.score })) }));
  const reviews = reviewByNode(tree, report);
  const r = reviews.get(current);
  const parent = node.parent ? tree.nodes[node.parent] : null;
  const line = mainline(tree);
  return {
    fen: node.fen,
    startFen: startFen(tree),
    historyUci: movesTo(tree, current),
    engines,
    selectedMove: node.san
      ? {
          san: node.san,
          classification: r ? CLASS_LABEL[r.classification] : null,
          bestSan: r?.bestSan ?? null,
          evalBefore: r?.evalBefore ?? null,
          evalAfter: r?.evalAfter ?? null,
          winLoss: r?.winLoss ?? null,
          reasons: r?.reasons ?? [],
          fenBefore: parent?.fen ?? null,
        }
      : null,
    game: tree.headers.length
      ? {
          white: header(tree, 'White') ?? null,
          black: header(tree, 'Black') ?? null,
          whiteElo: Number(header(tree, 'WhiteElo')) || null,
          blackElo: Number(header(tree, 'BlackElo')) || null,
          result: header(tree, 'Result') ?? null,
          event: header(tree, 'Event') ?? null,
          date: header(tree, 'Date') ?? null,
          opening: header(tree, 'Opening') ?? null,
        }
      : null,
    review: report
      ? report.turningPoints
          .map((t) => {
            const m = reviews.get(line[t.ply]);
            return m ? { ply: t.ply, san: m.san, classification: CLASS_LABEL[m.classification], bestSan: m.bestSan, swing: t.swing } : null;
          })
          .filter(Boolean)
      : [],
    tactic: null,
    userColor,
  };
}

/** Renders answer text with validated SAN moves as clickable buttons. */
export function AnswerText({ text, valid, onPlay }: { text: string; valid: ValidatedMove[]; onPlay: (m: ValidatedMove) => void }) {
  const body = text.replace(/\n?KEY MOVES:.*$/s, '').trimEnd();
  const map = new Map(valid.map((m) => [m.san.replace(/[+#]$/, ''), m]));
  const parts = body.split(/(\b(?:[KQRBN]?[a-h]?[1-8]?x?[a-h][1-8](?:=[QRBN])?|O-O(?:-O)?)[+#]?)/g);
  return (
    <div className="text">
      {parts.map((p, i) => {
        const m = map.get(p.replace(/[+#]$/, ''));
        return m ? <button key={i} className="san" onClick={() => onPlay(m)} title={`Play ${m.san} on the board`}>{p}</button> : <Fragment key={i}>{p}</Fragment>;
      })}
    </div>
  );
}

export interface ChatPosition { fen: string; label: string; context: unknown }

export function AiPanel({ position: override, session: chatSession }: { position?: ChatPosition | null; session?: ChatSession } = {}) {
  const s = useStore();
  const node = s.tree.nodes[s.current];
  const chat = useGameChat();
  const session = chatSession ?? s.analysisSession;
  const conversation = chat.conversations[session];
  const thread = conversation?.messages ?? [];
  const busy = chat.pending?.requestId ?? null;
  const here = chat.pending?.session === session;
  const streaming = here ? chat.pending?.streaming ?? '' : '';
  const [input, setInput] = useState('');
  const [settings, setSettings] = useState<AiSettings | null>(null);
  const threadRef = useRef<HTMLDivElement>(null);
  const followBottom = useRef(true);
  const fen = override?.fen ?? node.fen;

  useEffect(() => {
    api.aiProviders().then((p) => { setSettings(p.settings); }).catch(() => setSettings(null));
  }, [s.view]);

  useEffect(() => {
    if (followBottom.current) threadRef.current?.scrollTo({ top: threadRef.current.scrollHeight });
  }, [thread.length, streaming]);

  const ask = async (question: string, label: string | null) => {
    if (busy) return;
    followBottom.current = true;
    const requestId = nextJobId();
    useGameChat.setState({ pending: { session, requestId, streaming: '' } });
    const history = thread.filter((m) => !m.error).map((m) => ({ role: m.role, content: `${m.position ? `[${m.position}; FEN ${m.fen}]\n` : ''}${m.content}` }));
    const position = override?.label ?? (node.san ? `After ${s.tree.nodes[node.parent!].fen.split(' ')[5]}${s.tree.nodes[node.parent!].fen.split(' ')[1] === 'b' ? '…' : '.'} ${node.san}` : 'Starting position');
    const userMsg: ChatMessage = { role: 'user', content: question, label: label ?? undefined, fen, position };
    chat.append(session, userMsg);
    try {
      const answer = await api.aiAsk(
        requestId,
        {
          question,
          label,
          position: override?.context ?? positionContext(s.tree, s.current, s.live, s.compare, s.report, s.playerColor),
          history,
          chatId: conversation?.chatId ?? null,
        },
        (delta) => useGameChat.setState((state) => ({ pending: state.pending?.requestId === requestId ? { ...state.pending, streaming: state.pending.streaming + delta } : state.pending })),
      );
      const msg: ChatMessage = { role: 'assistant', fen, position, content: answer.text + (answer.cancelled ? '\n\n[stopped]' : ''), moves: answer.moves, provider: [answer.provider, answer.model].filter(Boolean).join(' · ') };
      chat.append(session, msg, answer.chatId);
    } catch (e) {
      chat.append(session, { role: 'assistant', content: errorText(e), error: true, fen, position });
    } finally {
      useGameChat.setState({ pending: null });
    }
  };

  const playMove = (m: ValidatedMove) => {
    // Re-validated by the tree: illegal input is rejected there too.
    if (!s.play(m.uci)) s.toast(`${m.san} is not legal in the current position.`, 'error');
  };

  const configured = !!settings?.active;
  const lastAnswer = [...thread].reverse().find((m) => m.role === 'assistant' && !m.error);

  return <Panel
      id="ai"
      className="ai-panel"
      bodyClass="ai-body"
      title={<h2><Icon name="ai" />Discuss with agent</h2>}
      collapsible={false}
      right={busy ? <button className="btn small" onClick={() => api.cancel(busy)}><Icon name="stop" />Stop</button> : <button className="btn small ghost" onClick={() => s.setView('settings')} aria-label="Configure agent in Settings"><Icon name="settings" /></button>}
    >
      {!configured && <button className="agent-setup" onClick={() => s.setView('settings')}>Connect an agent in Settings to start chatting.</button>}
      {(thread.length > 0 || here) && (
        <div className="ai-thread" ref={threadRef} role="log" aria-label="Conversation" onScroll={(e) => { const el = e.currentTarget; followBottom.current = el.scrollHeight - el.scrollTop - el.clientHeight < 48; }}>
          {thread.map((m, i) => (
            <div key={i} className={`ai-msg ${m.role}`}>
              {m.role === 'user' ? (
                <div className="text"><span className="chat-position">{m.position}</span>› {m.label ?? m.content}</div>
              ) : m.error ? (
                <div className="notice error">{m.content}</div>
              ) : (
                <>
                  <h4><Icon name="spark" />Position analysis <span className="ai-provider">{m.provider}</span></h4>
                  <AnswerText text={m.content} valid={!override && m.fen === fen ? [...(m.moves?.keyMoves ?? []), ...(m.moves?.mentioned ?? [])] : []} onPlay={playMove} />
                  {m === lastAnswer && (m.moves?.keyMoves.length || m.moves?.rejected.length) ? (
                    <div className="key-ideas">
                      {m.moves!.keyMoves.length > 0 && <span>Key ideas:</span>}
                      {m.moves!.keyMoves.map((k) => (
                        <button key={k.uci} disabled={!!override || m.fen !== fen} className="chip" onClick={() => playMove(k)} title="Legal in this position — click to play">{k.san}</button>
                      ))}
                      {m.moves!.rejected.length > 0 && <span className="faint" title="The model named these, but they are not legal here, so they are not playable.">ignored (not legal here): {m.moves!.rejected.join(', ')}</span>}
                    </div>
                  ) : null}
                </>
              )}
            </div>
          ))}
          {here && (
            <div className="ai-msg assistant">
              <h4><Icon name="spark" />Thinking<span className="blink">_</span></h4>
              {streaming && <div className="text">{streaming.replace(/\n?KEY MOVES:.*$/s, '')}</div>}
            </div>
          )}
        </div>
      )}
      {thread.length === 0 && !here && <p className="chat-empty dim small">Ask about the game or the position on the board.</p>}
      <form
        className="ai-input"
        onSubmit={(e) => {
          e.preventDefault();
          if (input.trim()) {
            ask(input.trim(), null);
            setInput('');
          }
        }}
      >
        <textarea value={input} onChange={(e) => setInput(e.target.value)} placeholder="Ask about the position, the game, or ideas…" disabled={!configured || !!busy} aria-label="Ask the AI assistant" onKeyDown={(e) => {
          if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); e.currentTarget.form?.requestSubmit(); }
        }} />
        <button type="submit" disabled={!configured || !!busy || !input.trim()} aria-label="Send"><Icon name="send" /></button>
      </form>
    </Panel>;
}
