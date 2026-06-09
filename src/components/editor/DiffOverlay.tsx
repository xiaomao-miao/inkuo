import React from 'react';
import { Check, X, Copy, ChevronDown } from 'lucide-react';
import { useEditorStore, useSidebarStore, type DiffHunk } from '../../store';
import styles from './DiffOverlay.module.css';

interface DiffOverlayProps {
  hunks: DiffHunk[];
}

export const DiffOverlay: React.FC<DiffOverlayProps> = ({ hunks }) => {
  const { selectedFile } = useSidebarStore();
  const { documentContents, setActiveHunkIndex, applyHunk, rejectHunk, applyAllHunks, rejectAllHunks } = useEditorStore();
  
  const currentDoc = selectedFile ? documentContents[selectedFile] : null;
  const activeHunkIndex = currentDoc?.activeHunkIndex || 0;

  const getHunkSummary = (hunk: DiffHunk) => {
    let added = 0, removed = 0;
    hunk.changes.forEach(c => {
      if (c.tag === 'insert') added++;
      else if (c.tag === 'delete') removed++;
    });
    if (added > 0 && removed > 0) return `+${added} -${removed}`;
    if (added > 0) return `+${added}`;
    if (removed > 0) return `-${removed}`;
    return '无变化';
  };

  if (hunks.length === 0) {
    return null;
  }

  const handleApply = (hunkId: string, e: React.MouseEvent) => {
    e.stopPropagation();
    if (selectedFile) {
      applyHunk(selectedFile, hunkId);
    }
  };

  const handleReject = (hunkId: string, e: React.MouseEvent) => {
    e.stopPropagation();
    if (selectedFile) {
      rejectHunk(selectedFile, hunkId);
    }
  };

  const handleCopy = (hunk: DiffHunk, e: React.MouseEvent) => {
    e.stopPropagation();
    // Build a unified-diff style representation of the hunk
    const lines = hunk.changes.map((c) => {
      const prefix =
        c.tag === 'delete' ? '-' : c.tag === 'insert' ? '+' : ' ';
      return `${prefix} ${c.content}`;
    });
    navigator.clipboard.writeText(lines.join('\n'));
  };

  const handleApplyAll = () => {
    if (selectedFile) {
      applyAllHunks(selectedFile);
    }
  };

  const handleRejectAll = () => {
    if (selectedFile) {
      rejectAllHunks(selectedFile);
    }
  };

  return (
    <div className={styles.overlay}>
      <div className={styles.controls}>
        <span className={styles.hunkCount}>
          {hunks.length} 个差异块
        </span>
        <div className={styles.controlButtons}>
          <button 
            className={styles.applyAllButton}
            onClick={handleApplyAll}
            title="应用全部 (Shift+Tab)"
          >
            <Check size={14} />
            <span>全部应用</span>
          </button>
          <button 
            className={styles.rejectAllButton}
            onClick={handleRejectAll}
            title="拒绝全部 (Cmd+Esc)"
          >
            <X size={14} />
            <span>全部拒绝</span>
          </button>
        </div>
      </div>
      
      <div className={styles.hunksList}>
        {hunks.map((hunk, index) => (
          <div 
            key={hunk.id}
            className={`${styles.hunkCard} ${index === activeHunkIndex ? styles.active : ''}`}
            onClick={() => selectedFile && setActiveHunkIndex(selectedFile, index)}
          >
            <div className={styles.hunkHeader}>
              <div className={styles.hunkSummary}>
                <span className={styles.summaryIcon}>
                  <ChevronDown size={14} />
                </span>
                <span className={styles.summaryText}>{getHunkSummary(hunk)}</span>
              </div>
              <div className={styles.hunkActions}>
                <button 
                  className={styles.actionIcon}
                  onClick={(e) => handleCopy(hunk, e)}
                  title="复制修改内容"
                >
                  <Copy size={12} />
                </button>
                <button 
                  className={styles.rejectIcon}
                  onClick={(e) => handleReject(hunk.id, e)}
                  title="拒绝 (Esc)"
                >
                  <X size={12} />
                </button>
                <button 
                  className={styles.applyIcon}
                  onClick={(e) => handleApply(hunk.id, e)}
                  title="应用 (Tab)"
                >
                  <Check size={12} />
                </button>
              </div>
            </div>
            
            <div className={styles.hunkContent}>
              {hunk.changes.map((change, changeIndex) => (
                <div 
                  key={changeIndex}
                  className={`${styles.changeLine} ${
                    change.tag === 'delete' ? styles.deleted :
                    change.tag === 'insert' ? styles.inserted :
                    ''
                  }`}
                >
                  <span className={styles.lineNumber}>
                    {change.tag === 'delete' ? change.old_line :
                     change.tag === 'insert' ? change.new_line :
                     change.old_line}
                  </span>
                  <span className={styles.lineTag}>
                    {change.tag === 'delete' ? '-' :
                     change.tag === 'insert' ? '+' : ' '}
                  </span>
                  <span className={styles.lineContent}>{change.content}</span>
                </div>
              ))}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
};
