// Ghost text overlay for inline completion
// Renders the completion suggestion as faded text after the cursor

import React, { useEffect, useState, useRef } from 'react';
import type { ReactCodeMirrorRef } from '@uiw/react-codemirror';
import { useInlineCompleteStore } from '../../store';
import styles from './InlineComplete.module.css';

interface GhostTextOverlayProps {
  editorRef: React.RefObject<ReactCodeMirrorRef | null>;
}

export function GhostTextOverlay({ editorRef }: GhostTextOverlayProps) {
  const currentCompletion = useInlineCompleteStore((s) => s.currentCompletion);
  const isLoading = useInlineCompleteStore((s) => s.isLoading);

  const [position, setPosition] = useState<{ top: number; left: number } | null>(null);
  const lastCursorPosRef = useRef<number>(0);
  const rafIdRef = useRef<number>(0);

  const hasCompletion = !!currentCompletion?.text;

  useEffect(() => {
    const view = editorRef.current?.view;
    if (!view) return;

    const updatePosition = () => {
      const cursor = view.state.selection.main.head;
      const storeState = useInlineCompleteStore.getState();

      // If there's a completion, check if cursor moved from trigger position
      if (storeState.currentCompletion && storeState.triggerPosition !== null) {
        if (cursor !== storeState.triggerPosition) {
          // Cursor moved - clear the completion
          storeState.clearCompletion();
          setPosition(null);
          return;
        }
      }

      // Track cursor for next comparison
      lastCursorPosRef.current = cursor;

      const coords = view.coordsAtPos(cursor);
      if (coords) {
        const scroller = view.dom.querySelector('.cm-scroller');
        if (scroller) {
          const scrollerRect = scroller.getBoundingClientRect();
          setPosition({
            top: coords.top - scrollerRect.top + scroller.scrollTop,
            left: coords.left - scrollerRect.left + scroller.scrollLeft,
          });
        } else {
          const editorRect = view.dom.getBoundingClientRect();
          setPosition({
            top: coords.top - editorRect.top,
            left: coords.left - editorRect.left,
          });
        }
      }
    };

    // Throttled update using requestAnimationFrame
    const scheduleUpdate = () => {
      cancelAnimationFrame(rafIdRef.current);
      rafIdRef.current = requestAnimationFrame(updatePosition);
    };

    // Listen to keydown for cursor movement detection
    view.dom.addEventListener('keydown', scheduleUpdate);

    // Initial position
    updatePosition();

    return () => {
      view.dom.removeEventListener('keydown', scheduleUpdate);
      cancelAnimationFrame(rafIdRef.current);
    };
  }, [editorRef, hasCompletion]);

  if (!hasCompletion) {
    return null;
  }

  return (
    <div
      data-testid="ghost-text-overlay"
      style={{
        position: 'absolute',
        top: position?.top ?? 0,
        left: position?.left ?? 0,
        zIndex: 9999,
        pointerEvents: 'none',
      }}
    >
      <span
        style={{
          color: '#7c5cff',
          opacity: 0.7,
          fontFamily: 'var(--font-mono, monospace)',
          fontSize: '14px',
          lineHeight: '1.5',
        }}
      >
        {currentCompletion?.text}
      </span>
      {isLoading && <span style={{ color: '#7c5cff', opacity: 0.5 }}>...</span>}
    </div>
  );
}

// Status indicator component
export function InlineCompleteStatus() {
  const currentCompletion = useInlineCompleteStore((s) => s.currentCompletion);
  const isLoading = useInlineCompleteStore((s) => s.isLoading);
  const error = useInlineCompleteStore((s) => s.error);
  const enabled = useInlineCompleteStore((s) => s.enabled);

  if (!enabled) return null;

  return (
    <div className={styles.statusContainer}>
      {isLoading && (
        <span className={styles.statusLoading}>
          <span className={styles.loadingDot} />
          <span className={styles.loadingDot} />
          <span className={styles.loadingDot} />
        </span>
      )}
      {!isLoading && currentCompletion && (
        <span className={styles.statusReady}>
          <kbd>Tab</kbd> 接受 · <kbd>Esc</kbd> 拒绝
        </span>
      )}
      {!isLoading && error && (
        <span className={styles.statusError} title={error}>
          补全失败
        </span>
      )}
    </div>
  );
}
