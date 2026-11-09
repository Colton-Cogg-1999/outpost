import positions from './basics.json';
import { playUci, positionFromFen } from '../chess/position';
import type { Tactic, TacticVerdict } from '../lib/types';
export type PracticePuzzle = Tactic & { basic?: boolean; title?: string };
// Original teaching positions, in both colors. No download or game import needed.
export const basicPuzzles: PracticePuzzle[] = [false, true].flatMap((black) => positions.map((p, i) => {
  const board = p.fen.split(' ')[0];
  const mirrored = board.split('/').reverse().join('/').replace(/[a-z]/gi, (c) => c === c.toUpperCase() ? c.toLowerCase() : c.toUpperCase());
  return {
    id: -(i + 1 + (black ? positions.length : 0)), gameId: 0, ply: 0,
    basic: true, title: p.title, fen: black ? `${mirrored} b - - 0 1` : p.fen,
    moves: [black ? p.move.replace(/[1-8]/g, (n) => String(9 - Number(n))) : p.move],
    solver: black ? 'black' as const : 'white' as const, kind: 'mate' as const, cp: 999999,
    themes: [p.theme, 'mateIn1'], owner: null, played: null, attempts: 0, solved: 0, streak: 0, dueAt: 0,
    white: '', black: '', date: null, url: null,
  };
}));

export function checkBasic(puzzle: PracticePuzzle, fen: string, uci: string): TacticVerdict {
  const after = playUci(fen, uci);
  // Accept any legal mate, including a second valid solution to a teaching position.
  return after && positionFromFen(after.fen)?.isCheckmate()
    ? { result: 'correct', reply: null, solved: true, alternativeMate: uci !== puzzle.moves[0] }
    : { result: 'incorrect', expected: puzzle.moves[0] };
}
