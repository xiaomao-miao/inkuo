import React, { useEffect, useMemo, useRef } from 'react';
import { Check, Loader2, FileEdit, Terminal, X, ChevronDown, ChevronRight } from 'lucide-react';
import type { DiffSummary } from '../../store';
import styles from './ToolCallCard.module.css';

interface ToolCallCardProps {
  id: string;
  name: string;
  arguments: Record<string, unknown>;
  /** Raw, possibly incomplete JSON string of the arguments. Used to render the
   * streaming preview when full JSON parsing is not yet possible. */
  rawArguments?: string;
  /** Streaming content extracted from partial JSON using jsonchunk. This is
   * the actual file content being written, displayed in real-time. */
  streamingContent?: string;
  status: 'pending' | 'executing' | 'success' | 'error';
  result?: string;
  error?: string;
  duration?: number;
  diffSummary?: DiffSummary;
  /** When true the arguments preview is treated as still streaming in and
   * the live container auto-scrolls to the bottom as new content arrives. */
  isStreamingArguments?: boolean;
}

const getToolDisplayName = (name: string): string => {
  const names: Record<string, string> = {
    read_file: '读取文件',
    write_file: '写入文件',
    edit_file: '编辑文件',
    list_dir: '列出目录',
    glob: '查找文件',
    grep: '搜索文本',
    read_office_file: '读取 Office',
    write_office_file: '写入 Office',
  };
  return names[name] || name;
};

const PREVIEW_STRING_KEYS = new Set(['content', 'new_text', 'pattern', 'json_content']);

export const ToolCallCard: React.FC<ToolCallCardProps> = React.memo(function ToolCallCard({
  id,
  name,
  arguments: args,
  rawArguments,
  streamingContent,
  status,
  error,
  duration,
  diffSummary,
  isStreamingArguments = false,
}) {
  const isFileModification = name === 'write_file' || name === 'edit_file' || name === 'write_office_file';
  const filePath = (args?.path as string | undefined) ?? (args?.file_path as string | undefined);
  const fileName = filePath
    ? filePath.split('/').pop() || filePath.split('\\').pop() || filePath
    : null;

  // Determine if tool is still executing
  const isExecuting = status === 'executing';

  // Determine final status - prefer merged card status if available
  const finalStatus = status === 'pending' && diffSummary ? 'success' : status;

  // Pick the most "interesting" string field to stream-preview (e.g. the
  // long `content` payload of write_file). Priority:
  // 1. streamingContent (extracted via jsonchunk from partial JSON)
  // 2. Long string fields from parsed args (content, new_text, etc.)
  // 3. Raw JSON string (fallback when JSON parsing fails)
  const preview = useMemo(() => {
    // Priority 1: Use streamingContent directly if available
    if (streamingContent && streamingContent.length > 0) {
      return { key: 'content', text: streamingContent };
    }

    if (!isFileModification && !rawArguments) return null;

    // Priority 2: Try the parsed object's long string fields.
    for (const key of PREVIEW_STRING_KEYS) {
      const v = args?.[key];
      if (typeof v === 'string' && v.length > 0) {
        return { key, text: v };
      }
    }

    // Priority 3: Fall back to raw JSON
    if (rawArguments && rawArguments.length > 0) {
      return { key: 'raw', text: rawArguments };
    }
    return null;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [rawArguments, streamingContent, isFileModification, name, args]);

  const previewRef = useRef<HTMLPreElement | null>(null);
  const [isExpanded, setIsExpanded] = React.useState(true);
  const [isDiffExpanded, setIsDiffExpanded] = React.useState(false);

  // Auto-scroll the live preview to the bottom as new text streams in.
  useEffect(() => {
    if (!isStreamingArguments) return;
    const el = previewRef.current;
    if (!el) return;
    // Stick to bottom — only when user hasn't scrolled up manually.
    const distanceFromBottom = el.scrollHeight - el.scrollTop - el.clientHeight;
    if (distanceFromBottom < 80) {
      el.scrollTop = el.scrollHeight;
    }
  }, [preview?.text, isStreamingArguments]);

  return (
    <div className={`${styles.card} ${styles[finalStatus]}`} data-tool-call-id={id}>
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
          {isExecuting && (
            <>
              <Loader2 size={12} className={styles.spinning} />
              <span>{isStreamingArguments ? '生成参数中...' : '执行中'}</span>
            </>
          )}
          {!isExecuting && finalStatus === 'success' && (
            <>
              <Check size={12} />
              <span>成功</span>
            </>
          )}
          {!isExecuting && finalStatus === 'error' && (
            <>
              <X size={12} />
              <span>失败</span>
            </>
          )}
          {!isExecuting && finalStatus === 'pending' && (
            <span>等待</span>
          )}
          {duration !== undefined && (
            <span className={styles.duration}>{duration}ms</span>
          )}
        </div>
      </div>

      {/* Live streaming preview of the tool arguments (e.g. write_file content).
          This is the key piece: it appears the moment the tool card is shown
          (the first SSE delta) and the content inside streams in real time. */}
      {preview && preview.text.length > 0 && (
        <div className={styles.previewSection}>
          <button
            type="button"
            className={styles.previewToggle}
            onClick={() => setIsExpanded((v) => !v)}
          >
            {isExpanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
            <span>
              {preview.key === 'content' || preview.key === 'new_text' || preview.key === 'json_content'
                ? `内容预览`
                : '参数预览'}
              {preview.text.length > 0 && (
                <span className={styles.previewSize}>
                  {' · '}
                  {preview.text.length.toLocaleString()} 字符
                </span>
              )}
            </span>
          </button>
          {isExpanded && (
            <div className={styles.previewContainer}>
              <pre ref={previewRef} className={styles.previewContent}>
                {preview.text}
                {isStreamingArguments && <span className={styles.streamingCaret} />}
              </pre>
            </div>
          )}
        </div>
      )}

      {/* Line counts for file modifications */}
      {diffSummary && (
        <div className={styles.lineCounts}>
          <button
            type="button"
            className={styles.diffToggle}
            onClick={() => setIsDiffExpanded((v) => !v)}
          >
            {isDiffExpanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
            <span className={styles.added}>+{diffSummary.added_lines}</span>
            <span className={styles.deleted}>-{diffSummary.deleted_lines}</span>
            <span className={styles.fileNameLabel}>{diffSummary.file_name}</span>
          </button>
        </div>
      )}

      {/* Line-level diff - collapsed by default */}
      {diffSummary && diffSummary.hunks.length > 0 && isDiffExpanded && (
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

      {/* Error display */}
      {error && (
        <div className={styles.error}>
          <span className={styles.errorLabel}>错误:</span>
          <pre className={styles.errorContent}>{error}</pre>
        </div>
      )}
    </div>
  );
});
