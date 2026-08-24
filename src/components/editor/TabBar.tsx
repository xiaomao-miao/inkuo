import { useRef } from 'react';
import {
  FileText, File, X, Circle, Settings, Cloud,
  FileImage, FileType, FileCode, FileAudio, FileVideo, FileArchive,
} from 'lucide-react';
import {
  useSidebarStore,
  useContextMenuStore,
} from '../../store';
import type { OpenTab } from '../../store';
import { requestCloseOpenTab } from '../../services/openTabLifecycle';
import { detectFileKind } from '../../types';
import styles from './TabBar.module.css';

export const TabBar = () => {
  const { openTabs, activeTabId, setActiveTab } = useSidebarStore();
  const tabBarRef = useRef<HTMLDivElement>(null);

  const handleWheel = (e: React.WheelEvent) => {
    if (tabBarRef.current) {
      tabBarRef.current.scrollLeft += e.deltaY * 0.5;
    }
  };

  const handleTabClick = (tab: OpenTab) => {
    setActiveTab(tab.id);
  };

  const handleTabContextMenu = (e: React.MouseEvent, tab: OpenTab) => {
    e.preventDefault();
    e.stopPropagation();
    useContextMenuStore.getState().open({
      kind: 'tab',
      path: tab.path,
      x: e.clientX,
      y: e.clientY,
      tab,
    });
  };

  const handleCloseTab = async (e: React.MouseEvent, tabId: string) => {
    e.stopPropagation();
    const tab = openTabs.find((item) => item.id === tabId);
    if (!tab) return;
    await requestCloseOpenTab(tab);
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
                onContextMenu={(e) => handleTabContextMenu(e, tab)}
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
                  onContextMenu={(e) => {
                    // Suppress the tab context menu when right-clicking the
                    // close button itself — the button is small enough that
                    // users will sometimes hit it accidentally, and showing
                    // a "Close" entry that does the same thing as a left
                    // click is more confusing than helpful.
                    e.stopPropagation();
                    e.preventDefault();
                  }}
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
