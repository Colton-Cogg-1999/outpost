import { beforeEach, expect, it } from 'vitest';
import { useGameChat } from './gameChat';

beforeEach(() => useGameChat.setState({ conversations: {}, recent: [], pending: null }));

it('keeps game and puzzle conversations separate and evicts the least recently active', () => {
  const append = (session: number | string) => useGameChat.getState().append(session, { role: 'user', content: String(session) });
  append(1);
  append('puzzle:1');
  for (let i = 2; i <= 7; i++) append(i);
  append(1);
  append('puzzle:2');
  const { conversations } = useGameChat.getState();
  expect(Object.keys(conversations)).toHaveLength(8);
  expect(conversations[1].messages).toHaveLength(2);
  expect(conversations['puzzle:1']).toBeUndefined();
  expect(conversations['puzzle:2'].messages).toHaveLength(1);
});

it('retains an older conversation while its response is streaming', () => {
  useGameChat.getState().append('puzzle:1', { role: 'user', content: 'Why?' });
  useGameChat.setState({ pending: { session: 'puzzle:1', requestId: 1, streaming: '' } });
  for (let i = 1; i <= 9; i++) useGameChat.getState().append(i, { role: 'user', content: String(i) });
  expect(useGameChat.getState().conversations['puzzle:1'].messages[0].content).toBe('Why?');
});
