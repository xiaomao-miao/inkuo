import React, { useState, useCallback } from 'react';
import { TitleBar } from '../titlebar/TitleBar';
import { ActivityBar, type ViewType } from '../activitybar/ActivityBar';
import { Sidebar } from '../sidebar/Sidebar';
import { SettingsPanel } from '../settings';
import { ResizableHandle } from '../resizable';
import { Editor } from '../editor/Editor';
import { TabBar } from '../editor/TabBar';
import { AIPanel } from '../aipanel/AIPanel';
import { useAIPanelStore } from '../../store';
import styles from './Layout.module.css';

export const Layout: React.FC = () => {
  const { isOpen: isAIPanelOpen } = useAIPanelStore();
  const [activeView, setActiveView] = useState<ViewType>('files');
  const [isSidebarVisible, setIsSidebarVisible] = useState(true);
  
  // Sidebar width state
  const [sidebarWidth, setSidebarWidth] = useState(260);
  
  // AIPanel width state
  const [aipanelWidth, setAipanelWidth] = useState(380);

  const handleToggleSidebar = () => {
    setIsSidebarVisible(!isSidebarVisible);
  };

  const handleViewChange = (view: ViewType) => {
    setActiveView(view);
    if (!isSidebarVisible) {
      setIsSidebarVisible(true);
    }
  };

  // Handle sidebar resize
  const handleSidebarResize = useCallback((delta: number) => {
    setSidebarWidth(prev => Math.max(180, Math.min(400, prev + delta)));
  }, []);

  // Handle AIPanel resize
  const handleAIPanelResize = useCallback((delta: number) => {
    setAipanelWidth(prev => Math.max(300, Math.min(600, prev - delta)));
  }, []);

  return (
    <div className={styles.layout}>
      <TitleBar />
      <div className={styles.body}>
        <ActivityBar 
          activeView={activeView}
          onViewChange={handleViewChange}
          onToggleSidebar={handleToggleSidebar}
        />
        
        {/* Left Sidebar */}
        {isSidebarVisible && (
          <>
            <div className={styles.sidebar} style={{ width: sidebarWidth }}>
              {activeView === 'files' && <Sidebar />}
              {activeView === 'settings' && <SettingsPanel />}
              {activeView === 'search' && (
                <div className={styles.placeholder}>
                  <p>搜索</p>
                  <span>功能开发中...</span>
                </div>
              )}
              {activeView === 'git' && (
                <div className={styles.placeholder}>
                  <p>源代码管理</p>
                  <span>功能开发中...</span>
                </div>
              )}
              {activeView === 'extensions' && (
                <div className={styles.placeholder}>
                  <p>扩展</p>
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
