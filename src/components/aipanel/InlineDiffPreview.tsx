import React, { useEffect, useRef } from 'react';
import { useAIPanelStore } from '../../store';
import { useSidebarStore } from '../../store';
import { useEditorStore, useInlineCompleteStore } from '../../store';
import styles from './InlineDiffPreview.module.css';

interface InlineDiffPreviewProps {
  originalText: string;
  newText: string;
  sessionId: string;
  isStreaming?: boolean;
}

export const InlineDiffPreview: React.FC<InlineDiffPreviewProps> = ({
  originalText,
  newText,
  sessionId,
  isStreaming = false,
}) => {
  const hasAutoAccepted = useRef(false);

  // Auto-accept when streaming completes
  useEffect(() => {
    if (isStreaming || hasAutoAccepted.current) return;

    const filePath = useSidebarStore.getState().selectedFile;
    if (!filePath) return;

    hasAutoAccepted.current = true;

    // Replace the selected text in place, preserving the rest of the file
    const currentDoc = useEditorStore.getState().documentContents[filePath];
    if (!currentDoc) return;

    const fullContent = currentDoc.content;

    // Find and replace the original selection in the full file content
    let replacedContent: string;
    const idx = fullContent.indexOf(originalText);
    if (idx !== -1) {
      // Found exact match - replace in place
      replacedContent = fullContent.slice(0, idx) + newText + fullContent.slice(idx + originalText.length);
    } else {
      // Fallback: try to match the first line of the selection
      const firstLine = originalText.split('\n')[0];
      const fallbackIdx = fullContent.indexOf(firstLine);
      if (fallbackIdx !== -1) {
        const endIdx = fallbackIdx + originalText.length;
        replacedContent = fullContent.slice(0, fallbackIdx) + newText + fullContent.slice(endIdx);
      } else {
        // Cannot locate the original text - do not replace
        replacedContent = fullContent;
      }
    }

    // Apply replaced content to editor
    useEditorStore.getState().setContent(filePath, replacedContent);
    // AI 直接改内容后，不应该立刻触发 inline complete
    useInlineCompleteStore.getState().clearCompletion();

    // Clear the diff from the session
    useAIPanelStore.getState().acceptAllHunks(sessionId);
  }, [isStreaming, newText, sessionId]);

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
