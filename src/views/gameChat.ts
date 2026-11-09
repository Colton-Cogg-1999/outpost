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
export type ChatSession = number | string;
interface ChatState {
  conversations: Record<string, Conversation>;
  recent: string[];
  pending: { session: ChatSession; requestId: number; streaming: string } | null;
  append: (session: ChatSession, message: ChatMessage, chatId?: string | null) => void;
}
// Conversation follows the game while navigating positions and application views.
// Keep only the eight most recently active sessions in memory.
export const useGameChat = create<ChatState>((set) => ({
  conversations: {}, recent: [], pending: null,
  append: (session, message, chatId) => set((s) => {
    const previous = s.conversations[session];
    const conversations = { ...s.conversations };
    delete conversations[session];
    conversations[session] = {
      messages: [...(previous?.messages ?? []), message],
      chatId: chatId === undefined ? previous?.chatId ?? null : chatId,
    };
    const keys = [...s.recent.filter((key) => key !== String(session) && conversations[key]), String(session)];
    for (const old of keys.slice(0, -8)) if (old !== String(s.pending?.session)) delete conversations[old];
    return { conversations, recent: keys.filter((key) => conversations[key]) };
  }),
}));
