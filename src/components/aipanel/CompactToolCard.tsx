import React from 'react';
import { Check, Loader2, X, FolderOpen, FileText, Search, FolderPlus, Move, Terminal } from 'lucide-react';
import { getToolDisplayName, extractFileNameFromPath } from './toolUtils';
import styles from './ToolCallCard.module.css';

interface CompactToolCardProps {
  id: string;
  name: string;
  arguments: Record<string, unknown>;
  status: 'pending' | 'executing' | 'success' | 'error';
  duration?: number;
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
}) {
  const isExecuting = status === 'executing';
  const filePath = (args?.path as string | undefined) ?? (args?.file_path as string | undefined);
  const fileName = extractFileNameFromPath(filePath);
  const pattern = (args?.pattern as string | undefined) ?? (args?.glob as string | undefined);
  const sourcePath = (args?.source_path as string | undefined) ?? (args?.source as string | undefined);
  const isMoveFile = name === 'move_file';
  const dirPath = (args?.dir_path as string | undefined) ?? (args?.directory as string | undefined) ?? filePath;
  const isCreateDir = name === 'create_dir';

  return (
    <div className={`${styles.compactCard} ${styles[status]}`} data-tool-call-id={id}>
      <div className={styles.compactLeft}>
        <div className={`${styles.compactIcon} ${isExecuting ? styles.compactIconExecuting : ''}`}>
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
        {isExecuting && (
          <Loader2 size={10} className={styles.spinning} />
        )}
        {!isExecuting && status === 'success' && (
          <Check size={10} className={styles.compactSuccessIcon} />
        )}
        {!isExecuting && status === 'error' && (
          <X size={10} className={styles.compactErrorIcon} />
        )}
        {duration !== undefined && (
          <span className={styles.compactDuration}>{duration}ms</span>
        )}
      </div>
    </div>
  );
});
