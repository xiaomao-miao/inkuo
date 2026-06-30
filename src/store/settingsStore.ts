import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import type { APIConfig, Settings } from '../types';
import { saveSettings } from '../utils/saveSettings';

interface SettingsState {
  settings: Settings;
  isSettingsOpen: boolean;

  setSettings: (settings: Settings) => void;
  persistSettings: (settings?: Settings) => Promise<void>;
  updateSetting: <K extends keyof Settings>(key: K, value: Settings[K]) => Settings;
  updateSettingAndPersist: <K extends keyof Settings>(key: K, value: Settings[K]) => Promise<Settings>;
  setIsSettingsOpen: (open: boolean) => void;

  addApiConfig: (config?: Partial<APIConfig>) => Settings;
  addApiConfigAndPersist: (config?: Partial<APIConfig>) => Promise<Settings>;
  updateApiConfig: (id: string, updates: Partial<APIConfig>) => Settings;
  updateApiConfigAndPersist: (id: string, updates: Partial<APIConfig>) => Promise<Settings>;
  removeApiConfig: (id: string) => Settings;
  removeApiConfigAndPersist: (id: string) => Promise<Settings>;
  setActiveApiConfig: (id: string) => Settings;
  setActiveApiConfigAndPersist: (id: string) => Promise<Settings>;
  getActiveApiConfig: () => APIConfig | null;
  setDefaultApiConfig: (id: string) => Settings;
  setDefaultApiConfigAndPersist: (id: string) => Promise<Settings>;
}

function createDefaultAPIConfig(): APIConfig {
  return {
    id: crypto.randomUUID(),
    name: 'DeepSeek V3',
    provider: 'deepseek',
    baseUrl: 'https://api.deepseek.com',
    apiKey: null,
    model: 'deepseek-chat',
    isDefault: true,
    enabled: true,
    temperature: 0.7,
    maxTokens: 4096,
  };
}

async function persistSettingsSnapshot(settings: Settings): Promise<void> {
  await saveSettings(settings);
}

function ensureValidApiConfigs(apiConfigs: APIConfig[]): APIConfig[] {
  if (apiConfigs.length === 0) {
    return [createDefaultAPIConfig()];
  }

  let hasDefault = false;
  return apiConfigs.map((config, index) => {
    const shouldBeDefault = !hasDefault && (config.isDefault || index === 0);
    if (shouldBeDefault) {
      hasDefault = true;
    }
    return {
      ...config,
      isDefault: shouldBeDefault,
    };
  });
}

const defaultAPIConfig = createDefaultAPIConfig();

const defaultSettings: Settings = {
  theme: 'cursor-dark',
  accent_color: '#7C5CFF',
  editor_font_size: 14,
  editor_font_family: 'JetBrains Mono, monospace',
  editor_word_wrap: true,
  editor_line_numbers: true,
  apiConfigs: [defaultAPIConfig],
  activeApiConfigId: defaultAPIConfig.id,
  embedding_model: 'BAAI/bge-small-zh-v1.5',
  embedding_model_path: null,
  chunk_size: 500,
  chunk_overlap: 50,
};

