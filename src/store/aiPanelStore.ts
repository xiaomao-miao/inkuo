import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import type {
  ActiveToolCall,
  BuildProgress,
  ChatMessage,
  ChatMode,
  ChatSession,
  CurrentDiff,
  FeatureToggleId,
  FeatureToggleMap,
  MessageRole,
  MessageToolCall,
  MessageToolResult,
  OutputItem,
  SearchResult,
} from '../types';
import {
  addMessageOutputItem,
  appendPlanDeltaToMessage,
  appendSessionMessage,
  appendSessionToolCall,
  clearSessionToolCalls,
  clearSessionConversation,
  collapseMessageHead,
  collapseOldSessionMessages,
  convertTrailingTextToPlanItem,
  createNewSession,
  expandCollapsedSessionMessages,
  finishSessionMessageStreaming,
  hardCollapseSessionHistory,
  patchMessageOutputState,
  removeSessionToolCall,
  setMessageDiffState,
  setMessageOutputItems,
  spliceMessagePrefix,
  trimSessionMessagesAfter,
  touchSession,
  updateMessages,
  updatePendingDiffHunks,
  updatePendingDiffState,
  updateSessionMessage,
  updateSessionState,
  updateSessions,
  updateToolCalls,
} from './aiPanelReducers';
import { editorDiffActions } from './editorStore';
import { TIMING } from '../constants/timing';
import type {
  AIPanelState,
  AIPanelStateCreator,
  DiffApplicationActions,
  SubagentActivitySlice,
} from './aiPanelStore.types';

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

const createUiSlice: AIPanelStateCreator<Pick<AIPanelState, 'isOpen' | 'activeTab' | 'featureToolbarExpanded' | 'setIsOpen' | 'togglePanel' | 'setActiveTab' | 'setFeatureToolbarExpanded' | 'toggleFeatureToolbar'>> = (set) => ({
  isOpen: true,
  activeTab: 'chat',
  featureToolbarExpanded: false,
  setIsOpen: (open) => set({ isOpen: open }),
  togglePanel: () => set((state) => ({ isOpen: !state.isOpen })),
  setActiveTab: (tab) => set({ activeTab: tab }),
  setFeatureToolbarExpanded: (open) => set({ featureToolbarExpanded: open }),
  toggleFeatureToolbar: () =>
    set((state) => ({ featureToolbarExpanded: !state.featureToolbarExpanded })),
});

