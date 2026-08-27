import React from 'react';
import {
  Files,
  Brain,
  History,
  Blocks,
  PanelLeft
} from 'lucide-react';
import { useSidebarStore } from '../../store';
import styles from './ActivityBar.module.css';

export type ViewType = 'files' | 'search' | 'git' | 'extensions' | 'knowledge' | 'snapshots';

interface ActivityBarProps {
  activeView: ViewType;
  onViewChange: (view: ViewType) => void;
  onToggleSidebar: () => void;
}

export const ActivityBar = ({
  activeView,
  onViewChange,
  onToggleSidebar,
}: ActivityBarProps) => {
  const knowledgeBase = useSidebarStore((s) => s.knowledgeBase);
  const knowledgeMemberCount = knowledgeBase?.members.length ?? 0;

  const views: { id: ViewType; icon: React.ReactNode; label: string }[] = [
    { id: 'files', icon: <Files size={22} />, label: '资源管理器' },
    { id: 'knowledge', icon: <Brain size={22} />, label: '知识库' },
    { id: 'snapshots', icon: <History size={22} />, label: '快照' },
    { id: 'extensions', icon: <Blocks size={22} />, label: '插件' },
  ];

  return (
    <div className={styles.activityBar}>
      <div className={styles.views}>
        {views.map(view => (
          <button
            key={view.id}
            className={`${styles.viewButton} ${activeView === view.id ? styles.active : ''}`}
            onClick={() => onViewChange(view.id)}
            title={view.label}
          >
            {view.icon}
            {view.id === 'knowledge' && knowledgeMemberCount > 0 && (
              <span className={styles.badge}>{knowledgeMemberCount}</span>
            )}
          </button>
        ))}
      </div>

      <div className={styles.bottom}>
        <button
          className={styles.viewButton}
          onClick={onToggleSidebar}
          title="切换侧边栏"
        >
          <PanelLeft size={22} />
        </button>
      </div>
    </div>
  );
};
