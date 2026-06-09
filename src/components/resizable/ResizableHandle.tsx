import React, { useCallback, useRef } from 'react';
import styles from './ResizableHandle.module.css';

interface ResizableHandleProps {
  direction: 'horizontal' | 'vertical';
  onResize: (delta: number) => void;
}

export const ResizableHandle = ({
  direction,
  onResize,
}: ResizableHandleProps) => {
  const activePointerId = useRef<number | null>(null);
  const startPos = useRef(0);

  const stopDragging = useCallback((target?: HTMLElement | null) => {
    if (target && activePointerId.current !== null) {
      target.releasePointerCapture(activePointerId.current);
    }
    activePointerId.current = null;
    document.body.style.cursor = '';
    document.body.style.userSelect = '';
  }, []);

  const handlePointerMove = useCallback((event: React.PointerEvent<HTMLDivElement>) => {
    if (activePointerId.current !== event.pointerId) return;

    const currentPos = direction === 'horizontal' ? event.clientX : event.clientY;
    const delta = currentPos - startPos.current;
    startPos.current = currentPos;
    onResize(delta);
  }, [direction, onResize]);

  const handlePointerDown = useCallback((event: React.PointerEvent<HTMLDivElement>) => {
    event.preventDefault();
    activePointerId.current = event.pointerId;
    startPos.current = direction === 'horizontal' ? event.clientX : event.clientY;
    event.currentTarget.setPointerCapture(event.pointerId);
    document.body.style.cursor = direction === 'horizontal' ? 'ew-resize' : 'ns-resize';
    document.body.style.userSelect = 'none';
  }, [direction]);

  const handlePointerUp = useCallback((event: React.PointerEvent<HTMLDivElement>) => {
    if (activePointerId.current !== event.pointerId) return;
    stopDragging(event.currentTarget);
  }, [stopDragging]);

  const handlePointerCancel = useCallback((event: React.PointerEvent<HTMLDivElement>) => {
    if (activePointerId.current !== event.pointerId) return;
    stopDragging(event.currentTarget);
  }, [stopDragging]);

  return (
    <div
      className={`${styles.handle} ${direction === 'horizontal' ? styles.horizontal : styles.vertical}`}
      onPointerDown={handlePointerDown}
      onPointerMove={handlePointerMove}
      onPointerUp={handlePointerUp}
      onPointerCancel={handlePointerCancel}
    >
      <div className={styles.line} />
    </div>
  );
};
