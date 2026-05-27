import React from 'react';
import { FileText, File, X, Circle, Settings } from 'lucide-react';
import { useSidebarStore, useEditorStore } from '../../store';
import { SETTINGS_TAB_ID } from '../../store';
import type { OpenTab } from '../../store';
import styles from './TabBar.module.css';

export const TabBar: React.FC = () => {
  const { openTabs, activeTabId, setActiveTab, closeTab } = useSidebarStore();
  const { documentContents } = useEditorStore();

  const handleTabClick = (tab: OpenTab) => {
    setActiveTab(tab.id);
  };

  const handleCloseTab = (e: React.MouseEvent, tabId: string) => {
    e.stopPropagation();
    closeTab(tabId);
  };

  if (openTabs.length === 0) {
    return null;
  }

  const getFileIcon = (tab: OpenTab) => {
    if (tab.isSettings) {
      return <Settings size={14} />;
    }
    const isMarkdown = tab.name.endsWith('.md') || tab.name.endsWith('.markdown');
    return isMarkdown ? <FileText size={14} /> : <File size={14} />;
  };

  return (
    <div className={styles.tabBar}>
      <div className={styles.tabList}>
        {openTabs.map(tab => {
          const isActive = tab.id === activeTabId;
          const tabDoc = documentContents[tab.path];
          const isDirty = tabDoc?.isDirty || false;
          
          return (
            <div
              key={tab.id}
              className={`${styles.tab} ${isActive ? styles.active : ''}`}
              onClick={() => handleTabClick(tab)}
            >
              <span className={styles.tabIcon}>
                {getFileIcon(tab)}
              </span>
              <span className={styles.tabName}>{tab.isSettings ? '设置' : tab.name}</span>
              {isDirty && (
                <span className={styles.dirtyIndicator}>
                  <Circle size={8} fill="currentColor" />
                </span>
              )}
              <button
                className={styles.closeButton}
                onClick={(e) => handleCloseTab(e, tab.id)}
                title="关闭"
              >
                <X size={14} />
              </button>
            </div>
          );
        })}
      </div>
    </div>
  );
};
