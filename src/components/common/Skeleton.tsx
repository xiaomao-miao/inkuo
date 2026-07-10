import React from 'react';
import styles from './Skeleton.module.css';

interface SkeletonProps {
  /** 圆角大小。'pill' 用 9999px,'circle' 用 50%,'rounded' 用 8px,'square' 用 0 */
  shape?: 'rounded' | 'circle' | 'pill' | 'square';
  /** CSS width,如 '100%' / '12px' / 80 */
  width?: React.CSSProperties['width'];
  height?: React.CSSProperties['height'];
  className?: string;
  style?: React.CSSProperties;
}

/**
 * 单个 skeleton 块。提供三种动画变体:
 *  - 默认:横向 shimmer 光带扫过
 *  - 整组在父容器上加 .skeletonGroup 类时,所有子项按 60ms 错位入场
 *  - prefers-reduced-motion 下退化为纯静态脉冲
 */
export const Skeleton: React.FC<SkeletonProps> = ({
  shape = 'rounded',
  width,
  height,
  className,
  style,
}) => {
  const classes = [styles.skeleton, styles[shape], className]
    .filter(Boolean)
    .join(' ');
  return (
    <div
      className={classes}
      style={{ width, height, ...style }}
      aria-hidden="true"
    />
  );
};

interface SkeletonGroupProps {
  children: React.ReactNode;
  className?: string;
}

/** Skeleton 整组容器,提供统一 shimmer 节奏 */
export const SkeletonGroup: React.FC<SkeletonGroupProps> = ({
  children,
  className,
}) => (
  <div className={[styles.group, className].filter(Boolean).join(' ')}>
    {children}
  </div>
);

/** 列表占位行(头像 + 文字 + 文字) */
export const SkeletonListItem: React.FC<{ dense?: boolean }> = ({ dense }) => (
  <div className={[styles.row, dense ? styles.rowDense : ''].filter(Boolean).join(' ')}>
    <Skeleton shape="circle" width={24} height={24} />
    <div className={styles.rowText}>
      <Skeleton width="70%" height={10} />
      <Skeleton width="45%" height={8} />
    </div>
  </div>
);

/** 卡片占位(大块 + 三行) */
export const SkeletonCard: React.FC = () => (
  <div className={styles.card}>
    <Skeleton width="40%" height={12} />
    <Skeleton width="90%" height={10} />
    <Skeleton width="80%" height={10} />
    <Skeleton width="60%" height={10} />
  </div>
);