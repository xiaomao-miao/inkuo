import { useState } from 'react';
import { Globe, Key, Eye, EyeOff, AlertCircle, Info } from 'lucide-react';
import { useSettingsStore } from '../../store';
import type { WebSearchProviderConfig } from '../../types';
import styles from './SettingsPanel.module.css';
import panelStyles from './ModelsSettings.module.css';

const BAIKE_PROVIDER_ID = 'baike';

/** Static metadata for the providers the UI knows about. Adding a new
 * provider only requires extending this list — the rest of the panel
 * is data-driven off `settings.web_search.providers`. */
const PROVIDER_META: Record<
  string,
  {
    label: string;
    description: string;
    keyHelp: string;
    /** When `true`, a key is mandatory for the provider. When `false`,
     * the tool still works without one (subject to limits); today all
     * wired-up providers require a key. */
    keyOptional: boolean;
  }
> = {
  [BAIKE_PROVIDER_ID]: {
    label: '百度百科',
    description:
      '百度智能云千帆 AppBuilder 的「百科查询」接口（appbuilder.baidu.com）。返回词条摘要、关联条目、视频等结构化字段。',
    keyHelp:
      '在百度智能云千帆 AppBuilder 控制台申请 API Key 后粘贴到这里。申请地址：https://console.bce.baidu.com/  →  AppBuilder  →  API Key',
    keyOptional: false,
  },
};

const MIN_MAX_RESULTS = 1;
const MAX_MAX_RESULTS = 20;
const DEFAULT_MAX_RESULTS = 5;

