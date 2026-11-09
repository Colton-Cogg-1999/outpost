// Reference-derived chess pieces; runs() supports the separate classification glyphs.
import { memo } from 'react';
import type { Color, Role } from 'chessops/types';

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

const PIECE_NAME: Record<Role, string> = { king: 'K', queen: 'Q', rook: 'R', bishop: 'B', knight: 'N', pawn: 'P' };

/** Vector silhouettes reconstructed from the supplied dashboard reference. */
export const PieceSprite = memo(function PieceSprite({ role, color }: { role: Role; color: Color }) {
  return <img className={`piece-svg ${color} ${role}`} src={`/pieces/reference/${color === 'white' ? 'w' : 'b'}${PIECE_NAME[role]}.svg`} alt="" draggable={false} />;
});
