import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import type { FileEntry } from '../types';

export interface OpenTab {
  id: string;
  path: string;
  name: string;
  isDirty: boolean;
  isSettings?: boolean;
}

export const SETTINGS_TAB_ID = '__settings__';

interface SidebarState {
  workspacePath: string | null;
  files: FileEntry[];
  expandedDirs: Set<string>;
  selectedFile: string | null;
  isLoading: boolean;
  openTabs: OpenTab[];
  activeTabId: string | null;
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
