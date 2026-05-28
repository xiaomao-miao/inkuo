import React from 'react';
import { Check, X } from 'lucide-react';
import { useEditorStore, useSidebarStore } from '../../store';
import styles from './DiffActionBar.module.css';

export const DiffActionBar: React.FC = () => {
  const { selectedFile } = useSidebarStore();
  const { documentContents, applyAllHunks, rejectAllHunks } = useEditorStore();

  if (!selectedFile) return null;

  const doc = documentContents[selectedFile];
  const pending = doc?.pendingChange;
  const hunks = doc?.diffHunks ?? [];

  if (!pending || hunks.length === 0) return null;

  const handleAcceptAll = async () => {
    try {
      await applyAllHunks(selectedFile);
    } catch (e) {
      console.error('accept all failed', e);
    }
  };

  const handleRejectAll = async () => {
    try {
      await rejectAllHunks(selectedFile);
    } catch (e) {
      console.error('reject all failed', e);
    }
  };

  return (
    <div className={styles.bar}>
      <div className={styles.left}>
        <span className={styles.title}>AI 修改待确认</span>
        <span className={styles.meta}>{hunks.length} 个差异块</span>
      </div>
      <div className={styles.actions}>
        <button className={styles.acceptAll} type="button" onClick={handleAcceptAll}>
          <Check size={14} />
          全部同意
        </button>
        <button className={styles.rejectAll} type="button" onClick={handleRejectAll}>
          <X size={14} />
          全部拒绝
        </button>
      </div>
    </div>
  );
};
