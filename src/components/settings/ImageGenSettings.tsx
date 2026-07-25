import { invoke } from '@tauri-apps/api/core';
import { useState } from 'react';
import {
  Key,
  Globe,
  Image as ImageIcon,
  Check,
  AlertCircle,
  Loader2,
  Eye,
  EyeOff,
  RefreshCw,
  Plus,
  Trash2,
  X,
  ChevronDown,
  ChevronRight,
} from 'lucide-react';
import { useNotificationStore, useSettingsStore, flushSettings } from '../../store';
import { Select } from './Select';
import type { ImageGenProviderConfig, ImageGenProviderType } from '../../types';
import styles from './ModelsSettings.module.css';
import { reportError } from '../../utils/errors';

interface ImageGenSettingsProps {
  onClose?: () => void;
}

export const ImageGenSettings = ({ onClose }: ImageGenSettingsProps) => {
  const settings = useSettingsStore((state) => state.settings);
  const updateImageGen = useSettingsStore((state) => state.updateImageGen);
  const updateImageGenProvider = useSettingsStore(
    (state) => state.updateImageGenProvider
  );
  const setActiveImageGenProvider = useSettingsStore(
    (state) => state.setActiveImageGenProvider
  );
  const pushNotification = useNotificationStore((state) => state.pushNotification);

  const [testingId, setTestingId] = useState<string | null>(null);
  const [testResults, setTestResults] = useState<
    Record<string, { success: boolean; message: string }>
  >({});
  const [showApiKey, setShowApiKey] = useState<Record<string, boolean>>({});
  const [expandedId, setExpandedId] = useState<string | null>(null);

  const handleTestConnection = async (provider: ImageGenProviderConfig) => {
    setTestingId(provider.id);
    setTestResults((prev) => ({
      ...prev,
      [provider.id]: { success: false, message: '' },
    }));

    try {
      const result = await invoke<{ success: boolean; message: string }>(
        'test_image_gen_config',
        {
          request: {
            provider_id: provider.providerType,
            api_key: provider.apiKey,
            base_url:
              provider.baseUrl ?? getDefaultBaseUrl(provider.providerType),
            model: provider.defaultModel,
            secret_id: provider.secretId,
            secret_key: provider.secretKey,
            region: provider.region,
          },
        }
      );
      pushNotification({
        kind: 'success',
        title: '连接测试成功',
        message: result.message,
      });
      setTestResults((prev) => ({
        ...prev,
        [provider.id]: { success: true, message: result.message },
      }));
      // The user just verified a working endpoint — if they then route
      // to this provider (or restart the app) the configuration must
      // already be on disk. The keystroke-level fire-and-forget
      // persistence would normally cover this, but a quick app
      // restart between "fill the key" and "use the key" has been
      // observed to drop the entry.
      await flushSettings();
    } catch (err) {
      const message = reportError('image-gen-settings-test-connection', err);
      setTestResults((prev) => ({
        ...prev,
        [provider.id]: {
          success: false,
          message,
        },
      }));
      pushNotification({
        kind: 'error',
        title: '连接测试失败',
        message,
      });
    } finally {
      setTestingId(null);
    }
  };

  const handleAddProvider = async (preset?: ImageGenProviderType) => {
    const newId = `img_${crypto.randomUUID()}`;
    const initialType: ImageGenProviderType = preset ?? 'tencent_token';
    await updateImageGenProvider(newId, {
      id: newId,
      providerType: initialType,
      baseUrl: getDefaultBaseUrl(initialType),
      defaultModel: getDefaultModel(initialType),
      apiKey: null,
      secretId: null,
      secretKey: null,
      region: null,
      enabled: true,
    });
    // Force a synchronous disk flush: the user typically toggles the
    // provider type right after this (e.g. "tencent_token") and then
    // expects the new entry to be routable on the very next tool
    // invocation, possibly across an app reload.
    await flushSettings();
    setExpandedId(newId);
  };

  const handleRemoveProvider = async (id: string) => {
    const remaining = settings.image_gen.providers.filter((p) => p.id !== id);
    await updateImageGen({
      ...settings.image_gen,
      providers: remaining,
    });
    await flushSettings();
    if (expandedId === id) {
      setExpandedId(null);
    }
  };

  const handleUpdateProvider = async (
    id: string,
    updates: Partial<ImageGenProviderConfig>
  ) => {
    await updateImageGenProvider(id, updates);
  };

  const toggleShowApiKey = (id: string) => {
    setShowApiKey((prev) => ({ ...prev, [id]: !prev[id] }));
  };

  const toggleExpand = (id: string) => {
    setExpandedId((prev) => (prev === id ? null : id));
  };

  const getProviderTypeOptions = () => [
    { value: 'ollama', label: 'Ollama (本地)' },
    { value: 'openai', label: 'OpenAI 兼容 (DALL·E / DeepSeek / 自建网关)' },
    { value: 'tencent_token', label: '腾讯 Token Hub (Bearer 密钥)' },
    { value: 'tencent_tc3', label: '腾讯云 aiart (TC3 签名, hunyuan)' },
    { value: 'custom', label: '自定义 (OpenAI 协议)' },
  ];

  const getDefaultBaseUrl = (type: ImageGenProviderType): string => {
    switch (type) {
      case 'ollama':
        return 'http://localhost:11434';
      case 'openai':
        return 'https://api.openai.com/v1';
      case 'tencent_token':
        return 'https://tokenhub.tencentmaas.com';
      case 'tencent_tc3':
        return 'https://aiart.tencentcloudapi.com/';
      case 'custom':
        return '';
    }
  };

  const getDefaultModel = (type: ImageGenProviderType): string => {
    switch (type) {
      case 'ollama':
        return 'sdxl';
      case 'openai':
        return 'dall-e-3';
      case 'tencent_token':
        return 'hy-image-lite';
      case 'tencent_tc3':
        return 'hunyuan-pro';
      case 'custom':
        return '';
    }
  };

  const getTypeLabel = (type: ImageGenProviderType): string => {
    return (
      getProviderTypeOptions().find((o) => o.value === type)?.label ?? type
    );
  };

  // Switching transport family should reset URL/model to a sensible
  // default for the new family — but only if the user hasn't customised
  // them yet, so we don't blow away manual tweaks.
  const handleProviderTypeChange = (
    id: string,
    nextType: ImageGenProviderType
  ) => {
    const current = settings.image_gen.providers.find((p) => p.id === id);
    if (!current) return;
    const updates: Partial<ImageGenProviderConfig> = { providerType: nextType };
    // Only overwrite baseUrl if it's null or matches the old default.
    const oldDefault = getDefaultBaseUrl(current.providerType);
    if (!current.baseUrl || current.baseUrl === oldDefault) {
      updates.baseUrl = getDefaultBaseUrl(nextType);
    }
    const oldModelDefault = getDefaultModel(current.providerType);
    if (!current.defaultModel || current.defaultModel === oldModelDefault) {
      updates.defaultModel = getDefaultModel(nextType);
    }
    handleUpdateProvider(id, updates);
  };

  return (
    <div className={styles.container}>
      <div className={styles.header}>
        <div className={styles.headerTitle}>
          <ImageIcon size={16} />
          <h2>图像生成设置</h2>
        </div>
        {onClose && (
          <button className={styles.closeBtn} onClick={onClose}>
            <X size={16} />
          </button>
        )}
      </div>

      <div className={styles.content}>
        <div className={styles.section}>
          <h4 className={styles.sectionTitle}>
            <ImageIcon size={14} />
            当前使用
          </h4>
          {settings.image_gen.providers.length === 1 ? (
            <div className={styles.field}>
              <p className={styles.fieldHelp}>
                当前只有一个 API 配置。如果想切换到腾讯 Token Hub、OpenAI
                兼容服务等其它提供商，请点击下方「添加 API」创建并填入对应
                的密钥和模型。
              </p>
            </div>
          ) : (
            <div className={styles.field}>
              <Select
                value={settings.image_gen.routing}
                options={settings.image_gen.providers.map((p) => ({
                  value: p.id,
                  label: `${getTypeLabel(p.providerType)}${p.enabled ? '' : ' (已禁用)'}`,
                }))}
                onChange={async (value) => {
                  await setActiveImageGenProvider(value);
                  await flushSettings();
                }}
                className={styles.select}
              />
              <p className={styles.fieldHelp}>
                切换后，图片生成工具将使用选中的 API 提供商。
              </p>
            </div>
          )}
        </div>

        <div className={styles.apiList}>
          <div className={styles.listHeader}>
            <h3>图像生成 API</h3>
            <div style={{ display: 'flex', gap: 8 }}>
              <button
                className={styles.addBtn}
                onClick={() => handleAddProvider('tencent_token')}
                title="添加腾讯 Token Hub（推荐，Bearer Key 即可）"
              >
                <Plus size={14} />
                添加腾讯 Token Hub
              </button>
              <button
                className={styles.addBtn}
                onClick={() => handleAddProvider()}
                title="添加任意 OpenAI 兼容 / 自定义供应商"
              >
                <Plus size={14} />
                添加 API
              </button>
            </div>
          </div>

          <div className={styles.configList}>
            {settings.image_gen.providers.length === 0 ? (
              <div
                style={{
                  padding: '24px',
                  textAlign: 'center',
                  color: 'var(--fg-secondary)',
                  fontSize: 13,
                }}
              >
                还没有配置任何图像生成 API。
                点击上方「添加 API」开始配置。
              </div>
            ) : (
              settings.image_gen.providers.map((provider) => {
                const isExpanded = expandedId === provider.id;
                const isTesting = testingId === provider.id;
                // The API Key (Bearer token) field is shown for openai/tencent_token
                // /custom providers. Ollama doesn't need a key, and tencent_tc3
                // uses SecretId/SecretKey instead — those get their own row.
                const showApiKeyField =
                  provider.providerType !== 'ollama' &&
                  provider.providerType !== 'tencent_tc3';

                return (
                  <div
                    key={provider.id}
                    className={`${styles.configCard} ${
                      isExpanded ? styles.expanded : ''
                    }`}
                  >
                    <div className={styles.configHeader}>
                      <button
                        className={styles.summaryBtn}
                        onClick={() => toggleExpand(provider.id)}
                      >
                        <span className={styles.expandIcon}>
                          {isExpanded ? (
                            <ChevronDown size={14} />
                          ) : (
                            <ChevronRight size={14} />
                          )}
                        </span>
                        <div className={styles.configInfo}>
                          <div className={styles.configName}>
                            {getTypeLabel(provider.providerType)}
                            {provider.enabled && (
                              <span className={styles.usingBadge}>启用</span>
                            )}
                          </div>
                          <div className={styles.configMeta}>
                            {provider.defaultModel || '未设置模型'}
                            {provider.baseUrl && ` · ${provider.baseUrl}`}
                          </div>
                        </div>
                      </button>

                      <div className={styles.configActions}>
                        <button
                          className={styles.actionBtn}
                          onClick={() =>
                            handleUpdateProvider(provider.id, {
                              enabled: !provider.enabled,
                            })
                          }
                          title={provider.enabled ? '禁用' : '启用'}
                        >
                          {provider.enabled ? (
                            <Check size={14} />
                          ) : (
                            <AlertCircle size={14} />
                          )}
                        </button>
                        <button
                          className={styles.actionBtn}
                          onClick={() => handleRemoveProvider(provider.id)}
                          disabled={
                            settings.image_gen.providers.length <= 1
                          }
                          title="删除"
                        >
                          <Trash2 size={14} />
                        </button>
                      </div>
                    </div>

                    {isExpanded && (
                      <div className={styles.configFields}>
                        <div className={styles.field}>
                          <label className={styles.label}>提供商类型</label>
                          <Select
                            value={provider.providerType}
                            options={getProviderTypeOptions()}
                            onChange={(value) =>
                              handleProviderTypeChange(
                                provider.id,
                                value as ImageGenProviderType
                              )
                            }
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
                            value={provider.baseUrl ?? ''}
                            onChange={(e) =>
                              handleUpdateProvider(provider.id, {
                                baseUrl: e.target.value || null,
                              })
                            }
                            placeholder={
                              getDefaultBaseUrl(provider.providerType) ||
                              'https://...'
                            }
                          />
                        </div>

                        <div className={styles.field}>
                          <label className={styles.label}>
                            <ImageIcon size={12} />
                            模型
                          </label>
                          <input
                            type="text"
                            className={styles.input}
                            value={provider.defaultModel}
                            onChange={(e) =>
                              handleUpdateProvider(provider.id, {
                                defaultModel: e.target.value,
                              })
                            }
                            placeholder="sdxl / dall-e-3 / flux.1-schnell"
                          />
                        </div>

                        {showApiKeyField && (
                          <div className={styles.field} style={{ gridColumn: '1 / -1' }}>
                            <label className={styles.label}>
                              <Key size={12} />
                              API Key
                            </label>
                            <div className={styles.inputWrapper}>
                              <input
                                type={showApiKey[provider.id] ? 'text' : 'password'}
                                className={styles.input}
                                value={provider.apiKey ?? ''}
                                onChange={(e) =>
                                  handleUpdateProvider(provider.id, {
                                    apiKey: e.target.value || null,
                                  })
                                }
                                placeholder="sk-..."
                                autoComplete="off"
                                spellCheck={false}
                              />
                              <button
                                className={styles.toggleBtn}
                                onClick={() => toggleShowApiKey(provider.id)}
                                type="button"
                                title={showApiKey[provider.id] ? '隐藏' : '显示'}
                              >
                                {showApiKey[provider.id] ? (
                                  <EyeOff size={12} />
                                ) : (
                                  <Eye size={12} />
                                )}
                              </button>
                            </div>
                          </div>
                        )}

                        {provider.providerType === 'tencent_tc3' && (
                          <>
                            <div className={styles.field} style={{ gridColumn: '1 / -1' }}>
                              <label className={styles.label}>
                                <Key size={12} />
                                SecretId
                              </label>
                              <input
                                type="text"
                                className={styles.input}
                                value={provider.secretId ?? ''}
                                onChange={(e) =>
                                  handleUpdateProvider(provider.id, {
                                    secretId: e.target.value || null,
                                  })
                                }
                                placeholder="AKIDxxxxxxxxxxxxxxxx"
                                autoComplete="off"
                                spellCheck={false}
                              />
                            </div>

                            <div className={styles.field} style={{ gridColumn: '1 / -1' }}>
                              <label className={styles.label}>
                                <Key size={12} />
                                SecretKey
                              </label>
                              <div className={styles.inputWrapper}>
                                <input
                                  type={
                                    showApiKey[provider.id] ? 'text' : 'password'
                                  }
                                  className={styles.input}
                                  value={provider.secretKey ?? ''}
                                  onChange={(e) =>
                                    handleUpdateProvider(provider.id, {
                                      secretKey: e.target.value || null,
                                    })
                                  }
                                  placeholder="******"
                                  autoComplete="off"
                                  spellCheck={false}
                                />
                                <button
                                  className={styles.toggleBtn}
                                  onClick={() => toggleShowApiKey(provider.id)}
                                  type="button"
                                  title={showApiKey[provider.id] ? '隐藏' : '显示'}
                                >
                                  {showApiKey[provider.id] ? (
                                    <EyeOff size={12} />
                                  ) : (
                                    <Eye size={12} />
                                  )}
                                </button>
                              </div>
                            </div>

                            <div className={styles.field}>
                              <label className={styles.label}>
                                <Globe size={12} />
                                地域
                              </label>
                              <input
                                type="text"
                                className={styles.input}
                                value={provider.region ?? ''}
                                onChange={(e) =>
                                  handleUpdateProvider(provider.id, {
                                    region: e.target.value || null,
                                  })
                                }
                                placeholder="ap-guangzhou"
                              />
                            </div>
                          </>
                        )}
                      </div>
                    )}

                    {isExpanded && (
                      <div className={styles.configFooter}>
                        <button
                          className={styles.testBtn}
                          onClick={() => handleTestConnection(provider)}
                          disabled={isTesting}
                        >
                          {isTesting ? (
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
                      </div>
                    )}

                    {isExpanded && testResults[provider.id] && (
                      <div
                        className={`${styles.testResult} ${
                          testResults[provider.id].success
                            ? styles.success
                            : styles.error
                        }`}
                      >
                        {testResults[provider.id].success ? (
                          <Check size={12} />
                        ) : (
                          <AlertCircle size={12} />
                        )}
                        <span>{testResults[provider.id].message}</span>
                      </div>
                    )}
                  </div>
                );
              })
            )}
          </div>
        </div>
      </div>
    </div>
  );
};
