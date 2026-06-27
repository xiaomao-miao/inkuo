import { useCallback, useEffect, useRef } from 'react';
import { createPortal } from 'react-dom';
import { useConfirmDialogStore } from '../../store';
import styles from './ConfirmDialog.module.css';

/**
 * Portal-rendered confirmation dialog backed by `useConfirmDialogStore`.
 * Only one dialog can be shown at a time; calling `ask` while another is
 * open resolves immediately to `false` so the caller doesn't hang.
 */
export const ConfirmDialog = () => {
  const request = useConfirmDialogStore((s) => s.request);
  const close = useConfirmDialogStore((s) => s.close);
  const confirmRef = useRef<HTMLButtonElement | null>(null);

  useEffect(() => {
    if (request) {
      // Focus the most-actionable button so Enter immediately resolves.
      confirmRef.current?.focus();
    }
  }, [request]);

  const handleKey = useCallback(
    (e: KeyboardEvent) => {
      if (!request) return;
      if (e.key === 'Escape') {
        e.preventDefault();
        close(false);
      } else if (e.key === 'Enter') {
        e.preventDefault();
        close(true);
      }
    },
    [request, close],
  );

  useEffect(() => {
    if (!request) return;
    window.addEventListener('keydown', handleKey);
    return () => window.removeEventListener('keydown', handleKey);
  }, [request, handleKey]);

  if (!request || typeof document === 'undefined') return null;

  const confirmLabel = request.confirmLabel ?? '确定';
  const cancelLabel = request.cancelLabel ?? '取消';
  const buttonClass = request.danger ? styles.dangerBtn : styles.confirmBtn;

  return createPortal(
    <div
      className={styles.overlay}
      role="presentation"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) close(false);
      }}
    >
      <div
        className={styles.dialog}
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="confirm-dialog-title"
        aria-describedby="confirm-dialog-message"
      >
        <div id="confirm-dialog-title" className={styles.title}>
          {request.title}
        </div>
        <div id="confirm-dialog-message" className={styles.message}>
          {request.message}
        </div>
        <div className={styles.actions}>
          <button type="button" className={styles.cancelBtn} onClick={() => close(false)}>
            {cancelLabel}
          </button>
          <button
            ref={confirmRef}
            type="button"
            className={buttonClass}
            onClick={() => close(true)}
          >
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
};
