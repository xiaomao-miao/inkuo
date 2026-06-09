import React, { useCallback, useRef } from 'react';
import styles from './ResizableHandle.module.css';

interface ResizableHandleProps {
  direction: 'horizontal' | 'vertical';
  onResize: (delta: number) => void;
}

export const ResizableHandle: React.FC<ResizableHandleProps> = ({
  direction,
  onResize,
}) => {
  const isDragging = useRef(false);
  const startPos = useRef(0);

  const handleMouseDown = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    isDragging.current = true;
    startPos.current = direction === 'horizontal' ? e.clientX : e.clientY;

    const handleMouseMove = (e: MouseEvent) => {
      if (!isDragging.current) return;
      
      const currentPos = direction === 'horizontal' ? e.clientX : e.clientY;
      const delta = currentPos - startPos.current;
      startPos.current = currentPos;
      onResize(delta);
    };

    const handleMouseUp = () => {
      isDragging.current = false;
      document.removeEventListener('mousemove', handleMouseMove);
      document.removeEventListener('mouseup', handleMouseUp);
      document.body.style.cursor = '';
      document.body.style.userSelect = '';
    };

    document.addEventListener('mousemove', handleMouseMove);
    document.addEventListener('mouseup', handleMouseUp);
    document.body.style.cursor = direction === 'horizontal' ? 'ew-resize' : 'ns-resize';
    document.body.style.userSelect = 'none';
  }, [direction, onResize]);

  return (
    <div
      className={`${styles.handle} ${direction === 'horizontal' ? styles.horizontal : styles.vertical}`}
      onMouseDown={handleMouseDown}
    >
      <div className={styles.line} />
    </div>
  );
};
