// Knowledge-base types — SearchResult / KnowledgeSearchResult (the
// frontend-facing and Rust-mirror shapes used by the RAG layer), plus
// `KnowledgeBase` metadata and `BuildProgress` for the build pipeline.

export interface SearchResult {
  chunkId: string;
  documentId: string;
  content: string;
  score: number;
  documentTitle: string;
  filePath: string;
  startLine?: number;
  endLine?: number;
  collection?: string;
}

export interface KnowledgeSearchResult {
  chunk_id: string;
  document_id: string;
  content: string;
  score: number;
  document_title: string;
  file_path: string;
  start_line?: number;
  end_line?: number;
  collection?: string;
}

export interface KnowledgeBase {
  workspaceId: string;
  documentCount: number;
  chunkCount: number;
  lastUpdated: number;
  /** Explicitly selected member file paths (relative to workspace) */
  members: string[];
  /** Named collection -> member paths. `default` migrates legacy indexes. */
  collections: Record<string, string[]>;
  documents: KnowledgeDocumentStatus[];
  supportedExtensions: string[];
}

export type KnowledgeIndexStatus = 'indexed' | 'pending' | 'error';

export interface KnowledgeDocumentStatus {
  path: string;
  collection: string;
  status: KnowledgeIndexStatus;
  chunkCount: number;
  sourceType: string;
  sizeBytes: number;
  indexedAt?: number;
  error?: string;
}

export interface KnowledgeUpdateResult {
  added: number;
  removed: number;
  updated: number;
  unchanged: number;
  failed: number;
  failures: Array<{ path: string; error: string }>;
}

export interface BuildProgress {
  phase: 'scanning' | 'chunking' | 'embedding' | 'storing' | 'done';
  current: number;
  total: number;
  currentFile?: string;
}
