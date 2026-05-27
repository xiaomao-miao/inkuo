import React from 'react';
import { 
  Files, 
  Search, 
  GitBranch, 
  BookOpen,
  PanelLeft
} from 'lucide-react';
import styles from './ActivityBar.module.css';

export type ViewType = 'files' | 'search' | 'git' | 'extensions';

interface ActivityBarProps {
  activeView: ViewType;
  onViewChange: (view: ViewType) => void;
  onToggleSidebar: () => void;
}

export const ActivityBar: React.FC<ActivityBarProps> = ({
  activeView,
  onViewChange,
  onToggleSidebar,
}) => {
  const views: { id: ViewType; icon: React.ReactNode; label: string }[] = [
    { id: 'files', icon: <Files size={22} />, label: '资源管理器' },
    { id: 'search', icon: <Search size={22} />, label: '搜索' },
    { id: 'git', icon: <GitBranch size={22} />, label: '源代码管理' },
    { id: 'extensions', icon: <BookOpen size={22} />, label: '扩展' },
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
            {view.id === 'extensions' && (
              <span className={styles.badge}>5</span>
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
