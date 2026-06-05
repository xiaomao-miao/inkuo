import React, { useEffect, useMemo, useRef } from 'react';
import { Check, Loader2, FileEdit, Terminal, X, ChevronDown, ChevronRight, FolderOpen, FileText, Search, FolderPlus, Move } from 'lucide-react';
import type { StreamDiffSummary } from '../../store';
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
  diffSummary?: StreamDiffSummary;
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
    create_dir: '创建目录',
    knowledge_build: '构建知识库',
  };
  return names[name] || name;
};

const PREVIEW_STRING_KEYS = new Set(['content', 'new_text', 'pattern', 'json_content']);

// Tool categories for compact display
const FILE_MODIFICATION_TOOLS = new Set(['write_file', 'edit_file', 'write_office_file']);
export const COMPACT_TOOLS = new Set(['list_dir', 'glob', 'grep', 'read_file', 'read_office_file', 'create_dir', 'move_file']);

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
  const isFileModification = FILE_MODIFICATION_TOOLS.has(name);
  const filePath = (args?.path as string | undefined) ?? (args?.file_path as string | undefined);
  const fileName = filePath
    ? filePath.split('/').pop() || filePath.split('\\').pop() || filePath
    : null;

  // Determine if tool is still executing
  const isExecuting = status === 'executing';

  // Determine final status - prefer merged card status if available
  const finalStatus = status === 'pending' && diffSummary ? 'success' : status;

  // Hide cursor when not streaming
  const showCursor = isStreamingArguments && isExecuting;

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

    const knowledgeProgress = args?.progress;
    const knowledgeCurrentFile = args?.current_file;
    if (name === 'knowledge_build') {
      const lines = ['正在构建知识库'];
      if (typeof knowledgeProgress === 'string' && knowledgeProgress.length > 0) {
        lines.push(`进度: ${knowledgeProgress}`);
      }
      if (typeof knowledgeCurrentFile === 'string' && knowledgeCurrentFile.length > 0) {
        lines.push(`当前文件: ${knowledgeCurrentFile}`);
      }
      return { key: 'content', text: lines.join('\n') };
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
                {showCursor && <span className={styles.streamingCaret} />}
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

// Compact card for non-file-modification tools (list_dir, read_file, glob, grep, etc.)
interface CompactToolCardProps {
  id: string;
  name: string;
  arguments: Record<string, unknown>;
  status: 'pending' | 'executing' | 'success' | 'error';
  duration?: number;
}

const getToolIcon = (name: string) => {
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
};

export const CompactToolCard: React.FC<CompactToolCardProps> = React.memo(function CompactToolCard({
  id,
  name,
  arguments: args,
  status,
  duration,
}) {
  const isExecuting = status === 'executing';
  const filePath = (args?.path as string | undefined) ?? (args?.file_path as string | undefined);
  const fileName = filePath
    ? filePath.split('/').pop() || filePath.split('\\').pop() || filePath
    : null;
  const pattern = (args?.pattern as string | undefined) ?? (args?.glob as string | undefined);
  
  const sourcePath = (args?.source_path as string | undefined) ?? (args?.source as string | undefined);
  const isMoveFile = name === 'move_file';
  
  // For create_dir
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
            {sourcePath.split('/').pop()}
          </span>
        )}
        {isCreateDir && dirPath && (
          <span className={styles.compactFileName}>{dirPath}</span>
        )}
        {!isMoveFile && !isCreateDir && fileName && <span className={styles.compactFileName}>{fileName}</span>}
        {!isMoveFile && !isCreateDir && pattern && !fileName && <span className={styles.compactFileName}>{pattern}</span>}
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
