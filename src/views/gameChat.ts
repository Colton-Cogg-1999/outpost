import { create } from 'zustand';
import type { MoveCheck } from '../lib/types';

export interface ChatMessage {
  role: 'user' | 'assistant';
  content: string;
  label?: string;
  moves?: MoveCheck;
  provider?: string;
  error?: boolean;
  fen?: string;
  position?: string;
}
interface Conversation { messages: ChatMessage[]; chatId: string | null }
interface ChatState {
  conversations: Record<number, Conversation>;
  pending: { session: number; requestId: number; streaming: string } | null;
  append: (session: number, message: ChatMessage, chatId?: string | null) => void;
}
// Conversation follows the game while navigating positions and application views.
// Keep only the eight most recently active sessions in memory.
export const useGameChat = create<ChatState>((set) => ({
  conversations: {}, pending: null,
  append: (session, message, chatId) => set((s) => {
    const previous = s.conversations[session];
    const conversations = { ...s.conversations, [session]: {
      messages: [...(previous?.messages ?? []), message],
      chatId: chatId === undefined ? previous?.chatId ?? null : chatId,
    } };
    const keys = Object.keys(conversations).map(Number).sort((a, b) => b - a);
    for (const old of keys.slice(8)) if (old !== s.pending?.session) delete conversations[old];
    return { conversations };
  }),
}));
