import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import type {
  APIConfig,
  CloudAccount,
  CloudModelEntry,
  CloudSettings,
  Settings,
  WebSearchProviderConfig,
} from '../types';
import { saveSettings } from '../utils/saveSettings';

interface SettingsState {
  settings: Settings;
  isSettingsOpen: boolean;

  /**
   * Replace the entire `settings` blob. Callers that want durable
   * persistence must follow up with `persistSettings()` themselves;
   * mutation actions below persist automatically (fire-and-forget).
   */
  setSettings: (settings: Settings) => void;

  /**
   * Persist a settings snapshot to disk. Omitting the argument
   * persists the current state.
   */
  persistSettings: (settings?: Settings) => Promise<void>;

  /** Generic single-key updater with auto-persist. */
  updateSetting: <K extends keyof Settings>(key: K, value: Settings[K]) => Promise<Settings>;

  setIsSettingsOpen: (open: boolean) => void;

  /** API config CRUD — every entry auto-persists. */
  addApiConfig: (config?: Partial<APIConfig>) => Promise<Settings>;
  updateApiConfig: (id: string, updates: Partial<APIConfig>) => Promise<Settings>;
  removeApiConfig: (id: string) => Promise<Settings>;
  setActiveApiConfig: (id: string) => Promise<Settings>;
  /** Read-only helper (no side effect). */
  getActiveApiConfig: () => APIConfig | null;
  setDefaultApiConfig: (id: string) => Promise<Settings>;

  /**
   * Replace the entire `web_search` config. Mostly useful from the
   * settings panel's "Reset" affordance.
   */
  updateWebSearch: (next: Settings['web_search']) => Promise<Settings>;
  /**
   * Patch a single provider (matched by `id`). When the provider id
   * doesn't exist yet, it's added; when `null`, no-op.
   */
  updateWebSearchProvider: (
    providerId: string,
    updates: Partial<Settings['web_search']['providers'][number]>
  ) => Promise<Settings>;

  setCloudModeEnabled: (enabled: boolean) => Promise<Settings>;
  setCloudAccount: (account: CloudAccount | null) => Promise<Settings>;
  setCloudModels: (models: CloudModelEntry[]) => Promise<Settings>;
  setActiveCloudModelId: (id: string | null) => Promise<Settings>;
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
    // Default to local routing: existing users keep their current
    // behaviour (use their own Baidu key) without any opt-in. Users
    // who are cloud-authenticated can flip this to "cloud" from the
    // Web Search settings panel.
    routing: 'local',
  };
}

function createDefaultCloudConfig(): CloudSettings {
  return {
    cloud_mode_enabled: false,
    account: null,
    cached_models: [],
    active_cloud_model_id: null,
  };
}

async function persistSettingsSnapshot(settings: Settings): Promise<void> {
  await saveSettings(settings);
}

/**
 * Wrap a synchronous `Settings → Settings` updater with auto-persist.
 *
 * Each settings slice used to expose two methods — a synchronous `X`
 * and an async `XAndPersist` that called `X` then awaited
 * `saveSettings`. That doubled the store's surface area (~280 lines
 * of duplicated bodies), and every caller had to pick one form
 * (some `await`ed, some used `void`, some just called the sync version
 * and forgot to persist). This helper collapses each pair into one
 * action: it computes the next settings, sets them on the store, and
 * fire-and-forgets a persist.
 *
 * Errors during the persist are logged but never bubble back to the
 * caller — a failed disk write must not block the UI, and the
 * in-memory state is still consistent. The next persisted call (or a
 * window-close save) will retry.
 *
 * The updater's first argument is the *current* settings object; the
 * wrapper strips it before forwarding the remaining args to the
 * caller-facing action signature.
 */
