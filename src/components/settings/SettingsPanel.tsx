import React, { useState } from 'react';
import { 
  Key, 
  Globe, 
  Cpu, 
  Check, 
  AlertCircle,
  Loader2,
  Eye,
  EyeOff,
  RefreshCw
} from 'lucide-react';
import { useSettingsStore } from '../../store';
import styles from './SettingsPanel.module.css';

export const SettingsPanel: React.FC = () => {
  const { settings, updateSetting } = useSettingsStore();

  const [showApiKey, setShowApiKey] = useState(false);
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<{ success: boolean; message: string } | null>(null);
  const [saving, setSaving] = useState(false);

  const saveSettings = async () => {
    setSaving(true);
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      await invoke('save_settings', { settings });
    } catch (err) {
      console.error('Failed to save settings:', err);
    } finally {
      setSaving(false);
    }
  };

  const handleTestConnection = async () => {
    setTesting(true);
    setTestResult(null);
    
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      const result = await invoke<{ success: boolean; message: string }>('test_ai_connection', {
        apiKey: settings.ai_api_key,
        baseUrl: settings.ai_base_url,
        model: settings.ai_model,
      });
      setTestResult(result);
    } catch (err: any) {
      setTestResult({
        success: false,
        message: err.message || '连接失败，请检查配置',
      });
    } finally {
      setTesting(false);
    }
  };

  return (
    <div className={styles.panel}>
      <div className={styles.header}>
        <h3>设置</h3>
      </div>
      
      <div className={styles.content}>
        {/* AI Settings Section */}
        <div className={styles.section}>
          <h4 className={styles.sectionTitle}>
            <Cpu size={14} />
            AI 设置
          </h4>
          
          {/* Provider Selection */}
          <div className={styles.field}>
            <label className={styles.label}>AI 提供商</label>
            <select
              className={styles.select}
              value={settings.ai_provider}
              onChange={e => { updateSetting('ai_provider', e.target.value as any); saveSettings(); }}
            >
              <option value="openai">OpenAI (兼容)</option>
              <option value="ollama">Ollama (本地)</option>
              <option value="deepseek">DeepSeek</option>
            </select>
          </div>

          {/* Base URL */}
          <div className={styles.field}>
            <label className={styles.label}>
              <Globe size={14} />
              API Base URL
            </label>
            <input
              type="text"
              className={styles.input}
              value={settings.ai_base_url || ''}
              onChange={e => { updateSetting('ai_base_url', e.target.value); saveSettings(); }}
              placeholder="https://api.openai.com/v1"
            />
          </div>

          {/* Model */}
          <div className={styles.field}>
            <label className={styles.label}>
              <Cpu size={14} />
              模型
            </label>
            <input
              type="text"
              className={styles.input}
              value={settings.ai_model}
              onChange={e => { updateSetting('ai_model', e.target.value); saveSettings(); }}
              placeholder="gpt-4o-mini"
            />
          </div>

          {/* API Key */}
          <div className={styles.field}>
            <label className={styles.label}>
              <Key size={14} />
              API Key
            </label>
            <div className={styles.inputWrapper}>
              <input
                type={showApiKey ? 'text' : 'password'}
                className={styles.input}
                value={settings.ai_api_key || ''}
                onChange={e => { updateSetting('ai_api_key', e.target.value || null); saveSettings(); }}
                placeholder="sk-..."
              />
              <button
                className={styles.toggleBtn}
                onClick={() => setShowApiKey(!showApiKey)}
                type="button"
              >
                {showApiKey ? <EyeOff size={14} /> : <Eye size={14} />}
              </button>
            </div>
          </div>

          {/* Test Connection */}
          <div className={styles.field}>
            <button
              className={styles.testBtn}
              onClick={handleTestConnection}
              disabled={testing || !settings.ai_api_key || !settings.ai_base_url}
            >
              {testing ? (
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
            
            {testResult && (
              <div className={`${styles.testResult} ${testResult.success ? styles.success : styles.error}`}>
                {testResult.success ? <Check size={12} /> : <AlertCircle size={12} />}
                <span>{testResult.message}</span>
              </div>
            )}
          </div>
        </div>

        {/* Editor Settings */}
        <div className={styles.section}>
          <h4 className={styles.sectionTitle}>编辑器</h4>
          
          <div className={styles.field}>
            <label className={styles.label}>字体大小</label>
            <div className={styles.rangeWrapper}>
              <input
                type="range"
                min="10"
                max="24"
                value={settings.editor_font_size}
                onChange={e => { updateSetting('editor_font_size', parseInt(e.target.value)); saveSettings(); }}
                className={styles.range}
              />
              <span className={styles.rangeValue}>{settings.editor_font_size}px</span>
            </div>
          </div>

          <div className={styles.field}>
            <label className={styles.label}>字体</label>
            <select
              className={styles.select}
              value={settings.editor_font_family}
              onChange={e => { updateSetting('editor_font_family', e.target.value); saveSettings(); }}
            >
              <option value="JetBrains Mono, monospace">JetBrains Mono</option>
              <option value="Fira Code, monospace">Fira Code</option>
              <option value="Cascadia Code, Consolas, monospace">Cascadia Code</option>
              <option value="Consolas, monospace">Consolas</option>
              <option value="Monaco, monospace">Monaco</option>
            </select>
          </div>
        </div>

        {/* Theme Settings */}
        <div className={styles.section}>
          <h4 className={styles.sectionTitle}>主题</h4>
          
          <div className={styles.field}>
            <label className={styles.label}>配色方案</label>
            <div className={styles.themeGrid}>
              <button
                className={`${styles.themeOption} ${settings.theme === 'cursor-dark' ? styles.active : ''}`}
                                onClick={() => { updateSetting('theme', 'cursor-dark'); saveSettings(); }}
              >
                <div className={styles.themePreview} style={{ background: '#1e1e1e' }}>
                  <div style={{ color: '#7c5cff', fontSize: '10px' }}>Aa</div>
                </div>
                <span>深色</span>
              </button>
              <button
                className={`${styles.themeOption} ${settings.theme === 'cursor-light' ? styles.active : ''}`}
                                onClick={() => { updateSetting('theme', 'cursor-light'); saveSettings(); }}
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
                onChange={e => { updateSetting('accent_color', e.target.value); saveSettings(); }}
                className={styles.colorInput}
              />
              <span className={styles.colorValue}>{settings.accent_color}</span>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
};
