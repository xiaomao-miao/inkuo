import React from 'react';
import { ToolCallCard } from './ToolCallCard';
import type { ActiveToolCall } from '../../store';
import styles from './AIPanelChatView.module.css';

interface KnowledgeBuildToolCardProps {
  toolCall: ActiveToolCall;
  buildProgress?: {
    phase: string;
    current: number;
    total: number;
    currentFile?: string;
  };
}

export const KnowledgeBuildToolCard: React.FC<KnowledgeBuildToolCardProps> = ({
  toolCall,
  buildProgress,
}) => {
  return (
    <div className={styles.toolResultItem}>
      <ToolCallCard
        id={toolCall.id}
        name={toolCall.name}
        arguments={{
          ...toolCall.arguments,
          progress: buildProgress
            ? `${buildProgress.phase} ${buildProgress.current}/${buildProgress.total}`
            : toolCall.result,
          current_file: buildProgress?.currentFile,
        }}
        status={toolCall.status}
        result={toolCall.result}
        error={toolCall.error}
        duration={toolCall.duration}
      />
    </div>
  );
};
