import React, { useState } from 'react';
import { X, MessageSquare, PlusCircle, Search, Trash2, MessageSquareOff } from 'lucide-react';
import type { ChatSession } from '../../store';
import { EmptyState } from '../common/EmptyState';
import styles from './HistorySidebar.module.css';

interface HistorySidebarProps {
  sessions: ChatSession[];
  activeSessionId: string | null;
  /**
   * Restore the session back into the header chip bar AND make it the
   * active one. Used for both freshly-archived sessions (auto reopens
   * them) and already-open sessions in the sidebar (no-op for the
   * archived flag, but still activates).
   */
  onActivate: (sessionId: string) => void;
  onNewChat: () => void;
  /** Permanent delete — pass through confirmation logic in the caller. */
  onDelete: (sessionId: string) => void;
  onClose: () => void;
}

const MAX_TITLE_LEN = 32;

function getSessionTitle(session: ChatSession): string {
  const firstUser = session.messages.find((m) => m.role === 'user');
  if (firstUser?.content) {
    const text = firstUser.content.trim();
    return text.length > MAX_TITLE_LEN
      ? text.slice(0, MAX_TITLE_LEN) + '…'
      : text;
  }
  return '新对话';
}

function formatDate(timestamp: number): string {
  const now = Date.now();
  const diff = now - timestamp;
  const day = 86400000;

  if (diff < day) return '今天';
  if (diff < 2 * day) return '昨天';
  if (diff < 7 * day) return '本周';
  if (diff < 30 * day) return '本月';
  return new Date(timestamp).toLocaleDateString('zh-CN', { month: 'short', day: 'numeric' });
}

function getActivityAt(session: ChatSession): number {
  // Prefer lastActivityAt (updated on send/receive/reopen/clear) and
  // fall back to createdAt for old sessions persisted before the
  // field existed.
  return session.lastActivityAt ?? session.createdAt;
}

function groupByDate(sessions: ChatSession[]): [string, ChatSession[]][] {
  const groups = new Map<string, ChatSession[]>();
  for (const s of sessions) {
    const label = formatDate(getActivityAt(s));
    if (!groups.has(label)) groups.set(label, []);
    groups.get(label)!.push(s);
  }
  // Within each date bucket, newest activity first. Newly answered or
  // reopened conversations bubble to the top of their group.
  for (const [, items] of groups) {
    items.sort((a, b) => getActivityAt(b) - getActivityAt(a));
  }
  return [...groups.entries()];
}

export const HistorySidebar: React.FC<HistorySidebarProps> = ({
  sessions,
  activeSessionId,
  onActivate,
  onNewChat,
  onDelete,
  onClose,
}) => {
  const [search, setSearch] = useState('');

  const filtered = search.trim()
    ? sessions.filter((s) => getSessionTitle(s).toLowerCase().includes(search.toLowerCase()))
    : sessions;

  const groups = groupByDate(filtered);

  return (
    <div className={styles.sidebar}>
      <div className={styles.header}>
        <span className={styles.title}>历史对话</span>
        <div className={styles.headerActions}>
          <button
            className={styles.newBtn}
            onClick={onNewChat}
            title="新建对话"
            type="button"
          >
            <PlusCircle size={15} />
          </button>
          <button
            className={styles.closeBtn}
            onClick={onClose}
            title="关闭"
            type="button"
          >
            <X size={15} />
          </button>
        </div>
      </div>

      <div className={styles.searchBar}>
        <Search size={13} className={styles.searchIcon} />
        <input
          type="text"
          className={styles.searchInput}
          placeholder="搜索对话…"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />
      </div>

      <div className={styles.sessionList}>
        {groups.length === 0 ? (
          <EmptyState
            icon={MessageSquareOff}
            title={search ? '没有找到匹配的对话' : '暂无历史对话'}
            description={search ? '试试别的关键词' : '开启一个新的对话吧'}
          />
        ) : (
          groups.map(([dateLabel, group]) => (
            <div key={dateLabel} className={styles.group}>
              <div className={styles.groupLabel}>{dateLabel}</div>
              {group.map((session) => (
                <button
                  key={session.id}
                  type="button"
                  className={`${styles.sessionItem} ${
                    session.id === activeSessionId ? styles.sessionActive : ''
                  } ${session.archived ? styles.sessionArchived : ''}`}
                  onClick={() => onActivate(session.id)}
                >
                  <MessageSquare size={13} className={styles.sessionIcon} />
                  <span className={styles.sessionTitle}>
                    {getSessionTitle(session)}
                    {session.archived && (
                      <span className={styles.archivedBadge}>已关闭</span>
                    )}
                  </span>
                  <button
                    className={styles.deleteBtn}
                    onClick={(e) => {
                      e.stopPropagation();
                      if (window.confirm('确定永久删除此对话？此操作不可撤销。')) {
                        onDelete(session.id);
                      }
                    }}
                    title="永久删除"
                    type="button"
                  >
                    <Trash2 size={12} />
                  </button>
                </button>
              ))}
            </div>
          ))
        )}
      </div>
    </div>
  );
};
