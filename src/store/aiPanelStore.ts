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
  collapseMessageHead,
  createNewSession,
  finishSessionMessageStreaming,
  patchMessageOutputState,
  removeSessionToolCall,
  setMessageDiffState,
  setMessageOutputItems,
  spliceMessagePrefix,
  trimSessionMessagesAfter,
  updateMessages,
  updatePendingDiffHunks,
  updatePendingDiffState,
  updateSessionMessage,
  updateSessionState,
  updateSessions,
  updateToolCalls,
} from './aiPanelReducers';
import { editorDiffActions } from './editorStore';
import type { AIPanelState, AIPanelStateCreator, DiffApplicationActions } from './aiPanelStore.types';

function pickPersistedUiBits(
  persistedState: unknown,
  currentState: AIPanelState,
): AIPanelState {
  // Renamed from `mergePersistedState`: this is a *pick*, not a merge.
  // We deliberately keep only the two UI-mode bits that survive a reload
  // (panel open state and active tab). Session content is intentionally
  // dropped — the backend's workspace snapshots are the canonical source
  // of truth and are reloaded lazily — and `activeSessionId` is dropped
  // so a stale id from an older snapshot can't pin a session that no
  // longer exists.
  const persisted = (persistedState ?? {}) as Partial<Pick<
    AIPanelState,
    'isOpen' | 'activeTab' | 'activeSessionId'
  >>;

  return {
    ...currentState,
    isOpen: persisted.isOpen ?? currentState.isOpen,
    activeTab: persisted.activeTab ?? currentState.activeTab,
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

const createMessageSlice: AIPanelStateCreator<Pick<AIPanelState, 'addMessage' | 'updateMessage' | 'appendMessageContent' | 'setIsStreaming' | 'clearMessages' | 'truncateMessagesAfter' | 'getMessage' | 'updateMessageOutput' | 'addOutputToMessage' | 'patchOutputItem' | 'finishMessageStreaming' | 'setErrorMessage' | 'setMessageSearchResults' | 'expandMessagePrefix' | 'collapseMessagePrefix' | 'toggleReasoningExpansion' | 'autoExpandTruncatedPrefixes'>> = (set, get) => ({
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
  expandMessagePrefix: (sessionId, messageId, keepTail) =>
    set((state) => ({
      sessions: updateSessions(state.sessions, sessionId, (session) =>
        updateMessages(session, messageId, (message) => {
          const lastItem =
            message.outputItems[message.outputItems.length - 1];
          const itemPrefix =
            lastItem && lastItem.type === 'text'
              ? lastItem.truncatedPrefix
              : undefined;
          const prefix = message.truncatedPrefix || itemPrefix || '';
          if (!prefix) return message;
          return spliceMessagePrefix(message, prefix, keepTail);
        })
      ),
    })),
  collapseMessagePrefix: (sessionId, messageId, keepTail) =>
    set((state) => ({
      sessions: updateSessions(state.sessions, sessionId, (session) =>
        updateMessages(session, messageId, (message) =>
          collapseMessageHead(message, keepTail)
        )
      ),
    })),
  toggleReasoningExpansion: (sessionId, messageId, reasoningId) =>
    set((state) => ({
      sessions: updateSessions(state.sessions, sessionId, (session) =>
        updateMessages(session, messageId, (message) => {
          const current = message.expandedReasoningIds ?? [];
          const next = current.includes(reasoningId)
            ? current.filter((id) => id !== reasoningId)
            : [...current, reasoningId];
          return {
            ...message,
            expandedReasoningIds: next.length > 0 ? next : undefined,
          };
        })
      ),
    })),
  autoExpandTruncatedPrefixes: (sessionId) =>
    set((state) => ({
      sessions: updateSessions(state.sessions, sessionId, (session) => {
        let changed = false;
        const messages = session.messages.map((message) => {
          const items = message.outputItems;
          const lastItem = items[items.length - 1];
          const itemHasTruncation =
            lastItem &&
            (lastItem.type === 'text' || lastItem.type === 'reasoning') &&
            !!lastItem.truncatedPrefix;
          if (!itemHasTruncation && !message.truncatedPrefix) {
            return message;
          }
          const prefix =
            (lastItem &&
              (lastItem.type === 'text' || lastItem.type === 'reasoning') &&
              lastItem.truncatedPrefix) ||
            message.truncatedPrefix ||
            '';
          if (!prefix) return message;
          changed = true;
          return spliceMessagePrefix(message, prefix);
        });
        if (!changed) return session;
        return { ...session, messages };
      }),
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

const createDiffSlice = (
  applyDiffActions: DiffApplicationActions,
): AIPanelStateCreator<Pick<AIPanelState, 'setCurrentDiff' | 'setMessageDiff' | 'setPendingDiff' | 'setDiffFromToolResult' | 'acceptHunk' | 'rejectHunk' | 'acceptAllHunks' | 'rejectAllHunks'>> => (set) => ({
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
        applyDiffActions.applyHunk(diff.filePath, hunkId);
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
        applyDiffActions.applyAllHunks(diff.filePath);
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
      ...createDiffSlice(editorDiffActions)(...args),
    }),
    {
      name: 'inkuo-ai-panel',
      version: 1,
      partialize: (state) => ({
        isOpen: state.isOpen,
        activeTab: state.activeTab,
      }),
      merge: pickPersistedUiBits,
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
