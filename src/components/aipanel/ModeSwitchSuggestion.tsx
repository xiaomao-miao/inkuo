import React from 'react';
import { ArrowRight, X, Sparkles } from 'lucide-react';
import styles from './ModeSwitchSuggestion.module.css';

export type ChatMode = 'ask' | 'plan' | 'agent';

interface ModeSwitchSuggestionProps {
  currentMode: ChatMode;
  suggestedMode: ChatMode;
  reason: string;
  onApprove: () => void;
  onReject: () => void;
}

const MODE_LABELS: Record<ChatMode, string> = {
  ask: 'Ask',
  plan: 'Plan',
  agent: 'Agent',
};

const MODE_DESCRIPTIONS: Record<ChatMode, string> = {
  ask: '只读问答模式 - 只能阅读文件内容，不能修改',
  plan: '规划模式 - 生成结构化的实施计划，不执行代码',
  agent: '完整模式 - 可以读写文件、执行代码、完成任务',
};

export const ModeSwitchSuggestion: React.FC<ModeSwitchSuggestionProps> = ({
  currentMode,
  suggestedMode,
  reason,
  onApprove,
  onReject,
}) => {
  return (
    <div className={styles.container}>
      <div className={styles.header}>
        <Sparkles size={16} className={styles.icon} />
        <span className={styles.title}>AI 建议切换模式</span>
      </div>
      
      <div className={styles.content}>
        <div className={styles.flow}>
          <span className={styles.modeTag}>{MODE_LABELS[currentMode]}</span>
          <ArrowRight size={14} className={styles.arrow} />
          <span className={`${styles.modeTag} ${styles.suggested}`}>
            {MODE_LABELS[suggestedMode]}
          </span>
        </div>
        
        <p className={styles.reason}>{reason}</p>
        
        <div className={styles.suggestedInfo}>
          <strong>{MODE_LABELS[suggestedMode]}</strong>: {MODE_DESCRIPTIONS[suggestedMode]}
        </div>
      </div>
      
      <div className={styles.actions}>
        <button 
          className={styles.rejectBtn}
          onClick={onReject}
          title="保持当前模式"
        >
          <X size={14} />
          保持
        </button>
        <button 
          className={styles.approveBtn}
          onClick={onApprove}
          title={`切换到 ${MODE_LABELS[suggestedMode]} 模式`}
        >
          切换到 {MODE_LABELS[suggestedMode]}
        </button>
      </div>
    </div>
  );
};
