import { describe, expect, it } from 'vitest';
import { analyseCells, findBoard, guessWhiteAtBottom, placement, plausibleCastling, propagate, startingPositionLabels, type Img, type PieceCode } from './detect';
import { fenError } from '../chess/position';

/** Draws a synthetic screenshot: a board at (ox, oy) with simple shapes as pieces. */
function render(layout: string[], ox = 37, oy = 21, cell = 40, w = 460, h = 400): Img {
  const data = new Uint8ClampedArray(w * h * 4);
  const put = (x: number, y: number, c: [number, number, number]) => {
    if (x < 0 || y < 0 || x >= w || y >= h) return;
    const i = (y * w + x) * 4;
    data[i] = c[0];
    data[i + 1] = c[1];
    data[i + 2] = c[2];
    data[i + 3] = 255;
  };
  for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) put(x, y, [30, 30, 34]);
  for (let r = 0; r < 8; r++) {
    for (let f = 0; f < 8; f++) {
      const light = (r + f) % 2 === 0;
      const sq: [number, number, number] = light ? [118, 150, 86] : [238, 238, 210];
      for (let y = 0; y < cell; y++) for (let x = 0; x < cell; x++) put(ox + f * cell + x, oy + r * cell + y, sq);
      const ch = layout[r][f];
      if (ch === '.') continue;
      const color: [number, number, number] = ch === ch.toUpperCase() ? [250, 250, 250] : [20, 20, 20];
      const t = ch.toLowerCase();
      // Distinct silhouettes per type: pawn small disc, rook square, knight L, bishop diamond, queen big disc, king cross.
      const cx = ox + f * cell + cell / 2;
      const cy = oy + r * cell + cell / 2;
      for (let y = -cell / 2; y < cell / 2; y++) {
        for (let x = -cell / 2; x < cell / 2; x++) {
          const ax = Math.abs(x);
          const ay = Math.abs(y);
          const inside =
            t === 'p' ? x * x + y * y < (cell * 0.18) ** 2 :
            t === 'r' ? ax < cell * 0.28 && ay < cell * 0.3 :
            t === 'n' ? (x > -cell * 0.25 && x < cell * 0.05 && ay < cell * 0.32) || (y < -cell * 0.1 && y > -cell * 0.32 && x > -cell * 0.25 && x < cell * 0.3) :
            t === 'b' ? ax + ay < cell * 0.32 :
            t === 'q' ? x * x + y * y < (cell * 0.34) ** 2 :
            (ax < cell * 0.08 && ay < cell * 0.36) || (ay < cell * 0.08 && ax < cell * 0.3);
          if (inside) put(Math.round(cx + x), Math.round(cy + y), color);
        }
      }
    }
  }
  return { data, width: w, height: h };
}

const START = ['rnbqkbnr', 'pppppppp', '........', '........', '........', '........', 'PPPPPPPP', 'RNBQKBNR'];
const MIDDLE = ['r...k..r', 'ppp..ppp', '..n.....', '...qp...', '...P.b..', '..N..N..', 'PP...PPP', 'R..QK..R'];

describe('screenshot recognition', () => {
  it('finds the board grid inside a larger image', () => {
    const b = findBoard(render(START))!;
    expect(Math.abs(b.x - 37)).toBeLessThanOrEqual(2);
    expect(Math.abs(b.y - 21)).toBeLessThanOrEqual(2);
    expect(Math.abs(b.size - 320)).toBeLessThanOrEqual(4);
  });

  it('recognises the starting position without help', () => {
    const img = render(START);
    const cells = analyseCells(img, findBoard(img)!);
    expect(cells.filter((c) => c.occupied)).toHaveLength(32);
    expect(cells[0].color).toBe('b');
    expect(cells[63].color).toBe('w');
    const labels = startingPositionLabels(cells)!;
    expect(placement(labels, true)).toBe('rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR');
  });

  it('learns piece types from a few labels and produces a valid FEN', () => {
    const img = render(MIDDLE);
    const cells = analyseCells(img, findBoard(img)!);
    expect(cells.filter((c) => c.occupied)).toHaveLength(MIDDLE.join('').replace(/\./g, '').length);
    expect(guessWhiteAtBottom(cells)).toBe(true);
    // The user labels one square of each type (any colour).
    const labels: Record<number, PieceCode> = { 0: 'r', 4: 'k', 8: 'p', 18: 'n', 27: 'q', 37: 'b' };
    const guesses = propagate(cells, labels);
    const board = placement(guesses.map((g) => g.piece), true);
    expect(board).toBe('r3k2r/ppp2ppp/2n5/3qp3/3P1b2/2N2N2/PP3PPP/R2QK2R');
    expect(plausibleCastling(board)).toBe('KQkq');
    expect(fenError(`${board} w ${plausibleCastling(board)} - 0 1`)).toBeNull();
  });

  it('handles a flipped board', () => {
    const flipped = START.map((r) => r.split('').reverse().join('')).reverse();
    const img = render(flipped);
    const cells = analyseCells(img, findBoard(img)!);
    expect(guessWhiteAtBottom(cells)).toBe(false);
    const labels = startingPositionLabels(cells)!;
    expect(placement(labels, false)).toBe('rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR');
  });

  it('flags squares it cannot classify instead of guessing', () => {
    const img = render(MIDDLE);
    const cells = analyseCells(img, findBoard(img)!);
    const guesses = propagate(cells, {});
    expect(guesses.filter((g) => g.uncertain).length).toBeGreaterThan(0);
    expect(guesses.every((g) => g.piece === null)).toBe(true);
  });
});
