import React, { useLayoutEffect, useMemo, useRef, useState } from 'react';
import { Check, Circle, Loader2, ListChecks, ChevronDown } from 'lucide-react';
import { useAIPanelStore } from '../../store';
import type { TodoItem, TodoStatus } from '../../types';
import styles from './TodoPanel.module.css';

interface TodoPanelProps {
  sessionId: string;
}

interface PanelStats {
  total: number;
  completed: number;
  inProgress: number;
  /** The first in_progress item, if any. Shown in the collapsed header. */
  current: TodoItem | null;
  /** Progress percentage (0–100). Driven from completed/total. */
  percent: number;
}

const EMPTY_STATS: PanelStats = { total: 0, completed: 0, inProgress: 0, current: null, percent: 0 };

function computeStats(items: TodoItem[] | null): PanelStats {
  if (!items || items.length === 0) return EMPTY_STATS;
  let completed = 0;
  let inProgress = 0;
  let current: TodoItem | null = null;
  for (const item of items) {
    if (item.status === 'completed') completed += 1;
    else if (item.status === 'in_progress') {
      inProgress += 1;
      if (!current) current = item;
    }
  }
  const percent = Math.round((completed / items.length) * 100);
  return { total: items.length, completed, inProgress, current, percent };
}

/**
 * Reorder items so the user always sees progress top-down:
 *   1. in_progress (the one currently being worked on, at the top)
 *   2. completed (work that's done)
 *   3. pending (work still queued)
 *
 * Within each group we preserve the model's original order — that
 * matches how the model phrased the list in its snapshot, so flipping
 * statuses over time keeps a stable mental model for the user.
 */
function orderedForDisplay(items: TodoItem[]): TodoItem[] {
  const inProgress: TodoItem[] = [];
  const completed: TodoItem[] = [];
  const pending: TodoItem[] = [];
  for (const item of items) {
    if (item.status === 'in_progress') inProgress.push(item);
    else if (item.status === 'completed') completed.push(item);
    else pending.push(item);
  }
  return [...inProgress, ...completed, ...pending];
}

/**
 * Cursor-style task chip. Renders nothing when the active session has no
 * published todo snapshot. Collapsed by default — the header shows the
 * progress count and the current in-progress item's content; click to
 * expand the full checklist.
 *
 * Animations:
 *   - Header pulses a green dot while there's an in_progress item, so
 *     the eye is drawn to live work.
 *   - A 3px progress bar fills in under the header — width transition
 *     smoothly tweens between snapshots.
 *   - The list expands with a JS-measured height + opacity / translateY
 *     transition (same pattern as ChatInput's feature toolbar), and each
 *     row fades in with a small stagger so the list doesn't pop in.
 *   - Status flips (pending → in_progress → completed) animate via
 *     `background-color` / `border-color` transitions on the row.
 */
