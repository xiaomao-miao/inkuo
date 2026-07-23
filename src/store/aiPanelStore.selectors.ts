// Slice-scoped selector hooks for `useAIPanelStore`.
//
// Why this file exists:
//
//   The AI panel store spans 6 slices (ui, session, message, tool-call,
//   diff, subagent). All slices live in a single Zustand store so any
//   write triggers Zustand's standard "did the selected slice change?"
//   comparison — but only IF the consumer subscribes to a *narrow* slice.
//
//   Most consumers read 1-2 values, so a single `useAIPanelStore((state)
//   => state.X)` is fine. But there are dozens of callsites and the
//   selector bodies are easy to typo, easy to subscribe to a too-broad
//   object (causing unnecessary rerenders), and hard to refactor in bulk.
//
//   This module exports focused, documented selector hooks that wrap the
//   underlying store. New code should reach for these instead of writing
//   `useAIPanelStore((state) => ...)` inline. The hooks are typed against
//   `AIPanelState` so callers get full type inference on the returned
//   value.
//
//   Selector hooks here are stable; if you find yourself writing the same
//   `(state) => ...` arrow more than once across files, lift it into here
//   and document the contract.

import { useCallback } from 'react';
import { useShallow } from 'zustand/react/shallow';

import type { ChatSession } from '../types';
import type { AIPanelState } from './aiPanelStore.types';
import { useAIPanelStore } from './aiPanelStore';

// ─── UI slice ─────────────────────────────────────────────────────────────────

/** Whether the AI panel is visible. */
export const useAIPanelOpen = (): boolean => useAIPanelStore((s) => s.isOpen);

/** Currently active tab inside the AI panel. */
export const useAIPanelActiveTab = (): AIPanelState['activeTab'] =>
  useAIPanelStore((s) => s.activeTab);

/** Whether the feature toolbar is expanded above the chat input. */
export const useFeatureToolbarExpanded = (): boolean =>
  useAIPanelStore((s) => s.featureToolbarExpanded);

/** Stable callback to open / close / toggle the panel. */
export const useAIPanelToggle = (): (() => void) =>
  useAIPanelStore((s) => s.togglePanel);

export const useAIPanelSetIsOpen = (): ((open: boolean) => void) =>
  useAIPanelStore((s) => s.setIsOpen);

export const useSetActiveTab = (): ((tab: AIPanelState['activeTab']) => void) =>
  useAIPanelStore((s) => s.setActiveTab);

// ─── Session slice ────────────────────────────────────────────────────────────

/** All sessions in insertion order (most recently created first). */
export const useSessions = (): ChatSession[] =>
  useAIPanelStore((s) => s.sessions);

/** ID of the active session. */
export const useActiveSessionId = (): string => useAIPanelStore((s) => s.activeSessionId);

/**
 * The currently active session object. Returns `undefined` when the store
 * has been cleared (e.g. via SSR / test reset).
 */
export const useActiveSession = (): ChatSession | undefined =>
  useAIPanelStore((s) => s.sessions.find((session) => session.id === s.activeSessionId));

/** Look up a single session by id. Returns `undefined` if missing. */
export const useSessionById = (sessionId: string): ChatSession | undefined =>
  useAIPanelStore((s) => s.sessions.find((session) => session.id === sessionId));

/** Stable callback bound to the session-mutating actions. */
export const useSessionActions = () =>
  useAIPanelStore(
    useShallow((s) => ({
      createSession: s.createSession,
      deleteSession: s.deleteSession,
      closeSession: s.closeSession,
      reopenSession: s.reopenSession,
      setActiveSession: s.setActiveSession,
      setSessionMode: s.setSessionMode,
      setSessionFeatureToggle: s.setSessionFeatureToggle,
      updateSession: s.updateSession,
    })),
  );

// ─── Message slice ────────────────────────────────────────────────────────────

/**
 * Memoised action bundle for the most common message operations.
 * Components that only call these actions (e.g. `useAgentStream`) can
 * spread this into their deps to avoid stale closures.
 */
