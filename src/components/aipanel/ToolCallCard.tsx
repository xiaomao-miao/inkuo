import React, { useEffect, useMemo, useRef } from 'react';
import { Check, Loader2, FileEdit, Terminal, X, ChevronDown, ChevronRight } from 'lucide-react';
import type { StreamDiffSummary } from '../../store';
import { TIMING } from '../../constants/timing';
import {
  getToolDisplayName,
  isFileModificationTool,
  PREVIEW_STRING_KEYS,
  extractFileNameFromPath,
  formatArgumentsForDisplay,
} from './toolUtils';
import {
  ToolResultImagePreview,
  hasImageResultPreview,
} from './ToolResultImagePreview';
import styles from './ToolCallCard.module.css';

interface ToolCallCardProps {
  id: string;
  name: string;
  arguments: Record<string, unknown>;
  rawArguments?: string;
  streamingContent?: string;
  status: 'pending' | 'executing' | 'success' | 'error';
  result?: string;
  error?: string;
  duration?: number;
  diffSummary?: StreamDiffSummary;
  isStreamingArguments?: boolean;
  /** Callback when user clicks on a file name in diff summary */
  onFileClick?: (filePath: string) => void;
  /** Current workspace root path for resolving relative file paths */
  workspacePath?: string;
}

interface ToolPreview {
  key: string;
  text: string;
}

function resolveToolPreview(
  name: string,
  args: Record<string, unknown>,
  rawArguments: string | undefined,
  streamingContent: string | undefined
): ToolPreview | null {
  if (streamingContent && streamingContent.length > 0) {
    return { key: 'content', text: streamingContent };
  }

  if (name === 'knowledge_build') {
    const knowledgeProgress = args?.progress;
    const knowledgeCurrentFile = args?.current_file;
    const lines = ['正在构建知识库'];
    if (typeof knowledgeProgress === 'string' && knowledgeProgress.length > 0) {
      lines.push(`进度: ${knowledgeProgress}`);
    }
    if (typeof knowledgeCurrentFile === 'string' && knowledgeCurrentFile.length > 0) {
      lines.push(`当前文件: ${knowledgeCurrentFile}`);
    }
    return { key: 'content', text: lines.join('\n') };
  }

  const isFileModification = isFileModificationTool(name);
  if (!isFileModification && !rawArguments) return null;

  for (const key of PREVIEW_STRING_KEYS) {
    const v = args?.[key];
    if (typeof v === 'string' && v.length > 0) {
      return { key, text: v };
    }
  }

  // Use human-readable formatting instead of raw JSON
  const hasParsedArgs = args && Object.keys(args).length > 0;
  const formatted = formatArgumentsForDisplay(
    name,
    hasParsedArgs ? args : null,
    rawArguments
  );
  if (formatted) {
    return { key: 'args', text: formatted };
  }

  return null;
}

const ToolCardHeader: React.FC<{
  name: string;
  fileName: string | null;
  isFileModification: boolean;
  isWorking: boolean;
  isExecuting: boolean;
  isStreamingArguments: boolean;
  finalStatus: string;
  duration?: number;
}> = ({ name, fileName, isFileModification, isWorking, isExecuting, isStreamingArguments, finalStatus, duration }) => (
  <div className={styles.header}>
    <div className={styles.headerLeft}>
      <div className={styles.icon}>
        {isFileModification ? <FileEdit size={14} /> : <Terminal size={14} />}
      </div>
      <span className={styles.toolName}>{getToolDisplayName(name)}</span>
      {fileName && <span className={styles.fileName}>{fileName}</span>}
    </div>
    <div className={styles.headerRight}>
      {isWorking && (
        <>
          <Loader2 size={12} className={styles.spinning} />
          <span>{isExecuting ? (isStreamingArguments ? '生成参数中...' : '执行中') : '等待执行'}</span>
        </>
      )}
      {!isWorking && finalStatus === 'success' && (
        <>
          <Check size={12} />
          <span>成功</span>
        </>
      )}
      {!isWorking && finalStatus === 'error' && (
        <>
          <X size={12} />
          <span>失败</span>
        </>
      )}
      {duration !== undefined && (
        <span className={styles.duration}>{duration}ms</span>
      )}
    </div>
  </div>
);

const ToolCardPreview: React.FC<{
  preview: ToolPreview | null;
  showCursor: boolean;
  isStreamingArguments: boolean;
  toolName: string;
}> = ({ preview, showCursor, isStreamingArguments }) => {
  const [isExpanded, setIsExpanded] = React.useState(true);
  const previewRef = useRef<HTMLPreElement | null>(null);

  const previewMaxHeight = '80px';

  useEffect(() => {
    if (!isStreamingArguments) return;
    const el = previewRef.current;
    if (!el) return;
    const distanceFromBottom = el.scrollHeight - el.scrollTop - el.clientHeight;
    if (distanceFromBottom < TIMING.TOOL_CALL_AUTOSCROLL_THRESHOLD_PX) {
      el.scrollTop = el.scrollHeight;
    }
  }, [preview?.text, isStreamingArguments]);

  if (!preview || preview.text.length === 0) return null;

  return (
    <div className={styles.previewSection}>
      <button
        type="button"
        className={styles.previewToggle}
        onClick={() => setIsExpanded((v) => !v)}
      >
        {isExpanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
        <span>
          {preview.key === 'content' || preview.key === 'new_text' || preview.key === 'json_content'
            ? '内容预览'
            : preview.key === 'args'
              ? '参数'
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
        <div className={styles.previewContainer} style={{ maxHeight: previewMaxHeight }}>
          <pre ref={previewRef} className={styles.previewContent}>
            {preview.text}
            {showCursor && <span className={styles.streamingCaret} />}
          </pre>
        </div>
      )}
    </div>
  );
};

