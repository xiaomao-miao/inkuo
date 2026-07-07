import React from 'react';
import { InlineDiffPreview } from './InlineDiffPreview';
import { AssistantMessageBody } from './AssistantMessageBody';
import { UserMessageBubble } from './UserMessageBubble';
import {
  type ChatMessage,
  type ChatSession,
  type ActiveToolCall,
  type PlanOutput,
} from '../../store';
import styles from './AIPanelMessage.module.css';

interface MessageItemProps {
  message: ChatMessage;
  isStreaming: boolean;
  activeToolCalls: ActiveToolCall[];
  activeSession: ChatSession | undefined;
  editingMessageId: string | null;
  editingContent: string;
  onStartEdit: (id: string, content: string) => void;
  onCancelEdit: () => void;
  onSaveEdit: () => void;
  onSetEditingContent: (v: string) => void;
  onSetInput: (v: string) => void;
  /**
   * Apply a structured plan: flip session to agent + send follow-up turn.
   * Receives the messageId so the action handler can locate the trailing
   * plan item and destroy its `.inkuo/plans/<id>.md` artifact.
   */
  onApplyPlan?: (messageId: string, plan: PlanOutput) => void;
  /**
   * Adjust a structured plan: refill the input with a hint pointing the
   * user back at the plan for refinement.
   */
  onAdjustPlan?: (messageId: string, plan: PlanOutput) => void;
  /** Persist a structured plan to `<workspace>/.inkuo/plans/<id>.md`. */
  onSavePlan?: (messageId: string) => Promise<void>;
}

export const MessageItem: React.FC<MessageItemProps> = ({
  message,
  isStreaming,
  activeToolCalls,
  activeSession,
  editingMessageId,
  editingContent,
  onStartEdit,
  onCancelEdit,
  onSaveEdit,
  onSetEditingContent,
  onSetInput,
  onApplyPlan,
  onAdjustPlan,
  onSavePlan,
}) => {
  if (message.role === 'user') {
    return (
      <UserMessageBubble
        content={message.content || ''}
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
    const streamingMessageId = activeSession?.messages
      .slice()
      .reverse()
      .find((m) => m.role === 'assistant')?.id;
    const isThisStreaming = isStreaming && message.id === streamingMessageId;
    const hasOutputItems = message.outputItems && message.outputItems.length > 0;

    return (
      <div className={`${styles.message} ${styles.assistant}`}>
        <div className={styles.messageContent}>
          {activeSession && (
            <AssistantMessageBody
              message={message}
              isThisStreaming={isThisStreaming}
              activeToolCalls={activeToolCalls}
              sessionId={activeSession.id}
              onApplyPlan={onApplyPlan}
              onAdjustPlan={onAdjustPlan}
              onSavePlan={onSavePlan}
            />
          )}

          {message.diff && !isThisStreaming && activeSession && (
            <InlineDiffPreview
              originalText={message.diff.originalText}
              newText={message.diff.newText}
              sessionId={activeSession.id}
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
