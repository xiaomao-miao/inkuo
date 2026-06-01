import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import type { Document, Settings, FileEntry, APIConfig } from '../types';

function createDefaultAPIConfig(): APIConfig {
  return {
    id: crypto.randomUUID(),
    name: 'DeepSeek V3',
    provider: 'deepseek',
    baseUrl: 'https://api.deepseek.com',
    apiKey: null,
    model: 'deepseek-chat',
    isDefault: true,
    enabled: true,
    temperature: 0.7,
    maxTokens: 4096,
  };
}

interface DocumentState {
  document: Document | null;
  content: string;
  isDirty: boolean;
  selection: { from: number; to: number } | null;
  diffHunks: DiffHunk[];
  activeHunkIndex: number;
  isDiffMode: boolean;
}

interface EditorState {
  // Multi-document state - keyed by file path
  documentContents: Record<string, DocumentState>;

  // Actions
  setDocumentContent: (path: string, doc: Document, content: string) => void;
  setContent: (path: string, content: string) => void;
  setSelection: (path: string, selection: { from: number; to: number } | null) => void;
  setDiffHunks: (path: string, hunks: any[]) => void;
  setActiveHunkIndex: (path: string, index: number) => void;
  setIsDiffMode: (path: string, isDiff: boolean) => void;
  applyHunk: (path: string, hunkId: string) => void;
  rejectHunk: (path: string, hunkId: string) => void;
  applyAllHunks: (path: string) => void;
  rejectAllHunks: (path: string) => void;
  clearDiff: (path: string) => void;
  markSaved: (path: string) => void;
  updateTabDirty: (path: string, isDirty: boolean) => void;
  getSelection: () => string | null;
  applyDiff: (diff: { originalText: string; newText: string }) => void;
  removeDocumentContent: (path: string) => void;
}

export const useEditorStore = create<EditorState>()(
  persist(
    (set) => ({
  documentContents: {},

  setDocumentContent: (path, doc, content) => set((state) => ({
    documentContents: {
      ...state.documentContents,
      [path]: {
        document: doc,
        content: content,
        isDirty: false,
        selection: null,
        diffHunks: [] as any[],
        activeHunkIndex: 0,
        isDiffMode: false,
      }
    }
  })),

  setContent: (path, content) => set((state) => {
    const current = state.documentContents[path];
    if (!current) return state;
    return {
      documentContents: {
        ...state.documentContents,
        [path]: {
          ...current,
          content: content,
          isDirty: true,
        }
      }
    };
  }),

  setSelection: (path, selection) => set((state) => {
    const current = state.documentContents[path];
    if (!current) return state;
    return {
      documentContents: {
        ...state.documentContents,
        [path]: {
          ...current,
          selection,
        }
      }
    };
  }),

  setDiffHunks: (path, hunks) => set((state) => {
    const current = state.documentContents[path];
    if (!current) return state;
    return {
      documentContents: {
        ...state.documentContents,
        [path]: {
          ...current,
          diffHunks: hunks,
          isDiffMode: hunks.length > 0,
        }
      }
    };
  }),

  setActiveHunkIndex: (path, index) => set((state) => {
    const current = state.documentContents[path];
    if (!current) return state;
    return {
      documentContents: {
        ...state.documentContents,
        [path]: {
          ...current,
          activeHunkIndex: index,
        }
      }
    };
  }),

  setIsDiffMode: (path, isDiff) => set((state) => {
    const current = state.documentContents[path];
    if (!current) return state;
    return {
      documentContents: {
        ...state.documentContents,
        [path]: {
          ...current,
          isDiffMode: isDiff,
        }
      }
    };
  }),

  applyHunk: (path, hunkId) => set((state) => {
    const current = state.documentContents[path];
    if (!current) return state;

    const hunkIndex = current.diffHunks.findIndex(h => h.id === hunkId);
    if (hunkIndex === -1) return state;

    const newHunks = current.diffHunks.filter(h => h.id !== hunkId);
    return {
      documentContents: {
        ...state.documentContents,
        [path]: {
          ...current,
          diffHunks: newHunks,
          isDiffMode: newHunks.length > 0,
          isDirty: true,
        }
      }
    };
  }),

  rejectHunk: (path, hunkId) => set((state) => {
    const current = state.documentContents[path];
    if (!current) return state;

    const newHunks = current.diffHunks.filter(h => h.id !== hunkId);
    return {
      documentContents: {
        ...state.documentContents,
        [path]: {
          ...current,
          diffHunks: newHunks,
          isDiffMode: newHunks.length > 0,
        }
      }
    };
  }),

  applyAllHunks: (path) => set((state) => {
    const current = state.documentContents[path];
    if (!current) return state;

    return {
      documentContents: {
        ...state.documentContents,
        [path]: {
          ...current,
          diffHunks: [] as any[],
          isDiffMode: false,
          isDirty: true,
        }
      }
    };
  }),

  rejectAllHunks: (path) => set((state) => {
    const current = state.documentContents[path];
    if (!current) return state;

    return {
      documentContents: {
        ...state.documentContents,
        [path]: {
          ...current,
          diffHunks: [] as any[],
          isDiffMode: false,
        }
      }
    };
  }),

  clearDiff: (path) => set((state) => {
    const current = state.documentContents[path];
    if (!current) return state;

    return {
      documentContents: {
        ...state.documentContents,
        [path]: {
          ...current,
          diffHunks: [] as any[],
          isDiffMode: false,
          activeHunkIndex: 0,
        }
      }
    };
  }),

  markSaved: (path) => set((state) => {
    const current = state.documentContents[path];
    if (!current) return state;

    return {
      documentContents: {
        ...state.documentContents,
        [path]: {
          ...current,
          isDirty: false,
        }
      }
    };
  }),

  updateTabDirty: (path, isDirty) => set((state) => {
    const current = state.documentContents[path];
    if (!current) return state;

    return {
      documentContents: {
        ...state.documentContents,
        [path]: {
          ...current,
          isDirty,
        }
      }
    };
  }),

  getSelection: () => {
    // This is a temporary implementation - in real app, get from editor
    return null;
  },

  applyDiff: (diff) => {
    // This is a temporary implementation - in real app, apply to editor
    console.log('Applying diff:', diff);
  },

  removeDocumentContent: (path) => set((state) => {
    const { [path]: _, ...rest } = state.documentContents;
    return { documentContents: rest };
  }),
}),
    {
      name: 'inkuo-editor',
      partialize: (state) => ({
        documentContents: Object.fromEntries(
          Object.entries(state.documentContents).map(([path, doc]) => [
            path,
            {
              document: doc.document,
              content: doc.content,
              isDirty: doc.isDirty,
              selection: doc.selection,
              diffHunks: doc.diffHunks,
              activeHunkIndex: doc.activeHunkIndex,
              isDiffMode: doc.isDiffMode,
            }
          ])
        ),
      }),
    }
  )
);