function defineAutoPersist<U extends (current: Settings, ...args: any[]) => Settings>(
  updater: U,
): (...args: Parameters<U> extends [Settings, ...infer Rest] ? Rest : never) => Promise<Settings> {
  return async (...args) => {
    const current = useSettingsStore.getState().settings;
    const next = updater(current, ...(args as Parameters<U> extends [Settings, ...infer Rest] ? Rest : never));
    useSettingsStore.setState({ settings: next });
    void persistSettingsSnapshot(next).catch((err) => {
      console.warn('[settingsStore] auto-persist failed:', err);
    });
    return next;
  };
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
  theme: 'paper-white',
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
  cloud: createDefaultCloudConfig(),
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

  // Accept anything string-shaped and let the Rust side collapse unknown
  // values back to "local". This keeps the on-disk schema forward-compat:
  // adding a new routing mode later doesn't require a sanitiser upgrade.
  const routingValue: Settings['web_search']['routing'] =
    typeof raw.routing === 'string' &&
    (raw.routing === 'local' || raw.routing === 'cloud')
      ? raw.routing
      : fallback.routing;

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
      routing: routingValue,
    };
  }

  return { enabled, maxResults, providers, routing: routingValue };
}

/** Defensive sanitiser for the persisted cloud settings. Mirrors the
 * pattern used by `sanitiseWebSearchConfig`: any field with the wrong
 * shape falls back to the in-code default. Legacy settings files (saved
 * before cloud mode existed) have no `cloud` field, and we want them to
 * keep loading cleanly into local mode. */
