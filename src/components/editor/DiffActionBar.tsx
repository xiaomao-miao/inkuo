import { Check, X } from 'lucide-react';
import { useEditorStore, useSidebarStore } from '../../store';
import styles from './DiffActionBar.module.css';

export const DiffActionBar = () => {
  const { selectedFile } = useSidebarStore();
  const { documentContents, applyAllHunks, rejectAllHunks } = useEditorStore();

  if (!selectedFile) return null;

  const doc = documentContents[selectedFile];
  const diff = doc?.diff;
  const hunks = diff?.hunks ?? [];

  if (!diff?.isActive || hunks.length === 0) return null;

  const handleAcceptAll = () => {
    try {
      applyAllHunks(selectedFile);
    } catch (e) {
      console.error('accept all failed', e);
    }
  };

  const handleRejectAll = () => {
    try {
      rejectAllHunks(selectedFile);
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