// Sidebar store
interface SidebarState {
  workspacePath: string | null;
  files: FileEntry[];
  expandedDirs: Set<string>;
  selectedFile: string | null;
  isLoading: boolean;
  openTabs: OpenTab[];
  activeTabId: string | null;
  // Map from tab path -> isDirty flag
  // Needed because useEditorStore is not persisted, so we track dirty state here
  openTabDirtyMap: Record<string, boolean>;

  setWorkspacePath: (path: string) => void;
  setFiles: (files: FileEntry[] | ((prev: FileEntry[]) => FileEntry[])) => void;
  toggleDir: (path: string) => void;
  setSelectedFile: (path: string | null) => void;
  setIsLoading: (loading: boolean) => void;
  openTab: (tab: OpenTab) => void;
  closeTab: (tabId: string) => void;
  setActiveTab: (tabId: string) => void;
  setOpenTabDirty: (path: string, isDirty: boolean) => void;
}

export interface OpenTab {
  id: string;
  path: string;
  name: string;
  isDirty: boolean;
  isSettings?: boolean;
}

// Special tab IDs
export const SETTINGS_TAB_ID = '__settings__';

export const useSidebarStore = create<SidebarState>()(
  persist(
    (set) => ({
  workspacePath: null,
  files: [],
  expandedDirs: new Set(),
  selectedFile: null,
  isLoading: false,
  openTabs: [],
  activeTabId: null,
  openTabDirtyMap: {},

  setWorkspacePath: (path) => set({ workspacePath: path }),
  setFiles: (files) => set((state) => ({
    files: typeof files === 'function' ? files(state.files) : files
  })),
  toggleDir: (path) => set((state) => {
    const newExpanded = new Set(state.expandedDirs);
    if (newExpanded.has(path)) {
      newExpanded.delete(path);
    } else {
      newExpanded.add(path);
    }
    return { expandedDirs: newExpanded };
  }),
  setSelectedFile: (path) => set({ selectedFile: path }),
  setIsLoading: (loading) => set({ isLoading: loading }),
  openTab: (tab) => set((state) => {
    const existing = state.openTabs.find(t => t.path === tab.path);
    if (existing) {
      return { activeTabId: existing.id, selectedFile: tab.path };
    }
    const newTabs = [...state.openTabs, tab];
    // For settings tab, selectedFile is null
    const newSelectedFile = tab.isSettings ? null : tab.path;
    return {
      openTabs: newTabs,
      activeTabId: tab.id,
      selectedFile: newSelectedFile,
      openTabDirtyMap: {
        ...state.openTabDirtyMap,
        [tab.path]: false,
      }
    };
  }),
  closeTab: (tabId) => set((state) => {
    const tab = state.openTabs.find(t => t.id === tabId);
    const closedPath = tab?.path;
    const newTabs = state.openTabs.filter(t => t.id !== tabId);
    let newActiveId = state.activeTabId;
    if (state.activeTabId === tabId) {
      const closedIndex = state.openTabs.findIndex(t => t.id === tabId);
      newActiveId = newTabs.length > 0
        ? newTabs[Math.min(closedIndex, newTabs.length - 1)].id
        : null;
    }
    const { [closedPath as string]: _, ...restDirtyMap } = state.openTabDirtyMap;
    return {
      openTabs: newTabs,
      activeTabId: newActiveId,
      selectedFile: newActiveId ? (newTabs.find(t => t.id === newActiveId)?.path || null) : null,
      openTabDirtyMap: restDirtyMap,
    };
  }),
  setActiveTab: (tabId) => set((state) => {
    const tab = state.openTabs.find(t => t.id === tabId);
    const newSelectedFile = tab?.isSettings ? null : (tab?.path || state.selectedFile);
    return {
      activeTabId: tabId,
      selectedFile: newSelectedFile
    };
  }),
  setOpenTabDirty: (path, isDirty) => set((state) => ({
    openTabDirtyMap: {
      ...state.openTabDirtyMap,
      [path]: isDirty,
    }
  })),
}),
    {
      name: 'inkuo-sidebar',
      partialize: (state) => ({
        workspacePath: state.workspacePath,
        openTabs: state.openTabs,
        activeTabId: state.activeTabId,
        selectedFile: state.selectedFile,
        openTabDirtyMap: state.openTabDirtyMap,
      }),
    }
  )
);

