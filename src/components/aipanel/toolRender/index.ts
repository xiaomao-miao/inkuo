// Tool-argument rendering, split out of the original monolithic `toolUtils.ts`.
//
// This directory is organized so each module owns a single concern:
//
//   - `registries.ts`          — display-name / tool-set lookup tables
//   - `fieldLabels.ts`         — per-tool argument schema (key → Chinese label)
//   - `valueHelpers.ts`        — string/array/object → short preview helpers
//   - `streamingExtractors.ts` — regex-based extraction from partial JSON
//   - `renderers.ts`           — full render of `summarize` fields (parsed)
//   - `formatArguments.ts`     — top-level orchestrator
//
// Anything not exported here is an internal helper. The legacy
// `toolUtils.ts` barrel re-exports the public surface so existing
// consumers (`CompactToolCard`, `AssistantMessageBody`, etc.) keep
// working without changes.

export {
  COMPACT_TOOLS,
  FILE_MODIFICATION_TOOLS,
  PREVIEW_STRING_KEYS,
  extractFileNameFromPath,
  getExpertDisplayName,
  getToolDisplayName,
  isFileModificationTool,
} from './registries';

export { PATH_DEDUP_KEYS, TOOL_FIELD_LABELS } from './fieldLabels';

export {
  HEADING_PREFIX,
  cellText,
  previewValue,
  textFromRuns,
  unwrapCellValue,
} from './valueHelpers';

export {
  extractArrayBody,
  extractFieldFromRaw,
  renderElementsFromRaw,
  renderOperationsFromRaw,
  renderSheetsFromRaw,
  splitArrayEntries,
} from './streamingExtractors';

export { renderElements, renderOperations, renderSheets } from './renderers';

export { formatArgumentsForDisplay } from './formatArguments';