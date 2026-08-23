import type {
  ChatMessage,
  KnowledgeSearchResult,
  MessageRole,
  MessageToolCall,
  SearchResult,
} from '../../store';
import type { ImageAttachmentInput } from '../../types';

/**
 * OpenAI-compatible tool history requires that an assistant message containing
 * `tool_calls` must be followed immediately by one tool message per call ID.
 * The UI may temporarily contain placeholder assistant messages, interrupted
 * runs, or orphaned tool messages, so we sanitize before sending history.
 */
function sanitizeConversationHistory(messages: ChatMessage[]): ChatMessage[] {
  const sanitized: ChatMessage[] = [];

  for (let i = 0; i < messages.length; i += 1) {
    const message = messages[i];

    if (message.role === 'assistant') {
      const toolCalls = message.toolCalls?.filter((call) => call?.id && call?.name) ?? [];
      const hasAssistantText = typeof message.content === 'string' && message.content.trim().length > 0;

      if (toolCalls.length === 0) {
        if (hasAssistantText) {
          sanitized.push(message);
        }
        continue;
      }

      const expectedIds = new Set(toolCalls.map((call) => call.id));
      const collectedToolMessages: ChatMessage[] = [];
      let cursor = i + 1;

      while (cursor < messages.length) {
        const candidate = messages[cursor];
        if (candidate.role !== 'tool') break;
        if (candidate.toolCallId && expectedIds.has(candidate.toolCallId)) {
          collectedToolMessages.push(candidate);
        }
        cursor += 1;
      }

      const matchedIds = new Set(collectedToolMessages.map((toolMessage) => toolMessage.toolCallId).filter(Boolean));
      const hasAllToolResponses = toolCalls.every((call) => matchedIds.has(call.id));

      if (!hasAllToolResponses) {
        continue;
      }

      sanitized.push({
        ...message,
        content: hasAssistantText ? message.content : '',
        toolCalls,
      });

      const orderedToolMessages = toolCalls
        .map((call) => collectedToolMessages.find((toolMessage) => toolMessage.toolCallId === call.id))
        .filter((toolMessage): toolMessage is ChatMessage => Boolean(toolMessage));

      sanitized.push(...orderedToolMessages);
      i = cursor - 1;
      continue;
    }

    if (message.role === 'tool') {
      continue;
    }

    if (message.role === 'user' || message.role === 'system') {
      if (typeof message.content === 'string' && message.content.trim().length > 0) {
        sanitized.push(message);
      }
    }
  }

  return sanitized;
}

/**
 * Normalizes a list of search results for display:
 * - Strips entries with missing/invalid file paths
 * - Fills in missing document titles from file paths
 * - Ensures score is a finite number
 *
 * @param results - The raw wire-format search results from Rust (snake_case)
 */
export function normalizeSearchResults(results: KnowledgeSearchResult[] | undefined): SearchResult[] | undefined {
  if (!results?.length) return undefined;

  return results
    .filter((result): result is KnowledgeSearchResult & { file_path: string } => {
      return !!result && typeof result.file_path === 'string' && result.file_path.trim().length > 0;
    })
    .map((result) => ({
      chunkId: result.chunk_id,
      documentId: result.document_id,
      content: result.content,
      score: Number.isFinite(result.score) ? result.score : 0,
      documentTitle:
        typeof result.document_title === 'string' && result.document_title.trim().length > 0
          ? result.document_title
          : result.file_path.split('/').pop() ?? '未命名文档',
      filePath: result.file_path,
      startLine: result.start_line,
      endLine: result.end_line,
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
  imageAttachments?: ImageAttachmentInput[];
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
    imageAttachments: message.imageAttachments,
  };
}

export function buildConversationHistory(messages: ChatMessage[]): AgentMessagePayload[] {
  return sanitizeConversationHistory(messages).map(toAgentPayload);
}

/**
 * Build the conversation history that should be sent when re-asking a
 * previously-sent user message. The returned array is the messages that
 * strictly precede `targetMessageId` (the target itself is excluded so
 * the LLM never sees its own previous answer to the same question, and
 * the new instruction is sent as the current user message rather than
 * duplicated inside the history).
 *
 * The same `sanitizeConversationHistory` masking is applied so any
 * orphaned tool messages from earlier turns are dropped instead of
 * poisoning the reconstructed context.
 *
 * Returns `undefined` when `targetMessageId` is not found in
 * `messages`, so the caller can abort the resend instead of silently
 * resending with a stale or empty history.
 */
export function buildConversationHistoryBefore(
  messages: ChatMessage[],
  targetMessageId: string,
): AgentMessagePayload[] | undefined {
  const index = messages.findIndex((message) => message.id === targetMessageId);
  if (index <= 0) {
    // Either the target doesn't exist (caller should abort) or it is
    // the very first message — there is no prior history to include.
    return index === -1 ? undefined : [];
  }
  const prior = messages.slice(0, index);
  return sanitizeConversationHistory(prior).map(toAgentPayload);
}
