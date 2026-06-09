import { Database } from 'lucide-react';

export interface KnowledgeToolbarAction {
  label: string;
  onClick: () => void | Promise<void>;
  disabled?: boolean;
  icon?: React.ReactNode;
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
        label: isBuilding ? '正在构建知识库…' : '构建知识库',
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
