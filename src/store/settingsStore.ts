import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import type { APIConfig, Settings } from '../types';

interface SettingsState {
  settings: Settings;
  isSettingsOpen: boolean;

  setSettings: (settings: Settings) => void;
  updateSetting: <K extends keyof Settings>(key: K, value: Settings[K]) => void;
  setIsSettingsOpen: (open: boolean) => void;

  addApiConfig: (config?: Partial<APIConfig>) => string;
  updateApiConfig: (id: string, updates: Partial<APIConfig>) => void;
  removeApiConfig: (id: string) => void;
  setActiveApiConfig: (id: string) => void;
  getActiveApiConfig: () => APIConfig | null;
  setDefaultApiConfig: (id: string) => void;
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
      updateSetting: (key, value) => set((state) => ({
        settings: { ...state.settings, [key]: value },
      })),
      setIsSettingsOpen: (open) => set({ isSettingsOpen: open }),

      addApiConfig: (config) => {
        const newConfig: APIConfig = {
          id: crypto.randomUUID(),
          name: config?.name || 'New API',
          provider: config?.provider || 'openai',
          baseUrl: config?.baseUrl || 'https://api.openai.com/v1',
          apiKey: config?.apiKey || null,
          model: config?.model || 'gpt-4o-mini',
          isDefault: false,
          enabled: true,
          temperature: config?.temperature ?? 0.7,
          maxTokens: config?.maxTokens ?? 4096,
        };

        set((state) => ({
          settings: {
            ...state.settings,
            apiConfigs: [...state.settings.apiConfigs, newConfig],
          },
        }));

        return newConfig.id;
      },

      updateApiConfig: (id, updates) => set((state) => ({
        settings: {
          ...state.settings,
          apiConfigs: state.settings.apiConfigs.map((config) =>
            config.id === id ? { ...config, ...updates } : config
          ),
        },
      })),

      removeApiConfig: (id) => set((state) => {
        const remaining = state.settings.apiConfigs.filter((c) => c.id !== id);
        const newActiveId = state.settings.activeApiConfigId === id
          ? (remaining.length > 0 ? remaining[0].id : null)
          : state.settings.activeApiConfigId;

        const updatedConfigs = remaining.map((c, i) =>
          i === 0 && !remaining.some(r => r.isDefault) ? { ...c, isDefault: true } : c
        );

        return {
          settings: {
            ...state.settings,
            apiConfigs: remaining.length > 0 ? updatedConfigs : state.settings.apiConfigs,
            activeApiConfigId: newActiveId,
          },
        };
      }),

      setActiveApiConfig: (id) => set((state) => ({
        settings: {
          ...state.settings,
          activeApiConfigId: id,
        },
      })),

      getActiveApiConfig: () => {
        const state = get();
        const activeId = state.settings.activeApiConfigId;
        return state.settings.apiConfigs.find((c) => c.id === activeId) || null;
      },

      setDefaultApiConfig: (id) => set((state) => ({
        settings: {
          ...state.settings,
          apiConfigs: state.settings.apiConfigs.map((config) => ({
            ...config,
            isDefault: config.id === id,
          })),
        },
      })),
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
