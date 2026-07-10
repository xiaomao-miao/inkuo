import React, { useEffect, useId, useRef, useState } from 'react';
import styles from './Tooltip.module.css';

interface TooltipProps {
  content: React.ReactNode;
  /**
   * 触发时机:'hover'(默认,鼠标移入显示) | 'always'(一直显示,常用于 demo)
   */
  trigger?: 'hover' | 'always';
  /** 显示位置 */
  side?: 'top' | 'bottom' | 'left' | 'right';
  /** 显示延迟 (ms),用于避免闪烁 */
  delay?: number;
  /** 快捷键提示,会自动以 ⌘/Ctrl 风格的 kbd 渲染在右侧 */
  shortcut?: string;
  children: React.ReactElement;
}

/**
 * 轻量级 tooltip(无第三方依赖)。
 * - 通过 mouse enter/leave + focus 事件控制显隐
 * - 用 CSS var + fixed 定位,避免父容器 transform 影响
 * - 自动让 tooltip 在视口边缘反弹
 */
export const Tooltip: React.FC<TooltipProps> = ({
  content,
  trigger = 'hover',
  side = 'top',
  delay = 200,
  shortcut,
  children,
}) => {
  const [visible, setVisible] = useState(false);
  const [coords, setCoords] = useState<{ left: number; top: number; actualSide: typeof side }>({
    left: 0,
    top: 0,
    actualSide: side,
  });
  const triggerRef = useRef<HTMLElement | null>(null);
  const tooltipId = useId();
  const showTimer = useRef<number | null>(null);

  useEffect(() => {
    if (!visible) return;
    const onScroll = () => setVisible(false);
    window.addEventListener('scroll', onScroll, true);
    window.addEventListener('resize', onScroll);
    return () => {
      window.removeEventListener('scroll', onScroll, true);
      window.removeEventListener('resize', onScroll);
    };
  }, [visible]);

  const position = () => {
    const el = triggerRef.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    const tt = document.getElementById(tooltipId);
    if (!tt) return;
    const tw = tt.offsetWidth;
    const th = tt.offsetHeight;
    const margin = 6;
    let actual = side;
    let left = 0;
    let top = 0;

    if (side === 'top' || side === 'bottom') {
      left = r.left + r.width / 2 - tw / 2;
      top = side === 'top' ? r.top - th - margin : r.bottom + margin;
      if (top < 4) actual = 'bottom';
      if (top + th > window.innerHeight - 4) actual = 'top';
    } else {
      top = r.top + r.height / 2 - th / 2;
      left = side === 'left' ? r.left - tw - margin : r.right + margin;
      if (left < 4) actual = 'right';
      if (left + tw > window.innerWidth - 4) actual = 'left';
    }
    left = Math.max(4, Math.min(window.innerWidth - tw - 4, left));
    top = Math.max(4, Math.min(window.innerHeight - th - 4, top));
    setCoords({ left, top, actualSide: actual });
  };

  const handleShow = () => {
    if (showTimer.current) window.clearTimeout(showTimer.current);
    showTimer.current = window.setTimeout(() => {
      setVisible(true);
      requestAnimationFrame(position);
    }, delay);
  };

  const handleHide = () => {
    if (showTimer.current) window.clearTimeout(showTimer.current);
    setVisible(false);
  };

  const alwaysOn = trigger === 'always';

  return (
    <>
      {React.cloneElement(children, {
        ref: triggerRef,
        onMouseEnter: (e: React.MouseEvent) => {
          if (!alwaysOn) handleShow();
          children.props.onMouseEnter?.(e);
        },
        onMouseLeave: (e: React.MouseEvent) => {
          if (!alwaysOn) handleHide();
          children.props.onMouseLeave?.(e);
        },
        onFocus: (e: React.FocusEvent) => {
          if (!alwaysOn) handleShow();
          children.props.onFocus?.(e);
        },
        onBlur: (e: React.FocusEvent) => {
          if (!alwaysOn) handleHide();
          children.props.onBlur?.(e);
        },
        'aria-describedby': visible ? tooltipId : undefined,
      } as React.HTMLAttributes<HTMLElement>)}
      {(visible || alwaysOn) && (
        <div
          id={tooltipId}
          role="tooltip"
          className={`${styles.tooltip} ${styles[coords.actualSide]}`}
          style={{ left: coords.left, top: coords.top }}
        >
          <span>{content}</span>
          {shortcut && <kbd className={styles.kbd}>{shortcut}</kbd>}
        </div>
      )}
    </>
  );
};