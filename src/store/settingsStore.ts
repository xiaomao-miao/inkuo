import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import type { APIConfig, Settings, WebSearchProviderConfig } from '../types';
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

  /** Replace the entire `web_search` config. Mostly useful from the
   * settings panel's "Reset" affordance. */
  updateWebSearch: (next: Settings['web_search']) => Settings;
  updateWebSearchAndPersist: (next: Settings['web_search']) => Promise<Settings>;
  /** Patch a single provider (matched by `id`). When the provider id
   * doesn't exist yet, it's added; when `null`, no-op. */
  updateWebSearchProvider: (
    providerId: string,
    updates: Partial<Settings['web_search']['providers'][number]>
  ) => Settings;
  updateWebSearchProviderAndPersist: (
    providerId: string,
    updates: Partial<Settings['web_search']['providers'][number]>
  ) => Promise<Settings>;
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
    maxTokens: 16384,
  };
}

function createDefaultWebSearchConfig(): Settings['web_search'] {
  return {
    enabled: true,
    providers: [
      {
        id: 'baike',
        apiKey: null,
        baseUrl: null,
        enabled: true,
      },
    ],
    maxResults: 5,
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
  theme: 'inkuo-dark',
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
  snapshot: {
    maxCount: 50,
    autoBaseline: true,
  },
  agent_max_iterations: 50,
  // Per-sub-agent iteration cap overrides. Every expert gets the same
  // default of 50 (was 15/20/25 in the compile-time defaults; lifted so
  // sub-agents can comfortably run full read-modify-read loops). Missing
  // keys would fall back to the profile's compile-time default on the
  // backend, but we pre-populate them so the UI sliders can render a value
  // for every expert.
  expert_max_iterations: {
    office_word_expert: 50,
    office_excel_expert: 50,
    md_writer: 50,
    researcher: 50,
    batch_editor: 50,
    code_expert: 50,
    flowchart_expert: 50,
    word_image_expert: 50,
  },
  web_search: createDefaultWebSearchConfig(),
};

/** Range that mirrors the backend's `clamp(1, 200)`. */
const MIN_EXPERT_ITERATIONS = 1;
const MAX_EXPERT_ITERATIONS = 200;

/** Allowed keys for `expert_max_iterations`. Any other key would be
 * dropped by the backend's sanitiser, so we pre-filter here to keep
 * the persisted settings tidy. */
const ALLOWED_EXPERT_KEYS = [
  'office_word_expert',
  'office_excel_expert',
  'md_writer',
  'researcher',
  'batch_editor',
  'code_expert',
  'flowchart_expert',
  'word_image_expert',
] as const;

/** Sanitise a raw `expert_max_iterations` map: keep only known keys and
 * clamp each value into `[1, 200]`. The same clamp is also applied on
 * the backend as a defence in depth. */
function sanitiseExpertMaxIterations(
  raw: Partial<Record<string, number>> | undefined
): Record<string, number> {
  const fallback = defaultSettings.expert_max_iterations;
  if (!raw || typeof raw !== 'object') {
    return { ...fallback };
  }
  const result: Record<string, number> = {};
  for (const key of ALLOWED_EXPERT_KEYS) {
    const value = raw[key];
    if (typeof value === 'number' && Number.isFinite(value)) {
      result[key] = Math.min(
        MAX_EXPERT_ITERATIONS,
        Math.max(MIN_EXPERT_ITERATIONS, Math.trunc(value))
      );
    } else {
      result[key] = fallback[key];
    }
  }
  return result;
}

/** Range that mirrors the backend's `clamp(1, 20)` for the web_search
 * `max_results` knob. Mirrored here so a corrupted persisted blob with
 * a string or out-of-range value falls back to the default instead of
 * reaching the tool with garbage. */
const MIN_WEB_SEARCH_MAX_RESULTS = 1;
const MAX_WEB_SEARCH_MAX_RESULTS = 20;

/** Sanitise the persisted `web_search` config. Legacy settings files
 * (saved before the tool existed) don't have the field at all; we
 * fall back to the in-code default in that case. A partial object
 * (e.g. one provider with no apiKey) is preserved as-is so the user's
 * intentional edits survive an upgrade. */
function sanitiseWebSearchConfig(
  raw: Partial<Settings['web_search']> | undefined
): Settings['web_search'] {
  const fallback = defaultSettings.web_search;
  if (!raw || typeof raw !== 'object') {
    return { ...fallback, providers: fallback.providers.map((p) => ({ ...p })) };
  }

  const enabled = typeof raw.enabled === 'boolean' ? raw.enabled : fallback.enabled;

  const maxResultsRaw = raw.maxResults;
  const maxResults =
    typeof maxResultsRaw === 'number' && Number.isFinite(maxResultsRaw)
      ? Math.min(
          MAX_WEB_SEARCH_MAX_RESULTS,
          Math.max(MIN_WEB_SEARCH_MAX_RESULTS, Math.trunc(maxResultsRaw))
        )
      : fallback.maxResults;

  const providers: Settings['web_search']['providers'] = Array.isArray(raw.providers)
    ? raw.providers
        .filter(
          (provider): provider is Settings['web_search']['providers'][number] =>
            !!provider && typeof provider === 'object'
        )
        .map((provider) => ({
          // `id` must be a non-empty string; otherwise drop the entry
          // (otherwise the Rust side wouldn't know which provider to
          // dispatch to and would surface a confusing error).
          id: typeof provider.id === 'string' && provider.id.trim()
            ? provider.id.trim()
            : 'baike',
          apiKey: typeof provider.apiKey === 'string' ? provider.apiKey : null,
          baseUrl:
            typeof provider.baseUrl === 'string' && provider.baseUrl.trim()
              ? provider.baseUrl
              : null,
          enabled: typeof provider.enabled === 'boolean' ? provider.enabled : true,
        }))
    : fallback.providers.map((p) => ({ ...p }));

  // If sanitisation left us with no providers (e.g. the persisted list
  // was an empty array, or every entry had a non-object shape), fall
  // back to the default Baike entry. An empty array would silently
  // disable web search and confuse the user.
  if (providers.length === 0) {
    return {
      enabled,
      maxResults,
      providers: [{ ...fallback.providers[0] }],
    };
  }

  return { enabled, maxResults, providers };
}

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

      updateWebSearch: (next) => {
        // Defensive clone so a caller can't mutate the stored array
        // by reference after we set it.
        const cloned: Settings['web_search'] = {
          enabled: next.enabled,
          maxResults: next.maxResults,
          providers: next.providers.map((p) => ({ ...p })),
        };
        const nextSettings: Settings = {
          ...get().settings,
          web_search: cloned,
        };
        set({ settings: nextSettings });
        return nextSettings;
      },
      updateWebSearchAndPersist: async (next) => {
        const nextSettings = get().updateWebSearch(next);
        await persistSettingsSnapshot(nextSettings);
        return nextSettings;
      },

      updateWebSearchProvider: (providerId, updates) => {
        const current = get().settings.web_search;
        const existingIndex = current.providers.findIndex((p) => p.id === providerId);
        const newProviders = current.providers.map((p) => ({ ...p }));
        if (existingIndex >= 0) {
          newProviders[existingIndex] = {
            ...newProviders[existingIndex],
            ...updates,
            // Preserve the id even if the caller accidentally clears it
            // — losing it would orphan the provider entry.
            id: newProviders[existingIndex].id,
          };
        } else {
          // Build the new provider entry from defaults, then layer the
          // caller's updates on top, then re-assert the id. Using
          // spread-and-restructure (instead of a literal with two `id`
          // keys) sidesteps TS1117, which forbids duplicate property
          // names in object literals even when the later value would
          // win at runtime.
          const base: WebSearchProviderConfig = {
            id: providerId,
            apiKey: null,
            baseUrl: null,
            enabled: true,
          };
          newProviders.push({ ...base, ...updates, id: providerId });
        }
        return get().updateWebSearch({
          ...current,
          providers: newProviders,
        });
      },
      updateWebSearchProviderAndPersist: async (providerId, updates) => {
        const nextSettings = get().updateWebSearchProvider(providerId, updates);
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
          // Naively copy any unknown fields onto the merged settings — covers
          // forward-compatible additions without forcing a store migration.
          // Number fields below still get explicit type guards so a corrupted
          // persisted blob (e.g. a string in place of a number) falls back to
          // the current default instead of crashing the panel.
          ...persistedSettings,
          agent_max_iterations: typeof persistedSettings?.agent_max_iterations === 'number'
            && Number.isFinite(persistedSettings.agent_max_iterations)
            && persistedSettings.agent_max_iterations >= 1
            && persistedSettings.agent_max_iterations <= 200
              ? persistedSettings.agent_max_iterations
              : currentState.settings.agent_max_iterations,
          // Per-expert iteration overrides. Each value is sanitised into
          // [1, 200] and only known expert keys are kept. Missing values
          // for known keys fall back to the in-code default (50).
          expert_max_iterations: sanitiseExpertMaxIterations(
            persistedSettings?.expert_max_iterations as
              | Partial<Record<string, number>>
              | undefined
          ),
          // Web search config: missing on legacy settings files (older
          // than the tool itself), so we fill with the in-code default
          // and only overlay fields the user explicitly set. The merge
          // is intentionally per-field so a partial persisted object
          // (e.g. one provider with no apiKey) doesn't get clobbered.
          web_search: sanitiseWebSearchConfig(
            persistedSettings?.web_search as
              | Partial<Settings['web_search']>
              | undefined
          ),
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
