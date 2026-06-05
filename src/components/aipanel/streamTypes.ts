import type {
  DiffHunk,
  SearchResult,
} from '../../store';

export const TOOL_CALL_CLEAR_DELAY_MS = 2000;

export interface StreamDiffSummary {
  file_name: string;
  added_lines: number;
  deleted_lines: number;
  hunks: DiffHunk[];
}

export interface OfficeFileModifiedPayload {
  path: string;
  format: string;
}

export interface StreamPayload {
  session_id: string;
  message_id: string;
  event_type: string;
  content?: string;
  summary?: string;
  tool_call_id?: string;
  tool_name?: string;
  tool_args?: string;
  final_content?: string;
  error?: string;
  search_results?: SearchResult[];
  done: boolean;
  file_path?: string;
  original_content?: string;
  new_content?: string;
  diff_summary?: StreamDiffSummary;
  office_file_modified?: OfficeFileModifiedPayload;
}
