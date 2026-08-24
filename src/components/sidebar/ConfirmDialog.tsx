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
  const cancelRef = useRef<HTMLButtonElement | null>(null);

  useEffect(() => {
    if (request) {
      // Destructive/three-way dialogs default to Cancel. This prevents an
      // accidental Enter from discarding work.
      if (request.danger || request.secondaryLabel) cancelRef.current?.focus();
      else confirmRef.current?.focus();
    }
  }, [request]);

  const handleKey = useCallback(
    (e: KeyboardEvent) => {
      if (!request) return;
      if (e.key === 'Escape') {
        e.preventDefault();
        close('cancel');
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
  const secondaryLabel = request.secondaryLabel;
  const cancelLabel = request.cancelLabel ?? '取消';
  const buttonClass = request.danger ? styles.dangerBtn : styles.confirmBtn;

  return createPortal(
    <div
      className={styles.overlay}
      role="presentation"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) close('cancel');
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
          <button
            ref={cancelRef}
            type="button"
            className={styles.cancelBtn}
            onClick={() => close('cancel')}
          >
            {cancelLabel}
          </button>
          {secondaryLabel && (
            <button
              type="button"
              className={styles.dangerBtn}
              onClick={() => close('secondary')}
            >
              {secondaryLabel}
            </button>
          )}
          <button
            ref={confirmRef}
            type="button"
            className={buttonClass}
            onClick={() => close('confirm')}
          >
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
};
