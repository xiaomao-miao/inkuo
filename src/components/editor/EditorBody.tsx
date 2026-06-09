import React from 'react';
import { DiffActionBar } from './DiffActionBar';
import { MarkdownPreview } from './MarkdownPreview';
import { InlineCompleteStatus } from '../inline-complete';
import styles from './Editor.module.css';
import type { DiffHunk } from '../../types';
import type { Document } from '../../types';

interface EditorBodyProps {
  inPreviewMode: boolean;
  currentContent: string;
  selectedFile: string | null;
  isDiffMode: boolean;
  diffHunks: DiffHunk[];
  selection: { from: number; to: number } | null;
  document: Document | null | undefined;
  onTogglePreview: () => void;
  children: React.ReactNode;
}

export const EditorBody: React.FC<EditorBodyProps> = ({
  inPreviewMode,
  currentContent,
  selectedFile,
  isDiffMode,
  diffHunks,
  selection,
  document,
  onTogglePreview,
  children,
}) => {
  return (
    <>
      <div className={styles.editorWrapper}>
        {inPreviewMode ? (
          <MarkdownPreview content={currentContent} fileName={selectedFile?.split('/').pop() || ''} />
        ) : (
          children
        )}
      </div>

      {!inPreviewMode && <DiffActionBar />}

      <div className={styles.statusBar}>
        <span className={styles.statusItem}>
          {document?.doc_type || 'Markdown'}
        </span>
        <span className={styles.statusItem}>
          {currentContent.split('\n').length} 行
        </span>
        {selection && !inPreviewMode && (
          <span className={styles.statusItem}>
            已选择 {selection.to - selection.from} 字符
          </span>
        )}
        {isDiffMode && !inPreviewMode && (
          <span className={styles.statusItem} data-type="diff">
            {diffHunks.length} 个差异块
          </span>
        )}
        {!inPreviewMode && <InlineCompleteStatus />}
        <span className={styles.statusItem} style={{ marginLeft: 'auto' }}>
          <button
            className={styles.previewToggle}
            onClick={onTogglePreview}
          >
            {inPreviewMode ? '退出阅读' : '阅读模式'}
          </button>
        </span>
      </div>
    </>
  );
};
