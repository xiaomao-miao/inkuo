import React, { useState } from 'react';
import { X, MessageSquare, PlusCircle, Search, Trash2, ChevronRight, RotateCcw } from 'lucide-react';
import type { ChatSession } from '../../store';
import styles from './HistorySidebar.module.css';

interface HistorySidebarProps {
  sessions: ChatSession[];
  activeSessionId: string | null;
  onSelect: (sessionId: string) => void;
  onReopen: (sessionId: string) => void;
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

function groupByDate(sessions: ChatSession[]): [string, ChatSession[]][] {
  const groups = new Map<string, ChatSession[]>();
  for (const s of sessions) {
    const label = formatDate(s.createdAt);
    if (!groups.has(label)) groups.set(label, []);
    groups.get(label)!.push(s);
  }
  return [...groups.entries()];
}

export const HistorySidebar: React.FC<HistorySidebarProps> = ({
  sessions,
  activeSessionId,
  onSelect,
  onReopen,
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
          <div className={styles.empty}>
            {search ? '没有找到匹配的对话' : '暂无历史对话'}
          </div>
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
                  onClick={() => onSelect(session.id)}
                >
                  <MessageSquare size={13} className={styles.sessionIcon} />
                  <span className={styles.sessionTitle}>
                    {getSessionTitle(session)}
                    {session.archived && (
                      <span className={styles.archivedBadge}>已关闭</span>
                    )}
                  </span>
                  {session.archived && (
                    <button
                      className={styles.reopenBtn}
                      onClick={(e) => {
                        e.stopPropagation();
                        onReopen(session.id);
                      }}
                      title="恢复到标签栏"
                      type="button"
                    >
                      <RotateCcw size={12} />
                    </button>
                  )}
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
                  <ChevronRight size={12} className={styles.chevron} />
                </button>
              ))}
            </div>
          ))
        )}
      </div>
    </div>
  );
};
