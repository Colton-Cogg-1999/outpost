import { beforeEach, describe, expect, it } from 'vitest';
import { useStore } from './store';
import { reviewByNode, evalSeries } from './views/analysis';
import { mainline } from './chess/tree';
import type { GameReport, GameSummary } from './lib/types';

const s = () => useStore.getState();

beforeEach(() => s().newGame());

describe('analysis state', () => {
  it('plays, navigates and branches without losing the mainline', () => {
    expect(s().play('e2e4')).toBe(true);
    expect(s().play('e7e5')).toBe(true);
    expect(s().play('e1e3')).toBe(false);
    s().first();
    expect(s().current).toBe(s().tree.root);
    s().last();
    expect(s().tree.nodes[s().current].san).toBe('e5');
    s().prev();
    s().play('c7c5');
    expect(s().tree.nodes[s().tree.nodes[s().current].parent!].children).toHaveLength(2);
    s().variation(-1);
    expect(s().tree.nodes[s().current].san).toBe('e5');
    expect(s().dirty).toBe(true);
  });

  it('loads FEN and rejects illegal positions', () => {
    expect(s().loadFen('8/8/8/8/8/8/8/8 w - - 0 1')).toMatch(/Illegal/);
    expect(s().loadFen('4k3/8/8/8/8/8/8/4K2R w K - 0 1')).toBeNull();
    expect(s().tree.nodes[s().current].fen).toBe('4k3/8/8/8/8/8/8/4K2R w K - 0 1');
  });

  it('stores user shapes per position', () => {
    s().play('e2e4');
    s().setShapes([{ from: 'd7', to: 'd5', color: 'green' }]);
    s().prev();
    expect(s().tree.nodes[s().current].shapes).toBeUndefined();
    s().next();
    expect(s().tree.nodes[s().current].shapes).toHaveLength(1);
  });

  it('applies a review only while the mainline still matches it', () => {
    s().playLine(['e2e4', 'e7e5', 'g1f3']);
    const cp = (value: number) => ({ kind: 'cp' as const, value });
    const move = (uci: string, san: string, ply: number) => ({ ply, color: ply % 2 ? 'white' : 'black', san, uci, bestSan: san, bestUci: uci, bestLine: [], evalBefore: cp(0), evalAfter: cp(0), classification: 'best', winBefore: 50, winAfter: 50, winLoss: 0, cpLoss: 0, accuracy: 100, reasons: [], sacrifice: null });
    const report = {
      startFen: s().tree.nodes[s().tree.root].fen,
      moves: [move('e2e4', 'e4', 1), move('e7e5', 'e5', 2), move('g1f3', 'Nf3', 3)],
      positions: [0, 10, 20, 30].map((v) => ({ fen: '', depth: 10, lines: [{ multipv: 1, depth: 10, score: cp(v), rawScore: cp(v), wdl: null, san: [], uci: [] }], terminal: null, cached: false })),
    } as unknown as GameReport;
    expect(reviewByNode(s().tree, report).size).toBe(3);
    expect(evalSeries(s().tree, report, {}).map((p) => p.score?.value)).toEqual([0, 10, 20, 30]);
    // Replace the third move: the review stops applying there.
    s().prev();
    s().play('b1c3');
    s().promoteVariation(s().current);
    expect(reviewByNode(s().tree, report).size).toBe(2);
    expect(mainline(s().tree)).toHaveLength(4);
  });
});

describe('quiet preference migration', () => {
  it('migrates legacy defaults once while preserving later choices', async () => {
    const { migratePrefs } = await import('./store');
    const migrated = migratePrefs({ multipv: 3, arrowCount: 3, depthLimit: null, crt: 0.35 });
    expect(migrated).toMatchObject({ multipv: 1, arrowCount: 1, depthLimit: 16, crt: 0 });
    expect(migratePrefs({ ...migrated, multipv: 3, depthLimit: null })).toMatchObject({ multipv: 3, depthLimit: null });
  });
});


describe('imported player perspective', () => {
  it('faces the imported account, independently of manual flips', () => {
    const game = { id: 7, account: 'MY-account', white: 'Opponent', black: 'my-Account' } as GameSummary;
    s().loadPgn('[White "Opponent"]\n[Black "my-Account"]\n\n1. e4 e5 *', { game, gameId: 7 });
    expect(s().orientation).toBe('black');
    expect(s().playerColor).toBe('black');
    s().flip();
    expect(s().orientation).toBe('white');
    expect(s().playerColor).toBe('black');
    s().loadTree(s().tree, { game });
    expect(s().orientation).toBe('black');
    s().loadTree(s().tree, { game: { ...game, account: 'opponent' } });
    expect(s().orientation).toBe('white');
    expect(s().playerColor).toBe('white');
    s().newGame();
    expect(s().playerColor).toBeNull();
    expect(s().orientation).toBe('white');
  });
});
