import { useEffect, useRef, useState } from 'react';
import { FileText, File, X, Circle, Settings } from 'lucide-react';
import { useSidebarStore } from '../../store';
import type { OpenTab } from '../../store';
import styles from './TabBar.module.css';

export const TabBar = () => {
  const { openTabs, activeTabId, setActiveTab, closeTab } = useSidebarStore();
  const [confirmClosePath, setConfirmClosePath] = useState<string | null>(null);
  const tabBarRef = useRef<HTMLDivElement>(null);

  // If the user switches tabs while the "discard changes" dialog is open,
  // the dialog would otherwise stay mounted and display the previous tab's
  // name (driven by `confirmClosePath`). Dismissing the dialog here keeps
  // the UI consistent with the active tab the user just landed on.
  useEffect(() => {
    if (confirmClosePath) {
      setConfirmClosePath(null);
    }
    // We intentionally depend only on `activeTabId` — clearing the dialog
    // on tab switch is the only side effect we want, and including
    // `confirmClosePath` would put us in an infinite update loop.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeTabId]);

  const handleWheel = (e: React.WheelEvent) => {
    if (tabBarRef.current) {
      tabBarRef.current.scrollLeft += e.deltaY * 0.5;
    }
  };

  const handleTabClick = (tab: OpenTab) => {
    setActiveTab(tab.id);
  };

  const handleCloseTab = (e: React.MouseEvent, tabId: string) => {
    e.stopPropagation();
    const tab = openTabs.find((item) => item.id === tabId);
    if (!tab || tab.isSettings) {
      closeTab(tabId);
      return;
    }
    const isDirty = tab.isDirty;
    if (isDirty) {
      setConfirmClosePath(tab.path);
    } else {
      closeTab(tabId);
    }
  };

  const handleConfirmClose = () => {
    if (confirmClosePath) {
      const tab = openTabs.find((item) => item.path === confirmClosePath);
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

  const confirmTab = confirmClosePath ? openTabs.find((item) => item.path === confirmClosePath) : null;

  return (
    <>
      <div className={styles.tabBar} ref={tabBarRef} onWheel={handleWheel}>
        <div className={styles.tabList}>
          {openTabs.map(tab => {
            const isActive = tab.id === activeTabId;
            const isDirty = tab.isSettings ? false : tab.isDirty;

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
