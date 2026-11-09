import { act, fireEvent, render, screen, waitFor, cleanup } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { AiPanel } from './AiPanel';
import { useStore } from '../store';
import { useGameChat } from './gameChat';
import { api } from '../lib/ipc';
vi.mock('../lib/ipc', () => ({
  api: { aiProviders: vi.fn(), aiAsk: vi.fn(), settingsSet: vi.fn() },
  logError: vi.fn(), nextJobId: () => 17, errorText: String,
}));
beforeEach(() => {
  HTMLElement.prototype.scrollTo = vi.fn();
  useStore.getState().newGame();
  useGameChat.setState({ conversations: {}, pending: null });
  vi.mocked(api.aiProviders).mockResolvedValue({ settings: { active: 'test', providers: {} }, providers: [] } as never);
  vi.mocked(api.aiAsk).mockResolvedValue({ text: 'Consider e4.', chatId: 'chat-1', moves: { keyMoves: [{ san: 'e4', uci: 'e2e4' }], mentioned: [], rejected: [] }, provider: 'test', model: null, cancelled: false });
  vi.mocked(api.aiAsk).mockClear();
});
afterEach(cleanup);
it('keeps a conversation across moves and view remounts, sending the new position and prior turns', async () => {
  const view = render(<AiPanel />);
  await waitFor(() => expect((screen.getByLabelText('Ask the AI assistant') as HTMLTextAreaElement).disabled).toBe(false));
  fireEvent.change(screen.getByLabelText('Ask the AI assistant'), { target: { value: 'What is the plan?' } });
  fireEvent.click(screen.getByLabelText('Send'));
  await screen.findByText('Consider', { exact: false });
  act(() => { useStore.getState().play('e2e4'); });
  expect(screen.getByText('What is the plan?', { exact: false })).toBeTruthy();
  // An earlier answer's legal moves must not be applied to a different position.
  expect(screen.queryByTitle('Play e4 on the board')).toBeNull();
  view.unmount();
  render(<AiPanel />);
  await waitFor(() => expect((screen.getByLabelText('Ask the AI assistant') as HTMLTextAreaElement).disabled).toBe(false));
  fireEvent.change(screen.getByLabelText('Ask the AI assistant'), { target: { value: 'And now?' } });
  fireEvent.click(screen.getByLabelText('Send'));
  await waitFor(() => expect(api.aiAsk).toHaveBeenCalledTimes(2));
  const request = vi.mocked(api.aiAsk).mock.calls[1][1];
  expect(request.history).toHaveLength(2);
  expect(request.history[0].content).toContain('What is the plan?');
  expect(request.chatId).toBe('chat-1');
  expect(request.position).toMatchObject({ fen: useStore.getState().tree.nodes[useStore.getState().current].fen });
  await waitFor(() => expect(useGameChat.getState().pending).toBeNull());
  act(() => useStore.getState().newGame());
  expect(screen.queryByText('What is the plan?', { exact: false })).toBeNull();
});
it('stays silent until a typed question is sent and keeps configuration in Settings', async () => {
  render(<AiPanel />);
  await waitFor(() => expect((screen.getByLabelText('Ask the AI assistant') as HTMLTextAreaElement).disabled).toBe(false));
  act(() => useStore.getState().play('e2e4'));
  expect(api.aiAsk).not.toHaveBeenCalled();
  expect(screen.queryByRole('toolbar')).toBeNull();
  expect(screen.queryByLabelText('Agent provider')).toBeNull();
  fireEvent.click(screen.getByLabelText('Configure agent in Settings'));
  expect(useStore.getState().view).toBe('settings');
  expect(api.aiAsk).not.toHaveBeenCalled();
});
