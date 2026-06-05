import React, { useEffect, useRef } from 'react';
import { InlineDiffPreview } from './InlineDiffPreview';
import { ChatEmptyState } from './ChatEmptyState';
import { MessageItem } from './MessageItem';
import type {
  ChatMessage, ChatSession, ChatMode, ActiveToolCall, CurrentDiff,
} from '../../store';
import styles from './AIPanelChatView.module.css';

interface ChatViewProps {
  messages: ChatMessage[];
  activeSession: ChatSession | undefined;
  isStreaming: boolean;
  pendingDiff: CurrentDiff | null;
  mode: ChatMode;
  activeToolCalls: ActiveToolCall[];
  editingMessageId: string | null;
  editingContent: string;
  onStartEdit: (id: string, content: string) => void;
  onCancelEdit: () => void;
  onSaveEdit: () => void;
  onSetEditingContent: (v: string) => void;
  onSetInput: (v: string) => void;
  footer?: React.ReactNode;
}

export const ChatView: React.FC<ChatViewProps> = ({
  messages,
  activeSession,
  isStreaming,
  pendingDiff,
  mode,
  activeToolCalls,
  editingMessageId,
  editingContent,
  onStartEdit,
  onCancelEdit,
  onSaveEdit,
  onSetEditingContent,
  onSetInput,
  footer,
}) => {
  const contentRef = useRef<HTMLDivElement>(null);
  const isAtBottomRef = useRef(true);
  const messagesEndRef = useRef<HTMLDivElement>(null);

  const checkIfAtBottom = () => {
    if (!contentRef.current) return true;
    const { scrollTop, scrollHeight, clientHeight } = contentRef.current;
    isAtBottomRef.current = scrollHeight - scrollTop - clientHeight < 50;
  };

  useEffect(() => {
    if (isAtBottomRef.current || messages.length <= 2) {
      messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
    }
  }, [messages, activeToolCalls]);

  if (messages.length === 0) {
    return (
      <div className={styles.content} ref={contentRef}>
        <ChatEmptyState mode={mode} onSetInput={onSetInput} />
      </div>
    );
  }

  return (
    <div className={styles.content} ref={contentRef} onScroll={checkIfAtBottom}>
      <div className={styles.messages}>
        {messages.map((message) => (
          <MessageItem
            key={message.id}
            message={message}
            isStreaming={isStreaming}
            mode={mode}
            activeToolCalls={activeToolCalls}
            activeSession={activeSession}
            editingMessageId={editingMessageId}
            editingContent={editingContent}
            onStartEdit={onStartEdit}
            onCancelEdit={onCancelEdit}
            onSaveEdit={onSaveEdit}
            onSetEditingContent={onSetEditingContent}
            onSetInput={onSetInput}
          />
        ))}

        {pendingDiff && activeSession && (
          <InlineDiffPreview
            originalText={pendingDiff.originalText}
            newText={pendingDiff.newText}
            sessionId={activeSession.id}
            isStreaming={isStreaming}
          />
        )}

        {footer}
        <div ref={messagesEndRef} />
      </div>
    </div>
  );
};
