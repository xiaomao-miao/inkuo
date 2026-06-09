import { invoke } from '@tauri-apps/api/core';
import { useState } from 'react';
import {
  Key,
  Globe,
  Cpu,
  Check,
  AlertCircle,
  Loader2,
  Eye,
  EyeOff,
  RefreshCw,
  Plus,
  Trash2,
  Star,
  StarOff,
  Settings2,
  X,
  ChevronDown,
  ChevronRight
} from 'lucide-react';
import { useSettingsStore } from '../../store';
import { Select } from './Select';
import type { APIConfig, AIProviderType } from '../../types';
import styles from './ModelsSettings.module.css';
import { saveSettings } from '../../utils/saveSettings';

interface ModelsSettingsProps {
  onClose?: () => void;
}

export const ModelsSettings = ({ onClose }: ModelsSettingsProps) => {
  const {
    settings,
    addApiConfig,
    updateApiConfig,
    removeApiConfig,
    setActiveApiConfig,
    setDefaultApiConfig,
  } = useSettingsStore();

  const [testingId, setTestingId] = useState<string | null>(null);
  const [testResults, setTestResults] = useState<Record<string, { success: boolean; message: string }>>({});
  const [showApiKey, setShowApiKey] = useState<Record<string, boolean>>({});
  const [expandedId, setExpandedId] = useState<string | null>(null);

  const persistSettings = async (nextSettings = settings) => {
    try {
      await saveSettings(nextSettings);
    } catch (err) {
      console.error('Failed to save settings:', err);
    }
  };

  const persistReturnedSettings = (nextSettings: typeof settings) => {
    persistSettings(nextSettings);
  };

  const handleTestConnection = async (config: APIConfig) => {
    setTestingId(config.id);
    setTestResults((prev) => ({ ...prev, [config.id]: { success: false, message: '' } }));

    try {
      const result = await invoke<{ success: boolean; message: string }>('test_api_config', {
        request: {
          api_key: config.apiKey,
          base_url: config.baseUrl,
          model: config.model,
          provider: config.provider,
        },
      });
      setTestResults((prev) => ({ ...prev, [config.id]: result }));
    } catch (err) {
      setTestResults((prev) => ({
        ...prev,
        [config.id]: {
          success: false,
          message: err instanceof Error ? err.message : '连接失败，请检查配置',
        },
      }));
    } finally {
      setTestingId(null);
    }
  };

  const handleAddConfig = () => {
    const nextSettings = addApiConfig({
      name: `API ${settings.apiConfigs.length + 1}`,
      provider: 'openai',
      baseUrl: 'https://api.openai.com/v1',
      model: 'gpt-4o-mini',
    });
    const addedConfig = nextSettings.apiConfigs[nextSettings.apiConfigs.length - 1];
    if (addedConfig) {
      setExpandedId(addedConfig.id);
    }
    persistReturnedSettings(nextSettings);
  };

  const handleRemoveConfig = (id: string) => {
    const nextSettings = removeApiConfig(id);
    if (expandedId === id) {
      setExpandedId(null);
    }
    persistReturnedSettings(nextSettings);
  };

  const handleSelectConfig = (id: string) => {
    const nextSettings = setActiveApiConfig(id);
    persistReturnedSettings(nextSettings);
  };

  const handleSetDefault = (id: string) => {
    const nextSettings = setDefaultApiConfig(id);
    persistReturnedSettings(nextSettings);
  };

  const handleUpdateConfig = (id: string, updates: Partial<APIConfig>) => {
    const nextSettings = updateApiConfig(id, updates);
    persistReturnedSettings(nextSettings);
  };

  const toggleShowApiKey = (id: string) => {
    setShowApiKey((prev) => ({ ...prev, [id]: !prev[id] }));
  };

  const getProviderOptions = () => [
    { value: 'openai', label: 'OpenAI (兼容)' },
    { value: 'deepseek', label: 'DeepSeek' },
    { value: 'ollama', label: 'Ollama (本地)' },
    { value: 'official', label: 'Inkuo 官方' },
  ];

  const getDefaultBaseUrl = (provider: AIProviderType): string => {
    switch (provider) {
      case 'openai':
        return 'https://api.openai.com/v1';
      case 'deepseek':
        return 'https://api.deepseek.com';
      case 'ollama':
        return 'http://localhost:11434';
      case 'official':
        return 'https://api.inkuo.com/v1';
      default:
        return 'https://api.openai.com/v1';
    }
  };

  const getDefaultModel = (provider: AIProviderType): string => {
    switch (provider) {
      case 'openai':
        return 'gpt-4o-mini';
      case 'deepseek':
        return 'deepseek-chat';
      case 'ollama':
        return 'llama3';
      case 'official':
        return 'inkuo-default';
      default:
        return 'gpt-4o-mini';
    }
  };

  const handleProviderChange = (id: string, provider: AIProviderType) => {
    handleUpdateConfig(id, {
      provider,
      baseUrl: getDefaultBaseUrl(provider),
      model: getDefaultModel(provider),
    });
  };

  const toggleExpand = (id: string) => {
    setExpandedId((prev) => (prev === id ? null : id));
  };

  return (
    <div className={styles.container}>
      <div className={styles.header}>
        <div className={styles.headerTitle}>
          <Settings2 size={16} />
          <h2>模型设置</h2>
        </div>
        {onClose && (
          <button className={styles.closeBtn} onClick={onClose}>
            <X size={16} />
          </button>
        )}
      </div>

      <div className={styles.content}>
        <div className={styles.apiList}>
          <div className={styles.listHeader}>
            <h3>API 配置</h3>
            <button className={styles.addBtn} onClick={handleAddConfig}>
              <Plus size={14} />
              添加 API
            </button>
          </div>

          <div className={styles.configList}>
            {settings.apiConfigs.map((config) => {
              const isExpanded = expandedId === config.id;
              const isActive = config.id === settings.activeApiConfigId;

              return (
              <div
                key={config.id}
                className={`${styles.configCard} ${isActive ? styles.active : ''} ${isExpanded ? styles.expanded : ''}`}
              >
                <div className={styles.configHeader}>
                  <button className={styles.summaryBtn} onClick={() => toggleExpand(config.id)}>
                    <span className={styles.expandIcon}>
                      {isExpanded ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
                    </span>
                    <div className={styles.configInfo}>
                      <div className={styles.configName}>
                        <span
                          className={styles.defaultBadge}
                          style={{ opacity: config.isDefault ? 1 : 0 }}
                        >
                          <Star size={10} fill="currentColor" />
                        </span>
                        {config.name}
                        {isActive && <span className={styles.usingBadge}>使用中</span>}
                      </div>
                      <div className={styles.configMeta}>
                        {getProviderOptions().find((p) => p.value === config.provider)?.label} · {config.model} · {config.baseUrl}
                      </div>
                    </div>
                  </button>

                  <div className={styles.configActions}>
                    <button
                      className={styles.actionBtn}
                      onClick={() => handleSetDefault(config.id)}
                      title={config.isDefault ? '默认配置' : '设为默认'}
                    >
                      {config.isDefault ? (
                        <Star size={14} fill="currentColor" />
                      ) : (
                        <StarOff size={14} />
                      )}
                    </button>
                    <button
                      className={styles.actionBtn}
                      onClick={() => handleRemoveConfig(config.id)}
                      disabled={settings.apiConfigs.length <= 1}
                      title="删除"
                    >
                      <Trash2 size={14} />
                    </button>
                  </div>
                </div>

                {isExpanded && <div className={styles.configFields}>
                  <div className={styles.field}>
                    <label className={styles.label}>名称</label>
                    <input
                      type="text"
                      className={styles.input}
                      value={config.name}
                      onChange={(e) => handleUpdateConfig(config.id, { name: e.target.value })}
                      placeholder="API 名称"
                    />
                  </div>

                  <div className={styles.field}>
                    <label className={styles.label}>提供商</label>
                    <Select
                      value={config.provider}
                      options={getProviderOptions()}
                      onChange={(value) => handleProviderChange(config.id, value as AIProviderType)}
                      className={styles.select}
                    />
                  </div>

                  <div className={styles.field}>
                    <label className={styles.label}>
                      <Globe size={12} />
                      API URL
                    </label>
                    <input
                      type="text"
                      className={styles.input}
                      value={config.baseUrl}
                      onChange={(e) => handleUpdateConfig(config.id, { baseUrl: e.target.value })}
                      placeholder="https://api.openai.com/v1"
                    />
                  </div>

                  <div className={styles.field}>
                    <label className={styles.label}>
                      <Cpu size={12} />
                      模型
                    </label>
                    <input
                      type="text"
                      className={styles.input}
                      value={config.model}
                      onChange={(e) => handleUpdateConfig(config.id, { model: e.target.value })}
                      placeholder="gpt-4o-mini"
                    />
                  </div>

                  {config.provider !== 'ollama' && (
                    <div className={styles.field}>
                      <label className={styles.label}>
                        <Key size={12} />
                        API Key
                      </label>
                      <div className={styles.inputWrapper}>
                        <input
                          type={showApiKey[config.id] ? 'text' : 'password'}
                          className={styles.input}
                          value={config.apiKey || ''}
                          onChange={(e) =>
                            handleUpdateConfig(config.id, { apiKey: e.target.value || null })
                          }
                          placeholder="sk-..."
                        />
                        <button
                          className={styles.toggleBtn}
                          onClick={() => toggleShowApiKey(config.id)}
                          type="button"
                        >
                          {showApiKey[config.id] ? <EyeOff size={12} /> : <Eye size={12} />}
                        </button>
                      </div>
                    </div>
                  )}

                  <div className={styles.field}>
                    <label className={styles.label}>温度</label>
                    <div className={styles.rangeWrapper}>
                      <input
                        type="range"
                        min="0"
                        max="2"
                        step="0.1"
                        value={config.temperature}
                        onChange={(e) =>
                          handleUpdateConfig(config.id, {
                            temperature: parseFloat(e.target.value),
                          })
                        }
                        className={styles.range}
                      />
                      <span className={styles.rangeValue}>{config.temperature.toFixed(1)}</span>
                    </div>
                  </div>
                </div>}

                {isExpanded && <div className={styles.configFooter}>
                  <button
                    className={styles.testBtn}
                    onClick={() => handleTestConnection(config)}
                    disabled={testingId === config.id}
                  >
                    {testingId === config.id ? (
                      <>
                        <Loader2 size={12} className={styles.spinner} />
                        测试中...
                      </>
                    ) : (
                      <>
                        <RefreshCw size={12} />
                        测试连接
                      </>
                    )}
                  </button>

                  <button
                    className={`${styles.selectBtn} ${
                      config.id === settings.activeApiConfigId ? styles.selected : ''
                    }`}
                    onClick={() => handleSelectConfig(config.id)}
                  >
                    {config.id === settings.activeApiConfigId ? (
                      <>
                        <Check size={12} />
                        使用中
                      </>
                    ) : (
                      '使用此配置'
                    )}
                  </button>
                </div>}

                {isExpanded && testResults[config.id] && (
                  <div
                    className={`${styles.testResult} ${
                      testResults[config.id].success ? styles.success : styles.error
                    }`}
                  >
                    {testResults[config.id].success ? (
                      <Check size={12} />
                    ) : (
                      <AlertCircle size={12} />
                    )}
                    <span>{testResults[config.id].message}</span>
                  </div>
                )}
              </div>
            );
            })}
          </div>
        </div>
      </div>
    </div>
  );
};
