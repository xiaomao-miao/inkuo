import { invoke } from '@tauri-apps/api/core';
import React, { useState } from 'react';
import {
  X,
  Check,
  AlertCircle,
  Loader2,
} from 'lucide-react';
import { useSettingsStore } from '../../store';
import { ModelsSettings } from './ModelsSettings';
import styles from './Settings.module.css';

interface SettingsProps {
  onClose: () => void;
}

export const Settings: React.FC<SettingsProps> = ({ onClose }) => {
  const { getActiveApiConfig } = useSettingsStore();

  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<{ success: boolean; message: string } | null>(null);

  const activeConfig = getActiveApiConfig();

  const handleTestConnection = async () => {
    if (!activeConfig) {
      setTestResult({ success: false, message: '当前没有可用的 API 配置' });
      return;
    }

    setTesting(true);
    setTestResult(null);

    try {
      const result = await invoke<{ success: boolean; message: string }>('test_api_config', {
        request: {
          api_key: activeConfig.apiKey,
          base_url: activeConfig.baseUrl,
          model: activeConfig.model,
          provider: activeConfig.provider,
        },
      });
      setTestResult(result);
    } catch (err) {
      setTestResult({
        success: false,
        message: err instanceof Error ? err.message : '连接失败，请检查配置',
      });
    } finally {
      setTesting(false);
    }
  };

  return (
    <div className={styles.overlay} onClick={onClose}>
      <div className={styles.panel} onClick={e => e.stopPropagation()}>
        <div className={styles.header}>
          <h2>设置</h2>
          <button className={styles.closeBtn} onClick={onClose}>
            <X size={18} />
          </button>
        </div>

        <div className={styles.content}>
          <ModelsSettings />

          <div className={styles.section}>
            <h3 className={styles.sectionTitle}>当前生效配置</h3>
            {activeConfig ? (
              <>
                <div className={styles.field}>
                  <label className={styles.label}>名称</label>
                  <div className={styles.hint}>{activeConfig.name}</div>
                </div>
                <div className={styles.field}>
                  <label className={styles.label}>提供商</label>
                  <div className={styles.hint}>{activeConfig.provider}</div>
                </div>
                <div className={styles.field}>
                  <label className={styles.label}>模型</label>
                  <div className={styles.hint}>{activeConfig.model}</div>
                </div>
                <div className={styles.field}>
                  <label className={styles.label}>API URL</label>
                  <div className={styles.hint}>{activeConfig.baseUrl}</div>
                </div>
              </>
            ) : (
              <div className={styles.field}>
                <div className={styles.hint}>当前没有可用的 API 配置</div>
              </div>
            )}

            <div className={styles.field}>
              <button
                className={styles.testBtn}
                onClick={handleTestConnection}
                disabled={testing || !activeConfig || !activeConfig.baseUrl}
              >
                {testing ? (
                  <>
                    <Loader2 size={14} className={styles.spinner} />
                    测试中...
                  </>
                ) : (
                  '测试当前配置'
                )}
              </button>

              {testResult && (
                <div className={`${styles.testResult} ${testResult.success ? styles.success : styles.error}`}>
                  {testResult.success ? <Check size={14} /> : <AlertCircle size={14} />}
                  <span>{testResult.message}</span>
                </div>
              )}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
};
