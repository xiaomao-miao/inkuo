import React, { useCallback } from 'react';
import { TitleBar } from '../titlebar/TitleBar';
import { ActivityBar } from '../activitybar/ActivityBar';
import { Sidebar } from '../sidebar/Sidebar';
import { ResizableHandle } from '../resizable';
import { Editor } from '../editor/Editor';
import { TabBar } from '../editor/TabBar';
import { AIPanel } from '../aipanel/AIPanel';
import { useGlobalKeydown } from '../../hooks/useGlobalKeydown';
import { useAIPanelStore, useLayoutStore } from '../../store';
import styles from './Layout.module.css';

export const Layout: React.FC = () => {
  const { isOpen: isAIPanelOpen, togglePanel } = useAIPanelStore();
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

  return (
    <div className={styles.layout}>
      <TitleBar />
      <div className={styles.body}>
        <ActivityBar 
          activeView={activeView}
          onViewChange={handleViewChange}
          onToggleSidebar={handleToggleSidebar}
        />
        
        {/* Left Sidebar — always mounted so state persists across view switches */}
        {isSidebarVisible && (
          <>
            <div className={styles.sidebar} style={{ width: sidebarWidth }}>
              {activeView === 'files' ? (
                <Sidebar />
              ) : (
                <div className={styles.placeholder}>
                  <p>
                    {activeView === 'search' && '搜索'}
                    {activeView === 'git' && '源代码管理'}
                    {activeView === 'extensions' && '扩展'}
                  </p>
                  <span>功能开发中...</span>
                </div>
              )}
            </div>
            <ResizableHandle direction="horizontal" onResize={handleSidebarResize} />
          </>
        )}
        
        {/* Main Editor Area */}
        <main className={styles.main}>
          <TabBar />
          <Editor />
        </main>
        
        {/* Right Sidebar (AI Panel) */}
        {isAIPanelOpen && (
          <>
            <ResizableHandle direction="horizontal" onResize={handleAIPanelResize} />
            <div className={styles.aipanel} style={{ width: aipanelWidth }}>
              <AIPanel />
            </div>
          </>
        )}
      </div>
    </div>
  );
};