// ============================================================================
// AI Panel Store - Extended with Tool Calling Support
// ============================================================================

/** Diff change line */
export interface DiffChange {
  tag: 'delete' | 'insert' | 'equal';
  old_line: number | null;
  new_line: number | null;
  content: string;
}

/** Diff hunk for UI display */
export interface DiffHunk {
  id: string;
  old_start: number;
  old_lines: number;
  new_start: number;
  new_lines: number;
  changes: DiffChange[];
}

/** Diff summary for a file modification */
export interface DiffSummary {
  file_name: string;
  added_lines: number;
  deleted_lines: number;
  hunks: DiffHunk[];
}

export interface CurrentDiff {
  originalText: string;
  newText: string;
  hunks: DiffHunk[];
  summary: string;
}

/** Chat modes */
export type ChatMode = 'ask' | 'plan' | 'agent';

/** Message role including tool role for agent mode */
export type MessageRole = 'user' | 'assistant' | 'system' | 'tool';

/** Tool call as embedded in messages */
export interface MessageToolCall {
  id: string;
  name: string;
  arguments: Record<string, unknown>;
}

/** Tool result attached to a message */
export interface MessageToolResult {
  toolCallId: string;
  result: string;
  isError: boolean;
  duration?: number;
  diffSummary?: {
    file_name: string;
    added_lines: number;
    deleted_lines: number;
    hunks: {
      id: string;
      old_start: number;
      old_lines: number;
      new_start: number;
      new_lines: number;
      changes: {
        tag: 'delete' | 'insert' | 'equal';
        old_line: number | null;
        new_line: number | null;
        content: string;
      }[];
    }[];
  };
}

/** Output item types for interleaved rendering */
export type OutputItem =
  | { type: 'text'; content: string; isPendingMarkdown?: boolean }
  | { type: 'tool_call_start'; toolCallId: string; toolName: string; arguments: Record<string, unknown> }
  | { type: 'tool_result'; toolCallId: string; status: 'success' | 'error'; result: string; duration?: number; diffSummary?: MessageToolResult['diffSummary'] }
  | { type: 'tool_error'; toolCallId: string; error: string };

