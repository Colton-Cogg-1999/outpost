// Experimental board recognition from screenshots, using plain image statistics:
//  1. find the 8×8 grid from edge projections (square boundaries are strong edges),
//  2. per square, separate the piece from the square's own background colour,
//  3. classify piece colour by brightness, and piece type by nearest silhouette among
//     squares the user has labelled (or a recognised starting position).
// Nothing is guessed silently: unknown squares are flagged for the user to fix, and a
// FEN is only produced after the user confirms and the rules library validates it.

export interface Img {
  data: Uint8ClampedArray; // RGBA
  width: number;
  height: number;
}

export interface Bounds {
  x: number;
  y: number;
  size: number; // board edge length in pixels
}

export type PieceCode = 'P' | 'N' | 'B' | 'R' | 'Q' | 'K' | 'p' | 'n' | 'b' | 'r' | 'q' | 'k';

export interface Cell {
  index: number; // 0 = a8 when white is at the bottom (screen order, row-major)
  occupied: boolean;
  /** 'w' | 'b' when occupied. */
  color: 'w' | 'b' | null;
  /** Fraction of the inner square that differs from the background. */
  coverage: number;
  /** 16×16 silhouette, 1 = piece pixel. */
  mask: Uint8Array;
  brightness: number;
}

const lum = (d: Uint8ClampedArray, i: number) => 0.299 * d[i] + 0.587 * d[i + 1] + 0.114 * d[i + 2];

function edgeProfiles(img: Img): { cols: Float64Array; rows: Float64Array } {
  const { data, width, height } = img;
  const cols = new Float64Array(width);
  const rows = new Float64Array(height);
  for (let y = 1; y < height; y++) {
    for (let x = 1; x < width; x++) {
      const i = (y * width + x) * 4;
      const l = lum(data, i);
      cols[x] += Math.abs(l - lum(data, i - 4));
      rows[y] += Math.abs(l - lum(data, i - width * 4));
    }
  }
  return { cols, rows };
}

/** Best start and spacing of 9 evenly spaced edge lines in a 1-D profile. */
function bestGrid(profile: Float64Array, minCell: number, maxCell: number, fixedCell?: number): { start: number; cell: number; score: number } {
  const n = profile.length;
  let best = { start: 0, cell: 0, score: -Infinity };
  const cells = fixedCell ? [fixedCell - 1, fixedCell, fixedCell + 1] : Array.from({ length: Math.max(0, maxCell - minCell + 1) }, (_, i) => minCell + i);
  // Light smoothing makes 1-pixel misalignment cost little.
  const at = (x: number) => {
    const i = Math.round(x);
    if (i < 1 || i >= n - 1) return i >= 0 && i < n ? profile[i] : 0;
    return Math.max(profile[i - 1], profile[i], profile[i + 1]);
  };
  for (const cell of cells) {
    if (cell < 4) continue;
    for (let start = 0; start + 8 * cell < n; start++) {
      let lines = 0;
      let mids = 0;
      for (let k = 0; k <= 8; k++) lines += at(start + k * cell);
      for (let k = 0; k < 8; k++) mids += at(start + (k + 0.5) * cell);
      const score = lines / 9 - mids / 8;
      if (score > best.score) best = { start, cell, score };
    }
  }
  return best;
}

export function findBoard(img: Img): Bounds | null {
  const { cols, rows } = edgeProfiles(img);
  const minSide = Math.min(img.width, img.height);
  const minCell = Math.max(6, Math.floor(minSide / 40));
  const maxCell = Math.floor(minSide / 8);
  const gx = bestGrid(cols, minCell, maxCell);
  if (gx.cell === 0) return null;
  const gy = bestGrid(rows, minCell, maxCell, gx.cell);
  if (gy.cell === 0) return null;
  const cell = (gx.cell + gy.cell) / 2;
  return { x: gx.start, y: gy.start, size: Math.round(cell * 8) };
}

function pixel(img: Img, x: number, y: number): [number, number, number] {
  const i = (Math.min(img.height - 1, Math.max(0, Math.round(y))) * img.width + Math.min(img.width - 1, Math.max(0, Math.round(x)))) * 4;
  return [img.data[i], img.data[i + 1], img.data[i + 2]];
}

/** Median colour of small patches in the four corners of a square (rarely covered by a piece). */
function background(img: Img, x0: number, y0: number, s: number): [number, number, number] {
  const samples: [number, number, number][] = [];
  const m = Math.max(1, Math.floor(s * 0.04));
  const p = Math.max(2, Math.floor(s * 0.12));
  for (const [cx, cy] of [[m, m], [s - m - p, m], [m, s - m - p], [s - m - p, s - m - p]]) {
    for (let dy = 0; dy < p; dy += Math.max(1, Math.floor(p / 4))) for (let dx = 0; dx < p; dx += Math.max(1, Math.floor(p / 4))) samples.push(pixel(img, x0 + cx + dx, y0 + cy + dy));
  }
  const med = (k: 0 | 1 | 2) => samples.map((c) => c[k]).sort((a, b) => a - b)[Math.floor(samples.length / 2)];
  return [med(0), med(1), med(2)];
}

