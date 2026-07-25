export { useEditorStore } from './editorStore';
export {
  useSidebarStore,
  SETTINGS_TAB_ID,
  CLOUD_TAB_ID,
  type OpenTab,
  type KnowledgeBase as SidebarKnowledgeBase,
  type BuildProgress as SidebarBuildProgress,
  type InlineEditState,
  type WorkspaceSnapshot,
} from './sidebarStore';
export { useAIPanelStore } from './aiPanelStore';
export { useLayoutStore } from './layoutStore';
export { useSettingsStore } from './settingsStore';
export { useCmdKStore } from './cmdKStore';
export { useInlineCompleteStore } from './inlineCompleteStore';
export { useNotificationStore, type NotificationItem } from './notificationStore';
export { useBaselineStore } from './baselineStore';
export { useClipboardStore, type ClipboardMode, type ClipboardState } from './clipboardStore';
export {
  useContextMenuStore,
  type ContextMenuTarget,
  type ContextMenuKind,
  type DocxCommands,
  type EditorCommands,
} from './contextMenuStore';
export {
  useFloatingAiStore,
  type FloatingAiWindow,
  type FloatingAiStatus,
} from './floatingAiStore';
export { useConfirmDialogStore, type ConfirmRequest } from './confirmDialogStore';
export {
  useEditorHandleStore,
  getEditorCommands,
  getEditorCapabilities,
  type EditorCapabilities,
} from './editorHandleStore';

export type {
  ActiveToolCall,
  BuildProgress,
  ChatMessage,
  ChatMode,
  ChatSession,
  CurrentDiff,
  DiffChange,
  DiffHunk,
  MessageRole,
  MessageToolCall,
  MessageToolResult,
  OutputItem,
  PlanFileIntent,
  PlanFileTouch,
  PlanOutput,
  PlanRisk,
  SearchResult,
  StreamDiffSummary,
  StreamPayload,
  KnowledgeSearchResult,
  KnowledgeBase,
} from '../types';
