// AI streaming protocol — events emitted by the Rust agent loop on
// the `ai://stream` Tauri event and consumed by `useAgentStream`.

import type { StreamDiffSummary } from './diff';
import type { KnowledgeSearchResult } from './knowledge';

export interface OfficeFileModifiedPayload {
  path: string;
  format: string;
}

/** One option inside an `ask_user` question. Mirrors `AskUserOption` in
 * `src-tauri/src/runtime/ask_pending.rs`. */
export interface AskUserOptionPayload {
  label: string;
  description?: string;
}

/** One question in an `ask_user` invocation. The Rust agent loop emits
 * a `tool_paused` event with a list of these; the frontend renders an
 * `AskUserCard` for each. */
export interface AskUserQuestionPayload {
  question: string;
  options: AskUserOptionPayload[];
  multiSelect?: boolean;
  header?: string;
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
  /** Present only on `event_type === 'tool_paused'`. The resume
   * command echoes this back so a stale Submit (from a previous pause)
   * is rejected. */
  request_id?: string;
  /** Question schema. Present only on `event_type === 'tool_paused'`. */
  questions?: AskUserQuestionPayload[];
}

/** Payload for subagent_start event */
export interface SubagentStartPayload {
  session_id: string;
  parent_message_id: string;
  sub_message_id: string;
  tool_call_id?: string;
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
  /** Emitted by the agent loop when it called `ask_user`. The run is
   * parked in `runtime::ask_pending`; the frontend renders an
   * `AskUserCard` and replies via `ai_agent_resume`. */
  | 'tool_paused'
  /** Terminal event emitted by `ai_agent_stream` when the loop parked
   * itself in response to `ask_user`. Mirrors `cancelled` / `done`. */
  | 'stream_paused';
