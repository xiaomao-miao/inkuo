// AI streaming protocol — events emitted by the Rust agent loop on
// the `ai://stream` Tauri event and consumed by `useAgentStream`.

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
  | 'subagent_end';