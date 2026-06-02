import React from 'react';
import { MarkdownRenderer } from '../aipanel/MarkdownRenderer';
import styles from './MarkdownPreview.module.css';

interface MarkdownPreviewProps {
  content: string;
  fileName: string;
}

export const MarkdownPreview: React.FC<MarkdownPreviewProps> = ({ content, fileName }) => {
  return (
    <div className={styles.previewContainer}>
      <div className={styles.previewHeader}>
        <span className={styles.previewLabel}>阅读模式</span>
        <span className={styles.fileName}>{fileName}</span>
      </div>
      <div className={styles.previewContent}>
        <MarkdownRenderer content={content} />
      </div>
    </div>
  );
};
