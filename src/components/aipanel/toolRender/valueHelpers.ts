// Small value-shaping helpers used by the per-tool renderers.
//
// These don't know anything about the streaming case (no regex / string
// probing) — they just take an already-parsed JSON value and return a
// short human-readable preview. The "streaming" twin of each lives in
// `streamingExtractors.ts`.

/**
 * Format a value (any JSON value) into a short human-readable preview.
 * Truncates strings and summarizes arrays/objects without dumping raw JSON.
 */
export function previewValue(v: unknown, maxLen = 80): string {
  if (v === null || v === undefined) return '';
  if (typeof v === 'string') {
    return v.length > maxLen ? `${v.slice(0, maxLen)}…` : v;
  }
  if (typeof v === 'number' || typeof v === 'boolean') return String(v);
  if (Array.isArray(v)) {
    if (v.length === 0) return '[]';
    return `[${v.length} 项]`;
  }
  if (typeof v === 'object') {
    const keys = Object.keys(v);
    if (keys.length === 0) return '{}';
    return `{${keys.length} 字段}`;
  }
  return String(v);
}

export const HEADING_PREFIX: Record<string, string> = {
  Title: '',
  Heading1: '# ',
  Heading2: '## ',
  Heading3: '### ',
};

/** Extract text from a run list when a paragraph uses `runs[]` instead of `text`. */
export function textFromRuns(runs: unknown): string {
  if (!Array.isArray(runs)) return '';
  return runs
    .map((r) => (r && typeof r === 'object' ? (r as Record<string, unknown>).text : ''))
    .filter((t): t is string => typeof t === 'string')
    .join('');
}

/** Render a table cell (string or {text, ...} object) to text. */
export function cellText(cell: unknown): string {
  if (typeof cell === 'string') return cell;
  if (cell && typeof cell === 'object') {
    const t = (cell as Record<string, unknown>).text;
    if (typeof t === 'string') return t;
  }
  return '';
}

/** Unwrap a create_excel/modify_excel typed value object `{type, value}`. */
export function unwrapCellValue(value: unknown): string {
  if (value && typeof value === 'object' && 'value' in (value as Record<string, unknown>)) {
    const inner = (value as Record<string, unknown>).value;
    return inner === null || inner === undefined ? '' : String(inner);
  }
  if (value === null || value === undefined) return '';
  return String(value);
}
