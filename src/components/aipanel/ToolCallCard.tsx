import React from 'react';
import { Check, X, Loader2, FileEdit, Terminal } from 'lucide-react';
import type { DiffSummary } from '../../store';
import styles from './ToolCallCard.module.css';

interface DiffChange {
  tag: 'delete' | 'insert' | 'equal';
  old_line: number | null;
  new_line: number | null;
  content: string;
}

interface DiffHunk {
  id: string;
  old_start: number;
  old_lines: number;
  new_start: number;
  new_lines: number;
  changes: DiffChange[];
}

interface ToolCallCardProps {
  id: string;
  name: string;
  arguments: Record<string, unknown>;
  status: 'pending' | 'executing' | 'success' | 'error';
  result?: string;
  error?: string;
  duration?: number;
  diffSummary?: DiffSummary;
  onAccept?: () => void;
  onReject?: () => void;
}

const getToolDisplayName = (name: string): string => {
  const names: Record<string, string> = {
    read_file: '读取文件',
    write_file: '写入文件',
    edit_file: '编辑文件',
    list_dir: '列出目录',
    glob: '查找文件',
    grep: '搜索文本',
  };
  return names[name] || name;
};

export const ToolCallCard: React.FC<ToolCallCardProps> = ({
  id,
  name,
  arguments: args,
  status,
  result,
  error,
  duration,
  diffSummary,
  onAccept,
  onReject,
}) => {
  const isFileModification = name === 'write_file' || name === 'edit_file';
  const filePath = args.path as string | undefined;
  const fileName = filePath
    ? filePath.split('/').pop() || filePath
    : null;

  return (
    <div className={`${styles.card} ${styles[status]}`}>
      {/* Header */}
      <div className={styles.header}>
        <div className={styles.headerLeft}>
          <div className={styles.icon}>
            {isFileModification ? (
              <FileEdit size={14} />
            ) : (
              <Terminal size={14} />
            )}
          </div>
          <span className={styles.toolName}>{getToolDisplayName(name)}</span>
          {fileName && (
            <span className={styles.fileName}>{fileName}</span>
          )}
        </div>
        <div className={styles.headerRight}>
          {status === 'executing' && (
            <>
              <Loader2 size={12} className={styles.spinning} />
              <span>执行中</span>
            </>
          )}
          {status === 'success' && (
            <>
              <Check size={12} />
              <span>成功</span>
            </>
          )}
          {status === 'error' && (
            <>
              <X size={12} />
              <span>失败</span>
            </>
          )}
          {status === 'pending' && (
            <span>等待</span>
          )}
          {duration !== undefined && (
            <span className={styles.duration}>{duration}ms</span>
          )}
        </div>
      </div>

      {/* Line counts for file modifications */}
      {diffSummary && (
        <div className={styles.lineCounts}>
          <span className={styles.added}>+{diffSummary.added_lines}</span>
          <span className={styles.deleted}>-{diffSummary.deleted_lines}</span>
          <span className={styles.fileNameLabel}>{diffSummary.file_name}</span>
        </div>
      )}

      {/* Line-level diff */}
      {diffSummary && diffSummary.hunks.length > 0 && (
        <div className={styles.diffContainer}>
          {diffSummary.hunks.map((hunk) => (
            <div key={hunk.id} className={styles.hunk}>
              <div className={styles.hunkHeader}>
                <span className={styles.hunkRange}>
                  @@ -{hunk.old_start},{hunk.old_lines} +{hunk.new_start},{hunk.new_lines} @@
                </span>
              </div>
              <div className={styles.diffLines}>
                {hunk.changes.map((change, idx) => (
                  <div
                    key={idx}
                    className={`${styles.diffLine} ${styles[change.tag]}`}
                  >
                    <span className={styles.lineNumber}>
                      {change.tag === 'delete' ? change.old_line : change.tag === 'insert' ? change.new_line : ''}
                    </span>
                    <span className={styles.linePrefix}>
                      {change.tag === 'delete' ? '-' : change.tag === 'insert' ? '+' : ' '}
                    </span>
                    <span className={styles.lineContent}>{change.content}</span>
                  </div>
                ))}
              </div>
            </div>
          ))}
        </div>
      )}

      {/* Action buttons */}
      {(status === 'success' || status === 'pending') && diffSummary && onAccept && onReject && (
        <div className={styles.actions}>
          <button className={styles.acceptBtn} onClick={onAccept}>
            <Check size={14} />
            接受
          </button>
          <button className={styles.rejectBtn} onClick={onReject}>
            <X size={14} />
            拒绝
          </button>
        </div>
      )}

      {/* Error display */}
      {error && (
        <div className={styles.error}>
          <span className={styles.errorLabel}>错误:</span>
          <pre className={styles.errorContent}>{error}</pre>
        </div>
      )}
    </div>
  );
};
