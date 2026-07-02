import React, { useEffect, useRef, useState } from 'react';
import { Brain, ChevronRight } from 'lucide-react';
import styles from './AIPanelMessage.module.css';

interface ReasoningBlockProps {
  content: string;
  /**
   * `true` once the assistant has begun emitting final-answer content,
   * marking the reasoning block as complete and eligible for auto-collapse.
   */
  completed: boolean;
  /**
   * Whether the user has explicitly expanded this block. Read from the
   * message-level `expandedReasoningIds` set; tied to a stable
   * `reasoningId` so each block is independent of its siblings.
   */
  userExpanded: boolean;
  /**
   * Stable id for this block. Used to add/remove the block from the
   * message's expanded-id set when the header is clicked.
   */
  reasoningId: string;
  /**
   * Toggle this block's "user expanded" state in the store. Called when
   * the user clicks the header.
   */
  onToggleExpansion: () => void;
}

/**
 * Collapsible container for a single reasoning block.
 *
 * Auto-collapse policy:
 *   - While the assistant is still streaming reasoning, the block is
 *     forced open so the user can watch the chain-of-thought build.
 *   - When `completed` flips to `true` and `userExpanded === false`, the
 *     block collapses automatically. The header stays visible so the user
 *     can re-open it on demand.
 *   - When the user clicks the header, `userExpanded` toggles. Each
 *     reasoning block has its own `reasoningId` so toggling one block
 *     does not affect any other block in the same message.
 */
export const ReasoningBlock: React.FC<ReasoningBlockProps> = ({
  content,
  completed,
  userExpanded,
  onToggleExpansion,
}) => {
  // While the assistant is still streaming reasoning, the block is
  // forced open regardless of `userExpanded`. Once streaming ends the
  // block collapses by default and only re-opens when the user clicks
  // the header.
  const isOpen = !completed || userExpanded;

  const startRef = useRef<number | null>(null);
  const [, force] = useState(0);
  useEffect(() => {
    if (completed) return;
    if (startRef.current === null) startRef.current = Date.now();
    const id = setInterval(() => force((n) => n + 1), 250);
    return () => clearInterval(id);
  }, [completed]);

  const elapsedMs = startRef.current ? Date.now() - startRef.current : 0;

  const handleHeaderClick = () => {
    if (!completed) return; // ignore clicks while still streaming
    onToggleExpansion();
  };

  const label = completed
    ? userExpanded
      ? '已展开思考过程'
      : '已思考完成（点击展开）'
    : '正在思考…';

  const durationLabel = !completed && elapsedMs > 0
    ? `${(elapsedMs / 1000).toFixed(1)}s`
    : null;

  return (
    <div className={styles.reasoningBlock}>
      <button
        type="button"
        className={styles.reasoningHeader}
        onClick={handleHeaderClick}
        aria-expanded={isOpen}
      >
        <span
          className={`${styles.reasoningChevron} ${isOpen ? styles.expanded : ''}`}
        >
          <ChevronRight size={12} />
        </span>
        <Brain size={12} />
        <span className={styles.reasoningLabel}>
          <span>{label}</span>
          {durationLabel && (
            <span className={styles.reasoningDuration}>{durationLabel}</span>
          )}
        </span>
      </button>
      <div
        className={`${styles.reasoningBody} ${isOpen ? '' : styles.collapsed} ${
          !completed ? styles.reasoningBodyStreaming : ''
        }`}
      >
        {content}
      </div>
    </div>
  );
};