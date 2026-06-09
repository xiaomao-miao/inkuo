import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import type { ViewType } from '../components/activitybar/ActivityBar';

interface LayoutState {
  activeView: ViewType;
  isSidebarVisible: boolean;
  sidebarWidth: number;
  aipanelWidth: number;

  setActiveView: (view: ViewType) => void;
  toggleSidebar: () => void;
  showSidebar: () => void;
  setSidebarWidth: (width: number) => void;
  resizeSidebar: (delta: number) => void;
  setAIPanelWidth: (width: number) => void;
  resizeAIPanel: (delta: number) => void;
}

const SIDEBAR_MIN_WIDTH = 180;
const SIDEBAR_MAX_WIDTH = 400;
const AIPANEL_MIN_WIDTH = 300;
const AIPANEL_MAX_WIDTH = 600;

const clamp = (value: number, min: number, max: number) => Math.max(min, Math.min(max, value));
const applyResizeDelta = (width: number, delta: number, min: number, max: number) =>
  clamp(width + delta, min, max);

export const useLayoutStore = create<LayoutState>()(
  persist(
    (set) => ({
      activeView: 'files',
      isSidebarVisible: true,
      sidebarWidth: 260,
      aipanelWidth: 380,

      setActiveView: (view) => set({ activeView: view, isSidebarVisible: true }),
      toggleSidebar: () => set((state) => ({ isSidebarVisible: !state.isSidebarVisible })),
      showSidebar: () => set({ isSidebarVisible: true }),
      setSidebarWidth: (width) => set({ sidebarWidth: clamp(width, SIDEBAR_MIN_WIDTH, SIDEBAR_MAX_WIDTH) }),
      resizeSidebar: (delta) => set((state) => ({
        sidebarWidth: applyResizeDelta(state.sidebarWidth, delta, SIDEBAR_MIN_WIDTH, SIDEBAR_MAX_WIDTH),
      })),
      setAIPanelWidth: (width) => set({ aipanelWidth: clamp(width, AIPANEL_MIN_WIDTH, AIPANEL_MAX_WIDTH) }),
      resizeAIPanel: (delta) => set((state) => ({
        aipanelWidth: applyResizeDelta(state.aipanelWidth, -delta, AIPANEL_MIN_WIDTH, AIPANEL_MAX_WIDTH),
      })),
    }),
    {
      name: 'inkuo-layout',
      partialize: (state) => ({
        activeView: state.activeView,
        isSidebarVisible: state.isSidebarVisible,
        sidebarWidth: state.sidebarWidth,
        aipanelWidth: state.aipanelWidth,
      }),
    },
  ),
);
