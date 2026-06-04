// Re-export all stores for convenience
export { useEditorStore } from './editorStore';
export { useSidebarStore, SETTINGS_TAB_ID, type OpenTab } from './sidebarStore';
export { useAIPanelStore, type ChatMode, type MessageRole, type MessageToolCall, type MessageToolResult,
         type OutputItem, type ChatMessage, type ChatSession, type ActiveToolCall, type DiffSummary,
         type KnowledgeBase, type SearchResult, type BuildProgress } from './aiPanelStore';
export { useSettingsStore } from './settingsStore';
export { useCmdKStore } from './cmdKStore';
export { useInlineCompleteStore } from './inlineCompleteStore';

// Re-export shared types used across stores
export type { DiffHunk, DiffChange } from './editorStore';
export type { CurrentDiff } from './aiPanelStore';