/** Chat message with full tool support */
export interface ChatMessage {
  id: string;
  role: MessageRole;
  timestamp: number;
  // Legacy content field — maintained for backward compatibility during migration,
  // prefer using outputItems for new content.
  content?: string;
  // Ordered list of output items for interleaved rendering (text + tool cards).
  // When this is non-empty, renderers should iterate through it instead of content.
  outputItems: OutputItem[];
  // For assistant messages with tool calls
  toolCalls?: MessageToolCall[];
  // Tool results associated with this assistant message (rendered after its content)
  toolResults?: MessageToolResult[];
  // For tool result messages
  toolCallId?: string;
  toolResult?: MessageToolResult;
  // Inline diff associated with this message
  diff?: CurrentDiff;
}

/** Active tool call being executed */
export interface ActiveToolCall {
  id: string;
  name: string;
  arguments: Record<string, unknown>;
  status: 'pending' | 'executing' | 'success' | 'error';
  result?: string;
  error?: string;
  startTime: number;
  duration?: number;
  diffSummary?: {
    file_name: string;
    added_lines: number;
    deleted_lines: number;
    hunks: {
      id: string;
      old_start: number;
      old_lines: number;
      new_start: number;
      new_lines: number;
      changes: {
        tag: 'delete' | 'insert' | 'equal';
        old_line: number | null;
        new_line: number | null;
        content: string;
      }[];
    }[];
  };
}

/** Chat session */
export interface ChatSession {
  id: string;
  title: string;
  createdAt: number;
  mode: ChatMode;
  messages: ChatMessage[];
  isStreaming: boolean;
  currentDiff: CurrentDiff | null;
  // Active tool calls for agent mode
  activeToolCalls: ActiveToolCall[];
  // Pending diff preview during streaming (for inline editing)
  pendingDiff: CurrentDiff | null;
}

/** AI Panel state */
interface AIPanelState {
  isOpen: boolean;
  activeTab: 'chat' | 'edit';

  sessions: ChatSession[];
  activeSessionId: string;

  setIsOpen: (open: boolean) => void;
  togglePanel: () => void;
  setActiveTab: (tab: 'chat' | 'edit') => void;

  createSession: () => string;
  deleteSession: (sessionId: string) => void;
  setActiveSession: (sessionId: string) => void;
  setSessionMode: (sessionId: string, mode: ChatMode) => void;

  addMessage: (sessionId: string, message: ChatMessage) => void;
  updateMessage: (sessionId: string, messageId: string, content: string) => void;
  appendMessageContent: (sessionId: string, messageId: string, content: string) => void;
  setIsStreaming: (sessionId: string, streaming: boolean) => void;
  clearMessages: (sessionId: string) => void;
  truncateMessagesAfter: (sessionId: string, messageId: string) => void;

  // Tool call management
  addToolCall: (sessionId: string, toolCall: ActiveToolCall) => void;
  updateToolCall: (sessionId: string, toolCallId: string, update: Partial<ActiveToolCall>) => void;
  removeToolCall: (sessionId: string, toolCallId: string) => void;
  clearToolCalls: (sessionId: string) => void;

  setCurrentDiff: (sessionId: string, diff: CurrentDiff | null) => void;
  setMessageDiff: (sessionId: string, messageId: string, diff: CurrentDiff | null) => void;
  setPendingDiff: (sessionId: string, diff: CurrentDiff | null) => void;
  acceptHunk: (sessionId: string, hunkId: string) => void;
  rejectHunk: (sessionId: string, hunkId: string) => void;
  acceptAllHunks: (sessionId: string) => void;
  rejectAllHunks: (sessionId: string) => void;

  // Helper methods for safe state updates
  getSession: (sessionId: string) => ChatSession | undefined;
  getMessage: (sessionId: string, messageId: string) => ChatMessage | undefined;
  updateSession: (sessionId: string, updater: (session: ChatSession) => ChatSession) => void;
  updateMessageOutput: (sessionId: string, messageId: string, outputItems: OutputItem[]) => void;
  addOutputToMessage: (sessionId: string, messageId: string, outputItem: OutputItem) => void;
  finishMessageStreaming: (sessionId: string, messageId: string, finalContent: string) => void;
  setErrorMessage: (sessionId: string, messageId: string, error: string) => void;
}

function createSessionTitle(index: number) {
  return `对话 ${index}`;
}

function createNewSession(index: number): ChatSession {
  const now = Date.now();
  return {
    id: crypto.randomUUID(),
    title: createSessionTitle(index),
    createdAt: now,
    mode: 'ask',
    messages: [],
    isStreaming: false,
    currentDiff: null,
    activeToolCalls: [],
    pendingDiff: null,
  };
}

