//! Per-message slice of the AI panel store.
//!
//! Owns the lifecycle of a single chat message: creation, content
//! appending, reasoning/prefix expansion, output items (search
//! results, tool calls), search-result attachment, and the
//! auto-expansion heuristics for truncated prefixes.
//!
//! Note: nearly every action only mutates `state.sessions`. To keep each
//! action compact, this slice uses a `setSessions(updater)` helper that
//! wraps the standard `set((state) => ({ sessions: ... }))` boilerplate.

import { TIMING } from '../../../constants/timing';
import type { ChatSession } from '../../../types';
import type { AIPanelState, AIPanelStateCreator } from '../../aiPanelStore.types';
import {
  addMessageOutputItem,
  appendSessionMessage,
  clearSessionConversation,
  collapseMessageHead,
  collapseOldSessionMessages,
  expandCollapsedSessionMessages,
  finishSessionMessageStreaming,
  hardCollapseSessionHistory,
  patchMessageOutputState,
  pruneTrailingCompactToolInSession,
  setMessageOutputItems,
  spliceMessagePrefix,
  touchSession,
  trimSessionMessagesAfter,
  updateMessages,
  updateSessionMessage,
  updateSessionState,
  updateSessions,
} from '../../aiPanelReducers';

export const createMessageSlice: AIPanelStateCreator<Pick<AIPanelState, 'addMessage' | 'updateMessage' | 'appendMessageContent' | 'setIsStreaming' | 'clearMessages' | 'truncateMessagesAfter' | 'getMessage' | 'updateMessageOutput' | 'addOutputToMessage' | 'patchOutputItem' | 'pruneTrailingCompactTool' | 'finishMessageStreaming' | 'setErrorMessage' | 'setMessageSearchResults' | 'expandMessagePrefix' | 'collapseMessagePrefix' | 'toggleReasoningExpansion' | 'autoExpandTruncatedPrefixes' | 'collapseOldMessages' | 'expandCollapsedHistory' | 'hardCollapseHistory'>> = (set, get) => {
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
    /**
     * Drop the trailing compact-tool `OutputItem` (if any) from `messageId`,
     * provided it has not yet received a result. Called by the stream
     * dispatcher right before appending a new `tool_call_start` so a tight
     * `list_dir → read_file` sequence collapses into a single inline line.
     *
     * See `pruneTrailingCompactToolInSession` in
     * `aiPanelReducers/outputItemReducer.ts` for the full predicate.
     */
    pruneTrailingCompactTool: (sessionId, messageId) =>
      setSessions((sessions) =>
        updateSessions(sessions, sessionId, (session) =>
          pruneTrailingCompactToolInSession(session, messageId),
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
  };
};