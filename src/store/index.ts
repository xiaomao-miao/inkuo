import { create } from 'zustand';
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

export const useSidebarStore = create<SidebarState>((set) => ({
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
}));

// AI Panel store
export interface CurrentDiff {
  originalText: string;
  newText: string;
  hunks: DiffHunk[];
  summary: string;
}

interface AIPanelState {
  isOpen: boolean;
  activeTab: 'chat' | 'edit';
  messages: ChatMessage[];
  isStreaming: boolean;
  currentDiff: CurrentDiff | null;
  
  setIsOpen: (open: boolean) => void;
  togglePanel: () => void;
  setActiveTab: (tab: 'chat' | 'edit') => void;
  addMessage: (message: ChatMessage) => void;
  updateMessage: (id: string, content: string) => void;
  setIsStreaming: (streaming: boolean) => void;
  clearMessages: () => void;
  setCurrentDiff: (diff: CurrentDiff | null) => void;
  acceptHunk: (hunkId: string) => void;
  rejectHunk: (hunkId: string) => void;
  acceptAllHunks: () => void;
  rejectAllHunks: () => void;
}

export interface ChatMessage {
  id: string;
  role: 'user' | 'assistant';
  content: string;
  timestamp: number;
}

export const useAIPanelStore = create<AIPanelState>((set) => ({
  isOpen: true,
  activeTab: 'chat',
  messages: [],
  isStreaming: false,
  currentDiff: null,
  
  setIsOpen: (open) => set({ isOpen: open }),
  togglePanel: () => set((state) => ({ isOpen: !state.isOpen })),
  setActiveTab: (tab) => set({ activeTab: tab }),
  addMessage: (message) => set((state) => ({ 
    messages: [...state.messages, message] 
  })),
  updateMessage: (id, content) => set((state) => ({
    messages: state.messages.map(m => 
      m.id === id ? { ...m, content } : m
    ),
  })),
  setIsStreaming: (streaming) => set({ isStreaming: streaming }),
  clearMessages: () => set({ messages: [], isStreaming: false }),
  setCurrentDiff: (diff) => set({ currentDiff: diff }),
  acceptHunk: (hunkId) => set((state) => {
    if (!state.currentDiff) return state;
    const newHunks = state.currentDiff.hunks.filter(h => h.id !== hunkId);
    return { 
      currentDiff: newHunks.length > 0 
        ? { ...state.currentDiff, hunks: newHunks }
        : null 
    };
  }),
  rejectHunk: (hunkId) => set((state) => {
    if (!state.currentDiff) return state;
    const newHunks = state.currentDiff.hunks.filter(h => h.id !== hunkId);
    return { 
      currentDiff: newHunks.length > 0 
        ? { ...state.currentDiff, hunks: newHunks }
        : null 
    };
  }),
  acceptAllHunks: () => set({ currentDiff: null }),
  rejectAllHunks: () => set({ currentDiff: null }),
}));

// Settings store
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

export const useSettingsStore = create<SettingsState>((set) => ({
  settings: defaultSettings,
  isSettingsOpen: false,
  
  setSettings: (settings) => set({ settings }),
  updateSetting: (key, value) => set((state) => ({
    settings: { ...state.settings, [key]: value },
  })),
  setIsSettingsOpen: (open) => set({ isSettingsOpen: open }),
}));

// Cmd+K modal store
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