export const WebSearchSettings = () => {
  const settings = useSettingsStore((state) => state.settings);
  const updateWebSearchAndPersist = useSettingsStore(
    (state) => state.updateWebSearchAndPersist
  );
  const updateWebSearchProviderAndPersist = useSettingsStore(
    (state) => state.updateWebSearchProviderAndPersist
  );

  const [showKey, setShowKey] = useState<Record<string, boolean>>({});

  const provider: WebSearchProviderConfig | undefined = settings.web_search.providers.find(
    (p) => p.id === BAIKE_PROVIDER_ID
  );

  const meta = PROVIDER_META[BAIKE_PROVIDER_ID];

  const handleToggleMaster = (next: boolean) => {
    void updateWebSearchAndPersist({
      ...settings.web_search,
      enabled: next,
    });
  };

  const handleToggleProvider = (next: boolean) => {
    void updateWebSearchProviderAndPersist(BAIKE_PROVIDER_ID, { enabled: next });
  };

  const handleApiKeyChange = (raw: string) => {
    // Preserve the empty string the user typed so they can intentionally
    // clear the key (the backend falls back to the default when the
    // key is empty, which is the desired UX).
    void updateWebSearchProviderAndPersist(BAIKE_PROVIDER_ID, {
      apiKey: raw.trim() === '' ? null : raw,
    });
  };

  const handleBaseUrlChange = (raw: string) => {
    void updateWebSearchProviderAndPersist(BAIKE_PROVIDER_ID, {
      baseUrl: raw.trim() === '' ? null : raw,
    });
  };

  const handleMaxResultsChange = (raw: string) => {
    const parsed = parseInt(raw, 10);
    const clamped =
      Number.isFinite(parsed) && parsed > 0
        ? Math.min(MAX_MAX_RESULTS, Math.max(MIN_MAX_RESULTS, parsed))
        : DEFAULT_MAX_RESULTS;
    void updateWebSearchAndPersist({
      ...settings.web_search,
      maxResults: clamped,
    });
  };

  return (
    <div className={styles.tabContent}>
      <div className={styles.section}>
        <h4 className={styles.sectionTitle}>
          <Globe size={14} />
          联网搜索
        </h4>
        <p className={styles.sectionDescription}>
          允许 Agent 在对话中检索外部资料。当前内置百度百科；后续可扩展其他来源。
          启用后需在「联网搜索」工具栏 toggle 打开才会在当次对话中生效。
        </p>

        <div className={styles.field}>
          <label className={styles.label}>启用联网搜索</label>
          <div className={styles.toggleWrapper}>
            <label className={styles.toggle}>
              <input
                type="checkbox"
                checked={settings.web_search.enabled}
                onChange={(e) => handleToggleMaster(e.target.checked)}
              />
              <span className={styles.toggleSlider}></span>
            </label>
            <span className={styles.toggleLabel}>
              {settings.web_search.enabled ? '启用' : '禁用'}
            </span>
          </div>
          <p className={styles.fieldHelp}>
            关闭后，<code>web_search</code> 工具会被注册但所有调用都会返回「已禁用」的提示。
          </p>
        </div>
      </div>

      <div className={styles.section}>
        <h4 className={styles.sectionTitle}>
          <Key size={14} />
          数据源
        </h4>

        {!provider ? (
          <div className={styles.noConfigWarning}>
            <AlertCircle size={14} />
            <span>未找到「百度百科」提供方配置，请尝试重置设置或重启应用。</span>
          </div>
        ) : (
          <>
            <div className={styles.field}>
              <label className={styles.label}>{meta.label}</label>
              <div className={styles.toggleWrapper}>
                <label className={styles.toggle}>
                  <input
                    type="checkbox"
                    checked={provider.enabled}
                    disabled={!settings.web_search.enabled}
                    onChange={(e) => handleToggleProvider(e.target.checked)}
                  />
                  <span className={styles.toggleSlider}></span>
                </label>
                <span className={styles.toggleLabel}>
                  {provider.enabled ? '启用' : '禁用'}
                </span>
              </div>
              <p className={styles.fieldHelp}>{meta.description}</p>
            </div>

            <div className={styles.field}>
              <label className={styles.label}>
                API Key {meta.keyOptional ? '（可选）' : '（必填）'}
              </label>
              <div className={panelStyles.apiKeyWrapper ?? styles.field}>
                <div style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
                  <input
                    type={showKey[BAIKE_PROVIDER_ID] ? 'text' : 'password'}
                    value={provider.apiKey ?? ''}
                    placeholder="请粘贴百度智能云 AppBuilder API Key"
                    disabled={!settings.web_search.enabled || !provider.enabled}
                    onChange={(e) => handleApiKeyChange(e.target.value)}
                    style={{
                      flex: 1,
                      padding: '6px 10px',
                      border: '1px solid var(--border-color)',
                      borderRadius: 4,
                      backgroundColor: 'var(--bg-primary)',
                      color: 'var(--fg-primary)',
                      fontSize: 12,
                      fontFamily: 'var(--editor-font-family, monospace)',
                    }}
                  />
                  <button
                    type="button"
                    onClick={() =>
                      setShowKey((prev) => ({
                        ...prev,
                        [BAIKE_PROVIDER_ID]: !prev[BAIKE_PROVIDER_ID],
                      }))
                    }
                    style={{
                      padding: '6px 8px',
                      border: '1px solid var(--border-color)',
                      borderRadius: 4,
                      backgroundColor: 'var(--bg-primary)',
                      color: 'var(--fg-secondary)',
                      cursor: 'pointer',
                      display: 'flex',
                      alignItems: 'center',
                    }}
                    aria-label={showKey[BAIKE_PROVIDER_ID] ? '隐藏' : '显示'}
                  >
                    {showKey[BAIKE_PROVIDER_ID] ? (
                      <EyeOff size={14} />
                    ) : (
                      <Eye size={14} />
                    )}
                  </button>
                </div>
              </div>
              <p className={styles.fieldHelp}>
                <Info size={11} style={{ verticalAlign: -1, marginRight: 4 }} />
                {meta.keyHelp}
              </p>
            </div>

            <div className={styles.field}>
              <label className={styles.label}>自定义 API 端点（可选）</label>
              <input
                type="text"
                value={provider.baseUrl ?? ''}
                placeholder="留空使用默认端点"
                disabled={!settings.web_search.enabled || !provider.enabled}
                onChange={(e) => handleBaseUrlChange(e.target.value)}
                style={{
                  width: '100%',
                  padding: '6px 10px',
                  border: '1px solid var(--border-color)',
                  borderRadius: 4,
                  backgroundColor: 'var(--bg-primary)',
                  color: 'var(--fg-primary)',
                  fontSize: 12,
                  fontFamily: 'var(--editor-font-family, monospace)',
                  boxSizing: 'border-box',
                }}
              />
              <p className={styles.fieldHelp}>
                默认指向 <code>{defaultEndpoint()}</code>。仅在调试或使用反向代理时需要修改。
              </p>
            </div>
          </>
        )}
      </div>

      <div className={styles.section}>
        <h4 className={styles.sectionTitle}>
          <Globe size={14} />
          检索行为
        </h4>
        <div className={styles.field}>
          <label className={styles.label} htmlFor="web-search-max-results">
            每次调用返回的最大条目数
          </label>
          <div className={styles.rangeWrapper}>
            <input
              id="web-search-max-results"
              type="range"
              min={MIN_MAX_RESULTS}
              max={MAX_MAX_RESULTS}
              step={1}
              value={settings.web_search.maxResults}
              onChange={(e) => handleMaxResultsChange(e.target.value)}
              className={styles.range}
              disabled={!settings.web_search.enabled}
            />
            <span className={styles.rangeValue}>{settings.web_search.maxResults} 条</span>
          </div>
          <p className={styles.fieldHelp}>
            范围 {MIN_MAX_RESULTS}–{MAX_MAX_RESULTS}，默认 {DEFAULT_MAX_RESULTS}。值越大返回内容越长，但会消耗更多 token。
          </p>
        </div>
      </div>
    </div>
  );
};

/** Default endpoint exposed as a helper so the help text stays in
 * sync with the Rust default if it ever changes (the constant lives in
 * the tool module on the Rust side). */
function defaultEndpoint(): string {
  return 'https://appbuilder.baidu.com/v2/baike/lemma/get_content';
}