import { beforeEach, describe, expect, it } from 'vitest';
import { useStore } from './store';
import { reviewByNode, evalSeries } from './views/analysis';
import { mainline } from './chess/tree';
import type { GameReport } from './lib/types';

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