export function analyseCells(img: Img, b: Bounds): Cell[] {
  const cell = b.size / 8;
  const out: Cell[] = [];
  const brightnesses: number[] = [];
  for (let row = 0; row < 8; row++) {
    for (let col = 0; col < 8; col++) {
      const x0 = b.x + col * cell;
      const y0 = b.y + row * cell;
      const bg = background(img, x0, y0, cell);
      const mask = new Uint8Array(256);
      let fg = 0;
      let fgLum = 0;
      for (let my = 0; my < 16; my++) {
        for (let mx = 0; mx < 16; mx++) {
          const [r, g, bl] = pixel(img, x0 + cell * (0.1 + 0.8 * (mx + 0.5) / 16), y0 + cell * (0.1 + 0.8 * (my + 0.5) / 16));
          const diff = Math.abs(r - bg[0]) + Math.abs(g - bg[1]) + Math.abs(bl - bg[2]);
          if (diff > 60) {
            mask[my * 16 + mx] = 1;
            fg++;
            fgLum += 0.299 * r + 0.587 * g + 0.114 * bl;
          }
        }
      }
      const coverage = fg / 256;
      const occupied = coverage > 0.07;
      const brightness = fg ? fgLum / fg : 0;
      if (occupied) brightnesses.push(brightness);
      out.push({ index: row * 8 + col, occupied, color: null, coverage, mask, brightness });
    }
  }
  // Two-means on brightness separates white pieces from black pieces.
  if (brightnesses.length) {
    let lo = Math.min(...brightnesses);
    let hi = Math.max(...brightnesses);
    for (let iter = 0; iter < 10; iter++) {
      const mid = (lo + hi) / 2;
      const a = brightnesses.filter((v) => v < mid);
      const c = brightnesses.filter((v) => v >= mid);
      if (a.length) lo = a.reduce((s, v) => s + v, 0) / a.length;
      if (c.length) hi = c.reduce((s, v) => s + v, 0) / c.length;
    }
    const split = hi - lo < 25 ? 128 : (lo + hi) / 2;
    for (const c of out) if (c.occupied) c.color = c.brightness >= split ? 'w' : 'b';
  }
  return out;
}

function distance(a: Uint8Array, b: Uint8Array): number {
  let d = 0;
  for (let i = 0; i < a.length; i++) if (a[i] !== b[i]) d++;
  return d / a.length;
}

const START_ROWS = ['rnbqkbnr', 'pppppppp', '', '', '', '', 'PPPPPPPP', 'RNBQKBNR'];

/** If occupancy and colours match the initial position, label it (and learn templates from it). */
export function startingPositionLabels(cells: Cell[]): (PieceCode | null)[] | null {
  for (const flipped of [false, true]) {
    const labels: (PieceCode | null)[] = [];
    let ok = true;
    for (const c of cells) {
      const row = Math.floor(c.index / 8);
      const col = c.index % 8;
      const r = flipped ? 7 - row : row;
      const f = flipped ? 7 - col : col;
      const ch = START_ROWS[r][f] as PieceCode | undefined;
      if (!!ch !== c.occupied || (ch && (ch === ch.toUpperCase() ? 'w' : 'b') !== c.color)) {
        ok = false;
        break;
      }
      labels.push(ch ?? null);
    }
    if (ok) return labels;
  }
  return null;
}

export interface Guess {
  piece: PieceCode | null;
  /** True when the user should check this square. */
  uncertain: boolean;
}

/**
 * Labels every occupied square from the user's labelled examples, matching silhouettes
 * (colour is measured separately, so a labelled white knight also teaches black knights).
 */
export function propagate(cells: Cell[], labels: Record<number, PieceCode | null>): Guess[] {
  const examples = Object.entries(labels)
    .filter(([, p]) => p)
    .map(([i, p]) => ({ mask: cells[Number(i)].mask, type: (p as string).toLowerCase() }));
  return cells.map((c) => {
    if (c.index in labels) return { piece: labels[c.index], uncertain: false };
    if (!c.occupied) return { piece: null, uncertain: c.coverage > 0.04 };
    if (!examples.length) return { piece: null, uncertain: true };
    let best = { type: '', d: Infinity };
    let second = Infinity;
    for (const e of examples) {
      const d = distance(c.mask, e.mask);
      if (d < best.d) {
        if (e.type !== best.type) second = best.d;
        best = { type: e.type, d };
      } else if (e.type !== best.type && d < second) second = d;
    }
    const type = best.type;
    const piece = (c.color === 'w' ? type.toUpperCase() : type) as PieceCode;
    const uncertain = best.d > 0.12 || second - best.d < 0.03;
    return { piece, uncertain };
  });
}

/** Board placement field of a FEN from screen-ordered pieces. */
export function placement(pieces: (PieceCode | null)[], whiteAtBottom: boolean): string {
  const rows: string[] = [];
  for (let r = 0; r < 8; r++) {
    let row = '';
    let empty = 0;
    for (let f = 0; f < 8; f++) {
      const screen = whiteAtBottom ? r * 8 + f : (7 - r) * 8 + (7 - f);
      const p = pieces[screen];
      if (!p) empty++;
      else {
        if (empty) row += empty;
        empty = 0;
        row += p;
      }
    }
    if (empty) row += empty;
    rows.push(row);
  }
  return rows.join('/');
}

/** Castling rights that are plausible from king and rook placement. */
export function plausibleCastling(board: string): string {
  const rows = board.split('/').map((r) => r.replace(/\d/g, (d) => '.'.repeat(Number(d))));
  let out = '';
  if (rows[7][4] === 'K') {
    if (rows[7][7] === 'R') out += 'K';
    if (rows[7][0] === 'R') out += 'Q';
  }
  if (rows[0][4] === 'k') {
    if (rows[0][7] === 'r') out += 'k';
    if (rows[0][0] === 'r') out += 'q';
  }
  return out || '-';
}

export function guessWhiteAtBottom(cells: Cell[]): boolean {
  let score = 0;
  for (const c of cells) {
    if (!c.occupied) continue;
    const bottom = Math.floor(c.index / 8) >= 4;
    score += (c.color === 'w') === bottom ? 1 : -1;
  }
  return score >= 0;
}
