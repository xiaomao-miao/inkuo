import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useState, useEffect } from 'react';
import {
  Brain,
  FileText,
  Download,
  HardDrive,
  CheckCircle,
  Loader2,
} from 'lucide-react';
import { useNotificationStore, useSettingsStore } from '../../store';
import type { EmbeddingModelType } from '../../types';
import styles from './SettingsPanel.module.css';
import { reportError } from '../../utils/errors';

interface AvailableModel {
  name: string;
  available: boolean;
  path: string | null;
  dimensions: number;
  size: string;
}

interface DownloadProgress {
  model: string;
  current: number;
  total: number;
  filename: string;
  status: string;
}

interface ModelTier {
  id: EmbeddingModelType;
  name: string;
  tierLabel: string;
  tierColor: string;
  dimensions: number;
  size: string;
  description: string;
  onnxSize: string;
}

const MODEL_TIERS: ModelTier[] = [
  {
    id: 'BAAI/bge-small-zh-v1.5',
    name: 'BGE Small',
    tierLabel: '轻量',
    tierColor: 'var(--success)',
    dimensions: 512,
    size: '~500KB',
    description: '体积小，加载快，适合一般搜索',
    onnxSize: '~25MB',
  },
  {
    id: 'BAAI/bge-base-zh-v1.5',
    name: 'BGE Base',
    tierLabel: '均衡',
    tierColor: 'var(--info)',
    dimensions: 768,
    size: '~520KB',
    description: '性能和体积平衡，推荐日常使用',
    onnxSize: '~390MB',
  },
  {
    id: 'BAAI/bge-large-zh-v1.5',
    name: 'BGE Large',
    tierLabel: '高质量',
    tierColor: 'var(--accent)',
    dimensions: 1024,
    size: '~560KB',
    description: '效果最好，体积较大',
    onnxSize: '~1.3GB',
  },
];

