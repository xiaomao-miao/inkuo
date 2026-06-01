import React, { useState } from 'react';
import { FileText, File, X, Circle, Settings } from 'lucide-react';
import { useSidebarStore } from '../../store';
import type { OpenTab } from '../../store';
import styles from './TabBar.module.css';

export const TabBar: React.FC = () => {
  const { openTabs, activeTabId, setActiveTab, closeTab, openTabDirtyMap } = useSidebarStore();
  const [confirmClosePath, setConfirmClosePath] = useState<string | null>(null);

  const handleTabClick = (tab: OpenTab) => {
    setActiveTab(tab.id);
  };

  const handleCloseTab = (e: React.MouseEvent, tabId: string) => {
    e.stopPropagation();
    const tab = openTabs.find(t => t.id === tabId);
    if (!tab || tab.isSettings) {
      closeTab(tabId);
      return;
    }
    const isDirty = openTabDirtyMap[tab.path] || false;
    if (isDirty) {
      setConfirmClosePath(tab.path);
    } else {
      closeTab(tabId);
    }
  };

  const handleConfirmClose = () => {
    if (confirmClosePath) {
      const tab = openTabs.find(t => t.path === confirmClosePath);
      if (tab) {
        closeTab(tab.id);
      }
      setConfirmClosePath(null);
    }
  };

  const handleCancelClose = () => {
    setConfirmClosePath(null);
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

  const confirmTab = confirmClosePath ? openTabs.find(t => t.path === confirmClosePath) : null;

  return (
    <>
      <div className={styles.tabBar}>
        <div className={styles.tabList}>
          {openTabs.map(tab => {
            const isActive = tab.id === activeTabId;
            const isDirty = tab.isSettings ? false : (openTabDirtyMap[tab.path] || false);

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

      {confirmClosePath && confirmTab && (
        <div className={styles.confirmOverlay} onClick={handleCancelClose}>
          <div className={styles.confirmDialog} onClick={e => e.stopPropagation()}>
            <div className={styles.confirmTitle}>未保存的更改</div>
            <div className={styles.confirmMessage}>
              <strong>{confirmTab.name}</strong> 有未保存的更改。关闭将丢弃这些更改。
            </div>
            <div className={styles.confirmActions}>
              <button className={styles.cancelBtn} onClick={handleCancelClose}>
                取消
              </button>
              <button className={styles.discardBtn} onClick={handleConfirmClose}>
                丢弃更改
              </button>
            </div>
          </div>
        </div>
      )}
    </>
  );
};
