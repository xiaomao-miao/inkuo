import { useEffect, useRef } from 'react';
import { createPortal } from 'react-dom';
import { CheckCircle2, Info, XCircle, X } from 'lucide-react';
import { useNotificationStore } from '../../store';
import type { NotificationItem } from '../../store/notificationStore';
import styles from './NotificationStack.module.css';

/**
 * Renders the queue of `useNotificationStore` notifications as a stack of
 * stacked, auto-dismissing toasts in the bottom-right corner. Each toast
 * fades + slides in, persists for 4s, then fades out before being removed
 * from the store.
 *
 * Single instance mounted in <Layout> — call sites only need to invoke
 * `pushNotification(...)` and the toast appears automatically.
 */
const TOAST_DURATION_MS = 4000;
const FADE_OUT_MS = 180;

function ToastIcon({ kind }: { kind: NotificationItem['kind'] }) {
  if (kind === 'success') return <CheckCircle2 size={16} className={styles.iconSuccess} />;
  if (kind === 'error') return <XCircle size={16} className={styles.iconError} />;
  return <Info size={16} className={styles.iconInfo} />;
}

export const NotificationStack = () => {
  const notifications = useNotificationStore((s) => s.notifications);
  const dismiss = useNotificationStore((s) => s.dismissNotification);
  // Track which IDs are currently in their fade-out phase so we don't
  // remove from store until the CSS animation completes.
  const fadingRef = useRef<Set<string>>(new Set());

  useEffect(() => {
    if (notifications.length === 0) return;
    const timers = notifications.map((n) =>
      window.setTimeout(() => {
        fadingRef.current.add(n.id);
        // Force re-render so the .fading class is applied before removal.
        // We use the dismiss action which mutates the store; the CSS
        // transition runs, then a final setTimeout clears it after FADE_OUT_MS.
        dismiss(n.id);
        window.setTimeout(() => fadingRef.current.delete(n.id), FADE_OUT_MS + 50);
      }, TOAST_DURATION_MS),
    );
    return () => {
      timers.forEach((t) => window.clearTimeout(t));
    };
    // We only want to schedule auto-dismiss for NEW notifications, not re-run
    // when an in-progress one fades out (which would re-schedule itself).
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [notifications.map((n) => n.id).join('|')]);

  if (typeof document === 'undefined' || notifications.length === 0) return null;

  return createPortal(
    <div className={styles.stack} role="region" aria-label="通知">
      {notifications.map((n) => (
        <div
          key={n.id}
          className={`${styles.toast} ${styles[`toast_${n.kind}`] ?? ''}`}
          role={n.kind === 'error' ? 'alert' : 'status'}
        >
          <div className={styles.toastIcon}>
            <ToastIcon kind={n.kind} />
          </div>
          <div className={styles.toastBody}>
            <div className={styles.toastTitle}>{n.title}</div>
            {n.message && <div className={styles.toastMessage}>{n.message}</div>}
          </div>
          <button
            type="button"
            className={styles.toastClose}
            onClick={() => dismiss(n.id)}
            aria-label="关闭通知"
          >
            <X size={14} />
          </button>
        </div>
      ))}
    </div>,
    document.body,
  );
};
