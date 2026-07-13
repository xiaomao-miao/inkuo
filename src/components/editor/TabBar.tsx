import { useRef } from 'react';
import {
  FileText, File, X, Circle, Settings, Cloud,
  FileImage, FileType, FileCode, FileAudio, FileVideo, FileArchive,
} from 'lucide-react';
import { useSidebarStore, useConfirmDialogStore } from '../../store';
import type { OpenTab } from '../../store';
import { detectFileKind } from '../../types';
import styles from './TabBar.module.css';

export const TabBar = () => {
  const { openTabs, activeTabId, setActiveTab, closeTab } = useSidebarStore();
  const ask = useConfirmDialogStore((s) => s.ask);
  const tabBarRef = useRef<HTMLDivElement>(null);

  const handleWheel = (e: React.WheelEvent) => {
    if (tabBarRef.current) {
      tabBarRef.current.scrollLeft += e.deltaY * 0.5;
    }
  };

  const handleTabClick = (tab: OpenTab) => {
    setActiveTab(tab.id);
  };

  const handleCloseTab = async (e: React.MouseEvent, tabId: string) => {
    e.stopPropagation();
    const tab = openTabs.find((item) => item.id === tabId);
    if (!tab || tab.isSettings || tab.isCloud) {
      closeTab(tabId);
      return;
    }
    if (tab.isDirty) {
      const confirmed = await ask({
        title: '未保存的更改',
        message: `${tab.name} 有未保存的更改。关闭将丢弃这些更改。`,
        confirmLabel: '丢弃更改',
        cancelLabel: '取消',
        danger: true,
      });
      if (confirmed) closeTab(tabId);
    } else {
      closeTab(tabId);
    }
  };

  if (openTabs.length === 0) {
    return null;
  }

  const getFileIcon = (tab: OpenTab) => {
    if (tab.isSettings) {
      return <Settings size={14} />;
    }
    if (tab.isCloud) {
      return <Cloud size={14} />;
    }
    const kind = detectFileKind(tab.name);
    switch (kind) {
      case 'markdown':
      case 'text':
        return <FileText size={14} />;
      case 'image':
        return <FileImage size={14} />;
      case 'pdf':
        return <FileType size={14} />;
      case 'code':
      case 'config':
      case 'data':
        return <FileCode size={14} />;
      case 'audio':
        return <FileAudio size={14} />;
      case 'video':
        return <FileVideo size={14} />;
      case 'archive':
        return <FileArchive size={14} />;
      default:
        return <File size={14} />;
    }
  };

  return (
    <>
      <div className={styles.tabBar} ref={tabBarRef} onWheel={handleWheel}>
        <div className={styles.tabList}>
          {openTabs.map(tab => {
            const isActive = tab.id === activeTabId;
            const isDirty = tab.isSettings || tab.isCloud ? false : tab.isDirty;

            return (
              <div
                key={tab.id}
                className={`${styles.tab} ${isActive ? styles.active : ''}`}
                onClick={() => handleTabClick(tab)}
              >
                <span className={styles.tabIcon}>
                  {getFileIcon(tab)}
                </span>
                <span className={styles.tabName}>
                  {tab.isSettings ? '设置' : tab.isCloud ? 'inkuo Cloud' : tab.name}
                </span>
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
    </>
  );
};
