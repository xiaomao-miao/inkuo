import React from 'react';
import { CheckCircle2, Loader2 } from 'lucide-react';
import { InlineDiffPreview } from './InlineDiffPreview';
import { AssistantMessageBody } from './AssistantMessageBody';
import { UserMessageBubble } from './UserMessageBubble';
import {
  type ChatMessage,
  type ActiveToolCall,
} from '../../store';
import styles from './AIPanelMessage.module.css';

interface MessageItemProps {
  message: ChatMessage;
  isStreaming: boolean;
  activeToolCalls: ActiveToolCall[];
  sessionId: string | undefined;
  streamingMessageId: string | undefined;
  displayMode: 'minimal' | 'detailed';
  editingMessageId: string | null;
  editingContent: string;
  onStartEdit: (id: string, content: string) => void;
  onCancelEdit: () => void;
  onSaveEdit: () => void;
  onSetEditingContent: (v: string) => void;
  onSetInput: (v: string) => void;
  /**
   * Animation delay (ms) for the message entry animation. Used to stagger
   * the last few messages of a freshly-arrived batch so they fade in
   * sequentially instead of all at once. 0 disables the stagger.
   */
  entryDelayMs?: number;
}

const MessageItemImpl: React.FC<MessageItemProps> = ({
  message,
  isStreaming,
  activeToolCalls,
  sessionId,
  streamingMessageId,
  displayMode,
  editingMessageId,
  editingContent,
  onStartEdit,
  onCancelEdit,
  onSaveEdit,
  onSetEditingContent,
  onSetInput,
  entryDelayMs = 0,
}) => {
  if (message.role === 'user') {
    return (
      <UserMessageBubble
        content={message.content || ''}
        imageAttachments={message.imageAttachments}
        isEditing={editingMessageId === message.id}
        editingContent={editingContent}
        isStreaming={isStreaming}
        onStartEdit={() => onStartEdit(message.id, message.content || '')}
        onCancelEdit={onCancelEdit}
        onSaveEdit={onSaveEdit}
        onSetEditingContent={onSetEditingContent}
        onSetInput={onSetInput}
      />
    );
  }

  if (message.role === 'tool') {
    return null;
  }

  if (message.role === 'assistant') {
    const isThisStreaming = isStreaming && message.id === streamingMessageId;
    const hasOutputItems = message.outputItems && message.outputItems.length > 0;
    const hasVisibleAnswer = Boolean(message.content?.trim()) || message.outputItems.some(
      (item) => item.type === 'text' && item.content.trim().length > 0,
    );

    return (
      <div
        className={`${styles.message} ${styles.assistant}`}
        style={entryDelayMs ? { animationDelay: `${entryDelayMs}ms` } : undefined}
      >
        <div className={styles.messageContent}>
          {sessionId && (
            <AssistantMessageBody
              message={message}
              isThisStreaming={isThisStreaming}
              activeToolCalls={activeToolCalls}
              sessionId={sessionId}
              minimal={displayMode === 'minimal'}
            />
          )}

          {displayMode === 'minimal' && !hasVisibleAnswer && (
            <div className={styles.minimalProgress} role="status">
              {isThisStreaming ? (
                <>
                  <Loader2 size={13} className={styles.spinning} />
                  <span>正在处理任务…</span>
                </>
              ) : (
                <>
                  <CheckCircle2 size={13} />
                  <span>任务已完成</span>
                </>
              )}
            </div>
          )}

          {message.diff && !isThisStreaming && sessionId && (
            <InlineDiffPreview
              originalText={message.diff.originalText}
              newText={message.diff.newText}
              sessionId={sessionId}
            />
          )}

          {isThisStreaming && !hasOutputItems && (
            <span className={styles.streamingCursor} />
          )}
        </div>
      </div>
    );
  }

  return null;
};

/**
 * Memoised wrapper. The default shallow comparator is enough: every
 * prop the parent passes is either a stable function ref (callbacks
 * defined once in `AIPanel.tsx`), a primitive, or a store-derived
 * value whose identity changes only when the underlying slice changes
 * (`message`, `activeSession`, `activeToolCalls`).
 *
 * Without the memo, the trailing live message's stream updates would
 * re-render every older `MessageItem` because `ChatView` re-renders
 * on every streaming token. The memo keeps the older rows' React
 * trees stable — only the streaming row's tree mutates.
 */
export const MessageItem = React.memo(MessageItemImpl);