const ToolCardDiff: React.FC<{
  diffSummary: StreamDiffSummary | undefined;
  onFileClick?: (filePath: string) => void;
  workspacePath?: string;
}> = ({ diffSummary, onFileClick, workspacePath }) => {
  const [isDiffExpanded, setIsDiffExpanded] = React.useState(false);

  if (!diffSummary) return null;

  const handleFileClick = () => {
    if (onFileClick && diffSummary.file_name) {
      const fullPath = workspacePath
        ? `${workspacePath}/${diffSummary.file_name}`
        : diffSummary.file_name;
      onFileClick(fullPath);
    }
  };

  return (
    <>
      <div className={styles.lineCounts}>
        <button
          type="button"
          className={styles.diffToggle}
          onClick={() => setIsDiffExpanded((v) => !v)}
        >
          {isDiffExpanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
          <span className={styles.added}>+{diffSummary.added_lines}</span>
          <span className={styles.deleted}>-{diffSummary.deleted_lines}</span>
          <button
            type="button"
            className={styles.fileNameButton}
            onClick={(e) => {
              e.stopPropagation();
              handleFileClick();
            }}
            title="点击打开文件"
          >
            {diffSummary.file_name}
          </button>
        </button>
      </div>
      {diffSummary.hunks.length > 0 && isDiffExpanded && (
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
    </>
  );
};

const ToolCardError: React.FC<{ error?: string }> = ({ error }) => {
  if (!error) return null;
  return (
    <div className={styles.error}>
      <span className={styles.errorLabel}>错误:</span>
      <pre className={styles.errorContent}>{error}</pre>
    </div>
  );
};

export const ToolCallCard: React.FC<ToolCallCardProps> = React.memo(function ToolCallCard({
  id,
  name,
  arguments: args,
  rawArguments,
  streamingContent,
  status,
  result,
  error,
  duration,
  diffSummary,
  isStreamingArguments = false,
  onFileClick,
  workspacePath,
}) {
  const isFileModification = isFileModificationTool(name);
  const filePath = (args?.path as string | undefined) ?? (args?.file_path as string | undefined);
  const fileName = extractFileNameFromPath(filePath);

  const finalStatus = status === 'pending' && diffSummary ? 'success' : status;
  const isExecuting = finalStatus === 'executing';
  const isWorking = finalStatus === 'pending' || isExecuting;
  const showCursor = isStreamingArguments && isExecuting;

  // 当状态从 executing 切到 success / error 时,在节点上挂一个
  // `data-just-finished` 属性 1 秒,触发一次性的庆祝动画。
  const [justFinished, setJustFinished] = React.useState<
    'success' | 'error' | null
  >(null);
  const prevExecutingRef = useRef(isExecuting);
  useEffect(() => {
    if (prevExecutingRef.current && !isExecuting) {
      const next: 'success' | 'error' = error ? 'error' : 'success';
      setJustFinished(next);
      const t = window.setTimeout(() => setJustFinished(null), TIMING.TOOL_CALL_JUST_FINISHED_HOLD_MS);
      prevExecutingRef.current = false;
      return () => window.clearTimeout(t);
    }
    prevExecutingRef.current = isExecuting;
  }, [isExecuting, error]);

  const preview = useMemo(
    () => resolveToolPreview(name, args, rawArguments, streamingContent),
    [name, args, rawArguments, streamingContent]
  );

// For `generate_image` we want to surface the saved image directly,
// not just the JSON result blob. The path comes from the args (the
// LLM-supplied `output_path`) and matches the path stored by the
// Rust side on success.
const generatedImagePath =
  name === 'generate_image'
    ? (args?.output_path as string | undefined) ??
      (args?.outputPath as string | undefined)
    : undefined;

  const dataAttrs = useMemo<Record<string, string | undefined>>(() => {
    const attrs: Record<string, string | undefined> = {
      'data-tool-call-id': id,
      'data-status': finalStatus,
    };
    if (justFinished) {
      attrs['data-just-finished'] = justFinished;
    }
    return attrs;
  }, [id, finalStatus, justFinished]);

  return (
    <div className={`${styles.card} ${styles[finalStatus]}`} {...dataAttrs}>
      <ToolCardHeader
        name={name}
        fileName={fileName}
        isFileModification={isFileModification}
        isWorking={isWorking}
        isExecuting={isExecuting}
        isStreamingArguments={isStreamingArguments}
        finalStatus={finalStatus}
        duration={duration}
      />
      <ToolCardPreview
        preview={preview}
        showCursor={showCursor}
        isStreamingArguments={isStreamingArguments}
        toolName={name}
      />
      <ToolCardDiff
        diffSummary={diffSummary}
        onFileClick={onFileClick}
        workspacePath={workspacePath}
      />
      {hasImageResultPreview(name, finalStatus) && (
        <ToolResultImagePreview
          result={result}
          filePath={generatedImagePath}
          workspacePath={workspacePath}
          onFileClick={onFileClick}
        />
      )}
      <ToolCardError error={error} />
    </div>
  );
});
