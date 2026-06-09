import { useInlineCompleteStore } from '../../store';
import styles from './InlineComplete.module.css';

/**
 * Legacy overlay component.
 * Ghost text is now rendered via CodeMirror decoration for correct
 * scrolling/positioning and to avoid overlapping layout issues.
 */
export function GhostTextOverlay() {
  const enabled = useInlineCompleteStore((s) => s.enabled);
  void styles; // keep css module referenced
  if (!enabled) return null;
  return null;
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
          <span className={styles.loadingText}>正在补全</span>
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
