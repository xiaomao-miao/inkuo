import React, { useEffect, useState } from 'react';
import { Database, RefreshCw, Trash2, FileText, Layers, Clock, AlertTriangle, Settings } from 'lucide-react';
import { useAIPanelStore, useSettingsStore, type SearchResult, type BuildProgress } from '../../store';
import { useSidebarStore, SETTINGS_TAB_ID } from '../../store';
import styles from './KnowledgeView.module.css';

interface KnowledgeViewProps {
  sessionId: string;
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

export const KnowledgeView: React.FC<KnowledgeViewProps> = ({
  sessionId,
  onBuild,
  onClear,
}) => {
  const session = useAIPanelStore((state) => state.sessions.find((s) => s.id === sessionId));
  const { settings } = useSettingsStore();
  const [modelAvailable, setModelAvailable] = useState<{ available: boolean; name: string | null }>({
    available: false,
    name: null,
  });
  const { openTab } = useSidebarStore();

  const isBuilding = !!session?.buildProgress && !session?.knowledgeBase;

  useEffect(() => {
    const checkModel = async () => {
      try {
        const { invoke } = await import('@tauri-apps/api/core');
        const models = await invoke<AvailableModel[]>('check_available_models');
        const selectedModel = models.find((m) => m.name === settings.embedding_model);
        if (selectedModel) {
          setModelAvailable({ available: selectedModel.available, name: selectedModel.name });
        } else {
          const defaultModel = models.find((m) => m.name === 'BAAI/bge-small-zh-v1.5');
          setModelAvailable({ available: defaultModel?.available ?? false, name: defaultModel?.name ?? null });
        }
      } catch (err) {
        console.error('Failed to check model availability:', err);
      }
    };

    checkModel();
  }, [settings.embedding_model]);

  const openSettings = () => {
    openTab({
      id: SETTINGS_TAB_ID,
      path: SETTINGS_TAB_ID,
      name: '设置',
      isDirty: false,
      isSettings: true,
    });
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

        {session.buildProgress && (
          <BuildProgressView progress={session.buildProgress} />
        )}
      </div>
    );
  }

  if (!session?.knowledgeBase) {
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
            <span>{session.knowledgeBase.documentCount} 文档</span>
          </div>
          <div className={styles.stat}>
            <Layers size={14} />
            <span>{session.knowledgeBase.chunkCount} 块</span>
          </div>
          <div className={styles.stat}>
            <Clock size={14} />
            <span>{formatTime(session.knowledgeBase.lastUpdated)}</span>
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

      {session.searchResults && session.searchResults.length > 0 && (
        <div className={styles.results}>
          {session.searchResults.map((result) => (
            <SearchResultCard key={result.chunkId} result={result} />
          ))}
        </div>
      )}

      {session.buildProgress && (
        <BuildProgressView progress={session.buildProgress} />
      )}
    </div>
  );
};

interface SearchResultCardProps {
  result: SearchResult;
}

const SearchResultCard: React.FC<SearchResultCardProps> = ({ result }) => {
  return (
    <div className={styles.resultCard}>
      <div className={styles.resultHeader}>
        <FileText size={14} />
        <span className={styles.resultTitle}>{result.documentTitle}</span>
        <span className={styles.resultScore}>{(result.score * 100).toFixed(1)}%</span>
      </div>
      <div className={styles.resultPath}>{result.filePath}</div>
      <div className={styles.resultContent}>{result.content}</div>
    </div>
  );
};

interface BuildProgressViewProps {
  progress: BuildProgress;
}

const BuildProgressView: React.FC<BuildProgressViewProps> = ({ progress }) => {
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