export const useAIPanelStore = create<AIPanelState>()(
  persist(
    (set, get) => {
      const initialSession = createNewSession(1);

      return {
        isOpen: true,
        activeTab: 'chat',

        sessions: [initialSession],
        activeSessionId: initialSession.id,

        setIsOpen: (open) => set({ isOpen: open }),
        togglePanel: () => set((state) => ({ isOpen: !state.isOpen })),
        setActiveTab: (tab) => set({ activeTab: tab }),

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
            const remaining = state.sessions.filter((s) => s.id !== sessionId);
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
            sessions: state.sessions.map((s) => (s.id === sessionId ? { ...s, mode } : s)),
          })),

        addMessage: (sessionId, message) =>
          set((state) => ({
            sessions: state.sessions.map((s) =>
              s.id === sessionId ? { ...s, messages: [...s.messages, message] } : s
            ),
          })),

        updateMessage: (sessionId, messageId, content) =>
          set((state) => ({
            sessions: state.sessions.map((s) =>
              s.id === sessionId
                ? {
                    ...s,
                    messages: s.messages.map((m) => (m.id === messageId ? { ...m, content } : m)),
                  }
                : s
            ),
          })),

        appendMessageContent: (sessionId, messageId, content) =>
          set((state) => ({
            sessions: state.sessions.map((s) =>
              s.id === sessionId
                ? {
                    ...s,
                    messages: s.messages.map((m) =>
                      m.id === messageId
                        ? { ...m, content: m.content + content }
                        : m
                    ),
                  }
                : s
            ),
          })),

        setIsStreaming: (sessionId, streaming) =>
          set((state) => ({
            sessions: state.sessions.map((s) => (s.id === sessionId ? { ...s, isStreaming: streaming } : s)),
          })),

        clearMessages: (sessionId) =>
          set((state) => ({
            sessions: state.sessions.map((s) =>
              s.id === sessionId ? { ...s, messages: [], isStreaming: false, currentDiff: null, activeToolCalls: [], pendingDiff: null } : s
            ),
          })),

        truncateMessagesAfter: (sessionId, messageId) =>
          set((state) => ({
            sessions: state.sessions.map((s) =>
              s.id === sessionId
                ? {
                    ...s,
                    messages: s.messages.slice(0, s.messages.findIndex((m) => m.id === messageId) + 1),
                    isStreaming: false,
                    currentDiff: null,
                    activeToolCalls: [],
                    pendingDiff: null,
                  }
                : s
            ),
          })),

        // Tool call management
        addToolCall: (sessionId, toolCall) =>
          set((state) => ({
            sessions: state.sessions.map((s) =>
              s.id === sessionId
                ? { ...s, activeToolCalls: [...s.activeToolCalls, toolCall] }
                : s
            ),
          })),

        updateToolCall: (sessionId, toolCallId, update) =>
          set((state) => ({
            sessions: state.sessions.map((s) =>
              s.id === sessionId
                ? {
                    ...s,
                    activeToolCalls: s.activeToolCalls.map((tc) =>
                      tc.id === toolCallId ? { ...tc, ...update } : tc
                    ),
                  }
                : s
            ),
          })),

        removeToolCall: (sessionId, toolCallId) =>
          set((state) => ({
            sessions: state.sessions.map((s) =>
              s.id === sessionId
                ? { ...s, activeToolCalls: s.activeToolCalls.filter((tc) => tc.id !== toolCallId) }
                : s
            ),
          })),

        clearToolCalls: (sessionId) =>
          set((state) => ({
            sessions: state.sessions.map((s) =>
              s.id === sessionId
                ? { ...s, activeToolCalls: [] }
                : s
            ),
          })),

        setCurrentDiff: (sessionId, diff) =>
          set((state) => ({
            sessions: state.sessions.map((s) => (s.id === sessionId ? { ...s, currentDiff: diff } : s)),
          })),

        setMessageDiff: (sessionId, messageId, diff) =>
          set((state) => ({
            sessions: state.sessions.map((s) =>
              s.id === sessionId
                ? {
                    ...s,
                    messages: s.messages.map((m) =>
                      m.id === messageId ? { ...m, diff: diff ?? undefined } as ChatMessage : m
                    ),
                  }
                : s
            ),
          })),

        setPendingDiff: (sessionId, diff) =>
          set((state) => ({
            sessions: state.sessions.map((s) =>
              s.id === sessionId ? { ...s, pendingDiff: diff } : s
            ),
          })),

        acceptHunk: (sessionId, hunkId) =>
          set((state) => ({
            sessions: state.sessions.map((s) => {
              if (s.id !== sessionId) return s;

              // Check session-level diff
              if (s.currentDiff) {
                const newHunks = s.currentDiff.hunks.filter((h) => h.id !== hunkId);
                return {
                  ...s,
                  currentDiff: newHunks.length > 0 ? { ...s.currentDiff, hunks: newHunks } : null,
                };
              }

              // Check message-level diffs
              const updatedMessages = s.messages.map((m) => {
                if (!m.diff) return m;
                const newHunks = m.diff.hunks.filter((h) => h.id !== hunkId);
                const { diff: _, ...rest } = m;
                return { ...rest, diff: newHunks.length > 0 ? { ...m.diff, hunks: newHunks } : undefined } as ChatMessage;
              });

              return { ...s, messages: updatedMessages };
            }),
          })),

        rejectHunk: (sessionId, hunkId) =>
          set((state) => ({
            sessions: state.sessions.map((s) => {
              if (s.id !== sessionId) return s;

              // Check session-level diff
              if (s.currentDiff) {
                const newHunks = s.currentDiff.hunks.filter((h) => h.id !== hunkId);
                return {
                  ...s,
                  currentDiff: newHunks.length > 0 ? { ...s.currentDiff, hunks: newHunks } : null,
                };
              }

              // Check message-level diffs
              const updatedMessages = s.messages.map((m) => {
                if (!m.diff) return m;
                const newHunks = m.diff.hunks.filter((h) => h.id !== hunkId);
                const { diff: _, ...rest } = m;
                return { ...rest, diff: newHunks.length > 0 ? { ...m.diff, hunks: newHunks } : undefined } as ChatMessage;
              });

              return { ...s, messages: updatedMessages };
            }),
          })),

        acceptAllHunks: (sessionId) =>
          set((state) => ({
            sessions: state.sessions.map((s) => {
              if (s.id !== sessionId) return s;

              // Clear session-level diff
              if (s.currentDiff || s.pendingDiff) {
                return { ...s, currentDiff: null, pendingDiff: null };
              }

              // Clear message-level diffs
              const updatedMessages = s.messages.map((m) => {
                const { diff: _, ...rest } = m;
                return { ...rest, diff: undefined } as ChatMessage;
              });

              return { ...s, messages: updatedMessages };
            }),
          })),

        rejectAllHunks: (sessionId) =>
          set((state) => ({
            sessions: state.sessions.map((s) => {
              if (s.id !== sessionId) return s;

              // Clear session-level diff
              if (s.currentDiff || s.pendingDiff) {
                return { ...s, currentDiff: null, pendingDiff: null };
              }

              // Clear message-level diffs
              const updatedMessages = s.messages.map((m) => {
                const { diff: _, ...rest } = m;
                return { ...rest, diff: undefined } as ChatMessage;
              });

              return { ...s, messages: updatedMessages };
            }),
          })),

        // Helper methods for safe state updates
        getSession: (sessionId) => get().sessions.find((s) => s.id === sessionId),

        getMessage: (sessionId, messageId) => {
          const session = get().sessions.find((s) => s.id === sessionId);
          return session?.messages.find((m) => m.id === messageId);
        },

        updateSession: (sessionId, updater) =>
          set((state) => ({
            sessions: state.sessions.map((s) =>
              s.id === sessionId ? updater(s) : s
            ),
          })),

        updateMessageOutput: (sessionId, messageId, outputItems) =>
          set((state) => ({
            sessions: state.sessions.map((s) =>
              s.id === sessionId
                ? {
                    ...s,
                    messages: s.messages.map((m) =>
                      m.id === messageId ? { ...m, outputItems } : m
                    ),
                  }
                : s
            ),
          })),

        addOutputToMessage: (sessionId, messageId, outputItem) =>
          set((state) => ({
            sessions: state.sessions.map((s) =>
              s.id === sessionId
                ? {
                    ...s,
                    messages: s.messages.map((m) =>
                      m.id === messageId
                        ? { ...m, outputItems: [...m.outputItems, outputItem] }
                        : m
                    ),
                  }
                : s
            ),
          })),

        finishMessageStreaming: (sessionId, messageId, finalContent) =>
          set((state) => ({
            sessions: state.sessions.map((s) =>
              s.id === sessionId
                ? {
                    ...s,
                    isStreaming: false,
                    messages: s.messages.map((m) =>
                      m.id === messageId
                        ? {
                            ...m,
                            // Set legacy content field as fallback
                            content: m.content || finalContent,
                            // Clear isPendingMarkdown so markdown renders
                            outputItems: m.outputItems.length > 0
                              ? m.outputItems.map((item) =>
                                  item.type === 'text'
                                    ? { ...item, isPendingMarkdown: false }
                                    : item
                                )
                              : [{ type: 'text' as const, content: finalContent, isPendingMarkdown: false }],
                          }
                        : m
                    ),
                  }
                : s
            ),
          })),

        setErrorMessage: (sessionId, messageId, error) =>
          set((state) => ({
            sessions: state.sessions.map((s) =>
              s.id === sessionId
                ? {
                    ...s,
                    isStreaming: false,
                    messages: s.messages.map((m) =>
                      m.id === messageId ? { ...m, content: error } : m
                    ),
                  }
                : s
            ),
          })),
      };
    },
    {
      name: 'inkuo-aipanel',
      partialize: (state) => ({
        isOpen: state.isOpen,
        sessions: state.sessions.map((s) => ({
          ...s,
          isStreaming: false,
          currentDiff: null,
          activeToolCalls: [], // Don't persist active tool calls
        })),
        activeSessionId: state.activeSessionId,
      }),
    }
  )
);

