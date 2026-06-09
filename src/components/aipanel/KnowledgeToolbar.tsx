import React from 'react';
import styles from './AIPanelChatView.module.css';
import type { KnowledgeToolbarAction } from './knowledgeToolbarModel';

interface KnowledgeToolbarProps {
  statusLabel: string;
  primaryAction: KnowledgeToolbarAction | null;
  secondaryAction: KnowledgeToolbarAction | null;
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
