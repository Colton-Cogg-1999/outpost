// The analysis tree: the mainline plus any number of nested variations. Every node
// stores the FEN reached, computed when the move was validated, so the board shown
// and the notation can never disagree.
import { makePgn, parsePgn, parseComment, makeComment, startingPosition, type PgnNodeData, type Game, ChildNode, Node, defaultHeaders } from 'chessops/pgn';
import { makeSan, parseSan } from 'chessops/san';
import type { Chess } from 'chessops/chess';
import type { NormalMove } from 'chessops/types';
import { makeFen } from 'chessops/fen';
import { makeSquare, parseSquare } from 'chessops/util';
import { positionFromFen, moveFromUci, uciOf, fenOf, repetitionKey, START_FEN } from './position';

export type ShapeColor = 'green' | 'red' | 'yellow' | 'blue';
export interface Shape {
  from: string;
  to: string; // same as `from` for a circled square
  color: ShapeColor;
}

export interface TreeNode {
  id: string;
  parent: string | null;
  children: string[]; // children[0] continues the line
  ply: number; // 0 at the root
  fen: string;
  san?: string;
  uci?: string;
  comment?: string;
  nags?: number[];
  shapes?: Shape[];
  clock?: number;
}

export interface GameTree {
  nodes: Record<string, TreeNode>;
  root: string;
  headers: [string, string][];
  nextId: number;
}

export function newTree(fen: string = START_FEN, headers: [string, string][] = []): GameTree {
  const pos = positionFromFen(fen);
  if (!pos) throw new Error('Invalid starting position');
  const normalized = fenOf(pos);
  return { nodes: { n0: { id: 'n0', parent: null, children: [], ply: 0, fen: normalized } }, root: 'n0', headers, nextId: 1 };
}

export function startFen(tree: GameTree): string {
  return tree.nodes[tree.root].fen;
}

/** Adds (or reuses) the child reached by `uci`. Returns null for an illegal move. */
export function addMove(tree: GameTree, parentId: string, uci: string): { tree: GameTree; id: string } | null {
  const parent = tree.nodes[parentId];
  if (!parent) return null;
  const pos = positionFromFen(parent.fen);
  if (!pos) return null;
  const move = moveFromUci(pos, uci);
  if (!move) return null;
  const standard = uciOf(pos, move);
  const existing = parent.children.find((c) => tree.nodes[c].uci === standard);
  if (existing) return { tree, id: existing };
  const san = makeSan(pos, move);
  pos.play(move);
  const id = `n${tree.nextId}`;
  const node: TreeNode = { id, parent: parentId, children: [], ply: parent.ply + 1, fen: fenOf(pos), san, uci: standard };
  return {
    id,
    tree: { ...tree, nextId: tree.nextId + 1, nodes: { ...tree.nodes, [id]: node, [parentId]: { ...parent, children: [...parent.children, id] } } },
  };
}

/** Plays a sequence of UCI moves from `parentId`, stopping at the first illegal one. */
export function addLine(tree: GameTree, parentId: string, line: string[]): { tree: GameTree; id: string } {
  let current = { tree, id: parentId };
  for (const uci of line) {
    const next = addMove(current.tree, current.id, uci);
    if (!next) break;
    current = next;
  }
  return current;
}

export function pathTo(tree: GameTree, id: string): string[] {
  const out: string[] = [];
  let node: TreeNode | undefined = tree.nodes[id];
  while (node) {
    out.push(node.id);
    node = node.parent ? tree.nodes[node.parent] : undefined;
  }
  return out.reverse();
}

/** Follows first children from `from` (inclusive). */
export function lineFrom(tree: GameTree, from: string): string[] {
  const out = [from];
  let node = tree.nodes[from];
  while (node && node.children.length) {
    node = tree.nodes[node.children[0]];
    out.push(node.id);
  }
  return out;
}

export const mainline = (tree: GameTree) => lineFrom(tree, tree.root);

export function isOnMainline(tree: GameTree, id: string): boolean {
  let node = tree.nodes[id];
  while (node?.parent) {
    const parent = tree.nodes[node.parent];
    if (parent.children[0] !== node.id) return false;
    node = parent;
  }
  return true;
}

/** UCI moves from the root to `id`. */
export function movesTo(tree: GameTree, id: string): string[] {
  return pathTo(tree, id).slice(1).map((n) => tree.nodes[n].uci!);
}

export function repetitionCount(tree: GameTree, id: string): number {
  const key = repetitionKey(tree.nodes[id].fen);
  return pathTo(tree, id).filter((n) => repetitionKey(tree.nodes[n].fen) === key).length;
}

export function deleteFrom(tree: GameTree, id: string): { tree: GameTree; parent: string } | null {
  const node = tree.nodes[id];
  if (!node?.parent) return null;
  const nodes = { ...tree.nodes };
  const stack = [id];
  while (stack.length) {
    const n = stack.pop()!;
    stack.push(...nodes[n].children);
    delete nodes[n];
  }
  const parent = nodes[node.parent];
  nodes[node.parent] = { ...parent, children: parent.children.filter((c) => c !== id) };
  return { tree: { ...tree, nodes }, parent: node.parent };
}