export const useSettingsStore = create<SettingsState>()(
  persist(
    (set, get) => ({
      settings: defaultSettings,
      isSettingsOpen: false,

      setSettings: (settings) => set({ settings }),
      persistSettings: async (settings) => {
        await persistSettingsSnapshot(settings ?? get().settings);
      },
      updateSetting: (key, value) => {
        const nextSettings = {
          ...get().settings,
          [key]: value,
        };
        set({ settings: nextSettings });
        return nextSettings;
      },
      updateSettingAndPersist: async (key, value) => {
        const nextSettings = get().updateSetting(key, value);
        await persistSettingsSnapshot(nextSettings);
        return nextSettings;
      },
      setIsSettingsOpen: (open) => set({ isSettingsOpen: open }),

      addApiConfig: (config) => {
        const newConfig: APIConfig = {
          id: crypto.randomUUID(),
          name: config?.name || 'New API',
          provider: config?.provider || 'openai',
          baseUrl: config?.baseUrl || 'https://api.openai.com/v1',
          // Preserve the empty string the user typed (e.g. "I want to
          // clear the key") — only fall back to null when the caller
          // didn't pass the field at all. The backend will then surface a
          // proper "missing API key" error for cloud providers.
          apiKey: config?.apiKey ?? null,
          model: config?.model || 'gpt-4o-mini',
          isDefault: false,
          enabled: true,
          temperature: config?.temperature ?? 0.7,
          maxTokens: config?.maxTokens ?? 4096,
        };

        const currentSettings = get().settings;
        const nextSettings = {
          ...currentSettings,
          apiConfigs: ensureValidApiConfigs([...currentSettings.apiConfigs, newConfig]),
        };

        set({ settings: nextSettings });
        return nextSettings;
      },
      addApiConfigAndPersist: async (config) => {
        const nextSettings = get().addApiConfig(config);
        await persistSettingsSnapshot(nextSettings);
        return nextSettings;
      },

      updateApiConfig: (id, updates) => {
        const currentSettings = get().settings;
        const nextSettings = {
          ...currentSettings,
          apiConfigs: currentSettings.apiConfigs.map((configItem) =>
            configItem.id === id ? { ...configItem, ...updates } : configItem
          ),
        };
        set({ settings: nextSettings });
        return nextSettings;
      },
      updateApiConfigAndPersist: async (id, updates) => {
        const nextSettings = get().updateApiConfig(id, updates);
        await persistSettingsSnapshot(nextSettings);
        return nextSettings;
      },

      removeApiConfig: (id) => {
        const currentSettings = get().settings;
        const remaining = currentSettings.apiConfigs.filter((config) => config.id !== id);
        const apiConfigs = ensureValidApiConfigs(remaining);
        const activeApiConfigId = apiConfigs.some((config) => config.id === currentSettings.activeApiConfigId)
          ? currentSettings.activeApiConfigId
          : apiConfigs[0].id;

        const nextSettings = {
          ...currentSettings,
          apiConfigs,
          activeApiConfigId,
        };

        set({ settings: nextSettings });
        return nextSettings;
      },
      removeApiConfigAndPersist: async (id) => {
        const nextSettings = get().removeApiConfig(id);
        await persistSettingsSnapshot(nextSettings);
        return nextSettings;
      },

      setActiveApiConfig: (id) => {
        const currentSettings = get().settings;
        const nextSettings = {
          ...currentSettings,
          activeApiConfigId: currentSettings.apiConfigs.some((config) => config.id === id)
            ? id
            : currentSettings.activeApiConfigId,
        };
        set({ settings: nextSettings });
        return nextSettings;
      },
      setActiveApiConfigAndPersist: async (id) => {
        const nextSettings = get().setActiveApiConfig(id);
        await persistSettingsSnapshot(nextSettings);
        return nextSettings;
      },

      getActiveApiConfig: () => {
        const state = get();
        const activeId = state.settings.activeApiConfigId;
        return state.settings.apiConfigs.find((config) => config.id === activeId) || null;
      },

      setDefaultApiConfig: (id) => {
        const currentSettings = get().settings;
        const nextSettings = {
          ...currentSettings,
          apiConfigs: currentSettings.apiConfigs.map((config) => ({
            ...config,
            isDefault: config.id === id,
          })),
        };
        set({ settings: nextSettings });
        return nextSettings;
      },
      setDefaultApiConfigAndPersist: async (id) => {
        const nextSettings = get().setDefaultApiConfig(id);
        await persistSettingsSnapshot(nextSettings);
        return nextSettings;
      },
    }),
    {
      name: 'inkuo-settings',
      partialize: (state) => ({ settings: state.settings }),
      merge: (persistedState, currentState) => {
        const persisted = persistedState as Partial<SettingsState> | undefined;
        const persistedSettings = persisted?.settings as Partial<Settings> | undefined;

        let apiConfigs: Settings['apiConfigs'] = Array.isArray(persistedSettings?.apiConfigs)
          ? persistedSettings.apiConfigs as Settings['apiConfigs']
          : [];

        if (apiConfigs.length === 0) {
          apiConfigs = currentState.settings.apiConfigs;
        }

        let activeApiConfigId = persistedSettings?.activeApiConfigId ?? apiConfigs[0]?.id ?? null;
        if (!apiConfigs.some((config) => config.id === activeApiConfigId)) {
          activeApiConfigId = apiConfigs[0]?.id ?? null;
        }

        if (!apiConfigs.some((config) => config.isDefault)) {
          apiConfigs = apiConfigs.map((config, index) => ({
            ...config,
            isDefault: index === 0,
          }));
        }

        const mergedSettings: Settings = {
          ...currentState.settings,
          ...persistedSettings,
          apiConfigs,
          activeApiConfigId,
          embedding_model: persistedSettings?.embedding_model ?? currentState.settings.embedding_model,
          embedding_model_path: persistedSettings?.embedding_model_path ?? currentState.settings.embedding_model_path,
          chunk_size: typeof persistedSettings?.chunk_size === 'number'
            ? persistedSettings.chunk_size
            : currentState.settings.chunk_size,
          chunk_overlap: typeof persistedSettings?.chunk_overlap === 'number'
            ? persistedSettings.chunk_overlap
            : currentState.settings.chunk_overlap,
          editor_word_wrap: typeof persistedSettings?.editor_word_wrap === 'boolean'
            ? persistedSettings.editor_word_wrap
            : currentState.settings.editor_word_wrap,
          editor_line_numbers: typeof persistedSettings?.editor_line_numbers === 'boolean'
            ? persistedSettings.editor_line_numbers
            : currentState.settings.editor_line_numbers,
        };

        return {
          ...currentState,
          ...persisted,
          settings: mergedSettings,
        };
      },
    }
  )
);
