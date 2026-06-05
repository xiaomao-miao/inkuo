import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import type {
  ActiveToolCall,
  BuildProgress,
  ChatMessage,
  ChatMode,
  ChatSession,
  CurrentDiff,
  MessageRole,
  MessageToolCall,
  MessageToolResult,
  OutputItem,
  SearchResult,
} from '../types';

interface AIPanelState {
  isOpen: boolean;
  activeTab: 'chat' | 'edit';

  sessions: ChatSession[];
  activeSessionId: string;

  setIsOpen: (open: boolean) => void;
  togglePanel: () => void;
  setActiveTab: (tab: 'chat' | 'edit') => void;

  createSession: () => string;
  deleteSession: (sessionId: string) => void;
  setActiveSession: (sessionId: string) => void;
  setSessionMode: (sessionId: string, mode: ChatMode) => void;

  addMessage: (sessionId: string, message: ChatMessage) => void;
  updateMessage: (sessionId: string, messageId: string, content: string) => void;
  appendMessageContent: (sessionId: string, messageId: string, content: string) => void;
  setIsStreaming: (sessionId: string, streaming: boolean) => void;
  clearMessages: (sessionId: string) => void;
  truncateMessagesAfter: (sessionId: string, messageId: string) => void;

  addToolCall: (sessionId: string, toolCall: ActiveToolCall) => void;
  updateToolCall: (sessionId: string, toolCallId: string, update: Partial<ActiveToolCall>) => void;
  removeToolCall: (sessionId: string, toolCallId: string) => void;
  clearToolCalls: (sessionId: string) => void;

  setCurrentDiff: (sessionId: string, diff: CurrentDiff | null) => void;
  setMessageDiff: (sessionId: string, messageId: string, diff: CurrentDiff | null) => void;
  setPendingDiff: (sessionId: string, diff: CurrentDiff | null) => void;
  acceptHunk: (sessionId: string, hunkId: string) => void;
  rejectHunk: (sessionId: string, hunkId: string) => void;
  acceptAllHunks: (sessionId: string) => void;
  rejectAllHunks: (sessionId: string) => void;

