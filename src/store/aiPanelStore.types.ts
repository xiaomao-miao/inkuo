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
  /**
   * Hard delete. The session is removed from the array AND the next
   * workspace snapshot save will omit it. Use only for explicit "delete
   * forever" actions (e.g. the HistorySidebar trash button).
   */
  deleteSession: (sessionId: string) => void;
  /**
   * Soft close. Marks the session as `archived: true` and drops it from
   * the header chip bar, but the session (with its full message
   * history) stays in the array and keeps being persisted to disk via
   * `saveCurrentSnapshot`. The session is still selectable from the
   * HistorySidebar at any time. This is what "close" / "×" from the
   * header chip bar should call so users don't accidentally lose work.
   */
  closeSession: (sessionId: string) => void;
  /**
   * Un-archive a session. The session reappears in the header chip bar.
   */
  reopenSession: (sessionId: string) => void;
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
  /**
   * Restore a previously-truncated head of text for a message's trailing
   * OutputItem (or `content` if the message has no outputItems). Used by the
   * lazy-load "show earlier content" affordance in the chat panel.
   *
   * `keepTail` controls how many trailing chars of the visible content to
   * keep rendered after collapsing — the rest (the head) is folded back into
   * `truncatedPrefix` so the user can collapse again later.
   */
  expandMessagePrefix: (
    sessionId: string,
    messageId: string,
    keepTail?: number,
  ) => void;
  /**
   * Collapse the head of a message back into `truncatedPrefix`. Inverse of
   * `expandMessagePrefix`. Safe to call when nothing is truncated.
   */
  collapseMessagePrefix: (
    sessionId: string,
    messageId: string,
    keepTail: number,
  ) => void;
  /**
   * Toggle a specific reasoning block's "user expanded" state. Reads the
   * current set of expanded ids from the message, adds or removes the
   * given `reasoningId`, and writes the new set back. Each block is
   * toggled independently — expanding one block does NOT affect any
   * other reasoning block in the same message.
   */
  toggleReasoningExpansion: (
    sessionId: string,
    messageId: string,
    reasoningId: string,
  ) => void;
  /**
   * Auto-expand any message with a non-empty `truncatedPrefix` (i.e. head
   * content that's currently folded away). Designed to be called when the
   * user scrolls near the top of the chat panel so the older content is
   * restored without an explicit click.
   */
  autoExpandTruncatedPrefixes: (sessionId: string) => void;
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
  setDiffFromToolResult: (sessionId: string, diff: CurrentDiff | null) => void;
  acceptHunk: (sessionId: string, hunkId: string) => void;
  rejectHunk: (sessionId: string, hunkId: string) => void;
  acceptAllHunks: (sessionId: string) => void;
  rejectAllHunks: (sessionId: string) => void;
}

export interface DiffApplicationActions {
  applyHunk: (path: string, hunkId: string) => void;
  applyAllHunks: (path: string) => void;
}

export type AIPanelState =
  & AIPanelUiSlice
  & AIPanelSessionSlice
  & AIPanelMessageSlice
  & AIPanelToolCallSlice
  & AIPanelDiffSlice
  & SubagentActivitySlice;

export type AIPanelStateCreator<T> = StateCreator<
  AIPanelState,
  [],
  [],
  T
>;

/** Sub-agent activity slice interface */
export interface SubagentActivitySlice {
  addSubagentActivity: (
    sessionId: string,
    messageId: string,
    activity: import('../types').SubagentActivity,
  ) => void;
  addOutputToSubagentActivity: (
    sessionId: string,
    parentMessageId: string,
    subagentId: string,
    outputItem: OutputItem,
  ) => void;
  /**
   * Stream-append a text/reasoning delta into the trailing item of a
   * sub-agent's output list. Mirrors `applyStreamingTextDeltas` semantics
   * for the top-level stream so users see progressive updates instead of
   * a new item every flush tick.
   */
  appendOutputDeltaToSubagentActivity: (
    sessionId: string,
    parentMessageId: string,
    subagentId: string,
    delta: { content: string; type: 'text' | 'reasoning' },
  ) => void;
  completeSubagentActivity: (
    sessionId: string,
    parentMessageId: string,
    subagentId: string,
    status: 'completed' | 'error',
    summary?: string,
    error?: string,
  ) => void;
  toggleSubagentActivityExpanded: (
    sessionId: string,
    parentMessageId: string,
    subagentId: string,
  ) => void;
}
