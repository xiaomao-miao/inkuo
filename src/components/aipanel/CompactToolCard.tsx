import React, { useEffect, useRef, useState } from 'react';
import { Check, Loader2, X, FolderOpen, FileText, Search, FolderPlus, Move, Terminal } from 'lucide-react';
import { TIMING } from '../../constants/timing';
import { getToolDisplayName, extractFileNameFromPath } from './toolUtils';
import styles from './ToolCallCard.module.css';

interface CompactToolCardProps {
  id: string;
  name: string;
  arguments: Record<string, unknown>;
  status: 'pending' | 'executing' | 'success' | 'error';
  duration?: number;
  error?: string;
}

function getToolIcon(name: string) {
  switch (name) {
    case 'list_dir':
      return <FolderOpen size={12} />;
    case 'create_dir':
      return <FolderPlus size={12} />;
    case 'move_file':
      return <Move size={12} />;
    case 'read_file':
    case 'read_office_file':
      return <FileText size={12} />;
    case 'glob':
    case 'grep':
      return <Search size={12} />;
    default:
      return <Terminal size={12} />;
  }
}

export const CompactToolCard: React.FC<CompactToolCardProps> = React.memo(function CompactToolCard({
  id,
  name,
  arguments: args,
  status,
  duration,
  error,
}) {
  const isExecuting = status === 'executing';
  const isWorking = status === 'pending' || isExecuting;
  const filePath = (args?.path as string | undefined) ?? (args?.file_path as string | undefined);
  const fileName = extractFileNameFromPath(filePath);
  const pattern = (args?.pattern as string | undefined) ?? (args?.glob as string | undefined);
  const sourcePath = (args?.source_path as string | undefined) ?? (args?.source as string | undefined);
  const isMoveFile = name === 'move_file';
  const dirPath = (args?.dir_path as string | undefined) ?? (args?.directory as string | undefined) ?? filePath;
  const isCreateDir = name === 'create_dir';

  // 完成时一次性庆祝动画:用 ref + effect 监听 executing → 非 executing 的转换,
  // 在节点上挂 `data-just-finished` 属性 1 秒,触发 ToolCallCard.module.css
  // 中的 success-flash / shake 动画。
  const [justFinished, setJustFinished] = useState<'success' | 'error' | null>(null);
  const prevExecutingRef = useRef(isWorking);
  useEffect(() => {
    if (prevExecutingRef.current && !isWorking) {
      const next: 'success' | 'error' = error ? 'error' : 'success';
      setJustFinished(next);
      const t = window.setTimeout(() => setJustFinished(null), TIMING.TOOL_CALL_JUST_FINISHED_HOLD_MS);
      prevExecutingRef.current = false;
      return () => window.clearTimeout(t);
    }
    prevExecutingRef.current = isWorking;
  }, [isWorking, error]);

  const dataAttrs = React.useMemo<Record<string, string | undefined>>(() => {
    const attrs: Record<string, string | undefined> = {
      'data-tool-call-id': id,
      'data-status': status,
    };
    if (justFinished) {
      attrs['data-just-finished'] = justFinished;
    }
    return attrs;
  }, [id, status, justFinished]);

  return (
    <div className={`${styles.compactCard} ${styles[status]}`} {...dataAttrs}>
      <div className={styles.compactLeft}>
        <div className={`${styles.compactIcon} ${isWorking ? styles.compactIconExecuting : ''}`}>
          {getToolIcon(name)}
        </div>
        <span className={styles.compactName}>{getToolDisplayName(name)}</span>
        {isMoveFile && sourcePath && (
          <span className={styles.compactFileName}>
            {extractFileNameFromPath(sourcePath)}
          </span>
        )}
        {isCreateDir && dirPath && (
          <span className={styles.compactFileName}>{dirPath}</span>
        )}
        {!isMoveFile && !isCreateDir && fileName && (
          <span className={styles.compactFileName}>{fileName}</span>
        )}
        {!isMoveFile && !isCreateDir && pattern && !fileName && (
          <span className={styles.compactFileName}>{pattern}</span>
        )}
      </div>
      <div className={styles.compactRight}>
        {isWorking && (
          <Loader2 size={10} className={styles.spinning} />
        )}
        {!isWorking && status === 'success' && (
          <Check size={10} className={styles.compactSuccessIcon} />
        )}
        {!isWorking && status === 'error' && (
          <X size={10} className={styles.compactErrorIcon} />
        )}
        {duration !== undefined && (
          <span className={styles.compactDuration}>{duration}ms</span>
        )}
      </div>
    </div>
  );
});
