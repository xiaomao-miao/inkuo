// Barrel for AI panel reducer helpers. The functions used to live in a
// single 496-line `aiPanelReducers.ts` file; they are now split across
// three focused modules — `sessionReducer`, `toolCallReducer`, and
// `outputItemReducer` — for readability and to make test files
// easier to scope.
//
// External callers keep importing from `./aiPanelReducers`, which now
// re-exports everything from the focused modules.

export {
  createSessionTitle,
  createNewSession,
  touchSession,
  updateSessions,
  updateSessionState,
  updateSessionMessage,
  appendSessionMessage,
  finishSessionMessageStreaming,
  updatePendingDiffState,
  updateMessages,
  clearSessionConversation,
  trimSessionMessagesAfter,
} from './sessionReducer';

export {
  appendSessionToolCall,
  removeSessionToolCall,
  clearSessionToolCalls,
  updateToolCalls,
} from './toolCallReducer';

export type { OutputItemMatchKey } from './outputItemReducer';
export {
  patchMessageOutputItems,
  addMessageOutputItem,
  setMessageDiffState,
  setMessageOutputItems,
  patchMessageOutputState,
  appendPlanDeltaToMessage,
  convertTrailingTextToPlanItem,
  updatePendingDiffHunks,
  spliceMessagePrefix,
  collapseMessageHead,
  collapseOldSessionMessages,
  expandCollapsedSessionMessages,
  hardCollapseSessionHistory,
  pruneTrailingCompactTool,
  pruneTrailingCompactToolInSession,
} from './outputItemReducer';