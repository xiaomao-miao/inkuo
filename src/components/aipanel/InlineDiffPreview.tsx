import { useEffect, useRef } from 'react';
import { useAIPanelStore, useEditorStore, useInlineCompleteStore } from '../../store';
import styles from './InlineDiffPreview.module.css';

interface InlineDiffPreviewProps {
  originalText: string;
  newText: string;
  sessionId: string;
  isStreaming?: boolean;
}

export const InlineDiffPreview = ({
  originalText,
  newText,
  sessionId,
  isStreaming = false,
}: InlineDiffPreviewProps) => {
  // Auto-accept when streaming completes.
  //
  // History: the previous implementation did a full-document
  // `indexOf(originalText)` and replaced the first hit with `newText`. That
  // silently corrupted documents whenever the same selection appeared
  // elsewhere in the file (e.g. boilerplate code, repeated sentences), and
  // its first-line fallback dropped `originalText.length` bytes from the
  // wrong position when the exact match wasn't found.
  //
  // The fix routes the application through the editor store's diff
  // machinery, which uses accurate hunk offsets. For CmdK-driven diffs the
  // hunks were already pushed via `editorStore.setDiffHunks`, so calling
  // `acceptAllHunks` is enough. For agent-mode diffs (stream payload with
  // `original_content` / `new_content`), the hunks live on `pendingDiff`
  // but haven't been pushed to the editor store yet — we sync them first
  // so the same apply path works for both call sites.
  const hasAutoAccepted = useRef(false);
  const lastSyncedKey = useRef<string | null>(null);

  useEffect(() => {
    if (isStreaming) return;

    // Sync pendingDiff hunks into the editor store exactly once per diff.
    // We only push to the store on the first non-streaming render for a
    // given diff identity so we don't clobber state when subsequent
    // re-renders fire with the same props.
    const session = useAIPanelStore.getState().sessions.find((s) => s.id === sessionId);
    const pendingDiff = session?.pendingDiff;
    if (!pendingDiff || !pendingDiff.filePath) return;

    const syncKey = `${sessionId}::${pendingDiff.filePath}::${pendingDiff.hunks.length}`;
    if (lastSyncedKey.current !== syncKey) {
      lastSyncedKey.current = syncKey;
      // In agent mode, `originalText` IS the full file (the stream emits
      // original_content/new_content as full-file snapshots). Hunks from
      // `compute_diff(oldText, newText)` are relative to that full
      // snapshot, so the editor-store's `originalOffset` should be 0.
      // In CmdK mode the hunks were already pushed to the editor store by
      // `CmdK.handleSubmit`, but pushing them again here is a no-op (the
      // apply path tolerates re-syncs).
      useEditorStore.getState().setDiffHunks(
        pendingDiff.filePath,
        pendingDiff.hunks,
        pendingDiff.originalText,
        0,
      );
    }

    if (hasAutoAccepted.current) return;
    hasAutoAccepted.current = true;

    // Apply hunks through the editor store. This uses the offsets we just
    // synced instead of doing another `indexOf` on the full document.
    useAIPanelStore.getState().acceptAllHunks(sessionId);

    // AI just replaced the document content; suppress any inline-complete
    // ghost suggestion that might have been queued for the cursor position.
    useInlineCompleteStore.getState().clearCompletion();
  }, [isStreaming, sessionId, originalText, newText]);

  return (
    <div className={`${styles.container} ${isStreaming ? styles.streaming : ''}`}>
      <div className={styles.header}>
        <div className={styles.headerLeft}>
          <span className={styles.label}>AI 修改预览</span>
          {isStreaming && <span className={styles.streamingBadge}>生成中...</span>}
        </div>
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