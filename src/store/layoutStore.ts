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
  /** Set the absolute sidebar width. Also writes the value into the
   *  `--sidebar-width` CSS variable so the DOM reflows without
   *  requiring a React re-render — see `Layout.tsx` for the wider
   *  drag-perf context. */
  setSidebarWidth: (width: number) => void;
  /** @deprecated Resize through the drag handle's CSS-variable path
   *  (see `Layout.handleSidebarResize`). This action stays in the
   *  store API only for back-compat with any external callers (e.g.
   *  keyboard shortcuts). It still mutates Zustand state and writes
   *  the CSS variable, but going through it forces a `Layout`
   *  re-render. Prefer the imperative `useLayoutStore.setState` plus
   *  the CSS-var write that the drag flow uses. */
  resizeSidebar: (delta: number) => void;
  setAIPanelWidth: (width: number) => void;
  /** @deprecated See `resizeSidebar`. */
  resizeAIPanel: (delta: number) => void;
}

const SIDEBAR_MIN_WIDTH = 180;
const SIDEBAR_MAX_WIDTH = 400;
const AIPANEL_MIN_WIDTH = 300;
const AIPANEL_MAX_WIDTH = 600;

const clamp = (value: number, min: number, max: number) => Math.max(min, Math.min(max, value));
const applyResizeDelta = (width: number, delta: number, min: number, max: number) =>
  clamp(width + delta, min, max);

/** Side-effect helper: write a width into the matching CSS custom
 *  property on `:root`. Called whenever the store's width value
 *  changes, so DOM-driven consumers (the `Layout` panels) stay in
 *  sync with persisted state without subscribing to a selector. */
const syncSidebarCssVar = (width: number) => {
  if (typeof document === 'undefined') return;
  document.documentElement.style.setProperty('--sidebar-width', `${width}px`);
};
const syncAIPanelCssVar = (width: number) => {
  if (typeof document === 'undefined') return;
  document.documentElement.style.setProperty('--aipanel-width', `${width}px`);
};

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
      setSidebarWidth: (width) => {
        const clamped = clamp(width, SIDEBAR_MIN_WIDTH, SIDEBAR_MAX_WIDTH);
        set({ sidebarWidth: clamped });
        syncSidebarCssVar(clamped);
      },
      resizeSidebar: (delta) => set((state) => {
        const next = applyResizeDelta(state.sidebarWidth, delta, SIDEBAR_MIN_WIDTH, SIDEBAR_MAX_WIDTH);
        syncSidebarCssVar(next);
        return { sidebarWidth: next };
      }),
      setAIPanelWidth: (width) => {
        const clamped = clamp(width, AIPANEL_MIN_WIDTH, AIPANEL_MAX_WIDTH);
        set({ aipanelWidth: clamped });
        syncAIPanelCssVar(clamped);
      },
      resizeAIPanel: (delta) => set((state) => {
        const next = applyResizeDelta(state.aipanelWidth, -delta, AIPANEL_MIN_WIDTH, AIPANEL_MAX_WIDTH);
        syncAIPanelCssVar(next);
        return { aipanelWidth: next };
      }),
    }),
    {
      name: 'inkuo-layout',
      partialize: (state) => ({
        activeView: state.activeView,
        isSidebarVisible: state.isSidebarVisible,
        sidebarWidth: state.sidebarWidth,
        aipanelWidth: state.aipanelWidth,
      }),
      // After zustand rehydrates from disk, push the persisted widths
      // into the CSS variables so the very first paint of `<Layout>`
      // already has the user's chosen panel sizes — without waiting
      // for a React effect to run.
      onRehydrateStorage: () => (state) => {
        if (!state) return;
        syncSidebarCssVar(state.sidebarWidth);
        syncAIPanelCssVar(state.aipanelWidth);
      },
    },
  ),
);
