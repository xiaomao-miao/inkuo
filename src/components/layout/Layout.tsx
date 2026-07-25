import { useCallback, useEffect, useRef } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { TitleBar } from '../titlebar/TitleBar';
import { ActivityBar } from '../activitybar/ActivityBar';
import { Sidebar } from '../sidebar/Sidebar';
import { KnowledgeView } from '../sidebar/KnowledgeView';
import { ConfirmDialog } from '../sidebar/ConfirmDialog';
import { NotificationStack } from '../sidebar/NotificationStack';
import { SnapshotPanel } from '../snapshots/SnapshotPanel';
import { ResizableHandle } from '../resizable';
import { Editor } from '../editor/Editor';
import { TabBar } from '../editor/TabBar';
import { AIPanel } from '../aipanel/AIPanel';
import { useGlobalKeydown } from '../../hooks/useGlobalKeydown';
import { useAIPanelStore, useLayoutStore, useNotificationStore } from '../../store';
import styles from './Layout.module.css';

const DISABLED_VIEW_LABELS = {
  search: '搜索',
  git: '源代码管理',
  extensions: '扩展',
} as const;

// Bounds MUST stay in sync with `layoutStore.ts` — clamping happens in
// `applyPanelDelta` below so the CSS variable never escapes the [min, max]
// range regardless of how the caller drives it.
const SIDEBAR_MIN_WIDTH = 180;
const SIDEBAR_MAX_WIDTH = 400;
const AIPANEL_MIN_WIDTH = 300;
const AIPANEL_MAX_WIDTH = 600;
const SIDEBAR_VAR = '--sidebar-width';
const AIPANEL_VAR = '--aipanel-width';

const clamp = (value: number, min: number, max: number) => Math.max(min, Math.min(max, value));

/** Read the current numeric value of a CSS custom property from `:root`.
 *  Falls back to the supplied default if the property hasn't been set
 *  (e.g. on first paint before the layout store hydrates). */
const readCssPx = (varName: string, fallback: number): number => {
  if (typeof window === 'undefined') return fallback;
  const raw = getComputedStyle(document.documentElement).getPropertyValue(varName).trim();
  if (!raw) return fallback;
  const parsed = parseFloat(raw);
  return Number.isFinite(parsed) ? parsed : fallback;
};

/** Apply a clamped delta to a CSS custom property. The function never
 *  touches React state — that's the whole point of the rAF-driven
 *  resize handle: the DOM reflows but the editor tree stays still. */
const applyPanelDelta = (
  varName: string,
  delta: number,
  min: number,
  max: number,
  fallback: number,
  direction: 'ltr' | 'rtl',
) => {
  const current = readCssPx(varName, fallback);
  const next = clamp(direction === 'ltr' ? current + delta : current - delta, min, max);
  if (next === current) return next;
  document.documentElement.style.setProperty(varName, `${next}px`);
  return next;
};

