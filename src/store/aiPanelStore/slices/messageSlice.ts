//! Per-message slice of the AI panel store.
//!
//! Owns the lifecycle of a single chat message: creation, content
//! appending, plan-item extraction, reasoning/prefix expansion, output
//! items (search results, tool calls), search-result attachment, and the
//! auto-expansion heuristics for truncated prefixes.
//!
//! Note: nearly every action only mutates `state.sessions`. To keep each
//! action compact, this slice uses a `setSessions(updater)` helper that
//! wraps the standard `set((state) => ({ sessions: ... }))` boilerplate.
//! Actions that need to mutate multiple fields (e.g. the plan-conversion
//! two-step) still use the raw setter.

import { TIMING } from '../../../constants/timing';
import type { ChatSession } from '../../../types';
import type { AIPanelState, AIPanelStateCreator } from '../../aiPanelStore.types';
import {
  addMessageOutputItem,
  appendPlanDeltaToMessage,
  appendSessionMessage,
  clearSessionConversation,
  collapseMessageHead,
  collapseOldSessionMessages,
  convertTrailingTextToPlanItem,
  expandCollapsedSessionMessages,
  finishSessionMessageStreaming,
  hardCollapseSessionHistory,
  patchMessageOutputState,
  setMessageOutputItems,
  spliceMessagePrefix,
  touchSession,
  trimSessionMessagesAfter,
  updateMessages,
  updateSessionMessage,
  updateSessionState,
  updateSessions,
} from '../../aiPanelReducers';

