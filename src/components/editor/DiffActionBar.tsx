import { Check, X } from 'lucide-react';
import { useEditorStore, useNotificationStore, useSidebarStore } from '../../store';
import { reportError } from '../../utils/errors';
import styles from './DiffActionBar.module.css';

export const DiffActionBar = () => {
  const selectedFile = useSidebarStore((state) => state.selectedFile);
  const documentContents = useEditorStore((state) => state.documentContents);
  const applyAllHunks = useEditorStore((state) => state.applyAllHunks);
  const rejectAllHunks = useEditorStore((state) => state.rejectAllHunks);
  const pushNotification = useNotificationStore((state) => state.pushNotification);

  if (!selectedFile) return null;

  const doc = documentContents[selectedFile];
  const diff = doc?.diff;
  const hunks = diff?.hunks ?? [];

  if (!diff?.isActive || hunks.length === 0) return null;

  const handleAcceptAll = () => {
    try {
      applyAllHunks(selectedFile);
    } catch (e) {
      const message = reportError('diff-accept-all', e);
      pushNotification({ kind: 'error', title: '应用全部修改失败', message });
    }
  };

  const handleRejectAll = () => {
    try {
      rejectAllHunks(selectedFile);
    } catch (e) {
      const message = reportError('diff-reject-all', e);
      pushNotification({ kind: 'error', title: '拒绝全部修改失败', message });
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
