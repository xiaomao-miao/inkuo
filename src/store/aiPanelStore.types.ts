import type { StateCreator } from 'zustand';
import type {
  ActiveToolCall,
  ChatMessage,
  ChatMode,
  ChatSession,
  CurrentDiff,
  OutputItem,
  SearchResult,
} from '../types';

export interface AIPanelUiSlice {
  isOpen: boolean;
  activeTab: 'chat' | 'edit';
  setIsOpen: (open: boolean) => void;
  togglePanel: () => void;
  setActiveTab: (tab: 'chat' | 'edit') => void;
}

export interface AIPanelSessionSlice {
  sessions: ChatSession[];
  activeSessionId: string;
  createSession: () => string;
  deleteSession: (sessionId: string) => void;
  setActiveSession: (sessionId: string) => void;
  setSessionMode: (sessionId: string, mode: ChatMode) => void;
  getSession: (sessionId: string) => ChatSession | undefined;
  updateSession: (sessionId: string, updater: (session: ChatSession) => ChatSession) => void;
}

export interface AIPanelMessageSlice {
  addMessage: (sessionId: string, message: ChatMessage) => void;
  updateMessage: (sessionId: string, messageId: string, content: string) => void;
  appendMessageContent: (sessionId: string, messageId: string, content: string) => void;
  setIsStreaming: (sessionId: string, streaming: boolean) => void;
  clearMessages: (sessionId: string) => void;
  truncateMessagesAfter: (sessionId: string, messageId: string) => void;
  getMessage: (sessionId: string, messageId: string) => ChatMessage | undefined;
  updateMessageOutput: (sessionId: string, messageId: string, outputItems: OutputItem[]) => void;
  addOutputToMessage: (sessionId: string, messageId: string, outputItem: OutputItem) => void;
  patchOutputItem: (
    sessionId: string,
    messageId: string,
    matchKey: { toolCallId: string } | { contentContains: string },
    patch: Partial<OutputItem>,
  ) => void;
  finishMessageStreaming: (sessionId: string, messageId: string, finalContent: string) => void;
  setErrorMessage: (sessionId: string, messageId: string, error: string) => void;
  setMessageSearchResults: (sessionId: string, messageId: string, results: SearchResult[]) => void;
}

export interface AIPanelToolCallSlice {
  addToolCall: (sessionId: string, toolCall: ActiveToolCall) => void;
  updateToolCall: (sessionId: string, toolCallId: string, update: Partial<ActiveToolCall>) => void;
  removeToolCall: (sessionId: string, toolCallId: string) => void;
  clearToolCalls: (sessionId: string) => void;
}

export interface AIPanelDiffSlice {
  setCurrentDiff: (sessionId: string, diff: CurrentDiff | null) => void;
  setMessageDiff: (sessionId: string, messageId: string, diff: CurrentDiff | null) => void;
  setPendingDiff: (sessionId: string, diff: CurrentDiff | null) => void;
  acceptHunk: (sessionId: string, hunkId: string) => void;
  rejectHunk: (sessionId: string, hunkId: string) => void;
  acceptAllHunks: (sessionId: string) => void;
  rejectAllHunks: (sessionId: string) => void;
}

export type AIPanelState =
  & AIPanelUiSlice
  & AIPanelSessionSlice
  & AIPanelMessageSlice
  & AIPanelToolCallSlice
  & AIPanelDiffSlice;

export type AIPanelStateCreator<T> = StateCreator<
  AIPanelState,
  [],
  [],
  T
>;