export const useMessageActions = () =>
  useAIPanelStore(
    useShallow((s) => ({
      addMessage: s.addMessage,
      updateMessage: s.updateMessage,
      appendMessageContent: s.appendMessageContent,
      setIsStreaming: s.setIsStreaming,
      clearMessages: s.clearMessages,
      truncateMessagesAfter: s.truncateMessagesAfter,
      getMessage: s.getMessage,
      finishMessageStreaming: s.finishMessageStreaming,
      setErrorMessage: s.setErrorMessage,
      setMessageSearchResults: s.setMessageSearchResults,
      expandMessagePrefix: s.expandMessagePrefix,
      collapseMessagePrefix: s.collapseMessagePrefix,
      toggleReasoningExpansion: s.toggleReasoningExpansion,
      autoExpandTruncatedPrefixes: s.autoExpandTruncatedPrefixes,
      collapseOldMessages: s.collapseOldMessages,
      expandCollapsedHistory: s.expandCollapsedHistory,
      hardCollapseHistory: s.hardCollapseHistory,
    })),
  );

// ─── Tool-call slice ──────────────────────────────────────────────────────────

export const useToolCallActions = () =>
  useAIPanelStore(
    useShallow((s) => ({
      addToolCall: s.addToolCall,
      updateToolCall: s.updateToolCall,
      removeToolCall: s.removeToolCall,
      clearToolCalls: s.clearToolCalls,
    })),
  );

// ─── Diff slice ───────────────────────────────────────────────────────────────

export const useDiffActions = () =>
  useAIPanelStore(
    useShallow((s) => ({
      setCurrentDiff: s.setCurrentDiff,
      setMessageDiff: s.setMessageDiff,
      setPendingDiff: s.setPendingDiff,
      setDiffFromToolResult: s.setDiffFromToolResult,
      acceptHunk: s.acceptHunk,
      rejectHunk: s.rejectHunk,
      acceptAllHunks: s.acceptAllHunks,
      rejectAllHunks: s.rejectAllHunks,
    })),
  );

// ─── Subagent slice ───────────────────────────────────────────────────────────

export const useSubagentActions = () =>
  useAIPanelStore(
    useShallow((s) => ({
      addSubagentActivity: s.addSubagentActivity,
      addOutputToSubagentActivity: s.addOutputToSubagentActivity,
      appendOutputDeltaToSubagentActivity: s.appendOutputDeltaToSubagentActivity,
      completeSubagentActivity: s.completeSubagentActivity,
      toggleSubagentActivityExpanded: s.toggleSubagentActivityExpanded,
    })),
  );

// ─── Output item + plan slice ─────────────────────────────────────────────────

export const useOutputActions = () =>
  useAIPanelStore(
    useShallow((s) => ({
      updateMessageOutput: s.updateMessageOutput,
      addOutputToMessage: s.addOutputToMessage,
      patchOutputItem: s.patchOutputItem,
      convertTrailingTextToPlanItem: s.convertTrailingTextToPlanItem,
      appendPlanDelta: s.appendPlanDelta,
      finishPlanItem: s.finishPlanItem,
      setPlanItemFile: s.setPlanItemFile,
      clearPlanItemFile: s.clearPlanItemFile,
      addPlanItem: s.addPlanItem,
    })),
  );

// ─── Todo snapshot slice ──────────────────────────────────────────────────────

/** Per-session TodoPanel snapshots. Read-only — mutations go through session slice actions. */
export const useTodoSnapshotBySession = (): AIPanelState['todoSnapshotBySession'] =>
  useAIPanelStore((s) => s.todoSnapshotBySession);

/** Look up the snapshot for a single session. */
export const useSessionTodoSnapshot = (sessionId: string) =>
  useAIPanelStore((s) => s.todoSnapshotBySession[sessionId]);

// ─── Convenience helpers ──────────────────────────────────────────────────────

/**
 * Resolve the session id a caller should act on. When `override` is supplied
 * (e.g. for restoring collapsed history of a previously-visited session),
 * use it; otherwise return the active session id. Pure selector — no
 * rerender side-effects since it reads a single primitive.
 */
export const useSessionIdOrOverride = (override?: string): string | undefined =>
  useAIPanelStore((s) => override ?? s.activeSessionId);

/**
 * Stable `useCallback`-stable getter for the active session id.
 * Useful in event handlers that close over the id at click time and
 * don't want to re-subscribe when the active session changes.
 */
export const useActiveSessionIdRef = (): (() => string) => {
  const getActiveSessionId = useAIPanelStore((s) => s.activeSessionId);
  return useCallback(() => getActiveSessionId, [getActiveSessionId]);
};
