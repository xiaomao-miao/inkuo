import React, { useState } from 'react';
import {
  Settings2,
  Cpu,
  Palette,
  Type,
  Keyboard,
  Info
} from 'lucide-react';
import { useSettingsStore } from '../../store';
import { ModelsSettings } from './ModelsSettings';
import { Select } from './Select';
import styles from './SettingsPanel.module.css';

type SettingsTab = 'models' | 'editor' | 'appearance' | 'about';

export const SettingsPanel: React.FC = () => {
  const { settings, updateSetting } = useSettingsStore();
  const [activeTab, setActiveTab] = useState<SettingsTab>('models');

  const saveSettings = async () => {
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      await invoke('save_settings', { settings });
    } catch (err) {
      console.error('Failed to save settings:', err);
    }
  };

  const tabs: { id: SettingsTab; label: string; icon: React.ReactNode }[] = [
    { id: 'models', label: '模型', icon: <Cpu size={14} /> },
    { id: 'editor', label: '编辑器', icon: <Type size={14} /> },
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
