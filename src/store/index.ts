import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import type { Document, Settings, DiffHunk, FileEntry } from '../types';

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
  setDiffHunks: (path: string, hunks: DiffHunk[]) => void;
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
}

export const useEditorStore = create<EditorState>((set) => ({
  documentContents: {},

  setDocumentContent: (path, doc, content) => set((state) => ({
    documentContents: {
      ...state.documentContents,
      [path]: {
        document: doc,
        content: content,
        isDirty: false,
        selection: null,
        diffHunks: [],
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
          diffHunks: [],
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
          diffHunks: [],
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
          diffHunks: [],
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
}));

// Sidebar store
interface SidebarState {
  workspacePath: string | null;
  files: FileEntry[];
  expandedDirs: Set<string>;
  selectedFile: string | null;
  isLoading: boolean;
  openTabs: OpenTab[];
  activeTabId: string | null;

  setWorkspacePath: (path: string) => void;
  setFiles: (files: FileEntry[]) => void;
  toggleDir: (path: string) => void;
  setSelectedFile: (path: string | null) => void;
  setIsLoading: (loading: boolean) => void;
  openTab: (tab: OpenTab) => void;
  closeTab: (tabId: string) => void;
  setActiveTab: (tabId: string) => void;
}

export interface OpenTab {
  id: string;
  path: string;
  name: string;
  isDirty: boolean;
}

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

  setWorkspacePath: (path) => set({ workspacePath: path }),
  setFiles: (files) => set({ files }),
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
    return { openTabs: newTabs, activeTabId: tab.id, selectedFile: tab.path };
  }),
  closeTab: (tabId) => set((state) => {
    const newTabs = state.openTabs.filter(t => t.id !== tabId);
    let newActiveId = state.activeTabId;
    if (state.activeTabId === tabId) {
      const closedIndex = state.openTabs.findIndex(t => t.id === tabId);
      if (newTabs.length > 0) {
        newActiveId = newTabs[Math.min(closedIndex, newTabs.length - 1)].id;
      } else {
        newActiveId = null;
      }
    }
    return {
      openTabs: newTabs,
      activeTabId: newActiveId,
      selectedFile: newActiveId ? (newTabs.find(t => t.id === newActiveId)?.path || null) : null
    };
  }),
  setActiveTab: (tabId) => set((state) => {
    const tab = state.openTabs.find(t => t.id === tabId);
    return {
      activeTabId: tabId,
      selectedFile: tab?.path || state.selectedFile
    };
  }),
}),
    {
      name: 'inkuo-sidebar',
      partialize: (state) => ({
        workspacePath: state.workspacePath,
        openTabs: state.openTabs,
        activeTabId: state.activeTabId,
        selectedFile: state.selectedFile,
      }),
    }
  )
);

// ============================================================================
// AI Panel Store - Extended with Tool Calling Support
// ============================================================================

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
}

/** Chat message with full tool support */
export interface ChatMessage {
  id: string;
  role: MessageRole;
  content: string;
  timestamp: number;
  // For assistant messages with tool calls
  toolCalls?: MessageToolCall[];
  // For tool result messages
  toolCallId?: string;
  toolResult?: MessageToolResult;
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

  // Tool call management
  addToolCall: (sessionId: string, toolCall: ActiveToolCall) => void;
  updateToolCall: (sessionId: string, toolCallId: string, update: Partial<ActiveToolCall>) => void;
  removeToolCall: (sessionId: string, toolCallId: string) => void;
  clearToolCalls: (sessionId: string) => void;

  setCurrentDiff: (sessionId: string, diff: CurrentDiff | null) => void;
  acceptHunk: (sessionId: string, hunkId: string) => void;
  rejectHunk: (sessionId: string, hunkId: string) => void;
  acceptAllHunks: (sessionId: string) => void;
  rejectAllHunks: (sessionId: string) => void;
}

function createSessionTitle(index: number) {
  return `对话 ${index}`;
}

function createNewSession(index: number): ChatSession {
  const now = Date.now();
  return {
    id: now.toString(),
    title: createSessionTitle(index),
    createdAt: now,
    mode: 'ask',
    messages: [],
    isStreaming: false,
    currentDiff: null,
    activeToolCalls: [],
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
              s.id === sessionId ? { ...s, messages: [], isStreaming: false, currentDiff: null, activeToolCalls: [] } : s
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

        acceptHunk: (sessionId, hunkId) =>
          set((state) => ({
            sessions: state.sessions.map((s) => {
              if (s.id !== sessionId) return s;
              if (!s.currentDiff) return s;
              const newHunks = s.currentDiff.hunks.filter((h) => h.id !== hunkId);
              return {
                ...s,
                currentDiff: newHunks.length > 0 ? { ...s.currentDiff, hunks: newHunks } : null,
              };
            }),
          })),

        rejectHunk: (sessionId, hunkId) =>
          set((state) => ({
            sessions: state.sessions.map((s) => {
              if (s.id !== sessionId) return s;
              if (!s.currentDiff) return s;
              const newHunks = s.currentDiff.hunks.filter((h) => h.id !== hunkId);
              return {
                ...s,
                currentDiff: newHunks.length > 0 ? { ...s.currentDiff, hunks: newHunks } : null,
              };
            }),
          })),

        acceptAllHunks: (sessionId) =>
          set((state) => ({
            sessions: state.sessions.map((s) => (s.id === sessionId ? { ...s, currentDiff: null } : s)),
          })),

        rejectAllHunks: (sessionId) =>
          set((state) => ({
            sessions: state.sessions.map((s) => (s.id === sessionId ? { ...s, currentDiff: null } : s)),
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
}

const defaultSettings: Settings = {
  theme: 'cursor-dark',
  accent_color: '#7C5CFF',
  editor_font_size: 14,
  editor_font_family: 'JetBrains Mono, monospace',
  ai_provider: 'openai',
  ai_model: 'deepseek-chat',
  ai_api_key: null,
  ai_base_url: 'https://api.deepseek.com',
};

export const useSettingsStore = create<SettingsState>()(
  persist(
    (set) => ({
      settings: defaultSettings,
      isSettingsOpen: false,

      setSettings: (settings) => set({ settings }),
      updateSetting: (key, value) => set((state) => ({
        settings: { ...state.settings, [key]: value },
      })),
      setIsSettingsOpen: (open) => set({ isSettingsOpen: open }),
    }),
    {
      name: 'inkuo-settings',
      partialize: (state) => ({ settings: state.settings }),
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
