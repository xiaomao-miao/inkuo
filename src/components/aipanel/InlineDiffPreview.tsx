import React from 'react';
import { Check, X } from 'lucide-react';
import styles from './InlineDiffPreview.module.css';

interface InlineDiffPreviewProps {
  originalText: string;
  newText: string;
  onAccept: () => void;
  onReject: () => void;
  isStreaming?: boolean;
}

export const InlineDiffPreview: React.FC<InlineDiffPreviewProps> = ({
  originalText,
  newText,
  onAccept,
  onReject,
  isStreaming = false,
}) => {
  return (
    <div className={`${styles.container} ${isStreaming ? styles.streaming : ''}`}>
      <div className={styles.header}>
        <div className={styles.headerLeft}>
          <span className={styles.label}>AI 修改预览</span>
          {isStreaming && <span className={styles.streamingBadge}>生成中...</span>}
        </div>
        <div className={styles.actions}>
          <button
            className={styles.acceptBtn}
            onClick={onAccept}
            title="接受修改"
          >
            <Check size={14} />
            <span>Accept</span>
          </button>
          <button
            className={styles.rejectBtn}
            onClick={onReject}
            title="拒绝修改"
          >
            <X size={14} />
            <span>Reject</span>
          </button>
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
