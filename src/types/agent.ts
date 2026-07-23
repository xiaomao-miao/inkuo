// Agent tool-calling types — `ToolDefinition` (OpenAI function-calling
// shape), `ToolCall`, `ToolCallResult`, `MessageRole`, `AgentMessage`,
// `StreamEvent`, and `AgentConfig`. These describe the wire format
// between the agent loop and the model, not the per-message UI shape
// (that lives in `aipanel.ts`).

export type MessageRole = 'user' | 'assistant' | 'system' | 'tool';

/** Tool definition following OpenAI function calling format */
export interface ToolDefinition {
  type: 'function';
  function: ToolFunction;
}

export interface ToolFunction {
  name: string;
  description: string;
  parameters: ToolParameters;
}

export interface ToolParameters {
  type: 'object';
  properties: Record<string, ToolParameter>;
  required: string[];
  additionalProperties?: boolean;
}

export interface ToolParameter {
  type: string;
  description?: string;
  default?: unknown;
}

/** Tool call request from AI */
export interface ToolCall {
  id: string;
  name: string;
  arguments: Record<string, unknown>;
}

/** Tool execution status */
export type ToolCallStatus = 'pending' | 'executing' | 'success' | 'error';

/** Tool call with execution result */
export interface ToolCallResult {
  toolCallId: string;
  toolName: string;
  arguments: Record<string, unknown>;
  status: ToolCallStatus;
  result?: string;
  error?: string;
  duration?: number; // Execution time in ms
}

/** Agent message in conversation */
export interface AgentMessage {
  id: string;
  role: MessageRole;
  content: string;
  timestamp: number;
  toolCalls?: ToolCall[];
  toolCallId?: string; // If role is 'tool', this is the associated call ID
}

/**
 * Legacy stream-event shape retained for the inline-edit / CmdK path.
 * The agent loop uses `StreamPayload` (see `stream.ts`); the two
 * fields overlap intentionally so a `StreamEvent` is a valid
 * structural subset of `StreamPayload` minus a few extras.
 */
export interface StreamEvent {
  session_id: string;
  message_id: string;
  event_type:
    | 'text'
    | 'reasoning'
    | 'error'
    | 'tool_call_start'
    | 'tool_result'
    | 'done'
    | 'subagent_start'
    | 'subagent_end'
    | 'plan_result'
    | 'ask_user';
  content?: string;
  summary?: string;
  tool_call_id?: string;
  tool_name?: string;
  tool_args?: string;
  final_content?: string;
  error?: string;
  done: boolean;
}

/** Agent session configuration */
export interface AgentConfig {
  maxIterations: number;
  autoExecute: boolean; // Execute tools automatically without confirmation
  workspacePath?: string;
}

/** Agent mode */
export type AgentMode = 'ask' | 'plan' | 'agent';

/** Agent status */
export type AgentStatus = 'idle' | 'thinking' | 'executing' | 'error';