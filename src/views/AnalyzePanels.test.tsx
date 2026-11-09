import { cleanup, render } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import { MoveList } from './AnalyzePanels';
import { useStore } from '../store';
import { mainline } from '../chess/tree';
import type { GameReport, GameSummary } from '../lib/types';
vi.mock('../lib/ipc', () => ({ api: {}, logError: vi.fn() }));
afterEach(cleanup);
it('attaches each rating to its own move and identifies the imported player', () => {
  const s = useStore.getState();
  s.loadPgn('[White "Opponent"]\n[Black "Me"]\n\n1. e4 e5 *', { game: { account: 'me', white: 'Opponent', black: 'Me' } as GameSummary });
  const state = useStore.getState();
  const ids = mainline(state.tree).slice(1);
  const report = { startFen: state.tree.nodes[state.tree.root].fen, moves: ids.map((id, index) => ({
    ply: index + 1, san: state.tree.nodes[id].san, uci: state.tree.nodes[id].uci,
    color: index ? 'black' : 'white', classification: index ? 'mistake' : 'best', evalAfter: { kind: 'cp', value: 20 },
  })) } as GameReport;
  useStore.setState({ report });
  const { container } = render(<MoveList reviewing={null} onReview={() => {}} />);
  const moves = container.querySelectorAll('.mv');
  expect(moves[0].textContent).toContain('e4');
  expect(moves[0].querySelector('.row-rating')?.textContent).toBe('Best');
  expect(moves[1].textContent).toContain('e5');
  expect(moves[1].querySelector('.row-rating')?.textContent).toBe('Mistake');
  expect(moves[1].getAttribute('title')).toContain('Me (you): e5');
  expect(container.querySelector('.move-players')?.textContent).toContain('Black · Me (you)');
});
