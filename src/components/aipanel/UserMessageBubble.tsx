import React from 'react';
import { Pencil, RotateCcw, X } from 'lucide-react';
import styles from './AIPanelMessage.module.css';

interface UserMessageBubbleProps {
  content: string;
  isEditing: boolean;
  editingContent: string;
  isStreaming: boolean;
  onStartEdit: () => void;
  onCancelEdit: () => void;
  onSaveEdit: () => void;
  onSetEditingContent: (value: string) => void;
  onSetInput: (value: string) => void;
}

export const UserMessageBubble: React.FC<UserMessageBubbleProps> = ({
  content,
  isEditing,
  editingContent,
  isStreaming,
  onStartEdit,
  onCancelEdit,
  onSaveEdit,
  onSetEditingContent,
  onSetInput,
}) => {
  return (
    <div className={`${styles.message} ${styles.user}`}>
      <div className={styles.messageBubble}>
        {isEditing ? (
          <div className={styles.editMode}>
            <textarea
              className={styles.editTextarea}
              value={editingContent}
              onChange={(e) => {
                onSetEditingContent(e.target.value);
                onSetInput(e.target.value);
              }}
              autoFocus
            />
            <div className={styles.editActions}>
              <button
                className={styles.editCancelBtn}
                onClick={onCancelEdit}
                title="取消"
                type="button"
              >
                <X size={12} />
                取消
              </button>
              <button
                className={styles.editSaveBtn}
                onClick={onSaveEdit}
                disabled={!editingContent.trim()}
                title="重新发送"
                type="button"
              >
                <RotateCcw size={12} />
                重新发送
              </button>
            </div>
          </div>
        ) : (
          <>
            <div className={styles.messageText}>{content}</div>
            {!isStreaming && (
              <button
                className={styles.editBtn}
                onClick={onStartEdit}
                title="编辑并重新发送"
                type="button"
              >
                <Pencil size={12} />
              </button>
            )}
          </>
        )}
      </div>
    </div>
  );
};
