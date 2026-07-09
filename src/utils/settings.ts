import type { Settings } from '../types';

export interface BackendSettings {
  theme: string;
  accent_color: string;
  editor_font_size: number;
  editor_font_family: string;
  editor_word_wrap: boolean;
  editor_line_numbers: boolean;
  api_configs: Array<{
    id: string;
    name: string;
    provider: string;
    base_url: string;
    api_key: string | null;
    model: string;
    is_default: boolean;
    enabled: boolean;
    temperature: number;
    max_tokens: number | null;
  }>;
  active_api_config_id: string | null;
  embedding_model: string;
  embedding_model_path: string | null;
  chunk_size: number;
  chunk_overlap: number;
  /** Wire shape for the `web_search` tool config. Sent on every
   * `save_settings` IPC so the Rust side can read it from the settings
   * cache without a second roundtrip. */
  web_search: {
    enabled: boolean;
    max_results: number;
    providers: Array<{
      id: string;
      api_key: string | null;
      base_url: string | null;
      enabled: boolean;
    }>;
  };
}

export function toBackendSettings(settings: Settings): BackendSettings {
  return {
    theme: settings.theme,
    accent_color: settings.accent_color,
    editor_font_size: settings.editor_font_size,
    editor_font_family: settings.editor_font_family,
    editor_word_wrap: settings.editor_word_wrap,
    editor_line_numbers: settings.editor_line_numbers,
    api_configs: settings.apiConfigs.map((c) => ({
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
    embedding_model: settings.embedding_model,
    embedding_model_path: settings.embedding_model_path,
    chunk_size: settings.chunk_size,
    chunk_overlap: settings.chunk_overlap,
    web_search: {
      enabled: settings.web_search.enabled,
      max_results: settings.web_search.maxResults,
      providers: settings.web_search.providers.map((p) => ({
        id: p.id,
        api_key: p.apiKey,
        base_url: p.baseUrl,
        enabled: p.enabled,
      })),
    },
  };
}
