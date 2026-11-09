import { create } from 'zustand';
import {
  addLine,
  addMove,
  deleteFrom,
  lineFrom,
  newTree,
  promote,
  setNode,
  treeFromPgn,
  type GameTree,
  type Shape,
} from './chess/tree';
import { fenError } from './chess/position';
import { logError } from './lib/ipc';
import type { GameReport, GameSummary, ScanProgress, Score, Snapshot } from './lib/types';

export type View = 'analyze' | 'games' | 'tactics' | 'engines' | 'ai' | 'settings';
export type ImportTab = 'lichess' | 'chesscom' | 'pgn' | 'fen';

export interface UiPrefs {
  workflowVersion: number;
  appearanceVersion: number;
  crt: number; // 0..1 intensity of scanlines/glow
  translucent: boolean;
  opacity: number; // window background opacity when translucent
  coordinates: boolean;
  engineArrows: boolean;
  arrowCount: number; // engine arrows shown (1 = best only)
  showEval: boolean;
  multipv: number;
  depthLimit: number | null; // null = infinite
  collapsed: Record<string, boolean>;
  aiPanel: boolean;
}

export const defaultPrefs: UiPrefs = {
  workflowVersion: 2,
  appearanceVersion: 1,
  crt: 0,
  translucent: false,
  opacity: 0.86,
  coordinates: true,
  engineArrows: true,
  arrowCount: 1,
  showEval: true,
  multipv: 1,
  depthLimit: 16,
  collapsed: {},
  aiPanel: true,
};

export interface Toast {
  id: number;
  text: string;
  kind: 'info' | 'error';
}

interface State {
  view: View;
  importTab: ImportTab | null;
  prefs: UiPrefs;
  tree: GameTree;
  current: string;
  orientation: 'white' | 'black';
  playerColor: 'white' | 'black' | null;
  analysisSession: number;
  gameId: number | null;
  game: GameSummary | null;
  report: GameReport | null;
  dirty: boolean;
  engineOn: boolean;
  engineId: string | null;
  compareId: string | null;
  live: Snapshot | null;
  compare: Snapshot | null;
  engineError: string | null;
  /** Best live evaluation seen per node (White POV), for the graph and move list. */
  nodeEvals: Record<string, { score: Score; depth: number }>;
  toasts: Toast[];

  setView: (v: View) => void;
  openImport: (tab: ImportTab | null) => void;
  setPrefs: (patch: Partial<UiPrefs>) => void;
  /** A running tactic scan (lives here so it survives leaving the Tactics page). */
  scan: { job: number; progress: ScanProgress | null } | null;
  setScan: (scan: { job: number; progress: ScanProgress | null } | null) => void;
  toast: (text: string, kind?: Toast['kind']) => void;
  dismiss: (id: number) => void;

  play: (uci: string) => boolean;
  playLine: (line: string[]) => void;
  goto: (id: string) => void;
  next: () => void;
  prev: () => void;
  first: () => void;
  last: () => void;
  variation: (delta: number) => void;
  flip: () => void;
  setShapes: (shapes: Shape[]) => void;
  setComment: (text: string) => void;
  deleteVariation: (id: string) => void;
  promoteVariation: (id: string) => void;

  loadTree: (tree: GameTree, opts?: { game?: GameSummary | null; gameId?: number | null; report?: GameReport | null; at?: 'start' | 'end' }) => void;
  loadPgn: (pgn: string, opts?: { game?: GameSummary | null; gameId?: number | null; report?: GameReport | null }) => string | null;
  loadFen: (fen: string) => string | null;
  newGame: () => void;
  setReport: (r: GameReport | null) => void;
  markSaved: (id: number) => void;

  setEngineOn: (on: boolean) => void;
  setEngine: (id: string | null) => void;
  setCompare: (id: string | null) => void;
  setLive: (s: Snapshot | null) => void;
  setCompareLive: (s: Snapshot | null) => void;
  setEngineError: (e: string | null) => void;
  setNodeEval: (id: string, score: Score, depth: number) => void;
}

let toastId = 0;

