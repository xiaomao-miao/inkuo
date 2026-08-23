import { invoke } from '@tauri-apps/api/core';

import type {
  AIProviderType,
  ChatMode,
  FeatureToggleId,
  ImageAttachmentInput,
  MessageRole,
  MessageToolCall,
} from '../types';

/** Provider-neutral history shape accepted by the Rust agent adapter. */
export interface AgentHistoryMessageInput {
  id: string;
  role: MessageRole;
  content: string;
  tool_calls?: MessageToolCall[];
  tool_call_id?: string;
  imageAttachments?: ImageAttachmentInput[];
}

export interface AgentAIConfigInput {
  provider: AIProviderType;
  api_key: string | null;
  base_url: string | null;
  model: string;
  temperature?: number;
  max_tokens?: number | null;
  /** Explicit override for custom model endpoints. Unknown models may still
   * attempt the provider's standard vision wire format. */
  supports_vision?: boolean;
}

/** Stable public invocation contract for text and screenshot-backed turns.
 * Renderers can pass stitched Word/PPT preview screenshots through
 * `imageAttachments` without learning a provider-specific payload shape. */
export interface AgentStreamInput {
  sessionId: string;
  messageId: string;
  instruction: string;
  workspacePath?: string;
  mode?: ChatMode;
  history?: AgentHistoryMessageInput[];
  maxIterations?: number;
  expertMaxIterations?: Record<string, number>;
  enabledToggles?: FeatureToggleId[];
  imageAttachments?: ImageAttachmentInput[];
  configInput: AgentAIConfigInput;
}

export function streamAgent(input: AgentStreamInput): Promise<void> {
  return invoke<void>('ai_agent_stream', {
    ...input,
    mode: input.mode ?? 'agent',
    history: input.history ?? [],
    enabledToggles: input.enabledToggles ?? [],
    imageAttachments: input.imageAttachments ?? [],
  });
}
