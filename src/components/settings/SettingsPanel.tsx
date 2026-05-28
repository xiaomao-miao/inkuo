import React, { useState } from 'react';
import {
  Settings2,
  Cpu,
  Palette,
  Type,
  Keyboard,
  Sparkles,
  Info,
  AlertCircle
} from 'lucide-react';
import { useSettingsStore, useInlineCompleteStore } from '../../store';
import { ModelsSettings } from './ModelsSettings';
import { Select } from './Select';
import styles from './SettingsPanel.module.css';

type SettingsTab = 'models' | 'editor' | 'ai' | 'appearance' | 'about';

export const SettingsPanel: React.FC = () => {
  const { settings, updateSetting } = useSettingsStore();
  const { enabled, debounceMs, setEnabled } = useInlineCompleteStore();
  const [activeTab, setActiveTab] = useState<SettingsTab>('models');

  const saveSettings = async () => {
    try {
      const { invoke } = await import('@tauri-apps/api/core');

      // Convert to snake_case for Rust backend
      const backendSettings = {
        theme: settings.theme,
        accent_color: settings.accent_color,
        editor_font_size: settings.editor_font_size,
        editor_font_family: settings.editor_font_family,
        ai_provider: settings.ai_provider,
        ai_model: settings.ai_model,
        ai_api_key: settings.ai_api_key,
        ai_base_url: settings.ai_base_url,
        ai_temperature: settings.ai_temperature,
        ai_max_tokens: settings.ai_max_tokens,
        api_configs: settings.apiConfigs.map(c => ({
          id: c.id,
          name: c.name,
          provider: c.provider,
          base_url: c.baseUrl,
          api_key: c.apiKey,
          model: c.model,
          is_default: c.isDefault,
          enabled: c.enabled,
          temperature: c.temperature,
          max_tokens: c.maxTokens,
        })),
        active_api_config_id: settings.activeApiConfigId,
      };

      await invoke('save_settings', { settings: backendSettings });
    } catch (err) {
      console.error('Failed to save settings:', err);
    }
  };

  const tabs: { id: SettingsTab; label: string; icon: React.ReactNode }[] = [
    { id: 'models', label: '模型', icon: <Cpu size={14} /> },
    { id: 'editor', label: '编辑器', icon: <Type size={14} /> },
    { id: 'ai', label: 'AI', icon: <Sparkles size={14} /> },
    { id: 'appearance', label: '外观', icon: <Palette size={14} /> },
    { id: 'about', label: '关于', icon: <Info size={14} /> },
  ];

  const renderTabContent = () => {
    switch (activeTab) {
      case 'models':
        return <ModelsSettings />;
      case 'editor':
        return (
          <div className={styles.tabContent}>
            <div className={styles.section}>
              <h4 className={styles.sectionTitle}>
                <Type size={14} />
                字体设置
              </h4>

              <div className={styles.field}>
                <label className={styles.label}>字体大小</label>
                <div className={styles.rangeWrapper}>
                  <input
                    type="range"
                    min="10"
                    max="24"
                    value={settings.editor_font_size}
                    onChange={(e) => {
                      updateSetting('editor_font_size', parseInt(e.target.value));
                      saveSettings();
                    }}
                    className={styles.range}
                  />
                  <span className={styles.rangeValue}>{settings.editor_font_size}px</span>
                </div>
              </div>

              <div className={styles.field}>
                <label className={styles.label}>字体</label>
                <Select
                  value={settings.editor_font_family}
                  options={[
                    { value: 'JetBrains Mono, monospace', label: 'JetBrains Mono' },
                    { value: 'Fira Code, monospace', label: 'Fira Code' },
                    { value: 'Cascadia Code, Consolas, monospace', label: 'Cascadia Code' },
                    { value: 'Consolas, monospace', label: 'Consolas' },
                    { value: 'Monaco, monospace', label: 'Monaco' },
                  ]}
                  onChange={(value) => {
                    updateSetting('editor_font_family', value);
                    saveSettings();
                  }}
                  className={styles.select}
                />
              </div>
            </div>

            <div className={styles.section}>
              <h4 className={styles.sectionTitle}>
                <Keyboard size={14} />
                编辑器选项
              </h4>

              <div className={styles.field}>
                <label className={styles.label}>自动换行</label>
                <div className={styles.toggleWrapper}>
                  <label className={styles.toggle}>
                    <input
                      type="checkbox"
                      defaultChecked={true}
                      onChange={(e) => {
                        // TODO: Implement word wrap setting
                        console.log('Word wrap:', e.target.checked);
                      }}
                    />
                    <span className={styles.toggleSlider}></span>
                  </label>
                  <span className={styles.toggleLabel}>启用</span>
                </div>
              </div>

              <div className={styles.field}>
                <label className={styles.label}>显示行号</label>
                <div className={styles.toggleWrapper}>
                  <label className={styles.toggle}>
                    <input
                      type="checkbox"
                      defaultChecked={true}
                      onChange={(e) => {
                        // TODO: Implement line numbers setting
                        console.log('Line numbers:', e.target.checked);
                      }}
                    />
                    <span className={styles.toggleSlider}></span>
                  </label>
                  <span className={styles.toggleLabel}>启用</span>
                </div>
              </div>
            </div>
          </div>
        );
      case 'ai':
        const activeConfig = settings.apiConfigs.find(
          (c) => c.id === settings.activeApiConfigId
        );

        const modelOptions = settings.apiConfigs.map((config) => ({
          value: config.id,
          label: `${config.name} (${config.model})`,
        }));

        return (
          <div className={styles.tabContent}>
            <div className={styles.section}>
              <h4 className={styles.sectionTitle}>
                <Sparkles size={14} />
                AI Tab 补全
              </h4>
              <p className={styles.sectionDescription}>
                在编辑器中按 Tab 键触发 AI 代码补全建议。
              </p>

              <div className={styles.field}>
                <label className={styles.label}>AI 模型</label>
                {settings.apiConfigs.length === 0 ? (
                  <div className={styles.noConfigWarning}>
                    <AlertCircle size={14} />
                    <span>请先在「模型」设置中添加 API 配置</span>
                  </div>
                ) : (
                  <Select
                    value={settings.activeApiConfigId || ''}
                    options={modelOptions}
                    onChange={(value) => {
                      useSettingsStore.getState().setActiveApiConfig(value);
                      saveSettings();
                    }}
                    className={styles.select}
                  />
                )}
                {activeConfig && (
                  <div className={styles.activeConfigInfo}>
                    <div className={styles.configDetail}>
                      <span className={styles.configLabel}>提供商:</span>
                      <span>{activeConfig.provider}</span>
                    </div>
                    <div className={styles.configDetail}>
                      <span className={styles.configLabel}>模型:</span>
                      <span>{activeConfig.model}</span>
                    </div>
                    <div className={styles.configDetail}>
                      <span className={styles.configLabel}>API URL:</span>
                      <span>{activeConfig.baseUrl}</span>
                    </div>
                  </div>
                )}
                {!activeConfig && settings.apiConfigs.length > 0 && (
                  <p className={styles.fieldHelp} style={{ color: '#f59e0b' }}>
                    当前没有选中的 API 配置
                  </p>
                )}
              </div>

              <div className={styles.field}>
                <label className={styles.label}>启用 AI Tab 补全</label>
                <div className={styles.toggleWrapper}>
                  <label className={styles.toggle}>
                    <input
                      type="checkbox"
                      checked={enabled}
                      onChange={(e) => {
                        setEnabled(e.target.checked);
                      }}
                    />
                    <span className={styles.toggleSlider}></span>
                  </label>
                  <span className={styles.toggleLabel}>{enabled ? '启用' : '禁用'}</span>
                </div>
              </div>

              <div className={styles.field}>
                <label className={styles.label}>触发延迟</label>
                <div className={styles.rangeWrapper}>
                  <input
                    type="range"
                    min="100"
                    max="1000"
                    step="50"
                    value={debounceMs}
                    onChange={(e) => {
                      useInlineCompleteStore.getState().updateSettings({
                        debounceMs: parseInt(e.target.value)
                      });
                    }}
                    className={styles.range}
                  />
                  <span className={styles.rangeValue}>{debounceMs}ms</span>
                </div>
                <p className={styles.fieldHelp}>
                  按下 Tab 键后等待多久触发补全请求
                </p>
              </div>
            </div>

            <div className={styles.section}>
              <h4 className={styles.sectionTitle}>快捷键</h4>
              <div className={styles.shortcutList}>
                <div className={styles.shortcutItem}>
                  <kbd>Tab</kbd>
                  <span>触发/接受补全</span>
                </div>
                <div className={styles.shortcutItem}>
                  <kbd>Esc</kbd>
                  <span>拒绝补全</span>
                </div>
              </div>
            </div>
          </div>
        );
      case 'appearance':
        return (
          <div className={styles.tabContent}>
            <div className={styles.section}>
              <h4 className={styles.sectionTitle}>
                <Palette size={14} />
                配色方案
              </h4>

              <div className={styles.field}>
                <label className={styles.label}>主题</label>
                <div className={styles.themeGrid}>
                  <button
                    className={`${styles.themeOption} ${
                      settings.theme === 'cursor-dark' ? styles.active : ''
                    }`}
                    onClick={() => {
                      updateSetting('theme', 'cursor-dark');
                      saveSettings();
                    }}
                  >
                    <div className={styles.themePreview} style={{ background: '#1e1e1e' }}>
                      <div style={{ color: '#7c5cff', fontSize: '10px' }}>Aa</div>
                    </div>
                    <span>深色</span>
                  </button>
                  <button
                    className={`${styles.themeOption} ${
                      settings.theme === 'cursor-light' ? styles.active : ''
                    }`}
                    onClick={() => {
                      updateSetting('theme', 'cursor-light');
                      saveSettings();
                    }}
                  >
                    <div className={styles.themePreview} style={{ background: '#ffffff' }}>
                      <div style={{ color: '#7c5cff', fontSize: '10px' }}>Aa</div>
                    </div>
                    <span>浅色</span>
                  </button>
                </div>
              </div>

              <div className={styles.field}>
                <label className={styles.label}>强调色</label>
                <div className={styles.colorPicker}>
                  <input
                    type="color"
                    value={settings.accent_color}
                    onChange={(e) => {
                      updateSetting('accent_color', e.target.value);
                      saveSettings();
                    }}
                    className={styles.colorInput}
                  />
                  <span className={styles.colorValue}>{settings.accent_color}</span>
                </div>
              </div>
            </div>
          </div>
        );
      case 'about':
        return (
          <div className={styles.tabContent}>
            <div className={styles.aboutSection}>
              <div className={styles.appLogo}>
                <Settings2 size={48} strokeWidth={1.5} />
              </div>
              <h2 className={styles.appName}>Inkuo</h2>
              <p className={styles.appVersion}>版本 1.0.0</p>
              <p className={styles.appDescription}>
                Inkuo 是一个本地优先的 AI 文档编辑器，帮助你更高效地编辑和管理文档。
              </p>

              <div className={styles.aboutLinks}>
                <div className={styles.aboutLink}>
                  <span className={styles.linkLabel}>GitHub</span>
                  <span className={styles.linkValue}>github.com/inkuo/inkuo</span>
                </div>
                <div className={styles.aboutLink}>
                  <span className={styles.linkLabel}>文档</span>
                  <span className={styles.linkValue}>docs.inkuo.com</span>
                </div>
              </div>

              <div className={styles.techStack}>
                <h4>技术栈</h4>
                <div className={styles.techTags}>
                  <span className={styles.techTag}>Rust</span>
                  <span className={styles.techTag}>Tauri</span>
                  <span className={styles.techTag}>React</span>
                  <span className={styles.techTag}>TypeScript</span>
                  <span className={styles.techTag}>CodeMirror</span>
                </div>
              </div>
            </div>
          </div>
        );
      default:
        return null;
    }
  };

  return (
    <div className={styles.panel}>
      <div className={styles.header}>
        <h3>设置</h3>
      </div>

      <div className={styles.tabs}>
        {tabs.map((tab) => (
          <button
            key={tab.id}
            className={`${styles.tab} ${activeTab === tab.id ? styles.activeTab : ''}`}
            onClick={() => setActiveTab(tab.id)}
          >
            {tab.icon}
            <span>{tab.label}</span>
          </button>
        ))}
      </div>

      <div className={styles.content}>
        {renderTabContent()}
      </div>
    </div>
  );
};
