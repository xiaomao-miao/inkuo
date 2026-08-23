import { AlertTriangle } from 'lucide-react';
import styles from './OfficeViewer.module.css';

interface ExternalFileConflictBannerProps {
  fileName: string;
  onKeepLocal: () => void;
  onReloadFromDisk: () => void;
}

/** Persistent, non-destructive choice shown when disk and editor diverge. */
export function ExternalFileConflictBanner({
  fileName,
  onKeepLocal,
  onReloadFromDisk,
}: ExternalFileConflictBannerProps) {
  return (
    <div className={styles.externalConflictBanner} role="alert" aria-live="assertive">
      <AlertTriangle size={17} aria-hidden="true" />
      <div className={styles.externalConflictMessage}>
        <strong>文件已在外部更新</strong>
        <span>
          {fileName} 还有未保存的本地修改。保留本地可继续编辑（下次保存会覆盖磁盘版本），
          或重新载入磁盘版本并放弃本地修改。
        </span>
      </div>
      <div className={styles.externalConflictActions}>
        <button type="button" onClick={onKeepLocal}>
          保留本地
        </button>
        <button type="button" className={styles.reloadExternalButton} onClick={onReloadFromDisk}>
          重新载入
        </button>
      </div>
    </div>
  );
}
