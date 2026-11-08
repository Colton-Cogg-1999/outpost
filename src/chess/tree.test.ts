import { describe, expect, it } from 'vitest';
import { addLine, addMove, deleteFrom, isOnMainline, mainline, movesTo, newTree, promote, repetitionCount, toPgn, treeFromPgn, treesFromPgn } from './tree';
import { legalDests, moveFromUci, playUci, positionFromFen, statusOf, toMove, uciLineToSan, uciOf, fenError, START_FEN } from './position';
import { parseSquare } from 'chessops/util';

const sq = (s: string) => parseSquare(s)!;

describe('position helpers', () => {
  it('rejects illegal input and normalises castling to standard UCI', () => {
    const pos = positionFromFen('r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1')!;
    // Board input: king two squares (e1g1) and king-onto-rook (e1h1) are both castling.
    const a = toMove(pos, sq('e1'), sq('g1'))!;
    const b = toMove(pos, sq('e1'), sq('h1'))!;
    expect(uciOf(pos, a)).toBe('e1g1');
    expect(uciOf(pos, b)).toBe('e1g1');
    expect(uciOf(pos, toMove(pos, sq('e1'), sq('c1'))!)).toBe('e1c1');
    expect(toMove(pos, sq('e1'), sq('e3'))).toBeNull();
    const dests = legalDests(pos).get(sq('e1'))!;
    expect(dests).toContain(sq('g1'));
    expect(dests).toContain(sq('c1'));
  });

  it('requires a promotion piece and supports underpromotion', () => {
    const pos = positionFromFen('8/P6k/8/8/8/8/8/K7 w - - 0 1')!;
    expect(toMove(pos, sq('a7'), sq('a8'))).toBeNull();
    for (const [role, ch] of [['queen', 'q'], ['rook', 'r'], ['bishop', 'b'], ['knight', 'n']] as const) {
      const m = toMove(pos, sq('a7'), sq('a8'), role)!;
      expect(uciOf(pos, m)).toBe(`a7a8${ch}`);
    }
    expect(playUci('8/P6k/8/8/8/8/8/K7 w - - 0 1', 'a7a8n')!.san).toBe('a8=N');
  });

  it('handles en passant', () => {
    const r = playUci('rnbqkbnr/ppp1pppp/8/3pP3/8/8/PPPP1PPP/RNBQKBNR w KQkq d6 0 3', 'e5d6');
    expect(r!.san).toBe('exd6');
    expect(r!.fen.startsWith('rnbqkbnr/ppp1pppp/3P4/8/8/8/PPPP1PPP/RNBQKBNR b')).toBe(true);
  });

  it('reports mate, stalemate and insufficient material', () => {
    expect(statusOf(positionFromFen('rnb1kbnr/pppp1ppp/8/4p3/6Pq/5P2/PPPPP2P/RNBQKBNR w KQkq - 1 3')!).text).toMatch(/Checkmate — Black wins/);
    expect(statusOf(positionFromFen('7k/5Q2/6K1/8/8/8/8/8 b - - 0 1')!).text).toMatch(/Stalemate/);
    expect(statusOf(positionFromFen('8/8/4k3/8/8/3K4/8/8 w - - 0 1')!).over).toBe(true);
    expect(statusOf(positionFromFen(START_FEN)!, 3).text).toMatch(/Threefold/);
    expect(fenError('8/8/8/8/8/8/8/8 w - - 0 1')).toMatch(/Illegal position/);
    expect(fenError('nonsense')).toMatch(/Invalid FEN/);
  });

  it('converts engine lines and stops at illegal moves', () => {
    expect(uciLineToSan(START_FEN, ['e2e4', 'e7e5', 'g1f3', 'a1a8'])).toEqual(['e4', 'e5', 'Nf3']);
    expect(moveFromUci(positionFromFen(START_FEN)!, 'e2e5')).toBeNull();
  });
});

describe('game tree', () => {
  it('builds lines, reuses identical moves and creates variations', () => {
    let t = newTree();
    const a = addLine(t, t.root, ['e2e4', 'e7e5', 'g1f3']);
    t = a.tree;
    const again = addMove(t, t.root, 'e2e4')!;
    expect(again.id).toBe(t.nodes[t.root].children[0]);
    const v = addMove(t, t.root, 'd2d4')!;
    t = v.tree;
    expect(t.nodes[t.root].children).toHaveLength(2);
    expect(isOnMainline(t, v.id)).toBe(false);
    expect(mainline(t)).toHaveLength(4);
    expect(addMove(t, t.root, 'e2e5')).toBeNull();
    t = promote(t, v.id);
    expect(t.nodes[t.root].children[0]).toBe(v.id);
    expect(movesTo(t, a.id)).toEqual(['e2e4', 'e7e5', 'g1f3']);
    const del = deleteFrom(t, v.id)!;
    expect(del.tree.nodes[v.id]).toBeUndefined();
    expect(del.tree.nodes[t.root].children).toHaveLength(1);
  });

  it('counts repetitions along the path', () => {
    const { tree, id } = addLine(newTree(), 'n0', ['g1f3', 'g8f6', 'f3g1', 'f6g8', 'g1f3', 'g8f6', 'f3g1', 'f6g8']);
    expect(repetitionCount(tree, id)).toBe(3);
  });

  it('round-trips PGN with variations, comments, NAGs, shapes and custom FEN', () => {
    const pgn = `[Event "Test"]\n[White "A"]\n[Black "B"]\n[Result "*"]\n\n1. e4 { best by test [%cal Gg1f3] } 1... e5 $2 (1... c5 2. Nf3) 2. Nf3 *`;
    const { tree, warning } = treeFromPgn(pgn);
    expect(warning).toBeNull();
    const e4 = tree.nodes[tree.nodes[tree.root].children[0]];
    expect(e4.comment).toBe('best by test');
    expect(e4.shapes).toEqual([{ from: 'g1', to: 'f3', color: 'green' }]);
    const out = toPgn(tree);
    expect(out).toContain('( 1... c5 2. Nf3 )');
    expect(out).toContain('$2');
    expect(out).toContain('[%cal Gg1f3]');
    const again = treeFromPgn(out).tree;
    expect(toPgn(again)).toBe(out);

    const custom = treeFromPgn('[FEN "8/P6k/8/8/8/8/8/K7 w - - 0 1"]\n[SetUp "1"]\n\n1. a8=Q *').tree;
    expect(toPgn(custom)).toContain('[FEN "8/P6k/8/8/8/8/8/K7 w - - 0 1"]');
  });

  it('reads several games and stops cleanly at illegal moves', () => {
    const games = treesFromPgn('1. e4 e5 *\n\n[Event "2"]\n\n1. d4 Ke3 2. c4 *');
    expect(games).toHaveLength(2);
    expect(games[1].warning).toMatch(/illegal/);
    expect(mainline(games[1].tree)).toHaveLength(2);
    expect(() => treesFromPgn('')).toThrow();
  });
});
