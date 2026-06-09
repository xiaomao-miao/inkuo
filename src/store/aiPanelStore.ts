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
import {
  addMessageOutputItem,
  appendSessionMessage,
  appendSessionToolCall,
  clearSessionToolCalls,
  clearSessionConversation,
  createNewSession,
  finishSessionMessageStreaming,
  patchMessageOutputState,
  removeSessionToolCall,
  setMessageDiffState,
  setMessageOutputItems,
  trimSessionMessagesAfter,
  updatePendingDiffHunks,
  updatePendingDiffState,
  updateSessionMessage,
  updateSessionState,
  updateSessions,
  updateToolCalls,
} from './aiPanelReducers';
import { useEditorStore } from './editorStore';
import type { AIPanelState, AIPanelStateCreator } from './aiPanelStore.types';

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

const createUiSlice: AIPanelStateCreator<Pick<AIPanelState, 'isOpen' | 'activeTab' | 'setIsOpen' | 'togglePanel' | 'setActiveTab'>> = (set) => ({
  isOpen: true,
  activeTab: 'chat',
  setIsOpen: (open) => set({ isOpen: open }),
  togglePanel: () => set((state) => ({ isOpen: !state.isOpen })),
  setActiveTab: (tab) => set({ activeTab: tab }),
});

const createSessionSlice: AIPanelStateCreator<Pick<AIPanelState, 'sessions' | 'activeSessionId' | 'createSession' | 'deleteSession' | 'setActiveSession' | 'setSessionMode' | 'getSession' | 'updateSession'>> = (set, get) => {
  const initialSession = createNewSession(1);

  return {
    sessions: [initialSession],
    activeSessionId: initialSession.id,
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
        sessions: updateSessionState(state.sessions, sessionId, { mode }),
      })),
    getSession: (sessionId) => get().sessions.find((session) => session.id === sessionId),
    updateSession: (sessionId, updater) =>
      set((state) => ({
        sessions: updateSessions(state.sessions, sessionId, updater),
      })),
  };
};

