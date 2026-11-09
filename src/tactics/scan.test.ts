import { beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../lib/ipc';
import { useStore } from '../store';
import { startCurrentGameScan } from './scan';

vi.mock('../lib/ipc', () => ({
  api: { gameSave: vi.fn(), tacticsScan: vi.fn(), analysisStop: vi.fn(), reviewGet: vi.fn() },
  errorText: String, library: new EventTarget(), nextJobId: () => 7, logError: vi.fn(),
}));

beforeEach(() => {
  vi.clearAllMocks();
  useStore.getState().newGame();
  useStore.getState().setScan(null);
  vi.mocked(api.gameSave).mockResolvedValue(42);
  vi.mocked(api.analysisStop).mockResolvedValue();
  vi.mocked(api.reviewGet).mockResolvedValue(null);
  vi.mocked(api.tacticsScan).mockResolvedValue({ games: 1, found: 0 } as Awaited<ReturnType<typeof api.tacticsScan>>);
});

describe('current game puzzles', () => {
  it('saves an unsaved game and scans only its id', async () => {
    useStore.getState().playLine(['e2e4', 'e7e5']);
    await startCurrentGameScan();
    expect(api.gameSave).toHaveBeenCalledWith(null, expect.stringContaining('1. e4 e5'));
    expect(api.tacticsScan).toHaveBeenCalledWith(7, expect.objectContaining({ gameIds: [42], onlyMine: false, rescan: false, maxGames: 1 }), expect.any(Function));
    expect(useStore.getState().gameId).toBe(42);
    expect(useStore.getState().view).toBe('analyze');
  });
  it('does not scan the library if saving fails', async () => {
    useStore.getState().playLine(['e2e4', 'e7e5']);
    vi.mocked(api.gameSave).mockRejectedValue(new Error('disk full'));
    await startCurrentGameScan();
    expect(api.tacticsScan).not.toHaveBeenCalled();
    expect(useStore.getState().scan).toBeNull();
  });
  it('does not attach a save to a different game opened during the request', async () => {
    useStore.getState().playLine(['e2e4', 'e7e5']);
    let resolve!: (id: number) => void;
    vi.mocked(api.gameSave).mockReturnValue(new Promise((r) => { resolve = r; }));
    const pending = startCurrentGameScan();
    useStore.getState().newGame();
    resolve(42);
    await pending;
    expect(api.tacticsScan).not.toHaveBeenCalled();
    expect(useStore.getState().gameId).toBeNull();
  });
});
