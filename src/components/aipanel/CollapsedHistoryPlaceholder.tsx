import React from 'react';
import { ChevronUp, Loader2 } from 'lucide-react';
import styles from './AIPanelMessage.module.css';

interface CollapsedHistoryPlaceholderProps {
  /**
   * How many older messages are folded behind this placeholder. The
   * renderer's live window starts after this many collapsed entries.
   */
  hiddenCount: number;
  /**
   * When `true`, the user already expanded once (so more history is now
   * live) and then a new turn was sent — every expanded placeholder got
   * re-flagged. We render the same compact card but suppress the
   * "load more" CTA so the user doesn't accidentally re-expand during
   * streaming.
   */
  busy?: boolean;
  /**
   * Optional manual "load earlier" CTA. With scroll-driven auto-expand
   * the ChatView typically doesn't pass this — but it's kept here as an
   * escape hatch (e.g. keyboard accessibility, fallback when JS scroll
   * detection is unavailable).
   */
  onLoadEarlier?: () => void;
  /**
   * When `true`, the placeholder is currently mid-fetch (a batch is
   * being unfolded and the DOM is about to grow). The card shows a
   * spinner instead of a chevron so the user gets feedback that more
   * history is on the way — useful because the scroll-position
   * compensation happens a frame later and a stationary card would
   * otherwise look frozen.
   */
  loading?: boolean;
}

/**
 * Compact card rendered in place of N older messages so the chat panel
 * DOM stays bounded regardless of how long the conversation gets. The
 * underlying message data is untouched in the store — when the user
 * scrolls near the top the parent expands the next batch and the real
 * messages re-mount.
 *
 * This is the list-level counterpart of the per-message
 * `LazyTextContent` "truncated prefix" affordance: both exist to keep
 * React from re-rendering dozens of (potentially markdown-heavy)
 * messages on every streaming token. Per-message truncation caps a
 * single message body; this card caps the message list itself.
 */
export const CollapsedHistoryPlaceholder: React.FC<CollapsedHistoryPlaceholderProps> = ({
  hiddenCount,
  busy,
  onLoadEarlier,
  loading,
}) => {
  if (hiddenCount <= 0) return null;
  return (
    <div
      className={styles.historyPlaceholder}
      role="separator"
      aria-label={`已折叠 ${hiddenCount} 条更早的消息`}
    >
      <div className={styles.historyPlaceholderRule} />
      <div className={styles.historyPlaceholderCard}>
        <span className={styles.historyPlaceholderSummary}>
          {loading ? (
            <>
              <Loader2 size={12} className={styles.historyPlaceholderSpinner} />
              正在加载更早的消息…
            </>
          ) : (
            <>前面还有 {hiddenCount.toLocaleString()} 条对话已收起，往上滚加载</>
          )}
        </span>
        {onLoadEarlier && !busy && !loading && (
          <button
            type="button"
            className={styles.historyPlaceholderBtn}
            onClick={onLoadEarlier}
            title="展开上方被收起的历史消息"
          >
            <ChevronUp size={12} />
            展开
          </button>
        )}
      </div>
    </div>
  );
};