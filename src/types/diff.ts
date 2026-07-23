// Diff types used by InlineDiffPreview, DiffOverlay, and the Rust
// `compute_diff` IPC command's payload.

export interface DiffResult {
  hunks: DiffHunk[];
  summary: DiffSummary;
}

export interface DiffHunk {
  id: string;
  old_start: number;
  old_lines: number;
  new_start: number;
  new_lines: number;
  /** Character offset in the original text where this hunk starts */
  old_offset: number;
  /** Character offset in the modified text where this hunk starts */
  new_offset: number;
  changes: DiffChange[];
}

export interface DiffChange {
  tag: 'delete' | 'insert' | 'equal';
  old_line: number | null;
  new_line: number | null;
  content: string;
}

export interface DiffSummary {
  added_lines: number;
  deleted_lines: number;
  unchanged_lines: number;
  description: string;
}

/**
 * Stream-time variant of a diff summary, embedded in tool-result /
 * active-tool-call payloads. Kept separate from `DiffSummary` so the
 * Rust `StreamDiffSummary` payload doesn't leak into editor diff
 * rendering (which uses the larger `DiffSummary`).
 */
export interface StreamDiffSummary {
  file_name: string;
  added_lines: number;
  deleted_lines: number;
  hunks: DiffHunk[];
}