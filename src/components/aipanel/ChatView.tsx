import React, { useEffect, useMemo, useRef, useState } from 'react';
import { InlineDiffPreview } from './InlineDiffPreview';
import { ChatEmptyState } from './ChatEmptyState';
import { CollapsedHistoryPlaceholder } from './CollapsedHistoryPlaceholder';
import { MessageItem } from './MessageItem';
import { SelectionQuickActions } from './SelectionQuickActions';
import { useAIPanelStore } from '../../store';
import { TIMING } from '../../constants/timing';
import type {
  ChatMessage, ChatSession, ActiveToolCall, CurrentDiff,
} from '../../store';
import styles from './AIPanelChatView.module.css';

interface ChatViewProps {
  messages: ChatMessage[];
  activeSession: ChatSession | undefined;
  isStreaming: boolean;
  pendingDiff: CurrentDiff | null;
  activeToolCalls: ActiveToolCall[];
  editingMessageId: string | null;
  editingContent: string;
  onStartEdit: (id: string, content: string) => void;
  onCancelEdit: () => void;
  onSaveEdit: () => void;
  onSetEditingContent: (v: string) => void;
  onSetInput: (v: string) => void;
  footer?: React.ReactNode;
  /**
   * Dispatch a fully-formed prompt for the floating selection toolbar.
   * `null` while the panel is still wiring up — the toolbar stays
   * hidden in that case so we never try to send with no handler.
   */
  onRunPrompt?: (prompt: string) => Promise<void> | void;
  /** Disable the selection toolbar (e.g. while the AI is streaming). */
  selectionToolbarDisabled?: boolean;
  displayMode: 'minimal' | 'detailed';
}

/**
 * Pixel distance from the top of the scroll container at which a new
 * batch of older history gets unfolded. The user has to actually scroll
 * up to (or past) this line before the expansion fires — passive
 * arrival at the placeholder's position is enough.
 */
const HISTORY_AUTOLOAD_COOLDOWN_MS = 160;

