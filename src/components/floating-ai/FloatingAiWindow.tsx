// Floating AI popover.
//
// One instance per ask (open via `useFloatingAiStore.open({...})`).
// The popover is `position: fixed` so it doesn't get clipped by
// ancestor `overflow` rules, and it lives in its own stacking
// context so dragging doesn't disturb the rest of the layout.
//
// Drag is implemented with a plain `pointerdown` + `pointermove` /
// `pointerup` triple — no library, no `react-dnd`. We commit the
// final position back to the store on `pointerup` so a re-render
// (e.g. streamed delta) doesn't reset the in-flight drag position.
//
// The popover drives the AI stream through `useFloatingAiStream` and
// subscribes to its own status (idle / streaming / done / error /
// cancelled) to render a footer hint + an optional "stop" button.

import React, { useCallback, useEffect, useRef, useState } from 'react';

import { Copy, Sparkles, Square, X } from 'lucide-react';

import { useFloatingAiStore } from '../../store';
import { StreamingMarkdownRenderer } from '../aipanel/StreamingMarkdownRenderer';
import { useFloatingAiStream } from './useFloatingAiStream';

import styles from './FloatingAiWindow.module.css';

interface FloatingAiWindowProps {
  id: string;
}

const MIN_HEIGHT = 220;
const DEFAULT_WIDTH = 480;
/** Default initial height. Doubled from MIN_HEIGHT so the popover
 *  starts tall enough to render a meaningful AI response without the
 *  user having to manually resize. The user can still shrink it down
 *  to MIN_HEIGHT via the resize handle. */
const DEFAULT_HEIGHT = 440;

/**
 * A single floating AI popover. Renders nothing if the store entry
 * doesn't exist (closed between renders).
 */