// ============================================================================
// Settings Store
// ============================================================================

interface SettingsState {
  settings: Settings;
  isSettingsOpen: boolean;

  setSettings: (settings: Settings) => void;
  updateSetting: <K extends keyof Settings>(key: K, value: Settings[K]) => void;
  setIsSettingsOpen: (open: boolean) => void;

  // API Config management
  addApiConfig: (config?: Partial<APIConfig>) => string;
  updateApiConfig: (id: string, updates: Partial<APIConfig>) => void;
  removeApiConfig: (id: string) => void;
  setActiveApiConfig: (id: string) => void;
  getActiveApiConfig: () => APIConfig | null;
  setDefaultApiConfig: (id: string) => void;
}

const defaultAPIConfig = createDefaultAPIConfig();

const defaultSettings: Settings = {
  theme: 'cursor-dark',
  accent_color: '#7C5CFF',
  editor_font_size: 14,
  editor_font_family: 'JetBrains Mono, monospace',
  ai_provider: 'deepseek',
  ai_model: 'deepseek-chat',
  ai_api_key: null,
  ai_base_url: 'https://api.deepseek.com',
  ai_temperature: 0.7,
  ai_max_tokens: 4096,
  apiConfigs: [defaultAPIConfig],
  activeApiConfigId: defaultAPIConfig.id,
};

