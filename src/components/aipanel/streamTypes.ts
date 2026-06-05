import type {
  KnowledgeSearchResult as WireSearchResult,
  OfficeFileModifiedPayload,
  StreamDiffSummary,
  StreamPayload,
} from '../../types';

export const TOOL_CALL_CLEAR_DELAY_MS = 2000;

export type {
  OfficeFileModifiedPayload,
  StreamDiffSummary,
  StreamPayload,
  WireSearchResult,
};