const createSessionSlice: AIPanelStateCreator<Pick<AIPanelState, 'sessions' | 'activeSessionId' | 'todoSnapshotBySession' | 'createSession' | 'deleteSession' | 'closeSession' | 'reopenSession' | 'setActiveSession' | 'setSessionMode' | 'setSessionFeatureToggle' | 'getSession' | 'updateSession' | 'setSessionTodoSnapshot' | 'clearSessionTodoSnapshot'>> = (set, get) => {
  const initialSession = createNewSession(1);

  return {
    sessions: [initialSession],
    activeSessionId: initialSession.id,
    todoSnapshotBySession: {},
    createSession: () => {
      const index = get().sessions.length + 1;
      const session = createNewSession(index);
      set((state) => ({
        sessions: [session, ...state.sessions],
        activeSessionId: session.id,
      }));
      return session.id;
    },
    /**
     * Hard delete. Permanent — the next snapshot save will omit the
     * session. Callers (e.g. HistorySidebar trash) must ask for explicit
     * confirmation first; a mis-click should not destroy history.
     */
    deleteSession: (sessionId) => {
      set((state) => {
        const remaining = state.sessions.filter((session) => session.id !== sessionId);
        const safeRemaining = remaining.length > 0 ? remaining : [createNewSession(1)];
        const nextActiveId =
          state.activeSessionId === sessionId ? safeRemaining[0].id : state.activeSessionId;

        // Also drop the todo panel snapshot for this session — the panel
        // is keyed on session.id, so a leftover snapshot for a deleted
        // session would resurrect in the UI if the user later creates a
        // new session with the same id (we use crypto.randomUUID, so
        // this is mostly defensive, but keeping the map clean avoids
        // confusion during debugging).
        const { [sessionId]: _drop, ...rest } = state.todoSnapshotBySession;
        return {
          sessions: safeRemaining,
          activeSessionId: nextActiveId,
          todoSnapshotBySession: rest,
        };
      });
    },
    /**
     * Soft-close. Marks the session as archived so it falls out of the
     * header chip bar, but the data stays put and is still loaded
     * back from disk after a restart.
     *
     * Invariant: after `closeSession`, `activeSessionId` always points
     * at a non-archived session (or a brand-new empty one). If the user
     * closes every single session we auto-create a fresh empty one
     * so the panel always has an active conversation in view — never
     * a closed one displayed as the "current" session.
     */
    closeSession: (sessionId) => {
      set((state) => {
        const sessions = state.sessions.map((session) =>
          session.id === sessionId ? { ...session, archived: true } : session,
        );

        let nextActiveId = state.activeSessionId;
        if (state.activeSessionId === sessionId) {
          // The session the user just closed was the active one — pick
          // a replacement that's still open. If nothing is open, mint a
          // brand-new empty session so `activeSession` never resolves
          // to an archived/empty-but-displayed state.
          const open = sessions.find((s) => !s.archived);
          if (open) {
            nextActiveId = open.id;
          } else {
            const fresh = createNewSession(sessions.length + 1);
            nextActiveId = fresh.id;
            sessions.unshift(fresh);
          }
        }
        return { sessions, activeSessionId: nextActiveId };
      });
    },
    reopenSession: (sessionId) => {
      set((state) => ({
        // Reopening is an explicit "I'm working on this again" — bump
        // lastActivityAt so it floats to the top of the history list.
        sessions: state.sessions.map((session) =>
          session.id === sessionId
            ? { ...session, archived: undefined, lastActivityAt: Date.now() }
            : session,
        ),
      }));
    },
    setActiveSession: (sessionId) => set({ activeSessionId: sessionId }),
    setSessionMode: (sessionId, mode) =>
      set((state) => ({
        sessions: updateSessionState(state.sessions, sessionId, { mode }),
      })),
    setSessionFeatureToggle: (sessionId, toggleId, enabled) =>
      set((state) => ({
        sessions: updateSessions(state.sessions, sessionId, (session) => {
          const current: FeatureToggleMap = { ...(session.featureToggles ?? {}) };
          if (enabled) {
            current[toggleId] = true;
          } else {
            // Drop the key so the on-disk shape stays compact — a session
            // with every toggle off shouldn't carry an empty `{}` either.
            delete current[toggleId];
          }
          return {
            ...session,
            featureToggles: Object.keys(current).length > 0 ? current : undefined,
          };
        }),
      })),
    getSession: (sessionId) => get().sessions.find((session) => session.id === sessionId),
    updateSession: (sessionId, updater) =>
      set((state) => ({
        sessions: updateSessions(state.sessions, sessionId, updater),
      })),
    setSessionTodoSnapshot: (sessionId, toolCallId, items) =>
      set((state) => ({
        todoSnapshotBySession: {
          ...state.todoSnapshotBySession,
          [sessionId]: {
            items,
            toolCallId,
            updatedAt: Date.now(),
          },
        },
      })),
    clearSessionTodoSnapshot: (sessionId) =>
      set((state) => {
        if (!(sessionId in state.todoSnapshotBySession)) return state;
        const { [sessionId]: _drop, ...rest } = state.todoSnapshotBySession;
        return { todoSnapshotBySession: rest };
      }),
  };
};