export const FloatingAiWindow: React.FC<FloatingAiWindowProps> = ({ id }) => {
  const window = useFloatingAiStore((s) => s.windows[id]);
  const close = useFloatingAiStore((s) => s.close);
  const bringToFront = useFloatingAiStore((s) => s.bringToFront);
  const setPosition = useFloatingAiStore((s) => s.setPosition);
  const setSize = useFloatingAiStore((s) => s.setSize);
  const order = useFloatingAiStore((s) => s.order);

  // Z-index derived from render order: the topmost window is the
  // most-recently-opened or most-recently-clicked one. We use a base
  // of 800 so the popover sits above the editor / sidebar z-stack
  // without overlapping the topbar / cmdK palette.
  const zIndex = React.useMemo(() => {
    const idx = order.indexOf(id);
    return idx < 0 ? 800 : 800 + idx;
  }, [order, id]);

  // `request` is the input for `useFloatingAiStream`. The popover's
  // `instruction` field is the actual prompt sent to the model —
  // distinct from `quote`, which is the user's original selection
  // text shown in the header. Most callers build `instruction` as
  // "请解释以下内容：\n\n<quote>" or similar.
  const instruction = window?.instruction ?? '';
  const request = window ? { id: window.id, instruction } : null;

  const { cancel } = useFloatingAiStream({ request });

  /**
   * Drag state. We keep the offset (pointer vs window top-left) so
   * the cursor stays "grabbed" at the same spot as we move.
   */
  const dragRef = useRef<{
    pointerId: number;
    offsetX: number;
    offsetY: number;
    nextPosition: { x: number; y: number };
  } | null>(null);

  const onDragPointerDown = useCallback(
    (e: React.PointerEvent<HTMLDivElement>) => {
      // Only the header (drag handle) initiates dragging. Buttons
      // inside the header stop propagation so click handlers still
      // work. We also bail out here if the pointer landed on a
      // button — otherwise `setPointerCapture` would swallow the
      // subsequent click (the browser dispatches click on the
      // common ancestor of mousedown + mouseup, but with capture
      // redirected to the header the click target shifts and the
      // button's onClick never fires, making the window unclosable).
      if (!window) return;
      const target = e.target as HTMLElement | null;
      if (target && target.closest('button')) {
        return;
      }
      e.preventDefault();
      const current = e.currentTarget;
      current.setPointerCapture(e.pointerId);
      dragRef.current = {
        pointerId: e.pointerId,
        offsetX: e.clientX - window.position.x,
        offsetY: e.clientY - window.position.y,
        nextPosition: { x: window.position.x, y: window.position.y },
      };
      // Bring to front on drag-start too — feels more responsive
      // than only on body click.
      bringToFront(id);
    },
    [window, id, bringToFront],
  );

  const onDragPointerMove = useCallback(
    (e: React.PointerEvent<HTMLDivElement>) => {
      const drag = dragRef.current;
      if (!drag || drag.pointerId !== e.pointerId || !window) return;
      // `window` here is the local store entry — globalThis.window is
      // the global viewport. We explicitly reference it because the
      // local variable shadows the global `window` symbol.
      const vw = globalThis.window.innerWidth;
      const vh = globalThis.window.innerHeight;
      const width = window.width ?? DEFAULT_WIDTH;
      const nextX = Math.max(8, Math.min(vw - width - 8, e.clientX - drag.offsetX));
      const nextY = Math.max(8, Math.min(vh - MIN_HEIGHT - 8, e.clientY - drag.offsetY));
      drag.nextPosition = { x: nextX, y: nextY };
      // Optimistic in-flight position. We commit it to the store only
      // on pointerup so re-renders don't fight the move handler.
      // The CSS reads from `drag.nextPosition` via a ref below.
      setLivePosition(drag.nextPosition);
    },
    [window],
  );

  const onDragPointerUp = useCallback(
    (e: React.PointerEvent<HTMLDivElement>) => {
      const drag = dragRef.current;
      if (!drag || drag.pointerId !== e.pointerId) return;
      e.currentTarget.releasePointerCapture(e.pointerId);
      dragRef.current = null;
      // Commit the final position. After this, the store is the
      // source of truth and the next render reads `window.position`.
      if (window) {
        setPosition(id, drag.nextPosition);
      }
    },
    [id, setPosition, window],
  );

  /**
   * Resize state. The handle lives in the bottom-right corner; we
   * capture the pointer to the handle itself so the drag follows
   * the cursor even when it leaves the handle's hit area.
   */
  const resizeRef = useRef<{
    pointerId: number;
    originWidth: number;
    originHeight: number;
    startClientX: number;
    startClientY: number;
    nextSize: { width: number; height: number };
  } | null>(null);

  const onResizePointerDown = useCallback(
    (e: React.PointerEvent<HTMLDivElement>) => {
      // Same button-guard pattern as the drag handler — without this
      // `setPointerCapture` would steal the click and prevent the
      // button's onClick from firing. The resize handle has no
      // children to guard against, but the explicit check documents
      // the intent and protects future edits that add inner controls.
      if (!window) return;
      const target = e.target as HTMLElement | null;
      if (target && target.closest('button')) return;
      e.preventDefault();
      e.stopPropagation();
      const current = e.currentTarget;
      current.setPointerCapture(e.pointerId);
      const width = window.width ?? DEFAULT_WIDTH;
      const height = window.height ?? DEFAULT_HEIGHT;
      resizeRef.current = {
        pointerId: e.pointerId,
        originWidth: width,
        originHeight: height,
        startClientX: e.clientX,
        startClientY: e.clientY,
        nextSize: { width, height },
      };
      bringToFront(id);
    },
    [window, id, bringToFront],
  );

  const onResizePointerMove = useCallback(
    (e: React.PointerEvent<HTMLDivElement>) => {
      const r = resizeRef.current;
      if (!r || r.pointerId !== e.pointerId || !window) return;
      const vw = globalThis.window.innerWidth;
      const vh = globalThis.window.innerHeight;
      const pos = window.position;
      const width = Math.max(240, Math.min(vw - pos.x - 8, r.originWidth + (e.clientX - r.startClientX)));
      const height = Math.max(MIN_HEIGHT, Math.min(vh - pos.y - 8, r.originHeight + (e.clientY - r.startClientY)));
      r.nextSize = { width, height };
      setLiveSize(r.nextSize);
    },
    [window],
  );

  const onResizePointerUp = useCallback(
    (e: React.PointerEvent<HTMLDivElement>) => {
      const r = resizeRef.current;
      if (!r || r.pointerId !== e.pointerId) return;
      e.currentTarget.releasePointerCapture(e.pointerId);
      resizeRef.current = null;
      if (window) {
        setSize(id, r.nextSize);
      }
    },
    [id, setSize, window],
  );

  // Live position while dragging. We use a state value to drive the
  // `transform: translate(...)` so we don't write to the store on
  // every frame. Once drag ends, the store takes over again.
  const [livePosition, setLivePosition] = useState<{ x: number; y: number } | null>(null);

  // Live size while resizing — same trick as `livePosition`. Commit
  // to the store on pointer-up so a re-render (stream delta) doesn't
  // reset the in-flight resize.
  const [liveSize, setLiveSize] = useState<{ width: number; height: number } | null>(null);

  // Reset live position / size when window switches (e.g. close + open
  // with new id). Without this the previous drag/resize state would
  // briefly apply to the new window.
  useEffect(() => {
    setLivePosition(null);
    setLiveSize(null);
  }, [id]);

  const [copied, setCopied] = useState(false);

  if (!window) return null;

  const width = liveSize?.width ?? window.width ?? DEFAULT_WIDTH;
  const height = liveSize?.height ?? window.height ?? DEFAULT_HEIGHT;
  const position = livePosition ?? window.position;

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(window.streamedContent);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch (err) {
      console.warn('[floating-ai] clipboard copy failed:', err);
    }
  };

  const status = window.status;
  const isStreaming = status === 'streaming';
  const isDone = status === 'done';
  const isError = status === 'error';
  const isCancelled = status === 'cancelled';

  return (
    <div
      className={styles.window}
      data-floating-ai-window={id}
      style={{
        left: position.x,
        top: position.y,
        width,
        height,
        zIndex,
      }}
      // Bring to front on any pointer down inside the window so
      // clicking the body of a hidden popover re-raises it.
      onPointerDown={() => bringToFront(id)}
    >
      <div
        className={styles.header}
        onPointerDown={onDragPointerDown}
        onPointerMove={onDragPointerMove}
        onPointerUp={onDragPointerUp}
        onPointerCancel={onDragPointerUp}
      >
        <span className={styles.titleIcon} aria-hidden="true">
          <Sparkles size={14} />
        </span>
        <div className={styles.titleBlock}>
          <span className={styles.title}>{window.title}</span>
          {window.subtitle && <span className={styles.subtitle}>{window.subtitle}</span>}
        </div>
        <button
          type="button"
          className={styles.headerButton}
          onClick={(e) => {
            e.stopPropagation();
            close(id);
          }}
          title="关闭"
          aria-label="关闭"
        >
          <X size={14} />
        </button>
      </div>

      <div className={styles.body}>
        <blockquote className={styles.quote}>{window.quote}</blockquote>
        <div className={styles.markdownWrap}>
          <StreamingMarkdownRenderer
            content={window.streamedContent}
            isStreaming={isStreaming}
          />
        </div>
      </div>

      <div className={styles.footer}>
        <span className={styles.status}>
          {isStreaming && '正在生成...'}
          {isDone && '已完成'}
          {isError && (window.errorMessage || '生成失败')}
          {isCancelled && '已取消'}
          {status === 'idle' && '准备中...'}
        </span>
        <div className={styles.footerActions}>
          <button
            type="button"
            className={styles.footerButton}
            onClick={handleCopy}
            disabled={!window.streamedContent}
            title="复制结果"
            aria-label="复制结果"
          >
            <Copy size={12} />
            <span>{copied ? '已复制' : '复制'}</span>
          </button>
          {isStreaming && (
            <button
              type="button"
              className={styles.footerButton}
              onClick={() => {
                void cancel();
              }}
              title="停止生成"
              aria-label="停止生成"
            >
              <Square size={12} />
              <span>停止</span>
            </button>
          )}
        </div>
      </div>
      {/*
        Resize handle. Lives in the bottom-right corner; uses the
        same pointer-capture + button-guard pattern as the header
        drag handle so a stray click can't trap us in a resize.
      */}
      <div
        className={styles.resizeHandle}
        role="presentation"
        onPointerDown={onResizePointerDown}
        onPointerMove={onResizePointerMove}
        onPointerUp={onResizePointerUp}
        onPointerCancel={onResizePointerUp}
      />
    </div>
  );
};

/**
 * Renders all open floating AI windows. Mount this once at the app
 * root inside a React portal target (the document body, or a
 * top-level layout container).
 */
export const FloatingAiLayer: React.FC = () => {
  const order = useFloatingAiStore((s) => s.order);
  return (
    <>
      {order.map((id) => (
        <FloatingAiWindow key={id} id={id} />
      ))}
    </>
  );
};
