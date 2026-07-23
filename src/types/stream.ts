// AI streaming protocol — events emitted by the Rust agent loop on
// the `ai://stream` Tauri event and consumed by `useAgentStream`.
// Re-exports related plan / ask-user payloads for convenience.

import type { StreamDiffSummary } from './diff';
import type { KnowledgeSearchResult } from './knowledge';

export interface OfficeFileModifiedPayload {
  path: string;
  format: string;
}

export interface StreamPayload {
  session_id: string;
  message_id: string;
  event_type: StreamEventType | 'tool_call_args_delta';
  content?: string;
  summary?: string;
  tool_call_id?: string;
  tool_name?: string;
  tool_args?: string;
  final_content?: string;
  error?: string;
  search_results?: KnowledgeSearchResult[];
  done: boolean;
  file_path?: string;
  original_content?: string;
  new_content?: string;
  diff_summary?: StreamDiffSummary;
  office_file_modified?: OfficeFileModifiedPayload;
  plan_result?: PlanResultData;
  ask_user?: AskUserPayload;
}

/** Payload for the ask_user stream event. */
export interface AskUserPayload {
  question: string;
  options: string[];
  allow_custom: boolean;
}

/** Payload for subagent_start event */
export interface SubagentStartPayload {
  session_id: string;
  parent_message_id: string;
  sub_message_id: string;
  expert: string;
  label: string;
  task: string;
}

/** Stream event types */
export type StreamEventType =
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

/**
 * Parsed plan data carried in the `plan_result` stream event, emitted by
 * the Rust `create_plan` tool handler after writing the plan file to disk.
 */
export interface PlanResultData {
  /** Markdown prose describing the plan. */
  content: string;
  /** One-sentence summary shown as the card subtitle. */
  plan_summary: string;
  /** Files the plan touches. */
  files_to_touch: Array<{
    path: string;
    intent: string;
    reason: string;
  }>;
  risk: string;
  risk_reason?: string;
  /** Absolute path to the saved plan file. */
  saved_path: string;
}