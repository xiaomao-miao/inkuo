import { invoke } from '@tauri-apps/api/core';
import { useEffect, useState } from 'react';
import { Database, RefreshCw, Trash2, FileText, Layers, Clock, AlertTriangle, Settings } from 'lucide-react';
import { useNotificationStore, useSettingsStore, type BuildProgress } from '../../store';
import { useSidebarStore } from '../../store';
import { reportError } from '../../utils/errors';
import { openSettingsTab } from '../../utils/openSettingsTab';
import styles from './KnowledgeView.module.css';

interface KnowledgeViewProps {
  onBuild: () => void;
  onClear: () => void;
}

interface AvailableModel {
  name: string;
  available: boolean;
  path: string | null;
  dimensions: number;
  size: string;
}

export const KnowledgeView = ({
  onBuild,
  onClear,
}: KnowledgeViewProps) => {
  const knowledgeBase = useSidebarStore((state) => state.knowledgeBase);
  const buildProgress = useSidebarStore((state) => state.buildProgress);
  const settings = useSettingsStore((state) => state.settings);
  const pushNotification = useNotificationStore((state) => state.pushNotification);
  const [modelAvailable, setModelAvailable] = useState<{ available: boolean; name: string | null }>({
    available: false,
    name: null,
  });

  const isBuilding = !!buildProgress && !knowledgeBase;

  useEffect(() => {
    const checkModel = async () => {
      try {
        const models = await invoke<AvailableModel[]>('check_available_models');
        const selectedModel = models.find((m) => m.name === settings.embedding_model);
        if (selectedModel) {
          setModelAvailable({ available: selectedModel.available, name: selectedModel.name });
        } else {
          const defaultModel = models.find((m) => m.name === 'BAAI/bge-small-zh-v1.5');
          setModelAvailable({ available: defaultModel?.available ?? false, name: defaultModel?.name ?? null });
        }
      } catch (err) {
        const message = reportError('knowledge-view-check-model', err);
        pushNotification({
          kind: 'error',
          title: '检查向量模型状态失败',
          message,
        });
      }
    };

    checkModel();
  }, [settings.embedding_model, pushNotification]);

  const openSettings = () => {
    openSettingsTab();
  };

  // Show model setup prompt if no model is available
  if (!modelAvailable.available) {
    return (
      <div className={styles.knowledgeView}>
        <div className={styles.emptyState}>
          <AlertTriangle size={48} className={styles.emptyIcon} />
          <h3 className={styles.emptyTitle}>向量模型未下载</h3>
          <p className={styles.emptyDescription}>
            当前选择的模型 "{modelAvailable.name || 'BGE Small'}" 尚未下载。
            请先下载模型后再使用知识库功能。
          </p>
          <div className={styles.setupActions}>
            <button className={styles.settingsButton} onClick={openSettings}>
              <Settings size={16} />
              前往设置下载模型
            </button>
          </div>
        </div>
      </div>
    );
  }

  // Show build progress
  if (isBuilding) {
    return (
      <div className={styles.knowledgeView}>
        <div className={styles.header}>
          <div className={styles.stats}>
            <div className={styles.stat}>
              <Layers size={14} />
              <span>正在构建知识库...</span>
            </div>
          </div>
        </div>

        {buildProgress && (
          <BuildProgressView progress={buildProgress} />
        )}
      </div>
    );
  }

  if (!knowledgeBase) {
    return (
      <div className={styles.knowledgeView}>
        <div className={styles.emptyState}>
          <Database size={48} className={styles.emptyIcon} />
          <h3 className={styles.emptyTitle}>知识库未初始化</h3>
          <p className={styles.emptyDescription}>
            构建知识库后，你可以直接在底部输入框提问，系统会自动检索工作区文档并生成带引用来源的回答。
          </p>
          <button className={styles.buildButton} onClick={onBuild}>
            <Layers size={16} />
            构建知识库
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className={styles.knowledgeView}>
      <div className={styles.header}>
        <div className={styles.stats}>
          <div className={styles.stat}>
            <FileText size={14} />
            <span>{knowledgeBase.documentCount} 文档</span>
          </div>
          <div className={styles.stat}>
            <Layers size={14} />
            <span>{knowledgeBase.chunkCount} 块</span>
          </div>
          <div className={styles.stat}>
            <Clock size={14} />
            <span>{formatTime(knowledgeBase.lastUpdated)}</span>
          </div>
        </div>
        <div className={styles.actions}>
          <button className={styles.actionButton} onClick={onBuild} title="重建知识库">
            <RefreshCw size={14} />
          </button>
          <button className={styles.actionButton} onClick={onClear} title="清空知识库">
            <Trash2 size={14} />
          </button>
        </div>
      </div>

      {buildProgress && (
        <BuildProgressView progress={buildProgress} />
      )}
    </div>
  );
};

interface BuildProgressViewProps {
  progress: BuildProgress;
}

const BuildProgressView = ({ progress }: BuildProgressViewProps) => {
  const phaseLabels: Record<string, string> = {
    scanning: '扫描文件',
    chunking: '分块处理',
    embedding: '生成向量',
    storing: '存储向量',
    done: '构建完成',
  };

  return (
    <div className={styles.progressContainer}>
      <div className={styles.progressHeader}>
        <RefreshCw size={14} className={styles.spinning} />
        <span>{phaseLabels[progress.phase] || progress.phase}</span>
        {progress.total > 0 && (
          <span className={styles.progressCount}>
            {progress.current}/{progress.total}
          </span>
        )}
      </div>
      {progress.currentFile && (
        <div className={styles.progressFile}>{progress.currentFile}</div>
      )}
      <div className={styles.progressBar}>
        <div
          className={styles.progressFill}
          style={{
            width: progress.total > 0
              ? `${(progress.current / progress.total) * 100}%`
              : '0%',
          }}
        />
      </div>
    </div>
  );
};

function formatTime(timestamp: number): string {
  if (!timestamp) return '';
  const date = new Date(timestamp);
  const now = new Date();
  const diff = now.getTime() - date.getTime();

  if (diff < 60000) return '刚刚';
  if (diff < 3600000) return `${Math.floor(diff / 60000)} 分钟前`;
  if (diff < 86400000) return `${Math.floor(diff / 3600000)} 小时前`;
  if (diff < 604800000) return `${Math.floor(diff / 86400000)} 天前`;

  return date.toLocaleDateString('zh-CN');
}
