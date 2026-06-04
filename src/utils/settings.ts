export interface BackendSettings {
  theme: string;
  accent_color: string;
  editor_font_size: number;
  editor_font_family: string;
  ai_provider: string;
  ai_model: string;
  ai_api_key: string | null;
  ai_base_url: string | null;
  ai_temperature: number;
  ai_max_tokens: number | null;
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
    ai_provider: settings.ai_provider,
    ai_model: settings.ai_model,
    ai_api_key: settings.ai_api_key,
    ai_base_url: settings.ai_base_url,
    ai_temperature: settings.ai_temperature,
    ai_max_tokens: settings.ai_max_tokens,
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
