// Main entry-point: render a tool call's arguments for human display.
//
// Wires the per-tool schema (`fieldLabels`) to either the full
// renderers (when the args JSON has finished parsing) or the streaming
// extractors (when we're mid-stream and only have `rawArguments`).
//
// Split out of the original monolithic `toolUtils.ts` so this single
// orchestrator sits separately from the registries, value helpers,
// renderers, and streaming extractors that it composes.

import { PATH_DEDUP_KEYS, TOOL_FIELD_LABELS } from './fieldLabels';
import { renderElements, renderOperations, renderSheets } from './renderers';
import {
  extractFieldFromRaw,
  renderElementsFromRaw,
  renderOperationsFromRaw,
  renderSheetsFromRaw,
} from './streamingExtractors';
import { previewValue } from './valueHelpers';

type SummarizeKind = 'elements' | 'operations' | 'sheets';

/**
 * Pick the best available value for `key` given the args we have on hand.
 * Returns `null` when nothing useful is extractable.
 */
function resolveFieldValue(
  key: string,
  summarize: SummarizeKind | undefined,
  parsedArgs: Record<string, unknown> | null,
  rawArguments: string | undefined,
): string | null {
  if (parsedArgs) {
    const v = parsedArgs[key];
    if (v === undefined || v === null || v === '') return null;
    if (summarize === 'elements') return renderElements(v);
    if (summarize === 'operations') return renderOperations(v);
    if (summarize === 'sheets') return renderSheets(v);
    if (typeof v === 'string') return v;
    return previewValue(v, 80);
  }
  if (rawArguments) {
    // Streaming fallback — extract as much readable text as possible from the
    // partial JSON so the user watches content appear live.
    if (summarize === 'elements') return renderElementsFromRaw(rawArguments, key);
    if (summarize === 'operations') return renderOperationsFromRaw(rawArguments, key);
    if (summarize === 'sheets') return renderSheetsFromRaw(rawArguments, key);
    return extractFieldFromRaw(rawArguments, key);
  }
  return null;
}

/**
 * Format tool arguments into a human-readable multi-line string.
 *
 * - If `parsedArgs` is available, uses it directly.
 * - Falls back to regex extraction from `rawArguments` (handles mid-stream).
 * - Returns null if nothing useful can be extracted.
 */
export function formatArgumentsForDisplay(
  toolName: string,
  parsedArgs: Record<string, unknown> | null,
  rawArguments: string | undefined
): string | null {
  const fieldDefs = TOOL_FIELD_LABELS[toolName];
  if (!fieldDefs || fieldDefs.length === 0) {
    // Unknown tool: best-effort pretty print of parsedArgs
    if (parsedArgs) {
      const lines = Object.entries(parsedArgs)
        .filter(([, v]) => v !== undefined && v !== null && v !== '')
        .map(([k, v]) => {
          const display = typeof v === 'string' ? v : previewValue(v, 80);
          return `${k}: ${display}`;
        });
      return lines.length > 0 ? lines.join('\n') : null;
    }
    return null;
  }

  const lines: string[] = [];
  for (const { key, label, summarize } of fieldDefs) {
    const value = resolveFieldValue(key, summarize, parsedArgs, rawArguments);
    if (value === null) continue;

    // Multi-line body values go on their own line under the label for readability.
    if (value.includes('\n')) {
      lines.push(`${label}：\n${value}`);
    } else {
      lines.push(`${label}：${value}`);
    }
    // Only show the first matching field for path-like dedup (e.g. dir_path vs directory)
    if (PATH_DEDUP_KEYS.has(key)) break;
  }

  return lines.length > 0 ? lines.join('\n') : null;
}