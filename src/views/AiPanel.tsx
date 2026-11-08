import { Fragment, useEffect, useMemo, useRef, useState } from 'react';
import { useStore } from '../store';
import { CLASS_LABEL, Icon, Panel } from '../components/ui';
import { api, errorText, nextJobId } from '../lib/ipc';
import { header, mainline, movesTo, startFen, type GameTree } from '../chess/tree';
import { reviewByNode } from './analysis';
import type { AiSettings, GameReport, MoveCheck, Snapshot, ValidatedMove } from '../lib/types';

type Action = 'explain' | 'tactics' | 'plans' | 'compare' | 'whynot';

const ACTIONS: { id: Action; label: string; prompt: (ctx: { san?: string; cls?: string; best?: string }) => string }[] = [
  { id: 'explain', label: 'Explain', prompt: () => 'Explain this position: who stands better and why, the key features (pawn structure, piece activity, king safety), and what each side should be aiming for.' },
  { id: 'tactics', label: 'Tactics', prompt: () => 'What tactical threats and opportunities exist here, for both sides? Name the motifs (forks, pins, skewers, discovered attacks, loose pieces, back-rank issues) and only mention legal moves.' },
  { id: 'plans', label: 'Plans', prompt: () => 'What are sensible plans for both sides over the next several moves? Focus on the strategic ideas rather than long variations.' },
  { id: 'compare', label: 'Compare', prompt: () => 'Compare the candidate moves in the engine lines (and the engines, if there is more than one). Why does each line make sense, and what does the difference in evaluation mean for a human player?' },
  {
    id: 'whynot',
    label: 'Why not?',
    prompt: ({ san, cls, best }) =>
      san
        ? `Why is ${san} ${cls ? `labelled "${cls}"` : 'weaker than the engine’s choice'}${best && best !== san ? `, and why is ${best} better` : ''}? What idea did the move miss?`
        : 'Why does the engine prefer its top move over the natural-looking alternatives here?',
  },
];

interface Msg {
  role: 'user' | 'assistant';
  content: string;
  label?: string;
  moves?: MoveCheck;
  provider?: string;
  error?: boolean;
}

