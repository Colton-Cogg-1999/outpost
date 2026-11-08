import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render } from '@testing-library/react';
import { Board } from './Board';
import { SPRITES } from './pieces';
import { START_FEN } from '../chess/position';

afterEach(cleanup);

/** jsdom has no layout: give the board a fixed 400px box at (0,0). */
function setup(props: Partial<Parameters<typeof Board>[0]> = {}) {
  const onMove = vi.fn();
  const onShapes = vi.fn();
  const utils = render(<Board fen={START_FEN} orientation="white" movable="both" onMove={onMove} onShapes={onShapes} {...props} />);
  const board = utils.container.querySelector('.board') as HTMLElement;
  board.getBoundingClientRect = () => ({ left: 0, top: 0, width: 400, height: 400, right: 400, bottom: 400, x: 0, y: 0, toJSON: () => ({}) });
  const at = (sq: string, flip = false) => {
    const f = sq.charCodeAt(0) - 97;
    const r = Number(sq[1]) - 1;
    const x = flip ? 7 - f : f;
    const y = flip ? r : 7 - r;
    return { clientX: x * 50 + 25, clientY: y * 50 + 25 };
  };
  const click = (sq: string, flip = false) => {
    fireEvent.pointerDown(board, { ...at(sq, flip), button: 0, pointerId: 1 });
    fireEvent.pointerUp(board, { ...at(sq, flip), button: 0, pointerId: 1 });
  };
  return { ...utils, board, onMove, onShapes, at, click };
}

describe('Board', () => {
  it('renders 32 pieces and 64 squares', () => {
    const { container } = setup();
    expect(container.querySelectorAll('.piece')).toHaveLength(32);
    expect(container.querySelectorAll('.sq')).toHaveLength(64);
  });

  it('click-to-move plays a legal move and shows destinations', () => {
    const { container, onMove, click, board, at } = setup();
    fireEvent.pointerDown(board, { ...at('e2'), button: 0, pointerId: 1 });
    fireEvent.pointerUp(board, { ...at('e2'), button: 0, pointerId: 1 });
    expect(container.querySelectorAll('.dest')).toHaveLength(2);
    click('e4');
    expect(onMove).toHaveBeenCalledWith('e2e4');
  });

  it('ignores illegal targets and the side not to move', () => {
    const { onMove, click } = setup();
    click('e2');
    click('e5');
    click('e7');
    click('e5');
    expect(onMove).not.toHaveBeenCalled();
  });

  it('drag and drop plays a move', () => {
    const { onMove, board, at } = setup();
    fireEvent.pointerDown(board, { ...at('g1'), button: 0, pointerId: 1 });
    fireEvent.pointerMove(board, { ...at('f3'), pointerId: 1 });
    fireEvent.pointerUp(board, { ...at('f3'), button: 0, pointerId: 1 });
    expect(onMove).toHaveBeenCalledWith('g1f3');
  });

  it('castles by moving the king two squares', () => {
    const { onMove, click } = setup({ fen: 'r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1' });
    click('e1');
    click('g1');
    expect(onMove).toHaveBeenCalledWith('e1g1');
  });

  it('asks for the promotion piece, including underpromotion', () => {
    const { onMove, click, getByLabelText } = setup({ fen: '8/P6k/8/8/8/8/8/K7 w - - 0 1' });
    click('a7');
    click('a8');
    expect(onMove).not.toHaveBeenCalled();
    fireEvent.click(getByLabelText('Promote to knight'));
    expect(onMove).toHaveBeenCalledWith('a7a8n');
  });

  it('respects orientation', () => {
    const { onMove, click } = setup({ orientation: 'black' });
    click('e2', true);
    click('e4', true);
    expect(onMove).toHaveBeenCalledWith('e2e4');
  });

  it('draws user arrows with the right button and renders engine arrows', () => {
    const { onShapes, board, at, container } = setup({ arrows: [{ from: 'e2', to: 'e4', kind: 'best' }, { from: 'd2', to: 'd4', kind: 'alt', weight: 0.5 }] });
    expect(container.querySelectorAll('.shape.best')).toHaveLength(1);
    expect(container.querySelectorAll('.shape.alt')).toHaveLength(1);
    fireEvent.pointerDown(board, { ...at('g1'), button: 2, pointerId: 2 });
    fireEvent.pointerMove(board, { ...at('f3'), pointerId: 2 });
    fireEvent.pointerUp(board, { ...at('f3'), button: 2, pointerId: 2 });
    expect(onShapes).toHaveBeenCalledWith([{ from: 'g1', to: 'f3', color: 'yellow' }]);
    fireEvent.pointerDown(board, { ...at('e4'), button: 2, shiftKey: true, pointerId: 2 });
    fireEvent.pointerUp(board, { ...at('e4'), button: 2, pointerId: 2 });
    expect(onShapes).toHaveBeenLastCalledWith([{ from: 'e4', to: 'e4', color: 'red' }]);
  });

  it('marks the king in check', () => {
    const { container } = setup({ fen: 'rnb1kbnr/pppp1ppp/8/4p3/6Pq/5P2/PPPPP2P/RNBQKBNR w KQkq - 1 3' });
    expect(container.querySelector('.sq.check')?.getAttribute('data-square')).toBe('e1');
  });
});

describe('pixel sprites', () => {
  it('are all 16×16', () => {
    for (const rows of Object.values(SPRITES)) {
      expect(rows).toHaveLength(16);
      for (const r of rows) expect(r).toHaveLength(16);
    }
  });
});
