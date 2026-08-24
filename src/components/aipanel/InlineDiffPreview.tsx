import { useEffect, useRef } from 'react';
import { Check, X } from 'lucide-react';
import { useAIPanelStore, useEditorStore, useInlineCompleteStore } from '../../store';
import type { CurrentDiff } from '../../types';
import styles from './InlineDiffPreview.module.css';

interface InlineDiffPreviewProps {
  originalText: string;
  newText: string;
  sessionId: string;
  isStreaming?: boolean;
  pendingDiff?: CurrentDiff | null;
}

export function syncPendingDiffToEditor(
  diff: CurrentDiff,
  sync: (
    path: string,
    hunks: CurrentDiff['hunks'],
    originalText: string,
    originalOffset: number,
  ) => void = useEditorStore.getState().setDiffHunks,
): boolean {
  if (!diff.filePath) return false;
  sync(diff.filePath, diff.hunks, diff.originalText, 0);
  return true;
}

export const InlineDiffPreview = ({
  originalText,
  newText,
  sessionId,
  isStreaming = false,
  pendingDiff = null,
}: InlineDiffPreviewProps) => {
  const lastSyncedKey = useRef<string | null>(null);

  useEffect(() => {
    if (isStreaming || !pendingDiff?.filePath) return;

    const syncKey = `${sessionId}::${pendingDiff.filePath}::${pendingDiff.hunks.length}`;
    if (lastSyncedKey.current !== syncKey) {
      lastSyncedKey.current = syncKey;
      syncPendingDiffToEditor(pendingDiff);
    }
  }, [isStreaming, pendingDiff, sessionId]);

  const handleAcceptAll = () => {
    useAIPanelStore.getState().acceptAllHunks(sessionId);
    useInlineCompleteStore.getState().clearCompletion();
  };

  const handleRejectAll = () => {
    useAIPanelStore.getState().rejectAllHunks(sessionId);
    useInlineCompleteStore.getState().clearCompletion();
  };

  return (
    <div className={`${styles.container} ${isStreaming ? styles.streaming : ''}`}>
      <div className={styles.header}>
        <div className={styles.headerLeft}>
          <span className={styles.label}>AI 修改预览</span>
          {isStreaming && <span className={styles.streamingBadge}>生成中...</span>}
        </div>
        {!isStreaming && pendingDiff?.filePath && (
          <div className={styles.actions}>
            <button type="button" className={styles.rejectButton} onClick={handleRejectAll}>
              <X size={13} />
              全部拒绝
            </button>
            <button type="button" className={styles.acceptButton} onClick={handleAcceptAll}>
              <Check size={13} />
              全部接受
            </button>
          </div>
        )}
      </div>

      <div className={styles.diffView}>
        <div className={styles.diffPane}>
          <div className={styles.paneHeader}>
            <span className={styles.paneLabel}>Original</span>
          </div>
          <pre className={styles.paneContent}>{originalText}</pre>
        </div>

        <div className={styles.diffArrow}>
          <span>→</span>
        </div>

        <div className={styles.diffPane}>
          <div className={styles.paneHeader}>
            <span className={styles.paneLabel}>Modified</span>
          </div>
          <pre className={styles.paneContent}>{newText}</pre>
        </div>
      </div>
    </div>
  );
};
