// Settings types — top-level `Settings` shape persisted to disk by
// `settingsStore.ts` (zustand `persist` middleware), plus the
// sub-types referenced only from `Settings` (web search, embedding
// models, theme, expert profile names).

import type { APIConfig, CloudSettings } from './cloud';

/** Per-provider configuration for the `web_search` tool. */
export interface WebSearchProviderConfig {
  /** Provider id. Today only `"baike"` is wired up. */
  id: string;
  /** Optional user-provided key (appid, api key, etc.). `null` means
   * "use the compile-time default" — the backend may then fall back to
   * a public key with rate limits. */
  apiKey: string | null;
  /** Optional override of the upstream endpoint. `null` means use the
   * provider's compile-time default URL. */
  baseUrl: string | null;
  /** Per-provider kill switch. Lets the user keep their key saved but
   * disable a specific provider without deleting it. */
  enabled: boolean;
}

/** Where to send `web_search` calls. The default `"local"` uses the
 * user's own provider credentials; `"cloud"` forwards the call through
 * the operator-managed inkuo Cloud server so the user doesn't have to
 * carry their own API key. Anything else collapses to `"local"` on the
 * Rust side so a typo in the persisted JSON never disables search. */
export type WebSearchRouting = 'local' | 'cloud';

/** Top-level settings for the `web_search` tool. */
export interface WebSearchSettings {
  /** Master kill switch. When `false`, the tool returns a polite
   * "disabled" message instead of hitting the network. */
  enabled: boolean;
  /** Per-provider configuration. Defaults to one entry: Baidu Baike. */
  providers: WebSearchProviderConfig[];
  /** Hard cap on results per call. Clamped to [1, 20] by the tool. */
  maxResults: number;
  /** Routing preference. See `WebSearchRouting`. */
  routing: WebSearchRouting;
}

export interface Settings {
  theme: ThemeType;
  accent_color: string;
  editor_font_size: number;
  editor_font_family: string;
  editor_word_wrap: boolean;
  editor_line_numbers: boolean;
  apiConfigs: APIConfig[];
  activeApiConfigId: string | null;
  embedding_model: EmbeddingModelType;
  embedding_model_path: string | null;
  chunk_size: number;
  chunk_overlap: number;
  snapshot: {
    maxCount: number;
    autoBaseline: boolean;
  };
  /**
   * Hard cap on the Agent's tool-calling loop. Roughly the upper bound on
   * how many "round trips" between the LLM and the tool registry the main
   * Agent session will perform before giving up with a `MaxIterationsReached`
   * error. 1–200, default 50 (matches the Rust default).
   */
  agent_max_iterations: number;
  /**
   * Per-expert (sub-agent) iteration cap overrides, keyed by sub-agent
   * profile name (e.g. `"office_excel_expert"`). The value at each key
   * replaces the compile-time default in the corresponding profile when
   * the main agent dispatches to that sub-agent via `delegate_to`. Missing
   * keys fall back to each profile's compile-time default.
   *
   * The frontend's settings panel exposes a single "sub-agent default"
   * slider that writes the same value into every expert entry; the
   * per-expert entries are then the source of truth sent to the backend.
   *
   * Values are integers in `[1, 200]`. The backend re-clamps as a defence
   * in depth.
   */
  expert_max_iterations: Record<string, number>;
  /**
   * Configuration for the `web_search` tool. The tool itself is always
   * registered (so the LLM can see it in every mode); the settings here
   * determine whether calling it actually hits the network.
   *
   * Provider list is forward-compatible — today only `"baike"` is
   * implemented on the Rust side, but additional providers can be added
   * without touching the wire format.
   */
  web_search: WebSearchSettings;
  /** Cloud-mode settings. Optional in legacy settings files (older than
   * cloud mode existed) — sanitised merge falls back to defaults. */
  cloud: CloudSettings;
}

/** Keys of the expert profile registry, mirroring `PROFILES` in
 * `src-tauri/src/agent/prompts.rs`. Kept in sync manually; the backend
 * drops unknown keys so a stale value here is safe. */
export type ExpertProfileName =
  | 'office_word_expert'
  | 'office_excel_expert'
  | 'md_writer'
  | 'researcher'
  | 'batch_editor'
  | 'code_expert'
  | 'flowchart_expert'
  | 'word_image_expert';

/** Supported embedding models */
export type EmbeddingModelType =
  | 'BAAI/bge-small-zh-v1.5'
  | 'BAAI/bge-base-zh-v1.5'
  | 'BAAI/bge-large-zh-v1.5';

/** Embedding model info for display */
export interface EmbeddingModelInfo {
  id: EmbeddingModelType;
  name: string;
  dimensions: number;
  size: string;
  description: string;
}

export type ThemeType =
  | 'paper-white'
  | 'paper-cream'
  | 'graphite'
  | 'verdant'
  | 'iris'
  /** 保留兼容,解析时映射到 paper-white */
  | 'inkuo-light'
  | 'high-contrast-dark'
  | 'high-contrast-light'
  /** 旧值,解析时映射到 graphite。 */
  | 'inkuo-dark';