  getSession: (sessionId: string) => ChatSession | undefined;
  getMessage: (sessionId: string, messageId: string) => ChatMessage | undefined;
  updateSession: (sessionId: string, updater: (session: ChatSession) => ChatSession) => void;
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

function createSessionTitle(index: number) {
  return `对话 ${index}`;
}

function createNewSession(index: number): ChatSession {
  const now = Date.now();
  return {
    id: crypto.randomUUID(),
    title: createSessionTitle(index),
    createdAt: now,
    mode: 'ask',
    messages: [],
    isStreaming: false,
    currentDiff: null,
    activeToolCalls: [],
    pendingDiff: null,
  };
}

export const useAIPanelStore = create<AIPanelState>()(
  persist(
    (set, get) => {
      const initialSession = createNewSession(1);

      return {
        isOpen: true,
        activeTab: 'chat',

        sessions: [initialSession],
        activeSessionId: initialSession.id,

        setIsOpen: (open) => set({ isOpen: open }),
        togglePanel: () => set((state) => ({ isOpen: !state.isOpen })),
        setActiveTab: (tab) => set({ activeTab: tab }),

        createSession: () => {
          const index = get().sessions.length + 1;
          const session = createNewSession(index);
          set((state) => ({
            sessions: [session, ...state.sessions],
            activeSessionId: session.id,
          }));
          return session.id;
        },

        deleteSession: (sessionId) => {
          set((state) => {
            const remaining = state.sessions.filter((s) => s.id !== sessionId);
            const safeRemaining = remaining.length > 0 ? remaining : [createNewSession(1)];
            const nextActiveId =
              state.activeSessionId === sessionId ? safeRemaining[0].id : state.activeSessionId;

            return {
              sessions: safeRemaining,
              activeSessionId: nextActiveId,
            };
          });
        },

        setActiveSession: (sessionId) => set({ activeSessionId: sessionId }),

        setSessionMode: (sessionId, mode) =>
          set((state) => ({
            sessions: state.sessions.map((session) =>
              session.id === sessionId ? { ...session, mode } : session
            ),
          })),

        addMessage: (sessionId, message) =>
          set((state) => ({
            sessions: state.sessions.map((session) =>
              session.id === sessionId
                ? { ...session, messages: [...session.messages, message] }
                : session
            ),
          })),

        updateMessage: (sessionId, messageId, content) =>
          set((state) => ({
            sessions: state.sessions.map((session) =>
              session.id === sessionId
                ? {
                    ...session,
                    messages: session.messages.map((message) =>
                      message.id === messageId ? { ...message, content } : message
                    ),
                  }
                : session
            ),
          })),

        appendMessageContent: (sessionId, messageId, content) =>
          set((state) => ({
            sessions: state.sessions.map((session) =>
              session.id === sessionId
                ? {
                    ...session,
                    messages: session.messages.map((message) =>
                      message.id === messageId
                        ? { ...message, content: (message.content || '') + content }
                        : message
                    ),
                  }
                : session
            ),
          })),

        setIsStreaming: (sessionId, streaming) =>
          set((state) => ({
            sessions: state.sessions.map((session) =>
              session.id === sessionId ? { ...session, isStreaming: streaming } : session
            ),
          })),

        clearMessages: (sessionId) =>
          set((state) => ({
            sessions: state.sessions.map((session) =>
              session.id === sessionId
                ? {
                    ...session,
                    messages: [],
                    currentDiff: null,
                    pendingDiff: null,
                    activeToolCalls: [],
                  }
                : session
            ),
          })),

        truncateMessagesAfter: (sessionId, messageId) =>
          set((state) => ({
            sessions: state.sessions.map((session) => {
              if (session.id !== sessionId) return session;
              const index = session.messages.findIndex((message) => message.id === messageId);
              if (index === -1) return session;
              return {
                ...session,
                messages: session.messages.slice(0, index + 1),
              };
            }),
          })),

        addToolCall: (sessionId, toolCall) =>
          set((state) => ({
            sessions: state.sessions.map((session) =>
              session.id === sessionId
                ? { ...session, activeToolCalls: [...session.activeToolCalls, toolCall] }
                : session
            ),
          })),

        updateToolCall: (sessionId, toolCallId, update) =>
          set((state) => ({
            sessions: state.sessions.map((session) =>
              session.id === sessionId
                ? {
                    ...session,
                    activeToolCalls: session.activeToolCalls.map((toolCall) =>
                      toolCall.id === toolCallId ? { ...toolCall, ...update } : toolCall
                    ),
                  }
                : session
            ),
          })),

        removeToolCall: (sessionId, toolCallId) =>
          set((state) => ({
            sessions: state.sessions.map((session) =>
              session.id === sessionId
                ? {
                    ...session,
                    activeToolCalls: session.activeToolCalls.filter((toolCall) => toolCall.id !== toolCallId),
                  }
                : session
            ),
          })),

        clearToolCalls: (sessionId) =>
          set((state) => ({
            sessions: state.sessions.map((session) =>
              session.id === sessionId ? { ...session, activeToolCalls: [] } : session
            ),
          })),

        setCurrentDiff: (sessionId, diff) =>
          set((state) => ({
            sessions: state.sessions.map((session) =>
              session.id === sessionId ? { ...session, currentDiff: diff } : session
            ),
          })),

        setMessageDiff: (sessionId, messageId, diff) =>
          set((state) => ({
            sessions: state.sessions.map((session) =>
              session.id === sessionId
                ? {
                    ...session,
                    messages: session.messages.map((message) =>
                      message.id === messageId ? { ...message, diff: diff ?? undefined } : message
                    ),
                  }
                : session
            ),
          })),

        setPendingDiff: (sessionId, diff) =>
          set((state) => ({
            sessions: state.sessions.map((session) =>
              session.id === sessionId ? { ...session, pendingDiff: diff } : session
            ),
          })),

        acceptHunk: (sessionId, hunkId) =>
          set((state) => ({
            sessions: state.sessions.map((session) => {
              if (session.id !== sessionId || !session.pendingDiff) return session;
              const remainingHunks = session.pendingDiff.hunks.filter((hunk) => hunk.id !== hunkId);
              return {
                ...session,
                pendingDiff:
                  remainingHunks.length > 0
                    ? { ...session.pendingDiff, hunks: remainingHunks }
                    : null,
              };
            }),
          })),

        rejectHunk: (sessionId, hunkId) =>
          set((state) => ({
            sessions: state.sessions.map((session) => {
              if (session.id !== sessionId || !session.pendingDiff) return session;
              const remainingHunks = session.pendingDiff.hunks.filter((hunk) => hunk.id !== hunkId);
              return {
                ...session,
                pendingDiff:
                  remainingHunks.length > 0
                    ? { ...session.pendingDiff, hunks: remainingHunks }
                    : null,
              };
            }),
          })),

        acceptAllHunks: (sessionId) =>
          set((state) => ({
            sessions: state.sessions.map((session) =>
              session.id === sessionId ? { ...session, pendingDiff: null } : session
            ),
          })),

        rejectAllHunks: (sessionId) =>
          set((state) => ({
            sessions: state.sessions.map((session) =>
              session.id === sessionId ? { ...session, pendingDiff: null } : session
            ),
          })),

        getSession: (sessionId) => get().sessions.find((session) => session.id === sessionId),

        getMessage: (sessionId, messageId) =>
          get().sessions
            .find((session) => session.id === sessionId)
            ?.messages.find((message) => message.id === messageId),

        updateSession: (sessionId, updater) =>
          set((state) => ({
            sessions: state.sessions.map((session) =>
              session.id === sessionId ? updater(session) : session
            ),
          })),

        updateMessageOutput: (sessionId, messageId, outputItems) =>
          set((state) => ({
            sessions: state.sessions.map((session) =>
              session.id === sessionId
                ? {
                    ...session,
                    messages: session.messages.map((message) =>
                      message.id === messageId ? { ...message, outputItems } : message
                    ),
                  }
                : session
            ),
          })),

        addOutputToMessage: (sessionId, messageId, outputItem) =>
          set((state) => ({
            sessions: state.sessions.map((session) =>
              session.id === sessionId
                ? {
                    ...session,
                    messages: session.messages.map((message) =>
                      message.id === messageId
                        ? { ...message, outputItems: [...message.outputItems, outputItem] }
                        : message
                    ),
                  }
                : session
            ),
          })),

        patchOutputItem: (sessionId, messageId, matchKey, patch) =>
          set((state) => ({
            sessions: state.sessions.map((session) => {
              if (session.id !== sessionId) return session;
              return {
                ...session,
                messages: session.messages.map((message) => {
                  if (message.id !== messageId) return message;
                  const outputItems = message.outputItems.map((item) => {
                    const matchesByToolCallId =
                      'toolCallId' in matchKey &&
                      'toolCallId' in item &&
                      item.toolCallId === matchKey.toolCallId;
                    const matchesByContent =
                      'contentContains' in matchKey &&
                      'content' in item &&
                      typeof item.content === 'string' &&
                      item.content.includes(matchKey.contentContains);
                    return matchesByToolCallId || matchesByContent
                      ? ({ ...item, ...patch } as OutputItem)
                      : item;
                  });
                  return { ...message, outputItems };
                }),
              };
            }),
          })),

        finishMessageStreaming: (sessionId, messageId, finalContent) =>
          set((state) => ({
            sessions: state.sessions.map((session) =>
              session.id === sessionId
                ? {
                    ...session,
                    isStreaming: false,
                    messages: session.messages.map((message) =>
                      message.id === messageId
                        ? { ...message, content: finalContent }
                        : message
                    ),
                  }
                : session
            ),
          })),

        setErrorMessage: (sessionId, messageId, error) =>
          set((state) => ({
            sessions: state.sessions.map((session) =>
              session.id === sessionId
                ? {
                    ...session,
                    isStreaming: false,
                    messages: session.messages.map((message) =>
                      message.id === messageId ? { ...message, content: error } : message
                    ),
                  }
                : session
            ),
          })),

        setMessageSearchResults: (sessionId, messageId, results) =>
          set((state) => ({
            sessions: state.sessions.map((session) =>
              session.id === sessionId
                ? {
                    ...session,
                    messages: session.messages.map((message) =>
                      message.id === messageId ? { ...message, searchResults: results } : message
                    ),
                  }
                : session
            ),
          })),
      };
    },
    {
      name: 'inkuo-ai-panel',
      version: 1,
      partialize: (state) => ({
        isOpen: state.isOpen,
        activeTab: state.activeTab,
        sessions: state.sessions,
        activeSessionId: state.activeSessionId,
      }),
      merge: (persistedState, currentState) => {
        const typedState = persistedState as Partial<{
          isOpen: boolean;
          activeTab: 'chat' | 'edit';
          sessions: ChatSession[];
          activeSessionId: string;
        }>;

        const sessions = typedState.sessions?.length ? typedState.sessions : currentState.sessions;
        const activeSessionId =
          typedState.activeSessionId && sessions.some((session) => session.id === typedState.activeSessionId)
            ? typedState.activeSessionId
            : sessions[0]?.id ?? currentState.activeSessionId;

        return {
          ...currentState,
          ...typedState,
          sessions,
          activeSessionId,
        };
      },
    }
  )
);

export type {
  ActiveToolCall,
  BuildProgress,
  ChatMessage,
  ChatMode,
  ChatSession,
  CurrentDiff,
  MessageRole,
  MessageToolCall,
  MessageToolResult,
  OutputItem,
  SearchResult,
};