export const useSettingsStore = create<SettingsState>()(
  persist(
    (set, get) => ({
      settings: defaultSettings,
      isSettingsOpen: false,

      setSettings: (settings) => set({ settings }),
      updateSetting: (key, value) => set((state) => ({
        settings: { ...state.settings, [key]: value },
      })),
      setIsSettingsOpen: (open) => set({ isSettingsOpen: open }),

      // API Config management
      addApiConfig: (config) => {
        const newConfig: APIConfig = {
          id: crypto.randomUUID(),
          name: config?.name || 'New API',
          provider: config?.provider || 'openai',
          baseUrl: config?.baseUrl || 'https://api.openai.com/v1',
          apiKey: config?.apiKey || null,
          model: config?.model || 'gpt-4o-mini',
          isDefault: false,
          enabled: true,
          temperature: config?.temperature ?? 0.7,
          maxTokens: config?.maxTokens ?? 4096,
        };

        set((state) => ({
          settings: {
            ...state.settings,
            apiConfigs: [...state.settings.apiConfigs, newConfig],
          },
        }));

        return newConfig.id;
      },

      updateApiConfig: (id, updates) => set((state) => ({
        settings: {
          ...state.settings,
          apiConfigs: state.settings.apiConfigs.map((config) =>
            config.id === id ? { ...config, ...updates } : config
          ),
        },
      })),

      removeApiConfig: (id) => set((state) => {
        const remaining = state.settings.apiConfigs.filter((c) => c.id !== id);
        const newActiveId = state.settings.activeApiConfigId === id
          ? (remaining.length > 0 ? remaining[0].id : null)
          : state.settings.activeApiConfigId;

        // If we removed the default, make the first remaining one default
        const updatedConfigs = remaining.map((c, i) =>
          i === 0 && !remaining.some(r => r.isDefault) ? { ...c, isDefault: true } : c
        );

        return {
          settings: {
            ...state.settings,
            apiConfigs: remaining.length > 0 ? updatedConfigs : state.settings.apiConfigs,
            activeApiConfigId: newActiveId,
          },
        };
      }),

      setActiveApiConfig: (id) => set((state) => ({
        settings: {
          ...state.settings,
          activeApiConfigId: id,
        },
      })),

      getActiveApiConfig: () => {
        const state = get();
        const activeId = state.settings.activeApiConfigId;
        return state.settings.apiConfigs.find((c) => c.id === activeId) || null;
      },

      setDefaultApiConfig: (id) => set((state) => ({
        settings: {
          ...state.settings,
          apiConfigs: state.settings.apiConfigs.map((config) => ({
            ...config,
            isDefault: config.id === id,
          })),
        },
      })),
    }),
    {
      name: 'inkuo-settings',
      partialize: (state) => ({ settings: state.settings }),
      merge: (persistedState, currentState) => {
        const persisted = persistedState as Partial<SettingsState> | undefined;
        const persistedSettings = persisted?.settings as Partial<Settings> | undefined;

        const mergedSettings: Settings = {
          ...currentState.settings,
          ...persistedSettings,
        };

        const apiConfigs = Array.isArray((persistedSettings as any)?.apiConfigs)
          ? (persistedSettings as any).apiConfigs
          : currentState.settings.apiConfigs;

        mergedSettings.apiConfigs = apiConfigs.length > 0 ? apiConfigs : currentState.settings.apiConfigs;
        mergedSettings.activeApiConfigId =
          (persistedSettings as any)?.activeApiConfigId ?? mergedSettings.apiConfigs[0]?.id ?? null;

        if (!mergedSettings.apiConfigs.some((c) => c.isDefault)) {
          mergedSettings.apiConfigs = mergedSettings.apiConfigs.map((c, i) => ({
            ...c,
            isDefault: i === 0,
          }));
        }

        return {
          ...currentState,
          ...persisted,
          settings: mergedSettings,
        };
      },
    }
  )
);

