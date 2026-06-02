import React, { useRef, useEffect } from 'react';
import { Sparkles } from 'lucide-react';
import { InlineDiffPreview } from './InlineDiffPreview';
import { MessageItem } from './MessageItem';
import type {
  ChatMessage, ChatSession, ChatMode, ActiveToolCall, CurrentDiff,
} from '../../store';
import styles from './AIPanel.module.css';

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
}) => {
  const contentRef = useRef<HTMLDivElement>(null);
  const isAtBottomRef = { current: true };
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
        <div className={styles.emptyState}>
          <div className={styles.emptyIcon}><Sparkles size={32} /></div>
          <h3>开始对话</h3>
          <p>
            {mode === 'agent'
              ? '使用 Agent 模式，可以帮你读写文件、搜索代码'
              : '询问关于文档的问题或请求 AI 帮助你写作'}
          </p>
          <div className={styles.quickActions}>
            <QuickActionButton label="总结文档" hint="总结这篇文档的主要内容" onSetInput={onSetInput} />
            <QuickActionButton label="解释内容" hint="解释这段代码/文本的工作原理" onSetInput={onSetInput} />
            {mode === 'agent' && (
              <QuickActionButton
                label="查看项目结构"
                hint="查看项目结构，列出 src 目录下的所有文件"
                onSetInput={onSetInput}
              />
            )}
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className={styles.content} ref={contentRef} onScroll={checkIfAtBottom}>
      <div className={styles.messages}>
        {messages.flatMap((message) => (
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
        <div ref={messagesEndRef} />
      </div>
    </div>
  );
};

interface QuickActionButtonProps {
  label: string;
  hint: string;
  onSetInput: (v: string) => void;
}

const QuickActionButton: React.FC<QuickActionButtonProps> = ({ label, hint, onSetInput }) => {
  return (
    <button
      className={styles.quickAction}
      onClick={() => onSetInput(hint)}
    >
      {label}
    </button>
  );
};
