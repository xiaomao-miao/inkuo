// Cloud-mode (inkuo Cloud) types — used by `CloudClient` (Rust) and the
// `cloud/*` React components. The frontend mirrors the Rust types
// 1:1; a forward-compatible `provider_kind` field on `CloudModelEntry`
// carries the AI provider kind (`AIProviderType`) for routing.
//
// `AIProviderType` is shared with the local-only provider list
// (`Settings.apiConfigs[].provider`); it lives here because cloud
// mode is the only place a model can declare an `AIProviderType` of
// `'cloud'` — the local provider picker never picks `'cloud'`.

export type AIProviderType = 'openai' | 'ollama' | 'deepseek' | 'official' | 'cloud';

/** API configuration for a single model provider */
export interface APIConfig {
  id: string;                    // Unique identifier
  name: string;                 // Display name (e.g., "DeepSeek V3", "GPT-4")
  provider: AIProviderType;     // Provider type
  baseUrl: string;              // API base URL
  apiKey: string | null;        // BYOK credential; currently persisted with local app settings
  model: string;                // Model name
  isDefault: boolean;            // Whether this is the default API
  enabled: boolean;              // Whether this API is enabled
  temperature: number;          // Default temperature for this API
  maxTokens: number | null;      // Default max tokens for this API
}

/** Logged-in inkuo Cloud account. Persisted into `Settings.cloud.account`
 * so the Rust-side `build_settings_ai_config` can route chat traffic to
 * the cloud server. */
export interface CloudAccount {
  base_url: string;
  email: string;
  user_id: string;
  access_token: string;
  refresh_token: string;
  /** ISO-8601 UTC timestamp. */
  access_expires_at: string;
  plan_name: string | null;
  /** Canonical billing value. 1000 integer points = ¥1. */
  balance_points: number;
  /** Frozen subset of balance_points that is waiting for settlement. */
  reserved_points?: number;
  /** Unpaid usage. A positive value suspends further billable requests. */
  debt_points?: number;
  is_suspended?: boolean;
  /** @deprecated Read-only compatibility mirror for settings from older releases. */
  balance_cents?: number;
}

/** Single upstream model exposed by the cloud server. The `id` is the
 * server-side model_config id (Guid) and is what we send in the `model`
 * field of `/v1/chat/completions`. */
export interface CloudModelEntry {
  id: string;
  display_name: string;
  model_name: string;
  provider: string;
  /** Unit: yuan per 1 million input tokens (uncached) */
  input_price_per_m_tokens: number;
  /** Unit: yuan per 1 million output tokens */
  output_price_per_m_tokens: number;
  /** Unit: yuan per 1 million cached input tokens. The Rust side does
   * not bill, but this is surfaced in the UI for cost estimates. */
  cached_input_price_per_m_tokens: number;
  description: string | null;
  provider_kind: AIProviderType;
}

/** Cloud-mode configuration. `cloud_mode_enabled` is the user-facing
 * toggle; when `true`, the Rust side routes all chat traffic through
 * `account.base_url` instead of using `apiConfigs[]`. The cached
 * model list and active selection persist across restarts. */
export interface CloudSettings {
  cloud_mode_enabled: boolean;
  account: CloudAccount | null;
  cached_models: CloudModelEntry[];
  active_cloud_model_id: string | null;
}