const createMessageSlice: AIPanelStateCreator<Pick<AIPanelState, 'addMessage' | 'updateMessage' | 'appendMessageContent' | 'setIsStreaming' | 'clearMessages' | 'truncateMessagesAfter' | 'getMessage' | 'updateMessageOutput' | 'addOutputToMessage' | 'patchOutputItem' | 'finishMessageStreaming' | 'setErrorMessage' | 'setMessageSearchResults'>> = (set, get) => ({
  addMessage: (sessionId, message) =>
    set((state) => ({
      sessions: appendSessionMessage(state.sessions, sessionId, message),
    })),
  updateMessage: (sessionId, messageId, content) =>
    set((state) => ({
      sessions: updateSessionMessage(state.sessions, sessionId, messageId, (message) => ({
        ...message,
        content,
      })),
    })),
  appendMessageContent: (sessionId, messageId, content) =>
    set((state) => ({
      sessions: updateSessionMessage(state.sessions, sessionId, messageId, (message) => ({
        ...message,
        content: (message.content || '') + content,
      })),
    })),
  setIsStreaming: (sessionId, streaming) =>
    set((state) => ({
      sessions: updateSessionState(state.sessions, sessionId, { isStreaming: streaming }),
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
  getMessage: (sessionId, messageId) =>
    get().sessions
      .find((session) => session.id === sessionId)
      ?.messages.find((message) => message.id === messageId),
  updateMessageOutput: (sessionId, messageId, outputItems) =>
    set((state) => ({
      sessions: updateSessions(state.sessions, sessionId, (session) =>
        setMessageOutputItems(session, messageId, outputItems)
      ),
    })),
  addOutputToMessage: (sessionId, messageId, outputItem) =>
    set((state) => ({
      sessions: updateSessions(state.sessions, sessionId, (session) =>
        addMessageOutputItem(session, messageId, outputItem)
      ),
    })),
  patchOutputItem: (sessionId, messageId, matchKey, patch) =>
    set((state) => ({
      sessions: updateSessions(state.sessions, sessionId, (session) =>
        patchMessageOutputState(session, messageId, matchKey, patch)
      ),
    })),
  finishMessageStreaming: (sessionId, messageId, finalContent) =>
    set((state) => ({
      sessions: finishSessionMessageStreaming(state.sessions, sessionId, messageId, finalContent),
    })),
  setErrorMessage: (sessionId, messageId, error) =>
    set((state) => ({
      sessions: finishSessionMessageStreaming(state.sessions, sessionId, messageId, error),
    })),
  setMessageSearchResults: (sessionId, messageId, results) =>
    set((state) => ({
      sessions: updateSessionMessage(state.sessions, sessionId, messageId, (message) => ({
        ...message,
        searchResults: results,
      })),
    })),
});

const createToolCallSlice: AIPanelStateCreator<Pick<AIPanelState, 'addToolCall' | 'updateToolCall' | 'removeToolCall' | 'clearToolCalls'>> = (set) => ({
  addToolCall: (sessionId, toolCall) =>
    set((state) => ({
      sessions: appendSessionToolCall(state.sessions, sessionId, toolCall),
    })),
  updateToolCall: (sessionId, toolCallId, update) =>
    set((state) => ({
      sessions: updateSessions(state.sessions, sessionId, (session) =>
        updateToolCalls(session, toolCallId, (toolCall) => ({ ...toolCall, ...update }))
      ),
    })),
  removeToolCall: (sessionId, toolCallId) =>
    set((state) => ({
      sessions: removeSessionToolCall(state.sessions, sessionId, toolCallId),
    })),
  clearToolCalls: (sessionId) =>
    set((state) => ({
      sessions: clearSessionToolCalls(state.sessions, sessionId),
    })),
});

const createDiffSlice: AIPanelStateCreator<Pick<AIPanelState, 'setCurrentDiff' | 'setMessageDiff' | 'setPendingDiff' | 'setDiffFromToolResult' | 'acceptHunk' | 'rejectHunk' | 'acceptAllHunks' | 'rejectAllHunks'>> = (set) => ({
  setCurrentDiff: (sessionId, diff) =>
    set((state) => ({
      sessions: updateSessionState(state.sessions, sessionId, { currentDiff: diff }),
    })),
  setMessageDiff: (sessionId, messageId, diff) =>
    set((state) => ({
      sessions: updateSessions(state.sessions, sessionId, (session) =>
        setMessageDiffState(session, messageId, diff)
      ),
    })),
  setPendingDiff: (sessionId, diff) =>
    set((state) => ({
      sessions: updatePendingDiffState(state.sessions, sessionId, diff),
    })),
  setDiffFromToolResult: (sessionId, diff) =>
    set((state) => ({
      sessions: updatePendingDiffState(state.sessions, sessionId, diff),
    })),
  acceptHunk: (sessionId, hunkId) =>
    set((state) => {
      const session = state.sessions.find((s) => s.id === sessionId);
      const diff = session?.pendingDiff;
      if (!diff) return state;

      const hunk = diff.hunks.find((h) => h.id === hunkId);
      if (!hunk) return state;

      if (diff.filePath) {
        useEditorStore.getState().applyHunk(diff.filePath, hunkId);
      }

      const remainingHunks = diff.hunks.filter((h) => h.id !== hunkId);
      return {
        sessions: updateSessions(state.sessions, sessionId, (session) => ({
          ...session,
          pendingDiff: remainingHunks.length > 0
            ? { ...session.pendingDiff!, hunks: remainingHunks }
            : null,
        })),
      };
    }),
  rejectHunk: (sessionId, hunkId) =>
    set((state) => ({
      sessions: updateSessions(state.sessions, sessionId, (session) =>
        updatePendingDiffHunks(session, hunkId)
      ),
    })),
  acceptAllHunks: (sessionId) =>
    set((state) => {
      const session = state.sessions.find((s) => s.id === sessionId);
      const diff = session?.pendingDiff;
      if (diff?.filePath) {
        useEditorStore.getState().applyAllHunks(diff.filePath);
      }
      return {
        sessions: updateSessions(state.sessions, sessionId, (session) => ({
          ...session,
          pendingDiff: null,
        })),
      };
    }),
  rejectAllHunks: (sessionId) =>
    set((state) => ({
      sessions: updateSessions(state.sessions, sessionId, (session) => ({
        ...session,
        pendingDiff: null,
      })),
    })),
});

export const useAIPanelStore = create<AIPanelState>()(
  persist(
    (...args) => ({
      ...createUiSlice(...args),
      ...createSessionSlice(...args),
      ...createMessageSlice(...args),
      ...createToolCallSlice(...args),
      ...createDiffSlice(...args),
    }),
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
