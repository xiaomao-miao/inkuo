// `<ModelSwitcher>` — dropdown letting the user pick between cloud
// and local-API routing for the active chat.
//
// Reads cloud / local settings from `useSettingsStore`. Composes
// pure helpers from `./modelSwitcher.helpers` so each helper
// (`encodeSelectValue`, `currentSelectValue`, etc.) is unit-tested
// in isolation.

import React from 'react';

import { useSettingsStore } from '../../../store';

import {
  activeSelectionLabel,
  currentSelectValue,
  decodeSelectValue,
  shouldHideSwitcher,
} from './modelSwitcher.helpers';

import styles from '../AIPanelInput.module.css';

export const ModelSwitcher: React.FC = () => {
  const cloudMode = useSettingsStore((s) => s.settings.cloud.cloud_mode_enabled);
  const cloudAccount = useSettingsStore((s) => s.settings.cloud.account);
  const cloudModels = useSettingsStore((s) => s.settings.cloud.cached_models);
  const activeCloudModelId = useSettingsStore(
    (s) => s.settings.cloud.active_cloud_model_id,
  );
  const setCloudModeEnabled = useSettingsStore((s) => s.setCloudModeEnabled);
  const setActiveCloudModelId = useSettingsStore((s) => s.setActiveCloudModelId);
  const apiConfigs = useSettingsStore((s) => s.settings.apiConfigs);
  const activeApiConfigId = useSettingsStore((s) => s.settings.activeApiConfigId);
  const setActiveApiConfig = useSettingsStore((s) => s.setActiveApiConfig);

  const hasCloudOptions = !!cloudAccount && cloudModels.length > 0;
  const hasLocalOptions = apiConfigs.length > 0;

  // Hide entirely when nothing is available — both groups empty.
  if (shouldHideSwitcher({ hasCloudOptions, hasLocalOptions })) return null;

  const currentValue = currentSelectValue({
    cloudMode,
    activeCloudModelId,
    activeApiConfigId,
    fallbackLocalConfigId: apiConfigs[0]?.id ?? null,
  });

  const onChange = async (e: React.ChangeEvent<HTMLSelectElement>) => {
    const parsed = decodeSelectValue(e.target.value);
    if (!parsed) return;
    if (parsed.kind === 'cloud') {
      if (!cloudMode) await setCloudModeEnabled(true);
      await setActiveCloudModelId(parsed.id);
    } else {
      if (cloudMode) await setCloudModeEnabled(false);
      await setActiveApiConfig(parsed.id);
    }
  };

  const activeLabel = activeSelectionLabel(cloudMode, {
    activeCloudModelName:
      cloudModels.find((m) => m.id === activeCloudModelId)?.display_name ?? null,
    activeLocalConfigName:
      apiConfigs.find((c) => c.id === activeApiConfigId)?.name ?? null,
    firstLocalConfigName: apiConfigs[0]?.name ?? null,
  });

  return (
    <select
      className={styles.modelSwitcher}
      data-cloud={cloudMode ? 'true' : undefined}
      value={currentValue}
      onChange={onChange}
      title={`当前: ${activeLabel}`}
    >
      {hasCloudOptions && (
        <optgroup label="☁ 云端模型">
          {cloudModels.map((m) => (
            <option key={`cloud:${m.id}`} value={`cloud:${m.id}`}>
              {m.display_name}
            </option>
          ))}
        </optgroup>
      )}
      {hasLocalOptions && (
        <optgroup label="💻 本地 API">
          {apiConfigs.map((c) => (
            <option key={`local:${c.id}`} value={`local:${c.id}`}>
              {c.name}
            </option>
          ))}
        </optgroup>
      )}
    </select>
  );
};