// ============================================================================
// Cmd+K Modal Store
// ============================================================================

interface CmdKState {
  isOpen: boolean;
  scope: 'selection' | 'paragraph' | 'section' | 'document';
  instruction: string;
  isProcessing: boolean;

  open: () => void;
  close: () => void;
  setScope: (scope: 'selection' | 'paragraph' | 'section' | 'document') => void;
  setInstruction: (instruction: string) => void;
  setIsProcessing: (processing: boolean) => void;
  reset: () => void;
}

export const useCmdKStore = create<CmdKState>((set) => ({
  isOpen: false,
  scope: 'selection',
  instruction: '',
  isProcessing: false,

  open: () => set({ isOpen: true }),
  close: () => set({ isOpen: false, instruction: '', isProcessing: false }),
  setScope: (scope) => set({ scope }),
  setInstruction: (instruction) => set({ instruction }),
  setIsProcessing: (processing) => set({ isProcessing: processing }),
  reset: () => set({ scope: 'selection', instruction: '', isProcessing: false }),
}));

// ============================================================================
// Inline Completion Store
// ============================================================================

import type { CompletionItem } from '../types/inline-complete';

interface InlineCompleteState {
  // Feature toggle
  enabled: boolean;

  // Current completion state
  currentCompletion: CompletionItem | null;
  isLoading: boolean;
  error: string | null;

  // Position where completion was triggered (to detect cursor movement)
  triggerPosition: number | null;

  // Settings
  debounceMs: number;
  maxLines: number;

  // Actions
  setEnabled: (enabled: boolean) => void;
  setCompletion: (completion: CompletionItem | null, triggerPosition?: number) => void;
  setLoading: (loading: boolean) => void;
  setError: (error: string | null) => void;
  clearCompletion: () => void;
  updateSettings: (settings: Partial<Pick<InlineCompleteState, 'debounceMs' | 'maxLines'>>) => void;
}

export const useInlineCompleteStore = create<InlineCompleteState>()(
  persist(
    (set) => ({
      // Default state
      enabled: true,
      currentCompletion: null,
      isLoading: false,
      error: null,
      triggerPosition: null,
      debounceMs: 700,
      maxLines: 10,

      // Actions
      setEnabled: (enabled) => set({ enabled }),

      setCompletion: (completion, triggerPosition) => set({
        currentCompletion: completion,
        triggerPosition: completion ? (triggerPosition ?? null) : null,
        isLoading: false,
        error: null
      }),

      setLoading: (loading) => set({ isLoading: loading }),

      setError: (error) => set({ error, isLoading: false }),

      clearCompletion: () => set({
        currentCompletion: null,
        triggerPosition: null,
        isLoading: false,
        error: null
      }),

      updateSettings: (settings) => set((state) => ({
        ...state,
        ...settings
      })),
    }),
    {
      name: 'inkuo-inline-complete',
      partialize: (state) => ({
        enabled: state.enabled,
        debounceMs: state.debounceMs,
        maxLines: state.maxLines,
      }),
    }
  )
);