export const KnowledgeSettings = () => {
  const settings = useSettingsStore((state) => state.settings);
  const updateSettingAndPersist = useSettingsStore((state) => state.updateSettingAndPersist);
  const pushNotification = useNotificationStore((state) => state.pushNotification);
  const [availableModels, setAvailableModels] = useState<AvailableModel[]>([]);
  const [downloadingModel, setDownloadingModel] = useState<string | null>(null);
  const [downloadProgress, setDownloadProgress] = useState<DownloadProgress | null>(null);
  const [downloadError, setDownloadError] = useState<string | null>(null);

  // Fetch available models from backend
  useEffect(() => {
    const fetchModels = async () => {
      try {
        const models = await invoke<AvailableModel[]>('check_available_models');
        setAvailableModels(models);
      } catch (err) {
        const message = reportError('knowledge-settings-check-models', err);
        pushNotification({
          kind: 'error',
          title: '读取本地模型状态失败',
          message,
        });
      }
    };
    void fetchModels();
  }, [pushNotification]);

  // Listen to download progress events
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;

    const refreshAvailableModels = async () => {
      try {
        const models = await invoke<AvailableModel[]>('check_available_models');
        if (!disposed) {
          setAvailableModels(models);
        }
      } catch (err) {
        if (disposed) {
          return;
        }

        const message = reportError('knowledge-settings-refresh-models', err);
        pushNotification({
          kind: 'error',
          title: '刷新本地模型状态失败',
          message,
        });
      }
    };

    const setupListener = async () => {
      try {
        unlisten = await listen<DownloadProgress>('model-download-progress', (event) => {
          setDownloadProgress(event.payload);
          if (event.payload.status === 'complete') {
            setDownloadingModel(null);
            setDownloadProgress(null);
            void refreshAvailableModels();
          }
        });
      } catch (err) {
        const message = reportError('knowledge-settings-progress-listener', err);
        pushNotification({
          kind: 'error',
          title: '监听模型下载进度失败',
          message,
        });
      }
    };

    void setupListener();

    return () => {
      disposed = true;
      if (unlisten) unlisten();
    };
  }, [pushNotification]);

  const handleModelChange = async (modelId: string) => {
    await updateSettingAndPersist('embedding_model', modelId as EmbeddingModelType);
  };

  const downloadModel = async (modelName: string) => {
    setDownloadingModel(modelName);
    setDownloadError(null);
    setDownloadProgress({ model: modelName, current: 0, total: 5, filename: '准备下载...', status: 'downloading' });
    try {
      await invoke('download_model_files', { modelName });
    } catch (err) {
      const message = reportError('knowledge-settings-download-model', err);
      setDownloadError(message);
      pushNotification({
        kind: 'error',
        title: '下载模型失败',
        message,
      });
      setDownloadingModel(null);
      setDownloadProgress(null);
    }
  };

  const currentTier = MODEL_TIERS.find((t) => t.id === settings.embedding_model);
  const currentModelStatus = availableModels.find(
    (m) => m.name === settings.embedding_model
  );

  const isCurrentModelAvailable = currentModelStatus?.available ?? false;

  return (
    <div className={styles.tabContent}>
      <div className={styles.section}>
        <h4 className={styles.sectionTitle}>
          <Brain size={14} />
          Embedding 模型
        </h4>
        <p className={styles.sectionDescription}>
          选择模型质量档次。更高质量的模型效果更好，但体积更大、下载更慢。
        </p>

        {/* Model tier selection */}
        <div className={styles.tierList}>
          {MODEL_TIERS.map((tier) => {
            const status = availableModels.find((m) => m.name === tier.id);
            const isAvailable = status?.available ?? false;
            const isSelected = settings.embedding_model === tier.id;
            const isThisDownloading = downloadingModel === tier.id;

            return (
              <div
                key={tier.id}
                className={`${styles.tierItem} ${
                  isSelected ? styles.selected : ''
                } ${isThisDownloading ? styles.downloading : ''}`}
                onClick={() => !isThisDownloading && void handleModelChange(tier.id)}
              >
                <div className={styles.tierLeft}>
                  <div className={styles.tierRadio}>
                    {isSelected ? (
                      <div className={styles.tierRadioDot} />
                    ) : (
                      <div className={styles.tierRadioEmpty} />
                    )}
                  </div>
                  <div className={styles.tierInfo}>
                    <div className={styles.tierMain}>
                      <span
                        className={styles.tierBadge}
                        style={{ backgroundColor: tier.tierColor }}
                      >
                        {tier.tierLabel}
                      </span>
                      <span className={styles.tierName}>{tier.name}</span>
                    </div>
                    <div className={styles.tierMeta}>
                      {tier.description} · {tier.dimensions}维 · ONNX {tier.onnxSize}
                    </div>
                  </div>
                </div>
                <div className={styles.tierRight}>
                  {isThisDownloading ? (
                    <div className={styles.tierDownloading}>
                      <Loader2 size={12} className={styles.spinner} />
                      <span>{downloadProgress?.filename || '下载中'}</span>
                      <div className={styles.tierProgressBar}>
                        <div
                          className={styles.tierProgressFill}
                          style={{
                            width: downloadProgress
                              ? `${(downloadProgress.current / downloadProgress.total) * 100}%`
                              : '0%',
                          }}
                        />
                      </div>
                    </div>
                  ) : isAvailable ? (
                    <span className={styles.downloadedBadge}>
                      <CheckCircle size={12} />
                      已下载
                    </span>
                  ) : (
                    <button
                      className={styles.downloadBtn}
                      onClick={(e) => {
                        e.stopPropagation();
                        void downloadModel(tier.id);
                      }}
                    >
                      <Download size={12} />
                      下载
                    </button>
                  )}
                </div>
              </div>
            );
          })}
        </div>

        {downloadError && (
          <div className={`${styles.testResult} ${styles.error}`} role="alert">
            下载失败：{downloadError}
          </div>
        )}

        {currentTier && (
          <div className={styles.modelInfo}>
            <div className={styles.modelInfoItem}>
              <span className={styles.modelInfoLabel}>当前模型:</span>
              <span>{currentTier.name}</span>
              {!isCurrentModelAvailable && (
                <span className={styles.warningBadge}>
                  ONNX 将在首次使用时自动下载
                </span>
              )}
            </div>
            <div className={styles.modelInfoItem}>
              <span className={styles.modelInfoLabel}>模型质量:</span>
              <span
                style={{
                  color: currentTier.tierColor,
                  fontWeight: 500,
                }}
              >
                {currentTier.tierLabel}
              </span>
            </div>
          </div>
        )}
      </div>

      <div className={styles.section}>
        <h4 className={styles.sectionTitle}>
          <FileText size={14} />
          分块设置
        </h4>
        <p className={styles.sectionDescription}>
          控制文档被分割成块的大小。较大的块包含更多上下文，但可能降低搜索精度。
        </p>

        <div className={styles.field}>
          <label className={styles.label}>块大小 (字符数)</label>
          <div className={styles.rangeWrapper}>
            <input
              type="range"
              min="100"
              max="1000"
              step="50"
              value={settings.chunk_size}
              onChange={(e) => {
                void updateSettingAndPersist('chunk_size', parseInt(e.target.value));
              }}
              className={styles.range}
            />
            <span className={styles.rangeValue}>{settings.chunk_size} 字符</span>
          </div>
          <p className={styles.fieldHelp}>
            每个文本块的目标大小。推荐值: 300-500
          </p>
        </div>

        <div className={styles.field}>
          <label className={styles.label}>块重叠 (字符数)</label>
          <div className={styles.rangeWrapper}>
            <input
              type="range"
              min="0"
              max="200"
              step="10"
              value={settings.chunk_overlap}
              onChange={(e) => {
                void updateSettingAndPersist('chunk_overlap', parseInt(e.target.value));
              }}
              className={styles.range}
            />
            <span className={styles.rangeValue}>
              {settings.chunk_overlap} 字符
            </span>
          </div>
          <p className={styles.fieldHelp}>
            相邻块之间重叠的字符数。推荐值: 20-50
          </p>
        </div>
      </div>

      <div className={styles.section}>
        <h4 className={styles.sectionTitle}>
          <HardDrive size={14} />
          模型存储
        </h4>
        <p className={styles.sectionDescription}>
          模型文件存储位置。首次使用时会自动从 HuggingFace 下载到缓存目录。
        </p>

        <div className={styles.modelInfo}>
          <div className={styles.modelInfoItem}>
            <span className={styles.modelInfoLabel}>缓存目录:</span>
            <span>{settings.embedding_model_path ?? '使用默认目录'}</span>
          </div>
        </div>
      </div>
    </div>
  );
};
