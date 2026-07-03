import { useState } from 'react';
import { History } from 'lucide-react';
import { useSettingsStore } from '../../store';
import styles from './SettingsPanel.module.css';

export const SnapshotsSettings = () => {
  const settings = useSettingsStore((state) => state.settings);
  const updateSettingAndPersist = useSettingsStore(
    (state) => state.updateSettingAndPersist
  );
  const [maxCountInput, setMaxCountInput] = useState(
    String(settings.snapshot.maxCount)
  );

  const handleMaxCountChange = (value: string) => {
    setMaxCountInput(value);
    const parsed = parseInt(value, 10);
    if (!Number.isFinite(parsed) || parsed < 1) return;
    const clamped = Math.min(Math.max(parsed, 1), 9999);
    void updateSettingAndPersist('snapshot', {
      ...settings.snapshot,
      maxCount: clamped,
    });
  };

  const handleAutoBaselineChange = (checked: boolean) => {
    void updateSettingAndPersist('snapshot', {
      ...settings.snapshot,
      autoBaseline: checked,
    });
  };

  return (
    <div className={styles.tabContent}>
      <div className={styles.section}>
        <h4 className={styles.sectionTitle}>
          <History size={14} />
          快照
        </h4>
        <p className={styles.sectionDescription}>
          快照会保存工作区中所有文件的完整副本，用于在 AI 编辑出错或需要撤销
          时一键回滚到任意一个历史状态。
        </p>

        <div className={styles.field}>
          <label className={styles.label} htmlFor="snapshot-max-count">
            最大保留快照数
          </label>
          <div className={styles.rangeWrapper}>
            <input
              id="snapshot-max-count"
              type="number"
              min={1}
              max={9999}
              value={maxCountInput}
              onChange={(e) => handleMaxCountChange(e.target.value)}
              className={styles.input}
              style={{ width: 120 }}
            />
            <span className={styles.rangeValue}>个 / 工作区</span>
          </div>
          <p className={styles.fieldHelp}>
            超出上限时最旧的快照会被自动删除。设为 0 表示不限制（不推荐）。
          </p>
        </div>

        <div className={styles.field}>
          <label className={styles.label}>AI 流开始时自动创建基线</label>
          <div className={styles.toggleWrapper}>
            <label className={styles.toggle}>
              <input
                type="checkbox"
                checked={settings.snapshot.autoBaseline}
                onChange={(e) => handleAutoBaselineChange(e.target.checked)}
              />
              <span className={styles.toggleSlider}></span>
            </label>
            <span className={styles.toggleLabel}>
              {settings.snapshot.autoBaseline ? '启用' : '禁用'}
            </span>
          </div>
          <p className={styles.fieldHelp}>
            启用后，每次 AI Agent 开始执行指令前会自动打一个基线快照。
            在 AI 面板中编辑并重新发送用户消息时，会先自动回滚到该基线再重发。
          </p>
        </div>
      </div>
    </div>
  );
};

export default SnapshotsSettings;