export const ChatView: React.FC<ChatViewProps> = ({
  messages,
  activeSession,
  isStreaming,
  pendingDiff,
  activeToolCalls,
  editingMessageId,
  editingContent,
  onStartEdit,
  onCancelEdit,
  onSaveEdit,
  onSetEditingContent,
  onSetInput,
  footer,
  onRunPrompt,
  selectionToolbarDisabled = false,
  displayMode,
}) => {
  const contentRef = useRef<HTMLDivElement>(null);
  const isAtBottomRef = useRef(true);
  const sessionId = activeSession?.id;

  /**
   * Render-only signal: drives the placeholder's spinner UI. We keep
   * this in state so the spinner re-renders, but the AUTHORITATIVE
   * "is a batch in flight?" check is the ref below — using state here
   * alone would leave a one-render-tick gap between calling
   * `expandCollapsedHistory` and React committing the new `false`,
   * during which a stray scroll event could re-enter and start a
   * second batch. The ref closes that gap.
   */
  const [isExpandingHistory, setIsExpandingHistory] = useState(false);
  /**
   * Synchronous lock. Set the moment a batch is dispatched, cleared in
   * the same RAF callback that performs the scroll-position
   * compensation. Stays `true` across the entire React-commit → layout
   * → RAF round-trip, so any concurrent scroll handler (mouse wheel,
   * keyboard, programmatic scrollTop mutation) sees the lock and bails.
   */
  const expandingRef = useRef(false);
  /**
   * Capture of scroll geometry + the partition's hiddenCount right
   * before a batch expansion. The effect below uses this to:
   *   1. Apply the scroll-position compensation (`scrollTop +=
   *      scrollHeight delta`) so the user's viewport stays anchored
   *      to the same message they were reading.
   *   2. Detect the case where a non-expand mutation (a new turn's
   *      `hardCollapseHistory`) ran in parallel — in that case the
   *      hiddenCount will have INCREASED since we snapshotted, the
   *      anchor is no longer valid, and we drop the compensation.
   */
  const pendingAnchorRef = useRef<
    { top: number; height: number; hiddenCountBefore: number } | null
  >(null);
  const lastExpandAtRef = useRef(0);

  const checkIfAtBottom = () => {
    if (!contentRef.current) return true;
    const { scrollTop, scrollHeight, clientHeight } = contentRef.current;
    isAtBottomRef.current = scrollHeight - scrollTop - clientHeight < 50;
  };

  /**
   * Per-message auto-expand: when the user scrolls near the top,
   * restore any per-message `truncatedPrefix` that's currently
   * collapsed. Debounced so a quick flick doesn't expand mid-stream.
   * (List-level expansion is a separate concern, see
   * `tryExpandHistory` below.)
   */
  const autoExpandRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const handleScrollForAutoExpand = () => {
    if (!contentRef.current || !sessionId) return;
    const el = contentRef.current;
    if (el.scrollTop > TIMING.TRUNCATED_PREFIX_AUTOEXPAND_SCROLL_PX) return;
    if (autoExpandRef.current !== null) {
      clearTimeout(autoExpandRef.current);
    }
    autoExpandRef.current = setTimeout(() => {
      useAIPanelStore.getState().autoExpandTruncatedPrefixes(sessionId);
      autoExpandRef.current = null;
    }, TIMING.HISTORY_AUTOLOAD_DEBOUNCE_MS);
  };

  useEffect(() => {
    return () => {
      if (autoExpandRef.current !== null) {
        clearTimeout(autoExpandRef.current);
        autoExpandRef.current = null;
      }
    };
  }, []);

  /**
   * List-level virtualization: partition the message array into a
   * collapsed head and a live tail. The renderer only mounts
   * MessageItem for the live tail; everything older is replaced by a
   * single compact placeholder card. The collapse is data-driven via
   * `message.collapsed` (set by `collapseOldMessages` in the store).
   *
   * This runs purely from props — `messages` is already the live state
   * from the store, so any subsequent `expandCollapsedHistory` /
   * `hardCollapseHistory` call is reflected on the next render
   * without us needing to copy state here.
   *
   * Note on deps: streaming token deltas mutate the trailing message's
   * `outputItems`/`content` in place. The store's `setState` always
   * produces a fresh `messages` reference, so depending on the array
   * is unavoidable — but the partition work itself is cheap (single
   * linear scan to the first non-collapsed message, typically the
   * head). The previous version was fine on the hot path; the
   * optimizations that actually matter for this render are below
   * (memo on `MessageItem`, Map lookups in `OutputItemView`).
   */
  const partition = useMemo(() => {
    let firstLiveIndex = 0;
    for (let i = 0; i < messages.length; i += 1) {
      if (!messages[i]?.collapsed) {
        firstLiveIndex = i;
        break;
      }
      firstLiveIndex = i + 1;
    }
    const hiddenCount = firstLiveIndex;
    const liveMessages = messages.slice(firstLiveIndex);
    return { hiddenCount, liveMessages };
  }, [messages]);

  /**
   * Try to expand one more batch of older history. Called from the
   * scroll handler whenever the viewport is near the top.
   *
   * The scroll-position compensation is the interesting bit: simply
   * mutating the messages array would shift the placeholder off-screen
   * and the user's viewport would jump downward by the height of the
   * newly-mounted messages. We snapshot the scroll geometry right
   * before mutating the store, then after React commits + the browser
   * lays out the new messages we adjust `scrollTop` by the same delta
   * the content grew by. End result: the same message that was at the
   * top of the viewport is still at the top.
   */
  const tryExpandHistory = () => {
    if (!contentRef.current || !sessionId) return;
    if (isStreaming) return;
    // Synchronous lock: closes the gap between dispatching the store
    // update and React committing the next render. Without the ref,
    // a stray scroll event in that gap could see stale
    // `isExpandingHistory === false` and re-enter.
    if (expandingRef.current) return;
    const now = Date.now();
    if (now - lastExpandAtRef.current < HISTORY_AUTOLOAD_COOLDOWN_MS) return;
    const el = contentRef.current;
    if (el.scrollTop > TIMING.HISTORY_AUTOLOAD_SCROLL_PX) return;
    // Are there still collapsed messages to release?
    const stillCollapsed = messages.some((m) => m.collapsed);
    if (!stillCollapsed) return;

    pendingAnchorRef.current = {
      top: el.scrollTop,
      height: el.scrollHeight,
      hiddenCountBefore: partition.hiddenCount,
    };
    lastExpandAtRef.current = now;
    expandingRef.current = true;
    setIsExpandingHistory(true);
    useAIPanelStore.getState().expandCollapsedHistory(sessionId);
  };

  /**
   * After every render, if we have a pending anchor snapshot, apply
   * the scroll-position compensation now that the new messages are
   * in the DOM. We do this in a layout-effect so it runs before the
   * browser paints the jump — visually the user never sees the
   * intermediate frame.
   *
   * If a non-expand mutation happened in parallel (a new turn's
   * hard-collapse, for example), the next render's partition will
   * show `hiddenCount > anchor.hiddenCountBefore`, in which case we
   * bail out of the compensation and let the normal scroll-to-end
   * behavior take over.
   */
  useEffect(() => {
    const anchor = pendingAnchorRef.current;
    const el = contentRef.current;
    if (!anchor || !el) return;
    // Sanity check: hiddenCount should have DECREASED (or be unchanged
    // if the user already had everything expanded). If it INCREASED,
    // a non-expand mutation ran in parallel — most commonly the
    // `hardCollapseHistory` that fires on a new turn — and our anchor
    // is no longer a valid basis for delta compensation. Drop it and
    // let the normal scroll-to-end behavior take over on the next
    // render.
    if (partition.hiddenCount > anchor.hiddenCountBefore) {
      pendingAnchorRef.current = null;
      expandingRef.current = false;
      setIsExpandingHistory(false);
      return;
    }
    // Defer one frame: React's commit phase has finished, but the
    // browser may not have run layout yet for newly-mounted children.
    // Reading scrollHeight synchronously here still races layout, so
    // we wait for the next animation frame.
    const rafId = requestAnimationFrame(() => {
      const target = contentRef.current;
      if (!target) return;
      const delta = target.scrollHeight - anchor.height;
      target.scrollTop = anchor.top + delta;
      pendingAnchorRef.current = null;
      expandingRef.current = false;
      setIsExpandingHistory(false);
    });
    return () => cancelAnimationFrame(rafId);
  }, [messages, partition.hiddenCount]);

  const scrollFrameRef = useRef<number | null>(null);
  useEffect(() => {
    if (!isAtBottomRef.current && messages.length > 2) return;
    if (scrollFrameRef.current !== null) return;
    scrollFrameRef.current = requestAnimationFrame(() => {
      scrollFrameRef.current = null;
      const content = contentRef.current;
      if (content) content.scrollTop = content.scrollHeight;
    });
  }, [messages, activeToolCalls]);

  useEffect(() => () => {
    if (scrollFrameRef.current !== null) cancelAnimationFrame(scrollFrameRef.current);
  }, []);

  /**
   * Make sure the session actually has the data-side collapsed flags
   * set whenever we're rendering it. The store action is idempotent
   * so it's safe to call on every active-session change; the goal is
   * to converge a freshly-loaded session (e.g. after a tab switch or
   * a remount) into the same shape as a session that was streamed
   * live. Without this the placeholder would never render for a
   * long-loaded session because the store has no chance to run its
   * pre-render collapse step.
   */
  useEffect(() => {
    if (!activeSession) return;
    const total = activeSession.messages.length;
    if (total <= TIMING.SESSION_VIRTUALIZE_THRESHOLD) return;
    const head = activeSession.messages.slice(0, total - TIMING.SESSION_VIRTUALIZE_THRESHOLD);
    const headCollapsed = head.every((m) => m.collapsed);
    if (headCollapsed) return;
    useAIPanelStore.getState().collapseOldMessages(activeSession.id);
    // We only care about session id + length for triggering; using the full
    // `activeSession` reference would re-run on every message mutation.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeSession?.id, activeSession?.messages.length]);

  /**
   * Reset the scroll-anchor machinery when the user switches sessions
   * — otherwise a stale anchor snapshot from the previous session
   * could fire on the new one and yank its scroll position.
   */
  useEffect(() => {
    pendingAnchorRef.current = null;
    expandingRef.current = false;
    setIsExpandingHistory(false);
  }, [activeSession?.id]);

  const streamingMessageId = useMemo(() => {
    if (!isStreaming) return undefined;
    for (let index = messages.length - 1; index >= 0; index -= 1) {
      if (messages[index]?.role === 'assistant') return messages[index].id;
    }
    return undefined;
  }, [isStreaming, messages]);

  if (messages.length === 0) {
    return (
      <div
        className={styles.content}
        ref={contentRef}
        data-aipanel-chat-content
        onScroll={() => {
          checkIfAtBottom();
          handleScrollForAutoExpand();
          tryExpandHistory();
        }}
      >
        <ChatEmptyState onSetInput={onSetInput} />
      </div>
    );
  }

  return (
    <div
      className={styles.content}
      ref={contentRef}
      data-aipanel-chat-content
      onScroll={() => {
        checkIfAtBottom();
        handleScrollForAutoExpand();
        tryExpandHistory();
      }}
    >
      <div className={styles.messages}>
        {partition.hiddenCount > 0 && (
          <CollapsedHistoryPlaceholder
            hiddenCount={partition.hiddenCount}
            busy={isStreaming}
            loading={isExpandingHistory}
          />
        )}

        {partition.liveMessages.map((message, idx) => (
          <MessageItem
            key={message.id}
            message={message}
            isStreaming={isStreaming}
            activeToolCalls={activeToolCalls}
            sessionId={sessionId}
            streamingMessageId={streamingMessageId}
            displayMode={displayMode}
            editingMessageId={editingMessageId}
            editingContent={editingContent}
            onStartEdit={onStartEdit}
            onCancelEdit={onCancelEdit}
            onSaveEdit={onSaveEdit}
            onSetEditingContent={onSetEditingContent}
            onSetInput={onSetInput}
            // 整批加载历史消息时不做 stagger(会一连串"叮叮叮"),只有
            // 当前 session 的尾部新消息才按 30ms 一条滑入。streaming
            // 期间也跳过,避免每 chunk 都重启动画。
            entryDelayMs={
              !isStreaming && idx >= Math.max(0, partition.liveMessages.length - 4)
                ? Math.min((idx - Math.max(0, partition.liveMessages.length - 4)) * 30, 90)
                : 0
            }
          />
        ))}

        {pendingDiff && sessionId && (
          <InlineDiffPreview
            originalText={pendingDiff.originalText}
            newText={pendingDiff.newText}
            sessionId={sessionId}
            isStreaming={isStreaming}
          />
        )}

        {footer}
      </div>
      {/* Floating toolbar: lives inside the scroll container so the
       *  "is the selection inside the chat?" check below is a single
       *  contains() call, but positions itself with `position: fixed`
       *  so it doesn't disturb the message flow. */}
      {onRunPrompt && (
        <SelectionQuickActions
          scrollContainer={contentRef.current}
          onSend={onRunPrompt}
          disabled={selectionToolbarDisabled}
        />
      )}
    </div>
  );
};
