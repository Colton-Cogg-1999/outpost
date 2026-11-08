// OUTPOST's own pixel-art chess set: 16×16 sprites, drawn for this project.
// 'o' = outline, 'x' = body, '.' = transparent.
import { memo } from 'react';
import type { Color, Role } from 'chessops/types';

export const SPRITES: Record<Role, string[]> = {
  pawn: [
    '................',
    '................',
    '................',
    '......oooo......',
    '.....oxxxxo.....',
    '.....oxxxxo.....',
    '.....oxxxxo.....',
    '......oxxo......',
    '.....oxxxxo.....',
    '......oxxo......',
    '......oxxo......',
    '.....oxxxxo.....',
    '....oxxxxxxo....',
    '...oxxxxxxxxo...',
    '...oooooooooo...',
    '................',
  ],
  rook: [
    '................',
    '..ooo.oooo.ooo..',
    '..oxo.oxxo.oxo..',
    '..oxoooxxoooxo..',
    '..oxxxxxxxxxxo..',
    '..oooooooooooo..',
    '...oxxxxxxxxo...',
    '....oxxxxxxo....',
    '....oxxxxxxo....',
    '....oxxxxxxo....',
    '....oxxxxxxo....',
    '...oxxxxxxxxo...',
    '..oooooooooooo..',
    '..oxxxxxxxxxxo..',
    '..oooooooooooo..',
    '................',
  ],
  knight: [
    '................',
    '.......oo.......',
    '......oxxoo.....',
    '.....oxxxxxo....',
    '....oxxoxxxxo...',
    '...oxxxxxxxxxo..',
    '..oxxxxxxxxxxo..',
    '..oxxooxxxxxxo..',
    '...oo.oxxxxxxo..',
    '.....oxxxxxxo...',
    '....oxxxxxxxo...',
    '....oxxxxxxxo...',
    '...oxxxxxxxxxo..',
    '..ooooooooooooo.',
    '..oxxxxxxxxxxxo.',
    '..ooooooooooooo.',
  ],
  bishop: [
    '................',
    '.......oo.......',
    '......oxxo......',
    '.......oo.......',
    '......oxxo......',
    '.....oxxoxo.....',
    '....oxxxoxxo....',
    '....oxxoxxxo....',
    '....oxxxxxxo....',
    '.....oxxxxo.....',
    '......oxxo......',
    '.....oxxxxo.....',
    '....oxxxxxxo....',
    '..oooooooooooo..',
    '..oxxxxxxxxxxo..',
    '..oooooooooooo..',
  ],
  queen: [
    '................',
    '..o....oo....o..',
    '.oxo..oxxo..oxo.',
    '..o....oo....o..',
    '..oo..oxxo..oo..',
    '..oxo.oxxo.oxo..',
    '..oxxooxxooxxo..',
    '..oxxxxxxxxxxo..',
    '...oxxxxxxxxo...',
    '...oxxxxxxxxo...',
    '....oxxxxxxo....',
    '....oooooooo....',
    '...oxxxxxxxxo...',
    '..oooooooooooo..',
    '..oxxxxxxxxxxo..',
    '..oooooooooooo..',
  ],
  king: [
    '.......oo.......',
    '......oxxo......',
    '.....ooxxoo.....',
    '.....oxxxxo.....',
    '.....ooxxoo.....',
    '......oxxo......',
    '..oooo.oo.oooo..',
    '.oxxxxooooxxxxo.',
    '.oxxxxxxxxxxxxo.',
    '.oxxxxxxxxxxxxo.',
    '..oxxxxxxxxxxo..',
    '...oxxxxxxxxo...',
    '...oooooooooo...',
    '...oxxxxxxxxo...',
    '..oooooooooooo..',
    '................',
  ],
};

/** Merges horizontal runs of the same character into single rects. */
export function runs(rows: string[], ch: string): { x: number; y: number; w: number }[] {
  const out: { x: number; y: number; w: number }[] = [];
  rows.forEach((row, y) => {
    let x = 0;
    while (x < row.length) {
      if (row[x] === ch) {
        const start = x;
        while (x < row.length && row[x] === ch) x++;
        out.push({ x: start, y, w: x - start });
      } else x++;
    }
  });
  return out;
}

const cache = new Map<Role, { body: ReturnType<typeof runs>; outline: ReturnType<typeof runs> }>();
function shapes(role: Role) {
  let s = cache.get(role);
  if (!s) {
    s = { body: runs(SPRITES[role], 'x'), outline: runs(SPRITES[role], 'o') };
    cache.set(role, s);
  }
  return s;
}

export const PieceSprite = memo(function PieceSprite({ role, color }: { role: Role; color: Color }) {
  const { body, outline } = shapes(role);
  return (
    <svg className={`piece-svg ${color}`} viewBox="0 0 16 16" shapeRendering="crispEdges" aria-hidden="true">
      <g className="body">
        {body.map((r, i) => (
          <rect key={i} x={r.x} y={r.y} width={r.w} height={1} />
        ))}
      </g>
      <g className="outline">
        {outline.map((r, i) => (
          <rect key={i} x={r.x} y={r.y} width={r.w} height={1} />
        ))}
      </g>
    </svg>
  );
});