export const useStore = create<State>((set, get) => ({
  view: 'analyze',
  importTab: null,
  prefs: defaultPrefs,
  tree: newTree(),
  current: 'n0',
  orientation: 'white',
  playerColor: null,
  analysisSession: 0,
  gameId: null,
  game: null,
  report: null,
  dirty: false,
  engineOn: true,
  engineId: null,
  compareId: null,
  live: null,
  compare: null,
  engineError: null,
  nodeEvals: {},
  toasts: [],

  setView: (view) => set({ view }),
  openImport: (importTab) => set({ importTab }),
  setPrefs: (patch) => set({ prefs: { ...get().prefs, ...patch } }),
  scan: null,
  setScan: (scan) => set({ scan }),
  toast: (text, kind = 'info') => {
    const id = ++toastId;
    set({ toasts: [...get().toasts, { id, text, kind }] });
    if (kind === 'error') logError(`toast: ${text}`);
    setTimeout(() => get().dismiss(id), kind === 'error' ? 8000 : 4000);
  },
  dismiss: (id) => set({ toasts: get().toasts.filter((t) => t.id !== id) }),

  play: (uci) => {
    const { tree, current } = get();
    const r = addMove(tree, current, uci);
    if (!r) return false;
    set({ tree: r.tree, current: r.id, dirty: get().dirty || r.tree !== tree });
    return true;
  },
  playLine: (line) => {
    const { tree, current } = get();
    const r = addLine(tree, current, line);
    set({ tree: r.tree, current: r.id, dirty: get().dirty || r.tree !== tree });
  },
  goto: (id) => {
    if (get().tree.nodes[id]) set({ current: id });
  },
  next: () => {
    const { tree, current } = get();
    const child = tree.nodes[current].children[0];
    if (child) set({ current: child });
  },
  prev: () => {
    const { tree, current } = get();
    const parent = tree.nodes[current].parent;
    if (parent) set({ current: parent });
  },
  first: () => set({ current: get().tree.root }),
  last: () => {
    const { tree, current } = get();
    const line = lineFrom(tree, current);
    set({ current: line[line.length - 1] });
  },
  variation: (delta) => {
    const { tree, current } = get();
    const node = tree.nodes[current];
    if (!node.parent) return;
    const siblings = tree.nodes[node.parent].children;
    const i = siblings.indexOf(current);
    const target = siblings[i + delta];
    if (target) set({ current: target });
  },
  flip: () => set({ orientation: get().orientation === 'white' ? 'black' : 'white' }),
  setShapes: (shapes) => {
    const { tree, current } = get();
    set({ tree: setNode(tree, current, { shapes: shapes.length ? shapes : undefined }), dirty: true });
  },
  setComment: (text) => {
    const { tree, current } = get();
    set({ tree: setNode(tree, current, { comment: text.trim() || undefined }), dirty: true });
  },
  deleteVariation: (id) => {
    const r = deleteFrom(get().tree, id);
    if (!r) return;
    const current = r.tree.nodes[get().current] ? get().current : r.parent;
    set({ tree: r.tree, current, dirty: true });
  },
  promoteVariation: (id) => set({ tree: promote(get().tree, id), dirty: true }),

  loadTree: (tree, opts = {}) => {
    const line = lineFrom(tree, tree.root);
    const account = opts.game?.account?.trim().toLowerCase();
    const playerColor = account && opts.game?.black?.toLowerCase() === account ? 'black' : account && opts.game?.white?.toLowerCase() === account ? 'white' : null;
    set({
      playerColor,
      orientation: playerColor ?? 'white',
      analysisSession: get().analysisSession + 1,
      tree,
      current: opts.at === 'end' ? line[line.length - 1] : tree.root,
      game: opts.game ?? null,
      gameId: opts.gameId ?? null,
      report: opts.report ?? null,
      dirty: false,
      live: null,
      compare: null,
      nodeEvals: {},
      view: 'analyze',
    });
  },
  loadPgn: (pgn, opts = {}) => {
    try {
      const { tree, warning } = treeFromPgn(pgn);
      get().loadTree(tree, opts);
      return warning;
    } catch (e) {
      throw e instanceof Error ? e : new Error(String(e));
    }
  },
  loadFen: (fen) => {
    const err = fenError(fen);
    if (err) return err;
    get().loadTree(newTree(fen));
    return null;
  },
  newGame: () => get().loadTree(newTree()),
  setReport: (report) => set({ report }),
  markSaved: (id) => set({ gameId: id, dirty: false }),

  setEngineOn: (engineOn) => set({ engineOn, ...(engineOn ? {} : { live: null, compare: null }) }),
  setEngine: (engineId) => set({ engineId, live: null }),
  setCompare: (compareId) => set({ compareId, compare: null }),
  setLive: (live) => set({ live }),
  setCompareLive: (compare) => set({ compare }),
  setEngineError: (engineError) => set({ engineError }),
  setNodeEval: (id, score, depth) => {
    const prev = get().nodeEvals[id];
    if (prev && prev.depth > depth) return;
    set({ nodeEvals: { ...get().nodeEvals, [id]: { score, depth } } });
  },
}));

/** Adopt quieter defaults once, retaining non-default custom settings. */
export function migratePrefs(ui: Partial<UiPrefs>): UiPrefs {
  const next = { ...defaultPrefs, ...ui, collapsed: { ...ui.collapsed } };
  if (!ui.workflowVersion) {
    if (ui.multipv === 3) next.multipv = 1;
    if (ui.arrowCount === 3) next.arrowCount = 1;
    if (ui.depthLimit === null) next.depthLimit = 16;
    if (ui.crt === 0.35) next.crt = 0;
    if (ui.translucent === true) next.translucent = false;
  }
  if (!ui.appearanceVersion) next.crt = 0;
  return { ...next, workflowVersion: 2, appearanceVersion: 1 };
}
