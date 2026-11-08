import { useCallback, useEffect, useMemo, useRef, useState, type PointerEvent as ReactPointerEvent } from 'react';
import type { Role, Square } from 'chessops/types';
import { makeSquare, parseSquare } from 'chessops/util';
import { PieceSprite } from './pieces';
import { isPromotion, legalDests, positionFromFen, toMove, uciOf } from '../chess/position';
import type { Shape, ShapeColor } from '../chess/tree';

export type ArrowKind = 'best' | 'alt' | 'compare' | 'user' | 'hint';
export interface Arrow {
  from: string;
  to: string;
  kind: ArrowKind;
  color?: ShapeColor;
  /** 0..1, lets secondary lines fade with their rank. */
  weight?: number;
}

export interface Mark {
  square: string;
  kind: 'good' | 'bad' | 'hint' | 'focus';
}

export interface BoardProps {
  fen: string;
  orientation: 'white' | 'black';
  lastMove?: [string, string] | null;
  /** Which side may move pieces. */
  movable: 'white' | 'black' | 'both' | 'none';
  onMove?: (uci: string) => void;
  arrows?: Arrow[];
  shapes?: Shape[];
  onShapes?: (shapes: Shape[]) => void;
  marks?: Mark[];
  coordinates?: boolean;
  showDests?: boolean;
  label?: string;
}

const PROMOTION_ROLES: Role[] = ['queen', 'knight', 'rook', 'bishop'];

function shapeColor(e: { shiftKey: boolean; altKey: boolean; ctrlKey: boolean; metaKey: boolean }): ShapeColor {
  if (e.shiftKey) return 'red';
  if (e.altKey) return 'blue';
  if (e.ctrlKey || e.metaKey) return 'green';
  return 'yellow';
}

