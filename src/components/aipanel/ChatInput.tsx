import React from 'react';
import {
  Send, Trash2, StopCircle, Terminal, Loader2,
} from 'lucide-react';
import type { ChatMode } from '../../store';
import styles from './AIPanel.module.css';

const MODE_LABELS: Record<ChatMode, string> = {
  ask: 'Ask',
  plan: 'Plan',
  agent: 'Agent',
};

const MODE_HINTS: Record<ChatMode, string> = {
  ask: '只回答（不修改文件）',
  plan: '只输出计划（不修改文件）',
  agent: 'Full Agent（可调用工具读写文件）',
};

interface ChatInputProps {
  input: string;
  setInput: (v: string) => void;
  mode: ChatMode;
  isStreaming: boolean;
  hasMessages: boolean;
  onSend: () => void;
  onStop: () => void;
  onClear: () => void;
  onCycleMode: () => void;
}

export const ChatInput: React.FC<ChatInputProps> = ({
  input, setInput, mode, isStreaming, hasMessages,
  onSend, onStop, onClear, onCycleMode,
}) => {
  return (
    <div className={styles.inputArea}>
      <div className={styles.inputBubble}>
        <textarea
          className={styles.input}
          placeholder={
            mode === 'agent'
              ? '输入指令... (例如：帮我创建一个 README.md)'
              : '输入消息... (Enter 发送，Shift+Enter 换行)'
          }
          value={input}
          onChange={e => setInput(e.target.value)}
          onKeyDown={e => {
            if (e.key === 'Enter' && !e.shiftKey) {
              e.preventDefault();
              onSend();
            }
          }}
          rows={1}
        />
        <div className={styles.inputBottomRow}>
          <button
            type="button"
            className={`${styles.modeButton} ${mode === 'agent' ? styles.agentModeActive : ''}`}
            onClick={onCycleMode}
            title={MODE_HINTS[mode]}
          >
            {mode === 'agent' && <Terminal size={12} />}
            {MODE_LABELS[mode]}
          </button>

          <div className={styles.inputActions}>
            {isStreaming ? (
              <button className={styles.iconBtn} onClick={onStop} title="停止生成" type="button">
                <StopCircle size={14} />
              </button>
            ) : hasMessages ? (
              <button
                className={styles.iconBtn}
                onClick={onClear}
                title="清空对话"
              >
                <Trash2 size={14} />
              </button>
            ) : null}
            <button
              className={styles.sendBtn}
              onClick={onSend}
              disabled={!input.trim() || isStreaming}
            >
              {isStreaming ? (
                <Loader2 size={16} className={styles.loadingSpinner} />
              ) : (
                <Send size={16} />
              )}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
};