/** Builds the structured context the backend validates and renders for the model. */
export function positionContext(tree: GameTree, current: string, live: Snapshot | null, compare: Snapshot | null, report: GameReport | null) {
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
    userColor: null,
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

export function AiPanel() {
  const s = useStore();
  const node = s.tree.nodes[s.current];
  const [threads, setThreads] = useState<Record<string, Msg[]>>({});
  const [tab, setTab] = useState<Action>('explain');
  const [input, setInput] = useState('');
  const [busy, setBusy] = useState<number | null>(null);
  const [streaming, setStreaming] = useState('');
  const [settings, setSettings] = useState<AiSettings | null>(null);
  const chatIds = useRef<Record<string, string | null>>({});
  const threadRef = useRef<HTMLDivElement>(null);
  const thread = useMemo(() => threads[node.fen] ?? [], [threads, node.fen]);
  const review = reviewByNode(s.tree, s.report).get(s.current);

  useEffect(() => {
    api.aiProviders().then((p) => setSettings(p.settings)).catch(() => setSettings(null));
  }, [s.view]);

  useEffect(() => {
    threadRef.current?.scrollTo({ top: threadRef.current.scrollHeight });
  }, [thread.length, streaming]);

  const ask = async (question: string, label: string | null) => {
    if (busy) return;
    const fen = node.fen;
    const requestId = nextJobId();
    setBusy(requestId);
    setStreaming('');
    const history = thread.filter((m) => !m.error).map((m) => ({ role: m.role, content: m.content }));
    const userMsg: Msg = { role: 'user', content: question, label: label ?? undefined };
    setThreads((t) => ({ ...t, [fen]: [...(t[fen] ?? []), userMsg] }));
    try {
      const answer = await api.aiAsk(
        requestId,
        {
          question,
          label,
          position: positionContext(s.tree, s.current, s.live, s.compare, s.report),
          history,
          chatId: chatIds.current[fen] ?? null,
        },
        (delta) => setStreaming((prev) => prev + delta),
      );
      chatIds.current[fen] = answer.chatId;
      const msg: Msg = { role: 'assistant', content: answer.text + (answer.cancelled ? '\n\n[stopped]' : ''), moves: answer.moves, provider: [answer.provider, answer.model].filter(Boolean).join(' · ') };
      setThreads((t) => ({ ...t, [fen]: [...(t[fen] ?? []), msg] }));
    } catch (e) {
      setThreads((t) => ({ ...t, [fen]: [...(t[fen] ?? []), { role: 'assistant', content: errorText(e), error: true }] }));
    } finally {
      setBusy(null);
      setStreaming('');
    }
  };

  const runAction = (a: Action) => {
    setTab(a);
    const def = ACTIONS.find((x) => x.id === a)!;
    const prompt = def.prompt({ san: node.san, cls: review ? CLASS_LABEL[review.classification] : undefined, best: review?.bestSan ?? undefined });
    ask(prompt, `[${def.label.toUpperCase()}]${a === 'whynot' && node.san ? ` ${node.san}` : ''}`);
  };

  const playMove = (m: ValidatedMove) => {
    // Re-validated by the tree: illegal input is rejected there too.
    if (!s.play(m.uci)) s.toast(`${m.san} is not legal in the current position.`, 'error');
  };

  const configured = !!settings?.active;
  const lastAnswer = [...thread].reverse().find((m) => m.role === 'assistant' && !m.error);

  return (
    <Panel
      id="ai"
      className="ai-panel"
      bodyClass="ai-body"
      title={
        <div className="tabs" role="toolbar" aria-label="AI quick actions">
          {ACTIONS.map((a) => (
            <button key={a.id} className={tab === a.id && thread.length ? 'active' : ''} onClick={() => runAction(a.id)} disabled={!configured || !!busy} title={configured ? `Ask: ${a.prompt({ san: node.san })}` : 'Set up an AI provider first'}>
              [{a.label}]
            </button>
          ))}
        </div>
      }
      right={busy ? <button className="btn small" onClick={() => api.cancel(busy)}><Icon name="stop" />Stop</button> : null}
    >
      {!configured && (
        <p className="dim small">
          AI explanations are optional and off. OUTPOST works fully without them. To ask questions about positions, choose a provider — your
          Claude or ChatGPT subscription via their official CLIs, an API key, or a local model with Ollama.{' '}
          <button className="btn small" onClick={() => s.setView('ai')}>Set up AI</button>
        </p>
      )}
      {configured && thread.length === 0 && !busy && (
        <p className="dim small">
          Ask about this position, or use a quick action above. The current FEN, move history, engine lines and review facts are sent to{' '}
          <b>{settings?.active}</b> only when you ask. Moves in answers are checked against the rules before they can be played.
        </p>
      )}
      {thread.length > 0 && (
        <div className="ai-thread" ref={threadRef}>
          {thread.map((m, i) => (
            <div key={i} className={`ai-msg ${m.role}`}>
              {m.role === 'user' ? (
                <div className="text">› {m.label ?? m.content}</div>
              ) : m.error ? (
                <div className="notice error">{m.content}</div>
              ) : (
                <>
                  <h4><Icon name="spark" />Position analysis <span className="ai-provider">{m.provider}</span></h4>
                  <AnswerText text={m.content} valid={[...(m.moves?.keyMoves ?? []), ...(m.moves?.mentioned ?? [])]} onPlay={playMove} />
                  {m === lastAnswer && (m.moves?.keyMoves.length || m.moves?.rejected.length) ? (
                    <div className="key-ideas">
                      {m.moves!.keyMoves.length > 0 && <span>Key ideas:</span>}
                      {m.moves!.keyMoves.map((k) => (
                        <button key={k.uci} className="chip" onClick={() => playMove(k)} title="Legal in this position — click to play">{k.san}</button>
                      ))}
                      {m.moves!.rejected.length > 0 && <span className="faint" title="The model named these, but they are not legal here, so they are not playable.">ignored (not legal here): {m.moves!.rejected.join(', ')}</span>}
                    </div>
                  ) : null}
                </>
              )}
            </div>
          ))}
          {busy && (
            <div className="ai-msg assistant">
              <h4><Icon name="spark" />Thinking<span className="blink">_</span></h4>
              {streaming && <div className="text">{streaming.replace(/\n?KEY MOVES:.*$/s, '')}</div>}
            </div>
          )}
        </div>
      )}
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
        <input value={input} onChange={(e) => setInput(e.target.value)} placeholder={configured ? 'Ask about the position, a plan, or a move…' : 'AI is not set up'} disabled={!configured || !!busy} aria-label="Ask the AI assistant" />
        <button type="submit" disabled={!configured || !!busy || !input.trim()} aria-label="Send"><Icon name="send" /></button>
      </form>
    </Panel>
  );
}
