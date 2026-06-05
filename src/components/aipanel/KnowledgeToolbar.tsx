import React from 'react';
import { Database } from 'lucide-react';
import styles from './AIPanelChatView.module.css';

interface KnowledgeToolbarAction {
  label: string;
  onClick: () => void | Promise<void>;
  disabled?: boolean;
  icon?: React.ReactNode;
}

interface KnowledgeToolbarProps {
  statusLabel: string;
  primaryAction: KnowledgeToolbarAction | null;
  secondaryAction: KnowledgeToolbarAction | null;
}

export function buildKnowledgeToolbarModel({
  enabled,
  hasKnowledgeBase,
  isBuilding,
  onBuild,
  onClear,
}: {
  enabled: boolean;
  hasKnowledgeBase: boolean;
  isBuilding: boolean;
  onBuild: () => void | Promise<void>;
  onClear: () => void | Promise<void>;
}) {
  if (!enabled) {
    return {
      primaryAction: null,
      secondaryAction: null,
    };
  }

  const primaryAction: KnowledgeToolbarAction = hasKnowledgeBase
    ? {
        label: '重建知识库',
        onClick: onBuild,
        disabled: isBuilding,
        icon: <Database size={14} />,
      }
    : {
        label: isBuilding ? '正在构建知识库…' : '创建知识库',
        onClick: onBuild,
        disabled: isBuilding,
        icon: <Database size={14} />,
      };

  const secondaryAction = hasKnowledgeBase
    ? {
        label: '清空知识库',
        onClick: onClear,
        disabled: isBuilding,
      }
    : null;

  return {
    primaryAction,
    secondaryAction,
  };
}

export const KnowledgeToolbar: React.FC<KnowledgeToolbarProps> = ({
  statusLabel,
  primaryAction,
  secondaryAction,
}) => {
  return (
    <div className={styles.knowledgeToolbar}>
      <div className={styles.knowledgeToolbarSide}>
        {primaryAction ? (
          <button
            type="button"
            className={styles.knowledgeAction}
            onClick={primaryAction.onClick}
            disabled={primaryAction.disabled}
          >
            {primaryAction.icon}
            <span>{primaryAction.label}</span>
          </button>
        ) : <div />}
      </div>

      <div className={styles.knowledgeStatus}>
        <span>{statusLabel}</span>
      </div>

      <div className={`${styles.knowledgeToolbarSide} ${styles.knowledgeToolbarSideRight}`}>
        {secondaryAction ? (
          <button
            type="button"
            className={styles.knowledgeAction}
            onClick={secondaryAction.onClick}
            disabled={secondaryAction.disabled}
          >
            <span>{secondaryAction.label}</span>
          </button>
        ) : <div />}
      </div>
    </div>
  );
};