const createMessageSlice: AIPanelStateCreator<Pick<AIPanelState, 'addMessage' | 'updateMessage' | 'appendMessageContent' | 'setIsStreaming' | 'clearMessages' | 'truncateMessagesAfter' | 'getMessage' | 'updateMessageOutput' | 'addOutputToMessage' | 'patchOutputItem' | 'finishMessageStreaming' | 'setErrorMessage' | 'setMessageSearchResults' | 'expandMessagePrefix' | 'collapseMessagePrefix' | 'toggleReasoningExpansion' | 'autoExpandTruncatedPrefixes' | 'collapseOldMessages' | 'expandCollapsedHistory' | 'hardCollapseHistory' | 'convertTrailingTextToPlanItem' | 'appendPlanDelta' | 'finishPlanItem' | 'setPlanItemFile' | 'clearPlanItemFile' | 'addPlanItem'>> = (set, get) => ({
  addMessage: (sessionId, message) =>
    set((state) => ({
      // Every new message — user prompt or assistant reply — counts as
      // activity, so bump lastActivityAt so the history sidebar bubbles
      // it to the top of its date group.
      sessions: updateSessions(
        appendSessionMessage(state.sessions, sessionId, message),
        sessionId,
        touchSession,
      ),
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
      sessions: updateSessions(state.sessions, sessionId, (session) =>
        touchSession(clearSessionConversation(session)),
      ),
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
      // Stream completion is also "user-visible activity worth promoting
      // in history" — bump lastActivityAt alongside the content update.
      sessions: updateSessions(
        finishSessionMessageStreaming(state.sessions, sessionId, messageId, finalContent),
        sessionId,
        touchSession,
      ),
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
  collapseOldMessages: (sessionId, keepTail) =>
    set((state) => ({
      sessions: updateSessions(state.sessions, sessionId, (session) => {
        const k = keepTail ?? TIMING.SESSION_VIRTUALIZE_THRESHOLD;
        return collapseOldSessionMessages(session, k);
      }),
    })),
  expandCollapsedHistory: (sessionId, revealCount) =>
    set((state) => ({
      sessions: updateSessions(state.sessions, sessionId, (session) => {
        const k = revealCount ?? TIMING.SESSION_VIRTUALIZE_EXPAND_BATCH;
        return expandCollapsedSessionMessages(session, k);
      }),
    })),
  hardCollapseHistory: (sessionId) =>
    set((state) => ({
      sessions: updateSessions(state.sessions, sessionId, (session) =>
        hardCollapseSessionHistory(session)
      ),
    })),
  convertTrailingTextToPlanItem: (sessionId, messageId, rawText) =>
    set((state) => {
      const sessions = updateSessions(state.sessions, sessionId, (session) =>
        convertTrailingTextToPlanItem(session, messageId, rawText)
      );
      // If the message's trailing item wasn't a text item, fall back to
      // appending a fresh plan item. This keeps the streaming buffer
      // robust against re-ordering or first-chunk arrivals where the
      // initial text item never existed.
      const session = sessions.find((s) => s.id === sessionId);
      const message = session?.messages.find((m) => m.id === messageId);
      const hasPlanItem = message?.outputItems.some((it) => it.type === 'plan');
      if (hasPlanItem) return { sessions };
      return {
        sessions: updateSessions(sessions, sessionId, (s) =>
          addMessageOutputItem(s, messageId, {
            type: 'plan',
            rawText,
            plan: null,
            isStreaming: true,
          })
        ),
      };
    }),
  appendPlanDelta: (sessionId, messageId, delta) =>
    set((state) => ({
      sessions: updateSessions(state.sessions, sessionId, (session) =>
        appendPlanDeltaToMessage(session, messageId, delta)
      ),
    })),
  finishPlanItem: (sessionId, messageId) =>
    set((state) => ({
      sessions: updateSessionMessage(state.sessions, sessionId, messageId, (message) => ({
        ...message,
        outputItems: message.outputItems.map((item) =>
          item.type === 'plan' ? { ...item, isStreaming: false } : item
        ),
      })),
    })),
  /**
   * Stamp the trailing plan OutputItem with the `planFileId` /
   * `planFilePath` returned from `plan_save`. Lets later destroy flows
   * (apply, cancel, session close) identify which `<workspace>/.inkuo/plans/`
   * file to delete.
   */
  setPlanItemFile: (sessionId, messageId, planFileId, planFilePath) =>
    set((state) => ({
      sessions: updateSessionMessage(state.sessions, sessionId, messageId, (message) => ({
        ...message,
        outputItems: message.outputItems.map((item) =>
          item.type === 'plan' ? { ...item, planFileId, planFilePath } : item
        ),
      })),
    })),
  /**
   * Clear `planFileId` / `planFilePath` on the trailing plan OutputItem.
   * Called after the plan has been applied (or destroyed for any other
   * reason) so the UI no longer claims the file is on disk.
   */
  clearPlanItemFile: (sessionId, messageId) =>
    set((state) => ({
      sessions: updateSessionMessage(state.sessions, sessionId, messageId, (message) => {
        const next = message.outputItems.map((item) => {
          if (item.type !== 'plan') return item;
          const { planFileId: _id, planFilePath: _path, ...rest } = item;
          return rest;
        });
        return { ...message, outputItems: next };
      }),
    })),
  /**
   * Create a complete plan OutputItem from a `plan_result` stream event.
   * Converts the Rust `PlanResultData` (intent/intent strings) to the frontend
   * `PlanOutput` shape (intent: PlanFileIntent, needs_confirmation: true).
   * The `saved_path` is used to derive `planFileId` / `planFilePath`.
   */
  addPlanItem: (sessionId, messageId, data) => {
    // Derive planFileId from saved_path: ".../.inkuo/plans/<id>.md" → "<id>"
    const parts = data.saved_path.split('/');
    const filename = parts[parts.length - 1]; // "<id>.md"
    const planFileId = filename.replace(/\.md$/, '');
    const planFilePath = data.saved_path;

    const planOutput = {
      plan_summary: data.plan_summary,
      files_to_touch: data.files_to_touch.map((f) => ({
        path: f.path,
        intent: f.intent as import('../types').PlanFileIntent,
        reason: f.reason,
      })),
      risk: data.risk as import('../types').PlanRisk,
      risk_reason: data.risk_reason,
      needs_confirmation: true,
    };

    const planItem: OutputItem = {
      type: 'plan',
      rawText: data.content,
      plan: planOutput,
      // Start as streaming so the PlanCard stays hidden until the AI's
      // turn finishes (`done` event). The card is rendered as a tiny
      // tool-call placeholder during streaming, then promoted to the full
      // card by `finishPlanItem` in `handleStreamDone`.
      isStreaming: true,
      planFileId,
      planFilePath,
    };

    set((state) => ({
      sessions: updateSessions(
        updateSessionMessage(state.sessions, sessionId, messageId, (message) => ({
          ...message,
          // PlanCard is rendered as the FINAL element of the AI message.
          // Strip out any prior plan items (in case the LLM called
          // `create_plan` more than once, or a previous turn left one
          // behind) so we never end up with multiple PlanCards stacked
          // in the middle of the message — always exactly one, pinned
          // to the very end.
          outputItems: [
            ...message.outputItems.filter((it) => it.type !== 'plan'),
            planItem,
          ],
        })),
        sessionId,
        touchSession,
      ),
    }));
  },
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

const createSubagentSlice: AIPanelStateCreator<SubagentActivitySlice> = (set) => ({
  addSubagentActivity: (sessionId, messageId, activity) =>
    set((state) => ({
      sessions: updateSessions(state.sessions, sessionId, (session) => ({
        ...session,
        messages: session.messages.map((msg) =>
          msg.id === messageId
            ? {
                ...msg,
                subagentActivities: [
                  ...(msg.subagentActivities ?? []),
                  activity,
                ],
              }
            : msg,
        ),
      })),
    })),

  addOutputToSubagentActivity: (sessionId, parentMessageId, subagentId, outputItem) =>
    set((state) => ({
      sessions: updateSessions(state.sessions, sessionId, (session) => ({
        ...session,
        messages: session.messages.map((msg) =>
          msg.id === parentMessageId
            ? {
                ...msg,
                subagentActivities: msg.subagentActivities?.map((activity) =>
                  activity.id === subagentId
                    ? {
                        ...activity,
                        outputItems: [...activity.outputItems, outputItem],
                      }
                    : activity,
                ),
              }
            : msg,
        ),
      })),
    })),

  appendOutputDeltaToSubagentActivity: (
    sessionId: string,
    parentMessageId: string,
    subagentId: string,
    delta: { content: string; type: 'text' | 'reasoning' },
  ) =>
    set((state) => ({
      sessions: updateSessions(state.sessions, sessionId, (session) => ({
        ...session,
        messages: session.messages.map((msg) => {
          if (msg.id !== parentMessageId) return msg;
          return {
            ...msg,
            subagentActivities: msg.subagentActivities?.map((activity) => {
              if (activity.id !== subagentId) return activity;
              const items = activity.outputItems;
              const last = items[items.length - 1];
              if (last && last.type === delta.type && (last.type === 'text' || last.type === 'reasoning')) {
                const merged = {
                  ...last,
                  content: last.content + delta.content,
                };
                return {
                  ...activity,
                  outputItems: [...items.slice(0, -1), merged],
                };
              }
              const fresh =
                delta.type === 'text'
                  ? { type: 'text' as const, content: delta.content, isPendingMarkdown: false }
                  : { type: 'reasoning' as const, content: delta.content, isPendingMarkdown: false };
              return {
                ...activity,
                outputItems: [...items, fresh],
              };
            }),
          };
        }),
      })),
    })),

  completeSubagentActivity: (sessionId, parentMessageId, subagentId, status, summary, error) =>
    set((state) => ({
      sessions: updateSessions(state.sessions, sessionId, (session) => ({
        ...session,
        messages: session.messages.map((msg) =>
          msg.id === parentMessageId
            ? {
                ...msg,
                subagentActivities: msg.subagentActivities?.map((activity) =>
                  activity.id === subagentId
                    ? {
                        ...activity,
                        status,
                        summary,
                        error,
                        // Auto-collapse on completion
                        expanded: false,
                      }
                    : activity,
                ),
              }
            : msg,
        ),
      })),
    })),

  toggleSubagentActivityExpanded: (sessionId, parentMessageId, subagentId) =>
    set((state) => ({
      sessions: updateSessions(state.sessions, sessionId, (session) => ({
        ...session,
        messages: session.messages.map((msg) =>
          msg.id === parentMessageId
            ? {
                ...msg,
                subagentActivities: msg.subagentActivities?.map((activity) =>
                  activity.id === subagentId
                    ? { ...activity, expanded: !activity.expanded }
                    : activity,
                ),
              }
            : msg,
        ),
      })),
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
      ...createSubagentSlice(...args),
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
  FeatureToggleId,
  FeatureToggleMap,
  MessageRole,
  MessageToolCall,
  MessageToolResult,
  OutputItem,
  SearchResult,
};