export function Board(props: BoardProps) {
  const { fen, orientation, lastMove, movable, onMove, arrows = [], shapes = [], onShapes, marks = [], coordinates = true, showDests = true, label } = props;
  const ref = useRef<HTMLDivElement>(null);
  const pos = useMemo(() => positionFromFen(fen), [fen]);
  const dests = useMemo(() => (pos ? legalDests(pos) : new Map<Square, Square[]>()), [pos]);
  const canMove = !!pos && !!onMove && (movable === 'both' || movable === pos.turn);
  const [selected, setSelected] = useState<Square | null>(null);
  const [drag, setDrag] = useState<{ from: Square; x: number; y: number; moved: boolean } | null>(null);
  const [drawing, setDrawing] = useState<{ from: Square; to: Square; color: ShapeColor } | null>(null);
  const [promotion, setPromotion] = useState<{ from: Square; to: Square } | null>(null);

  useEffect(() => {
    setSelected(null);
    setPromotion(null);
    setDrag(null);
  }, [fen]);

  const flip = orientation === 'black';
  const toXY = useCallback((sq: Square) => {
    const file = sq & 7;
    const rank = sq >> 3;
    return flip ? { x: 7 - file, y: rank } : { x: file, y: 7 - rank };
  }, [flip]);

  const squareAt = useCallback((clientX: number, clientY: number): Square | null => {
    const el = ref.current;
    if (!el) return null;
    const r = el.getBoundingClientRect();
    const x = Math.floor(((clientX - r.left) / r.width) * 8);
    const y = Math.floor(((clientY - r.top) / r.height) * 8);
    if (x < 0 || x > 7 || y < 0 || y > 7) return null;
    return flip ? (y * 8 + (7 - x)) as Square : ((7 - y) * 8 + x) as Square;
  }, [flip]);

  const ownPiece = (sq: Square) => {
    const piece = pos?.board.get(sq);
    return !!piece && piece.color === pos!.turn && canMove;
  };

  const attempt = (from: Square, to: Square): boolean => {
    if (!pos || !(dests.get(from) ?? []).includes(to)) return false;
    if (isPromotion(pos, from, to)) {
      setPromotion({ from, to });
      return true;
    }
    const move = toMove(pos, from, to);
    if (!move) return false;
    setSelected(null);
    onMove?.(uciOf(pos, move));
    return true;
  };

  const choosePromotion = (role: Role) => {
    if (!pos || !promotion) return;
    const move = toMove(pos, promotion.from, promotion.to, role);
    setPromotion(null);
    setSelected(null);
    if (move) onMove?.(uciOf(pos, move));
  };

  const onPointerDown = (e: ReactPointerEvent) => {
    if (promotion) return;
    const sq = squareAt(e.clientX, e.clientY);
    if (sq === null) return;
    if (e.button === 2) {
      e.preventDefault();
      setDrawing({ from: sq, to: sq, color: shapeColor(e) });
      return;
    }
    if (e.button !== 0) return;
    if (selected !== null && selected !== sq && attempt(selected, sq)) return;
    if (ownPiece(sq)) {
      setSelected(sq);
      setDrag({ from: sq, x: e.clientX, y: e.clientY, moved: false });
      (e.target as Element).setPointerCapture?.(e.pointerId);
    } else {
      setSelected(null);
      if (shapes.length) onShapes?.([]);
    }
  };

  const onPointerMove = (e: ReactPointerEvent) => {
    if (drag) setDrag({ ...drag, x: e.clientX, y: e.clientY, moved: drag.moved || Math.hypot(e.clientX - drag.x, e.clientY - drag.y) > 4 });
    if (drawing) {
      const sq = squareAt(e.clientX, e.clientY);
      if (sq !== null && sq !== drawing.to) setDrawing({ ...drawing, to: sq });
    }
  };

  const onPointerUp = (e: ReactPointerEvent) => {
    if (drawing && e.button === 2) {
      const from = makeSquare(drawing.from);
      const to = makeSquare(drawing.to);
      const existing = shapes.find((s) => s.from === from && s.to === to);
      const rest = shapes.filter((s) => !(s.from === from && s.to === to));
      onShapes?.(existing && existing.color === drawing.color ? rest : [...rest, { from, to, color: drawing.color }]);
      setDrawing(null);
      return;
    }
    if (drag) {
      const target = squareAt(e.clientX, e.clientY);
      const from = drag.from;
      setDrag(null);
      if (target !== null && target !== from && drag.moved) attempt(from, target);
    }
  };

  const selectedDests = selected !== null ? dests.get(selected) ?? [] : [];
  const checkSquare = pos && pos.isCheck() ? pos.board.kingOf(pos.turn) : undefined;
  const lastSquares = new Set((lastMove ?? []).map((s) => parseSquare(s)));
  const markBy = new Map(marks.map((m) => [parseSquare(m.square), m.kind]));

  const squares = [];
  for (let y = 0; y < 8; y++) {
    for (let x = 0; x < 8; x++) {
      const sq = (flip ? y * 8 + (7 - x) : (7 - y) * 8 + x) as Square;
      const light = ((sq & 7) + (sq >> 3)) % 2 === 1;
      const cls = ['sq', light ? 'light' : 'dark'];
      if (lastSquares.has(sq)) cls.push('last');
      if (selected === sq) cls.push('selected');
      if (sq === checkSquare) cls.push('check');
      const mark = markBy.get(sq);
      if (mark) cls.push(`mark-${mark}`);
      const isDest = showDests && selectedDests.includes(sq);
      squares.push(
        <div key={sq} className={cls.join(' ')} data-square={makeSquare(sq)}>
          {isDest && <span className={pos?.board.get(sq) ? 'dest capture' : 'dest'} />}
        </div>,
      );
    }
  }

  const pieces = [];
  if (pos) {
    for (const sq of pos.board.occupied) {
      const piece = pos.board.get(sq)!;
      const { x, y } = toXY(sq);
      const dragging = drag?.from === sq && drag.moved;
      pieces.push(
        <div
          key={sq}
          className={`piece ${dragging ? 'ghosted' : ''}`}
          style={{ left: `${x * 12.5}%`, top: `${y * 12.5}%` }}
          data-piece={`${piece.color}-${piece.role}`}
        >
          <PieceSprite role={piece.role} color={piece.color} />
        </div>,
      );
    }
  }

  const center = (name: string) => {
    const sq = parseSquare(name);
    if (sq === undefined) return null;
    const { x, y } = toXY(sq);
    return { x: x + 0.5, y: y + 0.5 };
  };

  const allShapes: Arrow[] = [
    ...arrows,
    ...shapes.map((s) => ({ from: s.from, to: s.to, kind: 'user' as const, color: s.color })),
    ...(drawing ? [{ from: makeSquare(drawing.from), to: makeSquare(drawing.to), kind: 'user' as const, color: drawing.color, weight: 0.6 }] : []),
  ];

  const dragPiece = drag?.moved && pos ? pos.board.get(drag.from) : undefined;
  const boardRect = ref.current?.getBoundingClientRect();

  return (
    <div className={`board-frame ${coordinates ? 'with-coords' : ''}`}>
      {coordinates && (
        <div className="coords ranks" aria-hidden="true">
          {Array.from({ length: 8 }, (_, i) => <span key={i}>{flip ? i + 1 : 8 - i}</span>)}
        </div>
      )}
      <div
        ref={ref}
        className={`board ${canMove ? 'movable' : ''}`}
        role="application"
        aria-label={label ?? `Chess board, ${pos?.turn ?? ''} to move`}
        tabIndex={0}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onContextMenu={(e) => e.preventDefault()}
      >
        <div className="squares">{squares}</div>
        <div className="pieces">{pieces}</div>
        <svg className="overlay" viewBox="0 0 8 8" aria-hidden="true">
          {allShapes.map((a, i) => {
            const from = center(a.from);
            const to = center(a.to);
            if (!from || !to) return null;
            const cls = `shape ${a.kind} ${a.color ?? ''}`;
            if (a.from === a.to) return <circle key={i} className={cls} cx={from.x} cy={from.y} r={0.44} />;
            return <ArrowShape key={i} from={from} to={to} className={cls} kind={a.kind} weight={a.weight ?? 1} />;
          })}
        </svg>
        {promotion && pos && (
          <div className="promotion" onPointerDown={(e) => e.stopPropagation()}>
            {PROMOTION_ROLES.map((role, i) => {
              const { x } = toXY(promotion.to);
              const top = (toXY(promotion.to).y === 0 ? i : 7 - i) * 12.5;
              return (
                <button
                  key={role}
                  className="promo-choice"
                  style={{ left: `${x * 12.5}%`, top: `${top}%` }}
                  onClick={() => choosePromotion(role)}
                  aria-label={`Promote to ${role}`}
                >
                  <PieceSprite role={role} color={pos.turn} />
                </button>
              );
            })}
            <button className="promo-cancel" onClick={() => setPromotion(null)} aria-label="Cancel promotion" />
          </div>
        )}
      </div>
      {coordinates && (
        <div className="coords files" aria-hidden="true">
          {Array.from({ length: 8 }, (_, i) => <span key={i}>{'abcdefgh'[flip ? 7 - i : i]}</span>)}
        </div>
      )}
      {dragPiece && boardRect && (
        <div
          className="drag-ghost"
          style={{ width: boardRect.width / 8, height: boardRect.height / 8, left: drag!.x - boardRect.width / 16, top: drag!.y - boardRect.height / 16 }}
        >
          <PieceSprite role={dragPiece.role} color={dragPiece.color} />
        </div>
      )}
    </div>
  );
}

