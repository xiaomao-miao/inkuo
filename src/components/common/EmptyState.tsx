import React from 'react';
import type { LucideIcon } from 'lucide-react';
import styles from './EmptyState.module.css';

interface EmptyStateProps {
  icon: LucideIcon;
  title: string;
  description?: string;
  actions?: React.ReactNode;
  /**
   * 视觉密度:
   *  - 'compact': 默认,用于侧栏/抽屉(28px icon,padding 24px)
   *  - 'comfortable': 用于主面板(40px icon,padding 40px)
   */
  size?: 'compact' | 'comfortable';
  className?: string;
}

/**
 * 通用空状态容器。统一三件事:
 *   1) 居中布局 + icon/title/description/actions 三段视觉
 *   2) motion-fade-in 入场,跨主题的 muted 文字色
 *   3) 提供两种密度适配侧栏 / 主面板
 */
export const EmptyState: React.FC<EmptyStateProps> = ({
  icon: Icon,
  title,
  description,
  actions,
  size = 'compact',
  className,
}) => {
  const containerClass = [
    styles.empty,
    size === 'comfortable' ? styles.comfortable : styles.compact,
    className,
  ]
    .filter(Boolean)
    .join(' ');

  return (
    <div className={containerClass}>
      <div className={styles.iconWrap}>
        <Icon size={size === 'comfortable' ? 36 : 26} strokeWidth={1.5} />
      </div>
      <div className={styles.title}>{title}</div>
      {description && <div className={styles.description}>{description}</div>}
      {actions && <div className={styles.actions}>{actions}</div>}
    </div>
  );
};