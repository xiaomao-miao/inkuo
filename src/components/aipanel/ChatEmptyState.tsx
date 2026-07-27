import React from 'react';
import { Sparkles } from 'lucide-react';
import styles from './AIPanelChatView.module.css';

interface ChatEmptyStateProps {
  onSetInput: (value: string) => void;
}

const QuickActionButton: React.FC<{
  label: string;
  hint: string;
  onSetInput: (value: string) => void;
}> = React.memo(({ label, hint, onSetInput }) => {
  return (
    <button
      className={styles.quickAction}
      onClick={() => onSetInput(hint)}
    >
      {label}
    </button>
  );
});
QuickActionButton.displayName = 'QuickActionButton';

export const ChatEmptyState: React.FC<ChatEmptyStateProps> = React.memo(({ onSetInput }) => {
  return (
    <div className={styles.emptyState}>
      <div className={styles.emptyIcon}><Sparkles size={32} /></div>
      <h3>文档助手</h3>
      <p>使用自然语言处理文档、总结内容、解释代码</p>
      <div className={styles.quickActions}>
        <QuickActionButton label="总结文档" hint="总结这篇文档的主要内容" onSetInput={onSetInput} />
        <QuickActionButton label="解释内容" hint="解释这段代码/文本的工作原理" onSetInput={onSetInput} />
        <QuickActionButton
          label="列出文档目录"
          hint="查看当前文档目录结构"
          onSetInput={onSetInput}
        />
      </div>
    </div>
  );
});
ChatEmptyState.displayName = 'ChatEmptyState';