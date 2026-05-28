// Ghost text overlay for inline completion
// Renders the completion suggestion as faded text after the cursor

import React, { useEffect, useState } from 'react';
import type { ReactCodeMirrorRef } from '@uiw/react-codemirror';
import { useInlineCompleteStore } from '../../store';
import styles from './InlineComplete.module.css';

interface GhostTextOverlayProps {
  editorRef: React.RefObject<ReactCodeMirrorRef | null>;
}

export function GhostTextOverlay({ editorRef }: GhostTextOverlayProps) {
  // Only subscribe to state changes, don't trigger re-mounts
  const currentCompletion = useInlineCompleteStore((s) => s.currentCompletion);
  const isLoading = useInlineCompleteStore((s) => s.isLoading);

  const [position, setPosition] = useState<{ top: number; left: number } | null>(null);

  const hasCompletion = !!currentCompletion?.text;

  // Set up event listeners once when editor mounts
  useEffect(() => {
    const view = editorRef.current?.view;
    if (!view) return;

    // Use a flag to track if we should update position
    let rafId = 0;
    let isMounted = true;

    const updatePosition = () => {
      if (!isMounted || !view) return;

      const storeState = useInlineCompleteStore.getState();

      // Check if cursor moved from trigger position
      if (storeState.currentCompletion && storeState.triggerPosition !== null) {
        const cursor = view.state.selection.main.head;
        if (cursor !== storeState.triggerPosition) {
          storeState.clearCompletion();
          setPosition(null);
          return;
        }
      }

      const cursor = view.state.selection.main.head;
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

    // Throttled update
    const scheduleUpdate = () => {
      cancelAnimationFrame(rafId);
      rafId = requestAnimationFrame(updatePosition);
    };

    // Listen to both keydown and keyup
    view.dom.addEventListener('keydown', scheduleUpdate);
    view.dom.addEventListener('keyup', scheduleUpdate);

    // Initial update
    updatePosition();

    return () => {
      isMounted = false;
      view.dom.removeEventListener('keydown', scheduleUpdate);
      view.dom.removeEventListener('keyup', scheduleUpdate);
      cancelAnimationFrame(rafId);
    };
    // Only depend on editorRef - we don't want to re-set up listeners when completion changes
  }, [editorRef]);

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
