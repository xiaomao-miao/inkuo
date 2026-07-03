import React, { useCallback } from 'react';
import { ChevronDown, ChevronUp } from 'lucide-react';
import { MarkdownRenderer } from './MarkdownRenderer';
import { StreamingMarkdownRenderer } from './StreamingMarkdownRenderer';
import { useAIPanelStore } from '../../store';
import { TIMING } from '../../constants/timing';
import styles from './AIPanelMessage.module.css';

interface LazyTextContentProps {
  messageId: string;
  sessionId: string;
  /**
   * The visible (already-truncated) content. Always rendered as-is.
   */
  visibleContent: string;
  /**
   * Chars held in `truncatedPrefix` (the collapsed head). When non-empty, we
   * render the "load earlier content" affordance above the rendered markdown.
   */
  truncatedPrefixLength: number;
  /**
   * Whether to use the streaming (markdown-aware, safe-boundary) renderer or
   * the static one. Streaming also shows the live caret when used with the
   * pending indicator.
   */
  isStreaming: boolean;
  /**
   * Callback when user clicks on a file path in markdown content.
   */
  onFileClick?: (filePath: string) => void;
  /**
   * Current workspace root path for resolving relative file paths.
   */
  workspacePath?: string;
}

/**
 * Render an assistant message's trailing text OutputItem (or its `content`
 * fallback) with a lazy-load affordance when the head of the text is held
 * back in `truncatedPrefix`.
 *
 * The affordance shows the size of the collapsed head and exposes two
 * actions:
 *   - "展开" → splice the head back in, keeping the tail at
 *     `MESSAGE_TRUNCATE_KEEP_TAIL_CHARS` to bound re-rendering.
 *   - "收起" → collapse the head again (useful after reading).
 */
export const LazyTextContent: React.FC<LazyTextContentProps> = ({
  messageId,
  sessionId,
  visibleContent,
  truncatedPrefixLength,
  isStreaming,
  onFileClick,
  workspacePath,
}) => {
  const expandMessagePrefix = useAIPanelStore((state) => state.expandMessagePrefix);
  const collapseMessagePrefix = useAIPanelStore((state) => state.collapseMessagePrefix);

  const handleExpand = useCallback(() => {
    expandMessagePrefix(sessionId, messageId, TIMING.MESSAGE_TRUNCATE_KEEP_TAIL_CHARS);
  }, [expandMessagePrefix, sessionId, messageId]);

  const handleCollapse = useCallback(() => {
    collapseMessagePrefix(sessionId, messageId, TIMING.MESSAGE_TRUNCATE_KEEP_TAIL_CHARS);
  }, [collapseMessagePrefix, sessionId, messageId]);

  if (!visibleContent && truncatedPrefixLength === 0) return null;

  return (
    <>
      {truncatedPrefixLength > 0 && (
        <div className={styles.truncatedFold}>
          <span className={styles.truncatedFoldSummary}>
            前面还有 {truncatedPrefixLength.toLocaleString()} 字符未加载
          </span>
          <button
            type="button"
            className={styles.truncatedFoldBtn}
            onClick={handleExpand}
            title="展开上方被截断的内容"
          >
            <ChevronUp size={12} />
            展开
          </button>
          {visibleContent.length > TIMING.MESSAGE_TRUNCATE_KEEP_TAIL_CHARS && (
            <button
              type="button"
              className={styles.truncatedFoldBtn}
              onClick={handleCollapse}
              title="收起上方内容"
            >
              <ChevronDown size={12} />
              收起
            </button>
          )}
        </div>
      )}
      {visibleContent &&
        (isStreaming ? (
          <StreamingMarkdownRenderer
            content={visibleContent}
            isStreaming={true}
            onFileClick={onFileClick}
            workspacePath={workspacePath}
          />
        ) : (
          <MarkdownRenderer
            content={visibleContent}
            onFileClick={onFileClick}
            workspacePath={workspacePath}
          />
        ))}
    </>
  );
};