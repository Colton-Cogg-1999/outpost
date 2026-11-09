import { expect, it } from 'vitest';
import { basicPuzzles, checkBasic } from './basics';
import { positionFromFen } from '../chess/position';
it('offers twelve legal basic positions with working checkmates for both colors', () => {
  expect(basicPuzzles).toHaveLength(12);
  for (const puzzle of basicPuzzles) {
    const pos = positionFromFen(puzzle.fen);
    expect(pos, puzzle.title).not.toBeNull();
    expect(pos!.isCheckmate()).toBe(false);
    expect(pos!.turn).toBe(puzzle.solver);
    expect(checkBasic(puzzle, puzzle.fen, puzzle.moves[0]).result, `${puzzle.title}: ${puzzle.fen}`).toBe('correct');
    expect(checkBasic(puzzle, puzzle.fen, 'a1a1').result).toBe('incorrect');
  }
});
