import type { ChatMessage, MessageRole, MessageToolCall, SearchResult } from '../../store';

/**
 * Normalizes a list of search results for display:
 * - Strips entries with missing/invalid file paths
 * - Fills in missing document titles from file paths
 * - Ensures score is a finite number
 */
export function normalizeSearchResults(results: SearchResult[] | undefined): SearchResult[] | undefined {
  if (!results?.length) return undefined;

  return results
    .filter((result): result is SearchResult & { filePath: string } => {
      return !!result && typeof result.filePath === 'string' && result.filePath.trim().length > 0;
    })
    .map((result) => ({
      ...result,
      documentTitle:
        typeof result.documentTitle === 'string' && result.documentTitle.trim().length > 0
          ? result.documentTitle
          : result.filePath.split('/').pop() ?? '未命名文档',
      score: Number.isFinite(result.score) ? result.score : 0,
    }));
}

/**
 * DTO sent to the Rust backend for agent chat.
 * Uses snake_case to match the Rust/Tauri wire format.
 */
export interface AgentMessagePayload {
  id: string;
  role: MessageRole;
  content: string;
  tool_calls?: MessageToolCall[];
  tool_call_id?: string;
}

/**
 * Extracts the visible text content from a ChatMessage for sending to the backend.
 * - Tool messages: return content directly
 * - Assistant messages with outputItems: concatenate all text-type items
 * - All other roles: return content field
 */
function extractTextContent(message: ChatMessage): string {
  if (message.role === 'tool') {
    return message.content ?? '';
  }

  if (message.outputItems.length > 0) {
    return message.outputItems
      .filter((item) => item.type === 'text')
      .map((item) => item.content)
      .join('');
  }

  return message.content ?? '';
}

export function toAgentPayload(message: ChatMessage): AgentMessagePayload {
  return {
    id: message.id,
    role: message.role,
    content: extractTextContent(message),
    tool_calls: message.toolCalls,
    tool_call_id: message.toolCallId,
  };
}

export function buildConversationHistory(messages: ChatMessage[]): AgentMessagePayload[] {
  return messages.map(toAgentPayload);
}
