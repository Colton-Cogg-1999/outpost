// Thin helpers over chessops. All move input from the board passes through `toMove`,
// so the UI can only ever produce legal moves.
import { Chess, normalizeMove } from 'chessops/chess';
import { makeFen, parseFen } from 'chessops/fen';
import { makeSan, parseSan } from 'chessops/san';
import { kingCastlesTo, makeSquare, parseSquare, parseUci } from 'chessops/util';
import type { Move, NormalMove, Role, Square } from 'chessops/types';

export const START_FEN = 'rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1';

export function positionFromFen(fen: string): Chess | null {
  const setup = parseFen(fen.trim());
  if (setup.isErr) return null;
  const pos = Chess.fromSetup(setup.value);
  return pos.isOk ? pos.value : null;
}

export function fenError(fen: string): string | null {
  const setup = parseFen(fen.trim());
  if (setup.isErr) return `Invalid FEN (${setup.error.message.replace(/^ERR_/, '').toLowerCase()})`;
  const pos = Chess.fromSetup(setup.value);
  if (pos.isErr) return `Illegal position (${pos.error.message.replace(/^ERR_/, '').toLowerCase().replace(/_/g, ' ')})`;
  return null;
}

export const fenOf = (pos: Chess) => makeFen(pos.toSetup());

/** Position identity for repetition: board, turn, castling, en passant. */
export const repetitionKey = (fen: string) => fen.split(' ').slice(0, 4).join(' ');

/** Destination squares per origin, including the king-two-squares form of castling. */
export function legalDests(pos: Chess): Map<Square, Square[]> {
  const out = new Map<Square, Square[]>();
  const ctx = pos.ctx();
  for (const [from, set] of pos.allDests(ctx)) {
    const list = Array.from(set);
    if (from === ctx.king) {
      for (const to of Array.from(set)) {
        const piece = pos.board.get(to);
        if (piece && piece.color === pos.turn && piece.role === 'rook') {
          const side = to < from ? 'a' : 'h';
          list.push(kingCastlesTo(pos.turn, side));
        }
      }
    }
    if (list.length) out.set(from, list);
  }
  return out;
}

export function isPromotion(pos: Chess, from: Square, to: Square): boolean {
  const piece = pos.board.get(from);
  if (!piece || piece.role !== 'pawn') return false;
  const rank = to >> 3;
  return (piece.color === 'white' && rank === 7) || (piece.color === 'black' && rank === 0);
}

/** Builds a legal chessops move from board input, or null. */
export function toMove(pos: Chess, from: Square, to: Square, promotion?: Role): NormalMove | null {
  const move = normalizeMove(pos, { from, to, promotion }) as NormalMove;
  if (isPromotion(pos, from, to) && !promotion) return null;
  return pos.isLegal(move) ? move : null;
}

/** UCI in standard notation (castling as e1g1), as UCI engines expect. */
export function uciOf(pos: Chess, move: NormalMove): string {
  const piece = pos.board.get(move.from);
  const target = pos.board.get(move.to);
  let to = move.to;
  if (piece?.role === 'king' && target?.role === 'rook' && target.color === piece.color) {
    to = kingCastlesTo(piece.color, move.to < move.from ? 'a' : 'h');
  }
  return makeSquare(move.from) + makeSquare(to) + (move.promotion ? move.promotion === 'knight' ? 'n' : move.promotion[0] : '');
}

/** Parses standard or king-to-rook UCI into a legal move for `pos`, or null. */
export function moveFromUci(pos: Chess, uci: string): NormalMove | null {
  const raw = parseUci(uci);
  if (!raw || !('from' in raw)) return null;
  return toMove(pos, raw.from, raw.to, raw.promotion);
}

export const sanOf = (pos: Chess, move: Move) => makeSan(pos, move);

export function moveFromSan(pos: Chess, san: string): NormalMove | null {
  const m = parseSan(pos, san);
  return m && 'from' in m ? m : null;
}

export function playUci(fen: string, uci: string): { fen: string; san: string; uci: string } | null {
  const pos = positionFromFen(fen);
  if (!pos) return null;
  const move = moveFromUci(pos, uci);
  if (!move) return null;
  const san = makeSan(pos, move);
  const standard = uciOf(pos, move);
  pos.play(move);
  return { fen: fenOf(pos), san, uci: standard };
}

/** Converts a UCI line to SAN from a FEN, stopping at the first illegal move. */
export function uciLineToSan(fen: string, line: string[]): string[] {
  const pos = positionFromFen(fen);
  if (!pos) return [];
  const out: string[] = [];
  for (const uci of line) {
    const move = moveFromUci(pos, uci);
    if (!move) break;
    out.push(makeSan(pos, move));
    pos.play(move);
  }
  return out;
}

export const squareName = (sq: Square) => makeSquare(sq);
export const squareOf = (name: string) => parseSquare(name);

export function uciSquares(uci: string): [Square, Square] | null {
  const from = parseSquare(uci.slice(0, 2));
  const to = parseSquare(uci.slice(2, 4));
  return from === undefined || to === undefined ? null : [from, to];
}

export interface Status {
  over: boolean;
  check: boolean;
  text: string | null;
  result: string | null;
}

export function statusOf(pos: Chess, repetitions = 1): Status {
  const check = pos.isCheck();
  if (pos.isCheckmate()) {
    const winner = pos.turn === 'white' ? 'Black' : 'White';
    return { over: true, check, text: `Checkmate — ${winner} wins`, result: pos.turn === 'white' ? '0-1' : '1-0' };
  }
  if (pos.isStalemate()) return { over: true, check, text: 'Stalemate — draw', result: '1/2-1/2' };
  if (pos.isInsufficientMaterial()) return { over: true, check, text: 'Insufficient material — draw', result: '1/2-1/2' };
  if (repetitions >= 5) return { over: true, check, text: 'Fivefold repetition — draw', result: '1/2-1/2' };
  if (pos.halfmoves >= 150) return { over: true, check, text: 'Seventy-five-move rule — draw', result: '1/2-1/2' };
  if (repetitions >= 3) return { over: false, check, text: 'Threefold repetition — draw can be claimed', result: null };
  if (pos.halfmoves >= 100) return { over: false, check, text: 'Fifty-move rule — draw can be claimed', result: null };
  return { over: false, check, text: check ? 'Check' : null, result: null };
}
