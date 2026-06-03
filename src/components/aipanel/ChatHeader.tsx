import React, { useRef } from 'react';
import { PlusCircle, MessageSquare, X } from 'lucide-react';
import type { ChatSession } from '../../store';
import styles from './AIPanel.module.css';

const MAX_TITLE_LEN = 20;

interface ChatHeaderProps {
  sessions: ChatSession[];
  activeSessionId: string | null;
  onCreateSession: () => void;
  onSelectSession: (id: string) => void;
  onDeleteSession: (id: string) => void;
  onClose: () => void;
}

export const ChatHeader: React.FC<ChatHeaderProps> = ({
  sessions,
  activeSessionId,
  onCreateSession,
  onSelectSession,
  onDeleteSession,
  onClose,
}) => {
  const sessionListRef = useRef<HTMLDivElement>(null);

  const handleWheel = (e: React.WheelEvent<HTMLDivElement>) => {
    if (sessionListRef.current) {
      sessionListRef.current.scrollLeft += e.deltaY;
    }
  };

  const getTitle = (session: ChatSession): string => {
    const firstUserMsg = session.messages?.find(m => m.role === 'user');
    if (firstUserMsg?.content) {
      return firstUserMsg.content.slice(0, MAX_TITLE_LEN) +
        (firstUserMsg.content.length > MAX_TITLE_LEN ? '...' : '');
    }
    return '新对话';
  };

  return (
    <div className={styles.header}>
      <div className={styles.sessionBar}>
        <button
          className={styles.newSessionBtn}
          onClick={onCreateSession}
          title="新建对话"
          type="button"
        >
          <PlusCircle size={16} />
        </button>
        <div className={styles.sessionList} ref={sessionListRef} onWheel={handleWheel}>
          {sessions.map((session) => (
            <button
              key={session.id}
              type="button"
              className={`${styles.sessionChip} ${session.id === activeSessionId ? styles.sessionActive : ''}`}
              onClick={() => onSelectSession(session.id)}
            >
              <MessageSquare size={12} />
              <span className={styles.sessionTitle}>{getTitle(session)}</span>
              {sessions.length > 1 && (
                <span
                  className={styles.sessionClose}
                  onClick={(e) => { e.preventDefault(); e.stopPropagation(); onDeleteSession(session.id); }}
                  title="关闭"
                >
                  <X size={11} />
                </span>
              )}
            </button>
          ))}
        </div>
      </div>
      <button className={styles.closeButton} title="关闭面板" onClick={onClose} type="button">
        <PanelRightCloseIcon />
      </button>
    </div>
  );
};

const PanelRightCloseIcon: React.FC = () => (
  <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
    <path d="M21 3H3m18 18H3m18-9V3m0 9v9"/>
  </svg>
);
