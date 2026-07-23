// AI Edit request/response shape used by the inline edit (⌘K / CmdK)
// flow. The agent loop / tool-calling types live in `agent.ts`; this
// file holds the request-shape used by the *non-agent* edit path.

export interface AIEditRequest {
  instruction: string;
  original_text: string;
  scope: EditScope;
  context: ContextItem[];
}

export type EditScope = 'Selection' | 'Paragraph' | 'Section' | 'Document';

export interface ContextItem {
  title: string;
  path: string;
  range: string;
  excerpt: string;
}

export interface AIEditResponse {
  summary: string;
  content: string;
  rules_applied: string[];
}