function sanitiseCloudConfig(
  raw: Partial<CloudSettings> | undefined
): CloudSettings {
  const fallback = createDefaultCloudConfig();
  if (!raw || typeof raw !== 'object') return fallback;

  const cloud_mode_enabled =
    typeof raw.cloud_mode_enabled === 'boolean'
      ? raw.cloud_mode_enabled
      : fallback.cloud_mode_enabled;

  // The account is opaque to the frontend — we mostly forward it as-is
  // so the Rust side can refresh tokens. We only sanity-check the basic
  // shape; tokens are just opaque strings here.
  const account: CloudAccount | null =
    raw.account && typeof raw.account === 'object' && typeof raw.account.access_token === 'string'
      ? (raw.account as CloudAccount)
      : null;

  // The on-disk schema for CloudModelEntry has evolved: the price unit
  // was renamed from `*_per_1k_tokens` to `*_per_m_tokens` (and a
  // `cached_input_price_per_m_tokens` field was added). Old settings
  // files may still carry the `1k` fields, so migrate in-place and
  // multiply by 1000 so previously-shown cost estimates remain
  // numerically correct under the new unit.
  const migrateLegacyModel = (raw: unknown): CloudModelEntry | null => {
    if (!raw || typeof raw !== 'object') return null;
    const r = raw as Record<string, unknown>;
    const inputPerM =
      typeof r.input_price_per_m_tokens === 'number'
        ? r.input_price_per_m_tokens
        : typeof r.input_price_per_1k_tokens === 'number'
        ? r.input_price_per_1k_tokens * 1000
        : 0;
    const outputPerM =
      typeof r.output_price_per_m_tokens === 'number'
        ? r.output_price_per_m_tokens
        : typeof r.output_price_per_1k_tokens === 'number'
        ? r.output_price_per_1k_tokens * 1000
        : 0;
    const cachedPerM =
      typeof r.cached_input_price_per_m_tokens === 'number' ? r.cached_input_price_per_m_tokens : 0;
    if (typeof r.id !== 'string' || typeof r.display_name !== 'string') return null;
    return {
      id: r.id,
      display_name: r.display_name,
      model_name: typeof r.model_name === 'string' ? r.model_name : '',
      provider: typeof r.provider === 'string' ? r.provider : '',
      input_price_per_m_tokens: inputPerM,
      output_price_per_m_tokens: outputPerM,
      cached_input_price_per_m_tokens: cachedPerM,
      description: typeof r.description === 'string' ? r.description : null,
      provider_kind:
        typeof r.provider_kind === 'string' ? (r.provider_kind as CloudModelEntry['provider_kind']) : 'openai',
    };
  };

  const cached_models: CloudModelEntry[] = Array.isArray(raw.cached_models)
    ? (raw.cached_models
        .map(migrateLegacyModel)
        .filter((m): m is CloudModelEntry => m !== null))
    : fallback.cached_models;

  const active_cloud_model_id =
    typeof raw.active_cloud_model_id === 'string' &&
    cached_models.some((m) => m.id === raw.active_cloud_model_id)
      ? raw.active_cloud_model_id
      : null;

  return {
    cloud_mode_enabled,
    account,
    cached_models,
    active_cloud_model_id,
  };
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
      updateSetting: defineAutoPersist((current, key, value) => ({
        ...current,
        [key]: value,
      })),
      setIsSettingsOpen: (open) => set({ isSettingsOpen: open }),

      addApiConfig: defineAutoPersist((current, config?: Partial<APIConfig>) => {
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
        return {
          ...current,
          apiConfigs: ensureValidApiConfigs([...current.apiConfigs, newConfig]),
        };
      }),

      updateApiConfig: defineAutoPersist((current, id: string, updates: Partial<APIConfig>) => ({
        ...current,
        apiConfigs: current.apiConfigs.map((configItem) =>
          configItem.id === id ? { ...configItem, ...updates } : configItem
        ),
      })),

      removeApiConfig: defineAutoPersist((current, id: string) => {
        const remaining = current.apiConfigs.filter((config) => config.id !== id);
        const apiConfigs = ensureValidApiConfigs(remaining);
        const activeApiConfigId = apiConfigs.some((config) => config.id === current.activeApiConfigId)
          ? current.activeApiConfigId
          : apiConfigs[0].id;
        return { ...current, apiConfigs, activeApiConfigId };
      }),

      setActiveApiConfig: defineAutoPersist((current, id: string) => ({
        ...current,
        activeApiConfigId: current.apiConfigs.some((config) => config.id === id)
          ? id
          : current.activeApiConfigId,
      })),

      getActiveApiConfig: () => {
        const state = get();
        const activeId = state.settings.activeApiConfigId;
        return state.settings.apiConfigs.find((config) => config.id === activeId) || null;
      },

      setDefaultApiConfig: defineAutoPersist((current, id: string) => ({
        ...current,
        apiConfigs: current.apiConfigs.map((config) => ({
          ...config,
          isDefault: config.id === id,
        })),
      })),

      updateWebSearch: defineAutoPersist((current, next: Settings['web_search']) => {
        // Defensive clone so a caller can't mutate the stored array
        // by reference after we set it.
        const cloned: Settings['web_search'] = {
          enabled: next.enabled,
          maxResults: next.maxResults,
          providers: next.providers.map((p) => ({ ...p })),
          // Preserve the routing value from the incoming payload (the
          // form-level "Save" button never touches it; routing is a
          // separate control). If the caller omitted it, keep whatever
          // was previously stored so we don't accidentally reset the
          // user's preference.
          routing: next.routing ?? current.web_search.routing,
        };
        return { ...current, web_search: cloned };
      }),

      updateWebSearchProvider: defineAutoPersist(
        (current, providerId: string, updates: Partial<Settings['web_search']['providers'][number]>) => {
          const currentWebSearch = current.web_search;
          const existingIndex = currentWebSearch.providers.findIndex((p) => p.id === providerId);
          const newProviders = currentWebSearch.providers.map((p) => ({ ...p }));
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
          return {
            ...current,
            web_search: {
              ...currentWebSearch,
              providers: newProviders,
            },
          };
        }
      ),

      setCloudModeEnabled: defineAutoPersist((current, enabled: boolean) => ({
        ...current,
        cloud: { ...current.cloud, cloud_mode_enabled: enabled },
      })),

      setCloudAccount: defineAutoPersist((current, account: CloudAccount | null) => ({
        ...current,
        cloud: { ...current.cloud, account },
      })),

      setCloudModels: defineAutoPersist((current, models: CloudModelEntry[]) => ({
        ...current,
        cloud: { ...current.cloud, cached_models: models },
      })),

      setActiveCloudModelId: defineAutoPersist((current, id: string | null) => ({
        ...current,
        cloud: { ...current.cloud, active_cloud_model_id: id },
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
          cloud: sanitiseCloudConfig(
            persistedSettings?.cloud as
              | Partial<CloudSettings>
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
