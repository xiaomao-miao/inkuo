import { useCallback, useEffect } from 'react';
import { TitleBar } from '../titlebar/TitleBar';
import { ActivityBar } from '../activitybar/ActivityBar';
import { Sidebar } from '../sidebar/Sidebar';
import { KnowledgeView } from '../sidebar/KnowledgeView';
import { ConfirmDialog } from '../sidebar/ConfirmDialog';
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

export const Layout = () => {
  const { isOpen: isAIPanelOpen, togglePanel } = useAIPanelStore();
  const clearNotifications = useNotificationStore((state) => state.clearNotifications);
  const {
    activeView,
    isSidebarVisible,
    sidebarWidth,
    aipanelWidth,
    setActiveView,
    toggleSidebar,
    resizeSidebar,
    resizeAIPanel,
  } = useLayoutStore();

  const handleToggleSidebar = useCallback(() => {
    toggleSidebar();
  }, [toggleSidebar]);

  const handleViewChange = useCallback((view: Parameters<typeof setActiveView>[0]) => {
    setActiveView(view);
  }, [setActiveView]);

  const handleSidebarResize = useCallback((delta: number) => {
    resizeSidebar(delta);
  }, [resizeSidebar]);

  const handleAIPanelResize = useCallback((delta: number) => {
    resizeAIPanel(delta);
  }, [resizeAIPanel]);

  const handleGlobalKeyDown = useCallback((event: KeyboardEvent) => {
    if (event.ctrlKey && event.shiftKey && event.key.toLowerCase() === 'l') {
      event.preventDefault();
      togglePanel();
    }
  }, [togglePanel]);

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
            <div className={styles.sidebar} style={{ width: sidebarWidth }}>
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
            <ResizableHandle direction="horizontal" onResize={handleSidebarResize} />
          </>
        )}

        <main className={styles.main}>
          <TabBar />
          <Editor />
        </main>

        {isAIPanelOpen && (
          <>
            <ResizableHandle direction="horizontal" onResize={handleAIPanelResize} />
            <div className={styles.aipanel} style={{ width: aipanelWidth }}>
              <AIPanel />
            </div>
          </>
        )}
      </div>

      {/* Global dialog portals — must be rendered outside the sidebar
          tree so they're available from any view (files, snapshots, etc.). */}
      <ConfirmDialog />
    </div>
  );
};
