import React, { useRef, useEffect } from 'react';
import { Sparkles } from 'lucide-react';
import { InlineDiffPreview } from './InlineDiffPreview';
import { MessageItem } from './MessageItem';
import { ToolCallCard } from './ToolCallCard';
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
  knowledgeToolCall?: ActiveToolCall;
  knowledgeBuildProgress?: {
    phase: string;
    current: number;
    total: number;
    currentFile?: string;
  };
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
  knowledgeToolCall,
  knowledgeBuildProgress,
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
          <h3>文档助手</h3>
          <p>
            {mode === 'agent'
              ? '使用自然语言处理文档、总结内容、解释代码'
              : '询问关于文档的问题或请求 AI 帮助你写作'}
          </p>
          <div className={styles.quickActions}>
            <QuickActionButton label="总结文档" hint="总结这篇文档的主要内容" onSetInput={onSetInput} />
            <QuickActionButton label="解释内容" hint="解释这段代码/文本的工作原理" onSetInput={onSetInput} />
            {mode === 'agent' && (
              <QuickActionButton
                label="列出文档目录"
                hint="查看当前文档目录结构"
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

        {mode === 'knowledge' && knowledgeToolCall && (
          <div className={styles.toolResultItem}>
            <ToolCallCard
              id={knowledgeToolCall.id}
              name={knowledgeToolCall.name}
              arguments={{
                ...knowledgeToolCall.arguments,
                progress: knowledgeBuildProgress
                  ? `${knowledgeBuildProgress.phase} ${knowledgeBuildProgress.current}/${knowledgeBuildProgress.total}`
                  : knowledgeToolCall.result,
                current_file: knowledgeBuildProgress?.currentFile,
              }}
              status={knowledgeToolCall.status}
              result={knowledgeToolCall.result}
              error={knowledgeToolCall.error}
              duration={knowledgeToolCall.duration}
            />
          </div>
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
