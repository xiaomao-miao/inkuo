// Barrel re-export for the global type registry. The canonical domain
// modules live next to this file; importing from `types` directly is
// preserved for backwards compatibility so a search-and-replace of
// call sites isn't required.
//
// New code should import from the focused domain file when it knows
// exactly which slice it needs (`types/aipanel`, `types/cloud`, etc.)
// — that lets tree-shaking drop the rest.

export type {
  Document,
  DocumentType,
  Block,
  BlockKind,
  Range,
} from './document';

export type {
  DiffResult,
  DiffHunk,
  DiffChange,
  DiffSummary,
  StreamDiffSummary,
} from './diff';

export type {
  OfficeFileModifiedPayload,
  StreamPayload,
  AskUserPayload,
  SubagentStartPayload,
  StreamEventType,
  PlanResultData,
} from './stream';

export type {
  AIEditRequest,
  EditScope,
  ContextItem,
  AIEditResponse,
} from './ai';

export type {
  MessageRole,
  ToolDefinition,
  ToolFunction,
  ToolParameters,
  ToolParameter,
  ToolCall,
  ToolCallStatus,
  ToolCallResult,
  AgentMessage,
  StreamEvent,
  AgentConfig,
  AgentMode,
  AgentStatus,
} from './agent';

export type {
  SearchResult,
  KnowledgeSearchResult,
  KnowledgeBase,
  BuildProgress,
} from './knowledge';

export type {
  ChatMode,
  FeatureToggleId,
  FeatureToggleDescriptor,
  FeatureToggleMap,
  MessageToolCall,
  MessageToolResult,
  PlanFileIntent,
  PlanRisk,
  PlanFileTouch,
  PlanOutput,
  TodoStatus,
  TodoItem,
  TodoSnapshot,
  TodoAction,
  OutputItem,
  CurrentDiff,
  ActiveToolCall,
  ChatMessage,
  SubagentActivity,
  ChatSession,
} from './aipanel';

export type {
  FileKind,
  FileEntry,
  ViewerFilePayload,
  NewEntryPayload,
  CreateEntryResult,
  RenamePathResult,
  NewFileTemplate,
} from './files';
export {
  NEW_FILE_TEMPLATES,
  detectFileKind,
  detectLegacyFileType,
} from './files';

export type {
  AIProviderType,
  APIConfig,
  CloudAccount,
  CloudModelEntry,
  CloudSettings,
} from './cloud';

export type {
  WebSearchProviderConfig,
  WebSearchRouting,
  WebSearchSettings,
  Settings,
  ExpertProfileName,
  EmbeddingModelType,
  EmbeddingModelInfo,
  ThemeType,
} from './settings';