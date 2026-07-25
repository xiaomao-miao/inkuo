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

/** Stable identifier for an image-generation provider entry. The same id is
 * reused across renames/edits; renaming a provider MUST NOT change its id.
 * Built-in defaults seed their ids from the provider type for readability
 * (e.g. `"ollama-default"`), but user-added providers get a fresh uuid. */
export type ImageGenProviderId = string;

/** Routing/transport family for an image-generation provider. The image-gen
 * tool branches on this value to decide which HTTP path to use:
 *
 * - `"ollama"`           — talks to a local Ollama `/api/generate` endpoint.
 * - `"openai"`           — any OpenAI-compatible `/v1/images/generations`
 *                            endpoint (DALL·E, DeepSeek-Image, custom
 *                            gateways, etc.). Auth: Bearer token (`apiKey`).
 * - `"tencent_token"`     — Tencent Token Hub (tokenhub.tencentmaas.com).
 *                            OpenAI-compatible wire format with a single Bearer
 *                            API key — simpler than the TC3-signed aiart API.
 * - `"tencent_tc3"`      — Tencent Cloud aiart (aiart.tencentcloudapi.com).
 *                            TC3-HMAC-SHA256 signing with SecretId/SecretKey.
 *                            Supports hunyuan-pro / hunyuan-lite models.
 * - `"custom"`           — same wire format as `"openai"` but treated as a
 *                            generic upstream; useful for self-hosted SD-WebUI
 *                            or other compatible servers without claiming it
 *                            is "the OpenAI API".
 *
 * The string is persisted alongside `id` so a rename in the UI doesn't
 * accidentally demote a paid endpoint to local-only routing. */
export type ImageGenProviderType =
  | 'ollama'
  | 'openai'
  | 'tencent_token'
  | 'tencent_tc3'
  | 'custom';

/** Per-provider configuration for the `generate_image` tool.
 *
 * Mirrors `WebSearchProviderConfig` so the two panels share the same
 * UX (one block per provider, key + base URL + enabled toggle).
 *
 * `defaultModel` is what `generate_image` uses when the LLM doesn't
 * pin a specific model in its tool call. */
export interface ImageGenProviderConfig {
  /** Stable id; never changes once the entry is created. Used as the
   * React `key` and as the routing key for the LLM-provided
   * `model` override. */
  id: ImageGenProviderId;
  /** Transport family. Determines which request format `generate_image`
   * uses. See `ImageGenProviderType`. */
  providerType: ImageGenProviderType;
  /** Bearer-style API key. Used by `openai`, `tencent_token`, and `custom`
   * providers (they all follow the same Authorization: Bearer pattern).
   * Optional for `ollama` and `tencent_tc3` (the latter uses
   * `secretId` / `secretKey` instead). */
  apiKey: string | null;
  /** Optional override of the upstream endpoint. `null` means use
   * the provider's compile-time default URL. */
  baseUrl: string | null;
  /** Tencent TC3 Cloud `SecretId` (public identifier, paired with
   * `secretKey`). Only meaningful when `providerType === 'tencent_tc3'`. */
  secretId: string | null;
  /** Tencent TC3 Cloud `SecretKey` (HMAC signing secret). Stored only
   * when `providerType === 'tencent_tc3'`; `null` for everything else. */
  secretKey: string | null;
  /** Region hint for cloud providers (e.g. `"ap-guangzhou"` for
   * Tencent TC3). `null` falls back to the provider's
   * compile-time default region. */
  region: string | null;
  /** Default model id (e.g. `"sdxl"` / `"dall-e-3"`). When the LLM
   * calls `generate_image` without a `model` override, this is the
   * model that gets used. */
  defaultModel: string;
  /** Per-provider kill switch. */
  enabled: boolean;
}

/** Where to send `generate_image` calls.
 *
 * - `"local"` prefers the first enabled Ollama provider, falling back to
 *   any other enabled provider.
 * - `"cloud"` prefers the first non-Ollama enabled provider, falling
 *   back to Ollama.
 * - Anything else is treated as a literal provider id; unknown ids
 *   collapse to the first enabled provider so a typo never disables
 *   the tool.
 *
 * The Rust side mirrors this exact semantic. */
export type ImageGenRouting = 'local' | 'cloud' | string;

/** Top-level settings for the `generate_image` tool. */
export interface ImageGenSettings {
  /** Master kill switch. When `false`, the tool returns a polite
   * "disabled" message instead of calling any provider. */
  enabled: boolean;
  /** Per-provider configuration. Defaults to one Ollama entry pointing
   * at `localhost:11434`. */
  providers: ImageGenProviderConfig[];
  /** Routing preference. See `ImageGenRouting`. */
  routing: ImageGenRouting;
  /** Default image width when the LLM omits it (pixels). 256–2048,
   * default 1024. */
  defaultWidth: number;
  /** Default image height when the LLM omits it (pixels). 256–2048,
   * default 1024. */
  defaultHeight: number;
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
  /** Configuration for the `generate_image` tool. Same shape as the
   * Rust `ImageGenSettings` struct; missing on legacy settings files
   * (older than image generation existed) — sanitised merge falls
   * back to defaults. */
  image_gen: ImageGenSettings;
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