function ArrowShape({ from, to, className, kind, weight }: { from: { x: number; y: number }; to: { x: number; y: number }; className: string; kind: ArrowKind; weight: number }) {
  const dx = to.x - from.x;
  const dy = to.y - from.y;
  const len = Math.hypot(dx, dy);
  const ux = dx / len;
  const uy = dy / len;
  const width = (kind === 'best' ? 0.17 : kind === 'user' ? 0.15 : 0.12) * (0.75 + 0.25 * weight);
  const head = width * 2.4;
  const start = { x: from.x + ux * 0.18, y: from.y + uy * 0.18 };
  const tip = { x: to.x - ux * 0.08, y: to.y - uy * 0.08 };
  const base = { x: tip.x - ux * head, y: tip.y - uy * head };
  const px = -uy;
  const py = ux;
  const pts = [
    [tip.x, tip.y],
    [base.x + px * head * 0.62, base.y + py * head * 0.62],
    [base.x - px * head * 0.62, base.y - py * head * 0.62],
  ];
  const dash = kind === 'alt' ? '0.22 0.12' : kind === 'compare' ? '0.06 0.1' : undefined;
  return (
    <g className={className} style={{ opacity: kind === 'alt' ? 0.35 + 0.4 * weight : undefined }}>
      <line x1={start.x} y1={start.y} x2={base.x} y2={base.y} strokeWidth={width} strokeDasharray={dash} strokeLinecap={dash ? 'butt' : 'round'} />
      <polygon points={pts.map((p) => p.join(',')).join(' ')} />
    </g>
  );
}
