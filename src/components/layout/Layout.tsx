import React, { useState } from 'react';
import { TitleBar } from '../titlebar/TitleBar';
import { ActivityBar, type ViewType } from '../activitybar/ActivityBar';
import { Sidebar } from '../sidebar/Sidebar';
import { Editor } from '../editor/Editor';
import { TabBar } from '../editor/TabBar';
import { AIPanel } from '../aipanel/AIPanel';
import { useAIPanelStore } from '../../store';
import styles from './Layout.module.css';

interface LayoutProps {
  onOpenSettings: () => void;
}

export const Layout: React.FC<LayoutProps> = ({ onOpenSettings }) => {
  const { isOpen: isAIPanelOpen } = useAIPanelStore();
  const [activeView, setActiveView] = useState<ViewType>('files');
  const [isSidebarVisible, setIsSidebarVisible] = useState(true);

  const handleToggleSidebar = () => {
    setIsSidebarVisible(!isSidebarVisible);
  };

  return (
    <div className={styles.layout}>
      <TitleBar onOpenSettings={onOpenSettings} />
      <div className={styles.body}>
        <ActivityBar 
          activeView={activeView}
          onViewChange={setActiveView}
          onToggleSidebar={handleToggleSidebar}
        />
        {isSidebarVisible && activeView === 'files' && <Sidebar />}
        <main className={styles.main}>
          <TabBar />
          <Editor />
        </main>
        {isAIPanelOpen && <AIPanel />}
      </div>
    </div>
  );
};
