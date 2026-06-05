export interface BackendSettings {
  theme: string;
  accent_color: string;
  editor_font_size: number;
  editor_font_family: string;
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
}

import type { Settings } from '../types';

export function toBackendSettings(settings: Settings): BackendSettings {
  return {
    theme: settings.theme,
    accent_color: settings.accent_color,
    editor_font_size: settings.editor_font_size,
    editor_font_family: settings.editor_font_family,
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
  };
}