export const Layout = () => {
  const { isOpen: isAIPanelOpen, togglePanel } = useAIPanelStore();
  const clearNotifications = useNotificationStore((state) => state.clearNotifications);
  // NOTE: We deliberately do NOT subscribe to `sidebarWidth` / `aipanelWidth`
  // here. The widths are stored in CSS variables and written directly
  // during a drag, so re-rendering this component on every pointer tick
  // would defeat the whole optimization. We only need the stored values
  // to (a) hydrate the CSS variables on mount, and (b) commit the final
  // value back to the store on drag-end. Both flows go through
  // `useLayoutStore.getState()` rather than the subscribed selector.
  const {
    activeView,
    isSidebarVisible,
    setActiveView,
    toggleSidebar,
  } = useLayoutStore();

  const handleToggleSidebar = useCallback(() => {
    toggleSidebar();
  }, [toggleSidebar]);

  const handleViewChange = useCallback((view: Parameters<typeof setActiveView>[0]) => {
    setActiveView(view);
  }, [setActiveView]);

  // Cached hydrated widths. Set on mount from `useLayoutStore.getState()` so
  // we know the user's persisted values before the first drag. Used as
  // fallbacks for `applyPanelDelta` if a CSS read returns a stale or empty
  // string (e.g. during SSR or before the variable is set on `:root`).
  const initialSidebarWidthRef = useRef<number>(260);
  const initialAIPanelWidthRef = useRef<number>(380);

  // The store writes the CSS variable itself whenever `sidebarWidth` /
  // `aipanelWidth` change (see `layoutStore.ts`), so we don't need a
  // subscription here. The mount-time effect below just covers the
  // window between first render and `persist`'s rehydration callback
  // for users on a hot-loaded tab where rehydration already happened
  // before `Layout` mounted — in that case the CSS variables may still
  // be at their default (260px / 380px) from `global.css`. We re-write
  // them once on mount using `getState()` to avoid that flicker.
  useEffect(() => {
    const { sidebarWidth, aipanelWidth } = useLayoutStore.getState();
    document.documentElement.style.setProperty(SIDEBAR_VAR, `${sidebarWidth}px`);
    document.documentElement.style.setProperty(AIPANEL_VAR, `${aipanelWidth}px`);
    initialSidebarWidthRef.current = sidebarWidth;
    initialAIPanelWidthRef.current = aipanelWidth;
  }, []);

  // Per-drag session baseline (only consulted at pointer-down to capture
  // the panel's *current* width). Cheap to keep — no re-renders.
  const dragBaselineSidebarRef = useRef<number>(260);
  const dragBaselineAIPanelRef = useRef<number>(380);

  const handleSidebarResizeStart = useCallback(() => {
    dragBaselineSidebarRef.current = readCssPx(SIDEBAR_VAR, initialSidebarWidthRef.current);
  }, []);

  const handleSidebarResize = useCallback((delta: number) => {
    applyPanelDelta(
      SIDEBAR_VAR,
      delta,
      SIDEBAR_MIN_WIDTH,
      SIDEBAR_MAX_WIDTH,
      initialSidebarWidthRef.current,
      'ltr',
    );
  }, []);

  const handleSidebarResizeEnd = useCallback(() => {
    // Flush the final CSS-driven value into the store. We use the
    // imperative setter instead of `resizeSidebar` because the store
    // tracks absolute width, not delta — and we don't have a delta
    // here, just the final committed value. The store writes the CSS
    // variable back as well, so this is the single source of truth.
    const final = readCssPx(SIDEBAR_VAR, initialSidebarWidthRef.current);
    useLayoutStore.setState({ sidebarWidth: final });
  }, []);

  const handleAIPanelResizeStart = useCallback(() => {
    dragBaselineAIPanelRef.current = readCssPx(AIPANEL_VAR, initialAIPanelWidthRef.current);
  }, []);

  const handleAIPanelResize = useCallback((delta: number) => {
    applyPanelDelta(
      AIPANEL_VAR,
      delta,
      AIPANEL_MIN_WIDTH,
      AIPANEL_MAX_WIDTH,
      initialAIPanelWidthRef.current,
      // The AI panel lives on the right edge; dragging the handle to the
      // left should *grow* the panel, so we flip the delta sign.
      'rtl',
    );
  }, []);

  const handleAIPanelResizeEnd = useCallback(() => {
    const final = readCssPx(AIPANEL_VAR, initialAIPanelWidthRef.current);
    useLayoutStore.setState({ aipanelWidth: final });
  }, []);

  const handleGlobalKeyDown = useCallback((event: KeyboardEvent) => {
    if (event.ctrlKey && event.shiftKey && event.key.toLowerCase() === 'l') {
      event.preventDefault();
      togglePanel();
      return;
    }
    if (event.ctrlKey && !event.shiftKey && !event.altKey && !event.metaKey && event.key.toLowerCase() === 'b') {
      event.preventDefault();
      toggleSidebar();
      return;
    }
    if (event.key === 'F11') {
      event.preventDefault();
      const win = getCurrentWindow();
      void win.isFullscreen().then((isFull) => {
        void win.setFullscreen(!isFull);
      });
    }
  }, [togglePanel, toggleSidebar]);

  useGlobalKeydown(handleGlobalKeyDown);

  useEffect(() => {
    clearNotifications();
  }, [clearNotifications]);

  return (
    <div className={styles.layout}>
      <TitleBar />
      <div className={styles.body}>
        <ActivityBar
          activeView={activeView}
          onViewChange={handleViewChange}
          onToggleSidebar={handleToggleSidebar}
        />

        {isSidebarVisible && (
          <>
            <div className={styles.sidebar}>
              {activeView === 'files' ? (
                <Sidebar />
              ) : activeView === 'knowledge' ? (
                <KnowledgeView />
              ) : activeView === 'snapshots' ? (
                <SnapshotPanel />
              ) : (
                <div className={styles.placeholder} aria-live="polite">
                  <p>{DISABLED_VIEW_LABELS[activeView as keyof typeof DISABLED_VIEW_LABELS]}</p>
                  <span>该视图暂未开放，当前以禁用状态展示。</span>
                </div>
              )}
            </div>
            <ResizableHandle
              direction="horizontal"
              onResize={handleSidebarResize}
              onResizeStart={handleSidebarResizeStart}
              onResizeEnd={handleSidebarResizeEnd}
            />
          </>
        )}

        <main className={styles.main}>
          <TabBar />
          <Editor />
        </main>

        {isAIPanelOpen && (
          <>
            <ResizableHandle
              direction="horizontal"
              onResize={handleAIPanelResize}
              onResizeStart={handleAIPanelResizeStart}
              onResizeEnd={handleAIPanelResizeEnd}
            />
            <div className={styles.aipanel}>
              <AIPanel />
            </div>
          </>
        )}
      </div>

      {/* Global dialog portals — must be rendered outside the sidebar
          tree so they're available from any view (files, snapshots, etc.). */}
      <ConfirmDialog />
      <NotificationStack />
    </div>
  );
};
