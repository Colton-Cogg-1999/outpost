import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { AccountImport } from './ImportDialog';
import { api } from '../lib/ipc';
import { openGame } from './Games';
import type { HistoryGame } from '../lib/types';

vi.mock('../lib/ipc', () => ({ api: { history: vi.fn(), importSelected: vi.fn(), cancel: vi.fn() }, errorText: String, nextJobId: () => 3, logError: vi.fn() }));
vi.mock('./Games', () => ({ openGame: vi.fn().mockResolvedValue(undefined) }));
afterEach(() => { cleanup(); vi.clearAllMocks(); });

const game = { sourceId: 'one', white: 'Alice', black: 'Bob', date: '2026.11.01', speed: 'blitz', result: '1-0' } as HistoryGame;
describe('history selection', () => {
  it.each(['lichess', 'chesscom'] as const)('browses %s without importing, then imports only the chosen row', async (platform) => {
    vi.mocked(api.history).mockResolvedValue({ games: [game], nextCursor: 20 });
    vi.mocked(api.importSelected).mockResolvedValue({ id: 9, duplicate: false });
    render(<AccountImport platform={platform} />);
    fireEvent.change(screen.getByLabelText('Player username'), { target: { value: 'Alice' } });
    fireEvent.click(screen.getByText('Browse games'));
    await screen.findByText('Alice', { selector: 'b' });
    expect(api.importSelected).not.toHaveBeenCalled();
    fireEvent.click(screen.getByLabelText('Import Alice vs Bob'));
    await waitFor(() => expect(openGame).toHaveBeenCalledWith(9));
    expect(api.importSelected).toHaveBeenCalledExactlyOnceWith({ ...game, account: 'Alice' });
  });
  it('replaces the visible page when browsing older games', async () => {
    vi.mocked(api.history).mockResolvedValueOnce({ games: [game], nextCursor: 20 }).mockResolvedValueOnce({ games: [], nextCursor: null });
    render(<AccountImport platform="chesscom" />);
    fireEvent.change(screen.getByLabelText('Player username'), { target: { value: 'Alice' } });
    fireEvent.click(screen.getByText('Browse games'));
    await screen.findByText('Page 1');
    fireEvent.click(screen.getByText('Older'));
    await screen.findByText('Page 2');
    expect(screen.queryByText('Alice', { selector: 'b' })).toBeNull();
    expect(api.history).toHaveBeenLastCalledWith(3, expect.objectContaining({ cursor: 20 }));
  });
  it('imports another player without claiming that account belongs to the user', async () => {
    vi.mocked(api.history).mockResolvedValue({ games: [game], nextCursor: null });
    vi.mocked(api.importSelected).mockResolvedValue({ id: 9, duplicate: false });
    render(<AccountImport platform="lichess" />);
    fireEvent.change(screen.getByLabelText('Player username'), { target: { value: 'Alice' } });
    fireEvent.click(screen.getByLabelText('This is my account'));
    fireEvent.click(screen.getByText('Browse games'));
    await screen.findByText('Alice', { selector: 'b' });
    fireEvent.click(screen.getByLabelText('Import Alice vs Bob'));
    await waitFor(() => expect(api.importSelected).toHaveBeenCalledWith({ ...game, account: null }));
  });

});