export const createMessageSlice: AIPanelStateCreator<Pick<AIPanelState, 'addMessage' | 'updateMessage' | 'appendMessageContent' | 'setIsStreaming' | 'clearMessages' | 'truncateMessagesAfter' | 'getMessage' | 'updateMessageOutput' | 'addOutputToMessage' | 'patchOutputItem' | 'finishMessageStreaming' | 'setErrorMessage' | 'setMessageSearchResults' | 'expandMessagePrefix' | 'collapseMessagePrefix' | 'toggleReasoningExpansion' | 'autoExpandTruncatedPrefixes' | 'collapseOldMessages' | 'expandCollapsedHistory' | 'hardCollapseHistory' | 'convertTrailingTextToPlanItem' | 'appendPlanDelta' | 'finishPlanItem' | 'setPlanItemFile' | 'clearPlanItemFile' | 'addPlanItem'>> = (set, get) => {
  /** Replace the `sessions` array with the result of `updater(sessions)`. */
  const setSessions = (
    updater: (sessions: ChatSession[]) => ChatSession[],
  ): void => set((state) => ({ sessions: updater(state.sessions) }));

  // Action bodies keep a uniform "transform sessions only" shape. Each one
  // used to be `set((state) => ({ sessions: ... }))` — collapsing the
  // wrapper into `setSessions` removes ~24 lines of pure boilerplate.
  return {
    addMessage: (sessionId, message) =>
      setSessions((sessions) =>
        updateSessions(
          appendSessionMessage(sessions, sessionId, message),
          sessionId,
          touchSession,
        ),
      ),
    updateMessage: (sessionId, messageId, content) =>
      setSessions((sessions) =>
        updateSessionMessage(sessions, sessionId, messageId, (message) => ({
          ...message,
          content,
        })),
      ),
    appendMessageContent: (sessionId, messageId, content) =>
      setSessions((sessions) =>
        updateSessionMessage(sessions, sessionId, messageId, (message) => ({
          ...message,
          content: (message.content || '') + content,
        })),
      ),
    setIsStreaming: (sessionId, streaming) =>
      setSessions((sessions) =>
        updateSessionState(sessions, sessionId, { isStreaming: streaming }),
      ),
    clearMessages: (sessionId) =>
      setSessions((sessions) =>
        updateSessions(sessions, sessionId, (session) =>
          touchSession(clearSessionConversation(session)),
        ),
      ),
    truncateMessagesAfter: (sessionId, messageId) =>
      setSessions((sessions) =>
        updateSessions(sessions, sessionId, (session) =>
          trimSessionMessagesAfter(session, messageId),
        ),
      ),
    getMessage: (sessionId, messageId) =>
      get().sessions
        .find((session) => session.id === sessionId)
        ?.messages.find((message) => message.id === messageId),
    updateMessageOutput: (sessionId, messageId, outputItems) =>
      setSessions((sessions) =>
        updateSessions(sessions, sessionId, (session) =>
          setMessageOutputItems(session, messageId, outputItems),
        ),
      ),
    addOutputToMessage: (sessionId, messageId, outputItem) =>
      setSessions((sessions) =>
        updateSessions(sessions, sessionId, (session) =>
          addMessageOutputItem(session, messageId, outputItem),
        ),
      ),
    patchOutputItem: (sessionId, messageId, matchKey, patch) =>
      setSessions((sessions) =>
        updateSessions(sessions, sessionId, (session) =>
          patchMessageOutputState(session, messageId, matchKey, patch),
        ),
      ),
    finishMessageStreaming: (sessionId, messageId, finalContent) =>
      setSessions((sessions) =>
        updateSessions(
          finishSessionMessageStreaming(sessions, sessionId, messageId, finalContent),
          sessionId,
          touchSession,
        ),
      ),
    setErrorMessage: (sessionId, messageId, error) =>
      setSessions((sessions) =>
        finishSessionMessageStreaming(sessions, sessionId, messageId, error),
      ),
    setMessageSearchResults: (sessionId, messageId, results) =>
      setSessions((sessions) =>
        updateSessionMessage(sessions, sessionId, messageId, (message) => ({
          ...message,
          searchResults: results,
        })),
      ),
    expandMessagePrefix: (sessionId, messageId, keepTail) =>
      setSessions((sessions) =>
        updateSessions(sessions, sessionId, (session) =>
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
          }),
        ),
      ),
    collapseMessagePrefix: (sessionId, messageId, keepTail) =>
      setSessions((sessions) =>
        updateSessions(sessions, sessionId, (session) =>
          updateMessages(session, messageId, (message) =>
            collapseMessageHead(message, keepTail),
          ),
        ),
      ),
    toggleReasoningExpansion: (sessionId, messageId, reasoningId) =>
      setSessions((sessions) =>
        updateSessions(sessions, sessionId, (session) =>
          updateMessages(session, messageId, (message) => {
            const current = message.expandedReasoningIds ?? [];
            const next = current.includes(reasoningId)
              ? current.filter((id) => id !== reasoningId)
              : [...current, reasoningId];
            return {
              ...message,
              expandedReasoningIds: next.length > 0 ? next : undefined,
            };
          }),
        ),
      ),
    autoExpandTruncatedPrefixes: (sessionId) =>
      setSessions((sessions) =>
        updateSessions(sessions, sessionId, (session) => {
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
      ),
    collapseOldMessages: (sessionId, keepTail) =>
      setSessions((sessions) =>
        updateSessions(sessions, sessionId, (session) =>
          collapseOldSessionMessages(session, keepTail ?? TIMING.SESSION_VIRTUALIZE_THRESHOLD),
        ),
      ),
    expandCollapsedHistory: (sessionId, revealCount) =>
      setSessions((sessions) =>
        updateSessions(sessions, sessionId, (session) =>
          expandCollapsedSessionMessages(
            session,
            revealCount ?? TIMING.SESSION_VIRTUALIZE_EXPAND_BATCH,
          ),
        ),
      ),
    hardCollapseHistory: (sessionId) =>
      setSessions((sessions) =>
        updateSessions(sessions, sessionId, (session) =>
          hardCollapseSessionHistory(session),
        ),
      ),
    convertTrailingTextToPlanItem: (sessionId, messageId, rawText) =>
      set((state) => {
        const sessions = updateSessions(state.sessions, sessionId, (session) =>
          convertTrailingTextToPlanItem(session, messageId, rawText),
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
            }),
          ),
        };
      }),
    appendPlanDelta: (sessionId, messageId, delta) =>
      setSessions((sessions) =>
        updateSessions(sessions, sessionId, (session) =>
          appendPlanDeltaToMessage(session, messageId, delta),
        ),
      ),
    finishPlanItem: (sessionId, messageId) =>
      setSessions((sessions) =>
        updateSessionMessage(sessions, sessionId, messageId, (message) => ({
          ...message,
          outputItems: message.outputItems.map((item) =>
            item.type === 'plan' ? { ...item, isStreaming: false } : item,
          ),
        })),
      ),
    /**
     * Stamp the trailing plan OutputItem with the `planFileId` /
     * `planFilePath` returned from `plan_save`. Lets later destroy flows
     * (apply, cancel, session close) identify which `<workspace>/.inkuo/plans/`
     * file to delete.
     */
    setPlanItemFile: (sessionId, messageId, planFileId, planFilePath) =>
      setSessions((sessions) =>
        updateSessionMessage(sessions, sessionId, messageId, (message) => ({
          ...message,
          outputItems: message.outputItems.map((item) =>
            item.type === 'plan' ? { ...item, planFileId, planFilePath } : item,
          ),
        })),
      ),
    /**
     * Clear `planFileId` / `planFilePath` on the trailing plan OutputItem.
     * Called after the plan has been applied (or destroyed for any other
     * reason) so the UI no longer claims the file is on disk.
     */
    clearPlanItemFile: (sessionId, messageId) =>
      setSessions((sessions) =>
        updateSessionMessage(sessions, sessionId, messageId, (message) => {
          const next = message.outputItems.map((item) => {
            if (item.type !== 'plan') return item;
            const { planFileId: _id, planFilePath: _path, ...rest } = item;
            return rest;
          });
          return { ...message, outputItems: next };
        }),
      ),
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
          intent: f.intent as import('../../../types').PlanFileIntent,
          reason: f.reason,
        })),
        risk: data.risk as import('../../../types').PlanRisk,
        risk_reason: data.risk_reason,
        needs_confirmation: true,
      };

      const planItem: import('../../../types').OutputItem = {
        type: 'plan',
        rawText: data.content,
        plan: planOutput,
        isStreaming: false,
        planFileId,
        planFilePath,
      };

      setSessions((sessions) =>
        updateSessions(
          updateSessionMessage(sessions, sessionId, messageId, (message) => ({
            ...message,
            outputItems: [...message.outputItems, planItem],
          })),
          sessionId,
          touchSession,
        ),
      );
    },
  };
};