/** Makes the variation containing `id` the main continuation at every branch point above it. */
export function promote(tree: GameTree, id: string): GameTree {
  const nodes = { ...tree.nodes };
  let node = nodes[id];
  while (node?.parent) {
    const parent = nodes[node.parent];
    if (parent.children[0] !== node.id) {
      nodes[parent.id] = { ...parent, children: [node.id, ...parent.children.filter((c) => c !== node.id)] };
    }
    node = nodes[node.parent];
  }
  return { ...tree, nodes };
}

export function setNode(tree: GameTree, id: string, patch: Partial<TreeNode>): GameTree {
  return { ...tree, nodes: { ...tree.nodes, [id]: { ...tree.nodes[id], ...patch } } };
}

export function header(tree: GameTree, name: string): string | undefined {
  const v = tree.headers.find(([k]) => k.toLowerCase() === name.toLowerCase())?.[1];
  return v && v !== '?' && v !== '????.??.??' ? v : undefined;
}

// ───────────── PGN ─────────────

export interface PgnLoad {
  tree: GameTree;
  /** Moves dropped because they were illegal, if any. */
  warning: string | null;
}

function fromGame(game: Game<PgnNodeData>): PgnLoad {
  const start = startingPosition(game.headers);
  if (start.isErr) throw new Error(`Invalid starting position in PGN: ${start.error.message}`);
  const headers = Array.from(game.headers.entries()).filter(([k]) => k !== 'FEN' && k !== 'SetUp');
  let tree = newTree(makeFen(start.value.toSetup()), headers);
  let warning: string | null = null;
  const visit = (pgnNode: Node<PgnNodeData>, parentId: string) => {
    for (const child of pgnNode.children) {
      const parent = tree.nodes[parentId];
      const pos = positionFromFen(parent.fen)!;
      const move = parseSanSafe(pos, child.data.san);
      if (!move) {
        warning ??= `Stopped at an illegal move: ${child.data.san}`;
        continue;
      }
      const added = addMove(tree, parentId, uciOf(pos, move));
      if (!added) continue;
      tree = added.tree;
      const comment = (child.data.comments ?? []).map(parseComment);
      const text = comment.map((c) => c.text).filter(Boolean).join(' ').trim();
      const shapes: Shape[] = comment.flatMap((c) => c.shapes).map((s) => ({ from: makeSquare(s.from), to: makeSquare(s.to), color: s.color }));
      const clock = comment.find((c) => c.clock !== undefined)?.clock;
      tree = setNode(tree, added.id, {
        comment: text || undefined,
        nags: child.data.nags?.length ? child.data.nags : undefined,
        shapes: shapes.length ? shapes : undefined,
        clock,
      });
      visit(child, added.id);
    }
  };
  visit(game.moves, tree.root);
  return { tree, warning };
}

function parseSanSafe(pos: Chess, san: string): NormalMove | null {
  const m = parseSan(pos, san);
  return m && 'from' in m ? m : null;
}

/** Parses every game in a PGN text. Throws if none can be read. */
export function treesFromPgn(text: string): PgnLoad[] {
  const games = parsePgn(text.replace(/^﻿/, ''));
  const out: PgnLoad[] = [];
  for (const game of games) {
    if (game.moves.children.length === 0 && !game.headers.has('FEN') && game.headers.size === 0) continue;
    out.push(fromGame(game));
  }
  if (!out.length) throw new Error('No games found in that PGN.');
  return out;
}

export function treeFromPgn(text: string): PgnLoad {
  return treesFromPgn(text)[0];
}

export function toPgn(tree: GameTree, opts: { shapes?: boolean; result?: string } = {}): string {
  const headers = defaultHeaders();
  for (const [k, v] of tree.headers) headers.set(k, v);
  const start = startFen(tree);
  if (start !== START_FEN) {
    headers.set('SetUp', '1');
    headers.set('FEN', start);
  }
  if (opts.result) headers.set('Result', opts.result);
  const root = new Node<PgnNodeData>();
  const build = (id: string, into: Node<PgnNodeData>) => {
    for (const childId of tree.nodes[id].children) {
      const n = tree.nodes[childId];
      const shapes = opts.shapes !== false && n.shapes?.length
        ? n.shapes.map((s) => ({ color: s.color, from: parseSquare(s.from)!, to: parseSquare(s.to)! }))
        : [];
      const commentText = n.comment || shapes.length || n.clock !== undefined ? makeComment({ text: n.comment ?? '', shapes, clock: n.clock }) : '';
      const child = new ChildNode<PgnNodeData>({ san: n.san!, comments: commentText ? [commentText] : undefined, nags: n.nags });
      into.children.push(child);
      build(childId, child);
    }
  };
  build(tree.root, root);
  return makePgn({ headers, moves: root });
}

export function fenAt(tree: GameTree, id: string): string {
  return tree.nodes[id]?.fen ?? startFen(tree);
}

export { START_FEN };
