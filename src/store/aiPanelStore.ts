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

function updateSessions(
  sessions: ChatSession[],
  sessionId: string,
  updater: (session: ChatSession) => ChatSession,
): ChatSession[] {
  return sessions.map((session) =>
    session.id === sessionId ? updater(session) : session
  );
}

function updateMessages(
  session: ChatSession,
  messageId: string,
  updater: (message: ChatMessage) => ChatMessage,
): ChatSession {
  return {
    ...session,
    messages: session.messages.map((message) =>
      message.id === messageId ? updater(message) : message
    ),
  };
}

function updateToolCalls(
  session: ChatSession,
  toolCallId: string,
  updater: (toolCall: ActiveToolCall) => ActiveToolCall,
): ChatSession {
  return {
    ...session,
    activeToolCalls: session.activeToolCalls.map((toolCall) =>
      toolCall.id === toolCallId ? updater(toolCall) : toolCall
    ),
  };
}

function clearSessionConversation(session: ChatSession): ChatSession {
  return {
    ...session,
    messages: [],
    currentDiff: null,
    pendingDiff: null,
    activeToolCalls: [],
  };
}

function trimSessionMessagesAfter(session: ChatSession, messageId: string): ChatSession {
  const index = session.messages.findIndex((message) => message.id === messageId);
  if (index === -1) return session;
  return {
    ...session,
    messages: session.messages.slice(0, index + 1),
  };
}

function updatePendingDiffHunks(
  session: ChatSession,
  hunkId: string,
): ChatSession {
  if (!session.pendingDiff) return session;
  const remainingHunks = session.pendingDiff.hunks.filter((hunk) => hunk.id !== hunkId);
  return {
    ...session,
    pendingDiff:
      remainingHunks.length > 0
        ? { ...session.pendingDiff, hunks: remainingHunks }
        : null,
  };
}

function patchMessageOutputItems(
  message: ChatMessage,
  matchKey: { toolCallId: string } | { contentContains: string },
  patch: Partial<OutputItem>,
): ChatMessage {
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
}