export const TodoPanel: React.FC<TodoPanelProps> = ({ sessionId }) => {
  const snapshot = useAIPanelStore(
    (state) => state.todoSnapshotBySession[sessionId] ?? null,
  );

  const [expanded, setExpanded] = useState(false);
  const bodyRef = useRef<HTMLDivElement | null>(null);

  const stats = useMemo(() => computeStats(snapshot?.items ?? null), [snapshot]);
  const orderedItems = useMemo(
    () => (snapshot ? orderedForDisplay(snapshot.items) : []),
    [snapshot],
  );

  // Same height-animation strategy as ChatInput.togglePanel — see that
  // hook for the full rationale. We pin `height` to a measured pixel
  // value so the browser doesn't have to lay out the panel on every
  // animation frame; the height transition itself runs on that fixed
  // value (compositor-friendly). Opacity + translateY in CSS handle the
  // visual settle.
  useLayoutEffect(() => {
    const el = bodyRef.current;
    if (!el) return;

    if (expanded) {
      const target = el.scrollHeight;
      el.style.transition = 'none';
      el.style.height = `${target}px`;
      requestAnimationFrame(() => {
        el.style.transition = '';
        const onEnd = (e: TransitionEvent) => {
          if (e.propertyName !== 'height') return;
          // Hand the panel back to natural height so it adapts to
          // future snapshots / content changes without re-measuring.
          el.style.height = 'auto';
          el.removeEventListener('transitionend', onEnd);
        };
        el.addEventListener('transitionend', onEnd);
      });
    } else {
      // Collapse path: pin current pixel height so the transition has
      // a start value, then on the next frame animate to 0.
      const current = el.getBoundingClientRect().height;
      el.style.transition = 'none';
      el.style.height = `${current}px`;
      requestAnimationFrame(() => {
        el.style.height = '0px';
        el.style.transition = '';
      });
    }
  }, [expanded, snapshot?.updatedAt]);

  // No published list yet → render nothing. Keeps the input bar visually
  // clean for sessions that haven't kicked off a multi-step task.
  if (!snapshot || snapshot.items.length === 0) {
    return null;
  }

  const allDone = stats.completed === stats.total && stats.total > 0;

  return (
    <div className={styles.panel} data-expanded={expanded || undefined}>
      <button
        type="button"
        className={styles.header}
        onClick={() => setExpanded((v) => !v)}
        aria-expanded={expanded}
      >
        {/* Left: icon + progress count + current task preview. The dot
         * is the visual heartbeat — pulses while work is in flight,
         * settles to a quiet grey when everything is done. */}
        <span className={styles.headerIcon}>
          <ListChecks size={14} />
        </span>
        <span className={`${styles.headerDot} ${allDone ? styles.headerDotIdle : ''}`} />
        <span className={styles.headerTitle}>
          {stats.completed}/{stats.total}
        </span>
        {stats.current ? (
          <span className={styles.headerCurrent}>{stats.current.content}</span>
        ) : allDone ? (
          <span className={`${styles.headerCurrent} ${styles.headerCurrentEmpty}`}>
            全部完成
          </span>
        ) : (
          <span className={`${styles.headerCurrent} ${styles.headerCurrentEmpty}`}>
            {stats.total - stats.completed - stats.inProgress} 项待办
          </span>
        )}
        <span className={styles.headerSpacer} />
        <ChevronDown
          size={14}
          className={`${styles.chevron} ${expanded ? styles.chevronExpanded : ''}`}
        />
      </button>

      <div
        className={`${styles.body} ${expanded ? styles.bodyOpen : ''}`}
        ref={bodyRef}
        aria-hidden={!expanded}
      >
        <div className={styles.bodyInner}>
          {/* Progress bar — always rendered inside the expanded body
           * (the body itself has its own height/opacity transition). */}
          <div className={styles.progressTrack}>
            <div
              className={`${styles.progressFill} ${
                allDone ? styles.progressFillComplete : ''
              }`}
              style={{ width: `${stats.percent}%` }}
            />
          </div>

          <ol className={styles.bodyInner} style={{ listStyle: 'none', margin: 0, padding: 0 }}>
            {orderedItems.map((item, idx) => (
              <TodoRow
                key={item.id}
                item={item}
                index={idx}
                staggerIndex={idx}
              />
            ))}
          </ol>
        </div>
      </div>
    </div>
  );
};

const TodoRow: React.FC<{ item: TodoItem; index: number; staggerIndex: number }> = ({
  item,
  index,
  staggerIndex,
}) => {
  const rowClass = (() => {
    if (item.status === 'in_progress') return styles.itemInProgress;
    if (item.status === 'completed') return styles.itemCompleted;
    return styles.itemPending;
  })();
  const iconClass = (() => {
    if (item.status === 'in_progress') return `${styles.itemIcon} ${styles.itemIconInProgress}`;
    if (item.status === 'completed') return `${styles.itemIcon} ${styles.itemIconCompleted}`;
    return `${styles.itemIcon} ${styles.itemIconPending}`;
  })();
  const indexClass = (() => {
    if (item.status === 'in_progress') return `${styles.itemIndex} ${styles.itemIndexInProgress}`;
    if (item.status === 'completed') return `${styles.itemIndex} ${styles.itemIndexCompleted}`;
    return styles.itemIndex;
  })();

  return (
    <li
      className={`${styles.item} ${rowClass}`}
      style={{ ['--item-index' as string]: String(staggerIndex) }}
      data-status={item.status}
    >
      <span className={indexClass}>{index + 1}</span>
      <span className={iconClass}>
        <StatusIcon status={item.status} />
      </span>
      <span className={styles.itemContent}>{item.content}</span>
    </li>
  );
};

const StatusIcon: React.FC<{ status: TodoStatus }> = ({ status }) => {
  if (status === 'completed') return <Check size={13} strokeWidth={2.5} />;
  if (status === 'in_progress') return <Loader2 size={13} strokeWidth={2.5} />;
  return <Circle size={13} strokeWidth={2} />;
};