function mergePersistedState(
  persistedState: unknown,
  currentState: AIPanelState,
): AIPanelState {
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
            const remaining = state.sessions.filter((session) => session.id !== sessionId);
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
            sessions: updateSessions(state.sessions, sessionId, (session) => ({ ...session, mode })),
          })),

        addMessage: (sessionId, message) =>
          set((state) => ({
            sessions: updateSessions(state.sessions, sessionId, (session) => ({
              ...session,
              messages: [...session.messages, message],
            })),
          })),

        updateMessage: (sessionId, messageId, content) =>
          set((state) => ({
            sessions: updateSessions(state.sessions, sessionId, (session) =>
              updateMessages(session, messageId, (message) => ({ ...message, content }))
            ),
          })),

        appendMessageContent: (sessionId, messageId, content) =>
          set((state) => ({
            sessions: updateSessions(state.sessions, sessionId, (session) =>
              updateMessages(session, messageId, (message) => ({
                ...message,
                content: (message.content || '') + content,
              }))
            ),
          })),

        setIsStreaming: (sessionId, streaming) =>
          set((state) => ({
            sessions: updateSessions(state.sessions, sessionId, (session) => ({
              ...session,
              isStreaming: streaming,
            })),
          })),

        clearMessages: (sessionId) =>
          set((state) => ({
            sessions: updateSessions(state.sessions, sessionId, clearSessionConversation),
          })),

        truncateMessagesAfter: (sessionId, messageId) =>
          set((state) => ({
            sessions: updateSessions(state.sessions, sessionId, (session) =>
              trimSessionMessagesAfter(session, messageId)
            ),
          })),

        addToolCall: (sessionId, toolCall) =>
          set((state) => ({
            sessions: updateSessions(state.sessions, sessionId, (session) => ({
              ...session,
              activeToolCalls: [...session.activeToolCalls, toolCall],
            })),
          })),

        updateToolCall: (sessionId, toolCallId, update) =>
          set((state) => ({
            sessions: updateSessions(state.sessions, sessionId, (session) =>
              updateToolCalls(session, toolCallId, (toolCall) => ({ ...toolCall, ...update }))
            ),
          })),

        removeToolCall: (sessionId, toolCallId) =>
          set((state) => ({
            sessions: updateSessions(state.sessions, sessionId, (session) => ({
              ...session,
              activeToolCalls: session.activeToolCalls.filter((toolCall) => toolCall.id !== toolCallId),
            })),
          })),

        clearToolCalls: (sessionId) =>
          set((state) => ({
            sessions: updateSessions(state.sessions, sessionId, (session) => ({
              ...session,
              activeToolCalls: [],
            })),
          })),

        setCurrentDiff: (sessionId, diff) =>
          set((state) => ({
            sessions: updateSessions(state.sessions, sessionId, (session) => ({
              ...session,
              currentDiff: diff,
            })),
          })),

        setMessageDiff: (sessionId, messageId, diff) =>
          set((state) => ({
            sessions: updateSessions(state.sessions, sessionId, (session) =>
              updateMessages(session, messageId, (message) => ({
                ...message,
                diff: diff ?? undefined,
              }))
            ),
          })),

        setPendingDiff: (sessionId, diff) =>
          set((state) => ({
            sessions: updateSessions(state.sessions, sessionId, (session) => ({
              ...session,
              pendingDiff: diff,
            })),
          })),

        acceptHunk: (sessionId, hunkId) =>
          set((state) => ({
            sessions: updateSessions(state.sessions, sessionId, (session) =>
              updatePendingDiffHunks(session, hunkId)
            ),
          })),

        rejectHunk: (sessionId, hunkId) =>
          set((state) => ({
            sessions: updateSessions(state.sessions, sessionId, (session) =>
              updatePendingDiffHunks(session, hunkId)
            ),
          })),

        acceptAllHunks: (sessionId) =>
          set((state) => ({
            sessions: updateSessions(state.sessions, sessionId, (session) => ({
              ...session,
              pendingDiff: null,
            })),
          })),

        rejectAllHunks: (sessionId) =>
          set((state) => ({
            sessions: updateSessions(state.sessions, sessionId, (session) => ({
              ...session,
              pendingDiff: null,
            })),
          })),

        getSession: (sessionId) => get().sessions.find((session) => session.id === sessionId),

        getMessage: (sessionId, messageId) =>
          get().sessions
            .find((session) => session.id === sessionId)
            ?.messages.find((message) => message.id === messageId),

        updateSession: (sessionId, updater) =>
          set((state) => ({
            sessions: updateSessions(state.sessions, sessionId, updater),
          })),

        updateMessageOutput: (sessionId, messageId, outputItems) =>
          set((state) => ({
            sessions: updateSessions(state.sessions, sessionId, (session) =>
              updateMessages(session, messageId, (message) => ({ ...message, outputItems }))
            ),
          })),

        addOutputToMessage: (sessionId, messageId, outputItem) =>
          set((state) => ({
            sessions: updateSessions(state.sessions, sessionId, (session) =>
              updateMessages(session, messageId, (message) => ({
                ...message,
                outputItems: [...message.outputItems, outputItem],
              }))
            ),
          })),

        patchOutputItem: (sessionId, messageId, matchKey, patch) =>
          set((state) => ({
            sessions: updateSessions(state.sessions, sessionId, (session) =>
              updateMessages(session, messageId, (message) =>
                patchMessageOutputItems(message, matchKey, patch)
              )
            ),
          })),

        finishMessageStreaming: (sessionId, messageId, finalContent) =>
          set((state) => ({
            sessions: updateSessions(state.sessions, sessionId, (session) => ({
              ...updateMessages(session, messageId, (message) => ({
                ...message,
                content: finalContent,
              })),
              isStreaming: false,
            })),
          })),

        setErrorMessage: (sessionId, messageId, error) =>
          set((state) => ({
            sessions: updateSessions(state.sessions, sessionId, (session) => ({
              ...updateMessages(session, messageId, (message) => ({
                ...message,
                content: error,
              })),
              isStreaming: false,
            })),
          })),

        setMessageSearchResults: (sessionId, messageId, results) =>
          set((state) => ({
            sessions: updateSessions(state.sessions, sessionId, (session) =>
              updateMessages(session, messageId, (message) => ({
                ...message,
                searchResults: results,
              }))
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
      merge: mergePersistedState,
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
